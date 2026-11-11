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

pub(crate) fn ensure_codex_desktop_closed(codex_home: &Path) -> Result<(), String> {
    let system = System::new_all();
    let desktop_running = system.processes().values().any(|process| {
        let executable = process
            .exe()
            .map(Path::to_path_buf)
            .or_else(|| process.cmd().first().map(PathBuf::from));
        let Some(executable) = executable else {
            return false;
        };
        let has_desktop_resources = executable
            .parent()
            .is_some_and(|parent| parent.join("resources").join("app.asar").is_file());
        is_codex_desktop_main_process(&executable, process.cmd(), has_desktop_resources)
            && resolve_codex_home_from_environment(process.environ())
                .is_some_and(|process_home| same_path(&process_home, codex_home))
    });
    if desktop_running {
        Err(format!(
            "Close Codex desktop before switching accounts to preserve local projects (CODEX_HOME: {})",
            codex_home.display()
        ))
    } else {
        Ok(())
    }
}

fn is_codex_desktop_main_process(
    executable: &Path,
    command: &[String],
    has_desktop_resources: bool,
) -> bool {
    // Electron reuses the main executable for renderer and utility processes.
    // The CLI and app-server can also be bundled alongside the desktop app.
    if command.iter().any(|argument| {
        argument == "app-server" || argument == "--type" || argument.starts_with("--type=")
    }) {
        return false;
    }
    let executable = executable.to_string_lossy().replace('\\', "/");
    let executable = executable.to_ascii_lowercase();
    let name = executable.rsplit('/').next().unwrap_or_default();
    // Current Windows packages launch the Electron host as ChatGPT.exe;
    // Codex.exe in the same directory is only the launcher. Restrict that
    // executable alias to the Codex package so the ChatGPT app is not matched.
    let windows_codex_package = executable.contains("/windowsapps/openai.codex_")
        && matches!(name, "codex.exe" | "chatgpt.exe")
        && executable.ends_with(&format!("/app/{name}"));
    if name == "chatgpt.exe" {
        return windows_codex_package;
    }
    if !matches!(name, "codex" | "codex.exe") {
        return false;
    }
    // Bundle paths identify the desktop app even if resource metadata cannot
    // be read. Other installs (including Linux AppImages) require app.asar next
    // to the executable; the bare CLI name alone is deliberately insufficient.
    executable.ends_with("/codex.app/contents/macos/codex")
        || windows_codex_package
        || has_desktop_resources
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
    fn desktop_detection_accepts_packaged_windows_macos_and_linux_main_processes() {
        for (executable, has_resources) in [
            (
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1106.6628.0_x64__example\app\Codex.exe",
                false,
            ),
            (
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1106.6628.0_x64__example\app\ChatGPT.exe",
                false,
            ),
            ("/Applications/Codex.app/Contents/MacOS/Codex", false),
            ("/opt/codex/codex", true),
            ("/tmp/.mount_Codex123/codex", true),
            (
                r"C:\Users\Example\AppData\Local\Programs\Codex\Codex.exe",
                true,
            ),
        ] {
            assert!(
                is_codex_desktop_main_process(
                    Path::new(executable),
                    &strings(&[executable]),
                    has_resources,
                ),
                "desktop executable was not detected: {executable}"
            );
        }
    }

    #[test]
    fn desktop_detection_rejects_cli_app_servers_node_helpers_and_manager() {
        for (executable, arguments, has_resources) in [
            ("/usr/bin/codex", vec!["codex"], false),
            (r"C:\tools\codex.exe", vec!["codex.exe", "resume"], false),
            ("/opt/codex/codex", vec!["codex", "app-server"], true),
            ("/usr/bin/node", vec!["node", "codex.js"], true),
            ("/opt/codex/codex", vec!["codex", "--type=renderer"], true),
            ("/opt/codex/codex", vec!["codex", "--type", "utility"], true),
            (
                "/Applications/Codex.app/Contents/Frameworks/Codex Helper.app/Contents/MacOS/Codex Helper",
                vec!["Codex Helper"],
                true,
            ),
            ("/opt/codex/chrome_crashpad_handler", vec!["chrome_crashpad_handler"], true),
            (r"C:\CodexManager\CodexManager.exe", vec!["CodexManager.exe"], true),
            (
                r"C:\Program Files\WindowsApps\OpenAI.ChatGPT_1.0_x64__example\app\ChatGPT.exe",
                vec!["ChatGPT.exe"],
                true,
            ),
            (r"C:\tools\ChatGPT.exe", vec!["ChatGPT.exe"], true),
            (
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.1106.6628.0_x64__example\app\ChatGPT.exe",
                vec!["ChatGPT.exe", "--type=renderer"],
                false,
            ),
        ] {
            assert!(
                !is_codex_desktop_main_process(
                    Path::new(executable),
                    &strings(&arguments),
                    has_resources,
                ),
                "non-desktop process was detected: {executable} {arguments:?}"
            );
        }
    }

    #[test]
    fn desktop_home_matching_requires_process_environment_and_same_profile() {
        let target = Path::new("/profiles/account-a");
        for environment in [
            strings(&[]),
            strings(&["CODEX_HOME=/profiles/account-b"]),
            strings(&["HOME=/users/another"]),
        ] {
            assert!(!resolve_codex_home_from_environment(&environment)
                .is_some_and(|process_home| same_path(&process_home, target)));
        }
        assert!(resolve_codex_home_from_environment(&strings(&[
            "CODEX_HOME=/profiles/account-a",
            "HOME=/users/another",
        ]))
        .is_some_and(|process_home| same_path(&process_home, target)));
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
