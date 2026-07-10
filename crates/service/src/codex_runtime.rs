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
    let candidate_pids = system
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

    let matched_process_count = candidate_pids.len();
    let message = if matched_process_count == 0 {
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

fn same_path(left: &Path, right: &Path) -> bool {
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
