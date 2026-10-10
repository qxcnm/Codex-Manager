use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use sysinfo::{Signal, System};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRuntimeReloadResult {
    pub requested: bool,
    pub matched_process_count: usize,
    pub signaled_process_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

impl CodexRuntimeReloadResult {
    pub(crate) fn skipped() -> Self {
        Self {
            requested: false,
            matched_process_count: 0,
            signaled_process_count: 0,
            warnings: Vec::new(),
            message: "Codex runtime reload was disabled; running clients keep their current configuration until restarted".to_string(),
        }
    }
}

pub(crate) fn reload_codex_app_servers(codex_home: &Path) -> CodexRuntimeReloadResult {
    let system = System::new_all();
    let matched_pids = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            if !is_codex_app_server_command(process.cmd()) {
                return None;
            }
            let process_home = resolve_codex_home_from_environment(process.environ())?;
            same_path(&process_home, codex_home).then_some(*pid)
        })
        .collect::<HashSet<_>>();
    let protected_ancestor_pids =
        process_ancestor_pids(&system, sysinfo::Pid::from_u32(std::process::id()));
    let candidate_pids = matched_pids
        .difference(&protected_ancestor_pids)
        .copied()
        .collect::<HashSet<_>>();
    let protected_process_count = matched_pids.len().saturating_sub(candidate_pids.len());

    let root_pids = candidate_pids
        .iter()
        .copied()
        .filter(|pid| {
            system
                .process(*pid)
                .and_then(|process| process.parent())
                .is_none_or(|parent| !candidate_pids.contains(&parent))
        })
        .collect::<Vec<_>>();

    let mut signaled_process_count = 0;
    let mut warnings = Vec::new();
    for pid in root_pids {
        let Some(process) = system.process(pid) else {
            continue;
        };
        let signaled = process
            .kill_with(Signal::Term)
            .unwrap_or_else(|| process.kill());
        if signaled {
            signaled_process_count += 1;
        } else {
            warnings.push(format!(
                "failed to signal Codex app-server process {}",
                pid.as_u32()
            ));
        }
    }

    let matched_process_count = matched_pids.len();
    let message = if candidate_pids.is_empty() && protected_process_count > 0 {
        format!(
            "Skipped {protected_process_count} matching Codex app-server process(es) because they own the current CodexManager process; reopen Codex to load the updated configuration"
        )
    } else if matched_process_count == 0 {
        "No matching Codex app-server process was running; new clients will read the updated configuration"
            .to_string()
    } else if signaled_process_count == 0 {
        "Matching Codex app-server processes were found, but none accepted the reload signal"
            .to_string()
    } else {
        format!(
            "Sent a reload signal to {signaled_process_count} Codex app-server process(es); owning clients may restart them"
        )
    };

    CodexRuntimeReloadResult {
        requested: true,
        matched_process_count,
        signaled_process_count,
        warnings,
        message,
    }
}

fn process_ancestor_pids(system: &System, start: sysinfo::Pid) -> HashSet<sysinfo::Pid> {
    collect_ancestor_pids(start, |pid| {
        system.process(pid).and_then(|process| process.parent())
    })
}

fn collect_ancestor_pids<F>(start: sysinfo::Pid, mut parent_for: F) -> HashSet<sysinfo::Pid>
where
    F: FnMut(sysinfo::Pid) -> Option<sysinfo::Pid>,
{
    let mut ancestors = HashSet::new();
    let mut current = start;
    while let Some(parent) = parent_for(current) {
        if !ancestors.insert(parent) {
            break;
        }
        current = parent;
    }
    ancestors
}

fn is_codex_app_server_command(command: &[String]) -> bool {
    if !command.iter().any(|arg| arg == "app-server") {
        return false;
    }
    let Some(first) = command.first() else {
        return false;
    };
    if is_codex_executable(first) {
        return true;
    }
    is_node_executable(first)
        && command
            .get(1)
            .is_some_and(|entry| is_codex_executable(entry))
}

fn is_codex_executable(value: &str) -> bool {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "codex" | "codex.exe" | "codex.js"
            )
        })
}

fn is_node_executable(value: &str) -> bool {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name.to_ascii_lowercase().as_str(), "node" | "node.exe"))
}

fn resolve_codex_home_from_environment(environment: &[String]) -> Option<PathBuf> {
    if let Some(value) = environment_value(environment, "CODEX_HOME") {
        return Some(PathBuf::from(value));
    }
    if let Some(value) = environment_value(environment, "USERPROFILE") {
        return Some(PathBuf::from(value).join(".codex"));
    }
    if let Some(value) = environment_value(environment, "HOME") {
        return Some(PathBuf::from(value).join(".codex"));
    }
    let home_drive = environment_value(environment, "HOMEDRIVE").unwrap_or_default();
    let home_path = environment_value(environment, "HOMEPATH").unwrap_or_default();
    let combined = format!("{home_drive}{home_path}");
    (!combined.trim().is_empty()).then(|| PathBuf::from(combined).join(".codex"))
}

fn environment_value(environment: &[String], key: &str) -> Option<String> {
    environment.iter().find_map(|entry| {
        let (candidate, value) = entry.split_once('=')?;
        candidate
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let value = resolved
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn app_server_detection_accepts_codex_and_node_wrappers() {
        assert!(is_codex_app_server_command(&strings(&[
            "/usr/bin/codex",
            "-c",
            "feature=true",
            "app-server",
            "--listen",
            "unix://",
        ])));
        assert!(is_codex_app_server_command(&strings(&[
            "node",
            "/home/test/.local/bin/codex",
            "app-server",
            "proxy",
        ])));
    }

    #[cfg(windows)]
    #[test]
    fn same_path_accepts_windows_case_and_separator_differences() {
        assert!(same_path(
            Path::new(r"C:\Users\Example\.codex\gateway-models.json"),
            Path::new("c:/users/example/.CODEX/gateway-models.json")
        ));
    }

    #[test]
    fn app_server_detection_rejects_foreground_cli_and_shell_commands() {
        assert!(!is_codex_app_server_command(&strings(&[
            "/usr/bin/codex",
            "--model",
            "gpt-test",
        ])));
        assert!(!is_codex_app_server_command(&strings(&[
            "/bin/sh",
            "-c",
            "codex app-server proxy",
        ])));
        assert!(!is_codex_app_server_command(&strings(&[
            "/usr/bin/codexmanager-service",
            "app-server",
        ])));
    }

    #[test]
    fn ancestor_collection_stops_at_the_root_and_breaks_cycles() {
        let pid = |value| sysinfo::Pid::from_u32(value);
        let parents = std::collections::HashMap::from([
            (pid(40), pid(30)),
            (pid(30), pid(20)),
            (pid(20), pid(30)),
        ]);
        let ancestors =
            collect_ancestor_pids(pid(40), |candidate| parents.get(&candidate).copied());

        assert_eq!(ancestors, HashSet::from([pid(30), pid(20)]));
    }

    #[test]
    fn environment_resolution_prefers_explicit_codex_home() {
        let environment = strings(&["HOME=/home/test", "CODEX_HOME=/srv/codex-profile"]);
        assert_eq!(
            resolve_codex_home_from_environment(&environment),
            Some(PathBuf::from("/srv/codex-profile"))
        );
    }

    #[test]
    fn environment_resolution_falls_back_to_home() {
        let environment = strings(&["HOME=/home/test"]);
        assert_eq!(
            resolve_codex_home_from_environment(&environment),
            Some(PathBuf::from("/home/test/.codex"))
        );
    }
}
