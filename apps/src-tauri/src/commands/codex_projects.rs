use crate::app_storage::resolve_db_path_with_legacy_migration;
use codexmanager_core::storage::{now_ts, Storage};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

const PROJECTS_SETTING_KEY: &str = "desktop.codex_projects.v1";
const CODEX_HOME_SETTING_KEY: &str = "codex_profile.codex_home";
const PROJECT_STORE_VERSION: u32 = 1;
const MAX_PROJECTS: usize = 200;

static PROJECT_STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredCodexProject {
    path: String,
    name: String,
    added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CodexProjectStore {
    #[serde(default = "project_store_version")]
    version: u32,
    #[serde(default)]
    items: Vec<StoredCodexProject>,
}

impl Default for CodexProjectStore {
    fn default() -> Self {
        Self {
            version: PROJECT_STORE_VERSION,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProjectSummary {
    path: String,
    name: String,
    added_at: i64,
    available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProjectListResult {
    items: Vec<CodexProjectSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProjectAddResult {
    canceled: bool,
    added: bool,
    project: Option<CodexProjectSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProjectRemoveResult {
    removed: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexProjectLaunchAction {
    Start,
    Resume,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexProjectLaunchTarget {
    App,
    Cli,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProjectLaunchResult {
    path: String,
    action: CodexProjectLaunchAction,
    target: CodexProjectLaunchTarget,
    codex_home: Option<String>,
}

fn project_store_version() -> u32 {
    PROJECT_STORE_VERSION
}

fn lock_project_store() -> MutexGuard<'static, ()> {
    PROJECT_STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn open_storage(db_path: &Path) -> Result<Storage, String> {
    let storage = Storage::open(db_path).map_err(|err| format!("打开桌面项目存储失败：{err}"))?;
    storage
        .init()
        .map_err(|err| format!("初始化桌面项目存储失败：{err}"))?;
    Ok(storage)
}

fn load_store(storage: &Storage) -> Result<CodexProjectStore, String> {
    let Some(raw) = storage
        .get_app_setting(PROJECTS_SETTING_KEY)
        .map_err(|err| format!("读取桌面项目列表失败：{err}"))?
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(CodexProjectStore::default());
    };

    let store: CodexProjectStore = serde_json::from_str(&raw)
        .map_err(|err| format!("桌面项目列表数据损坏，无法安全读取：{err}"))?;
    if store.version != PROJECT_STORE_VERSION {
        return Err(format!("不支持的桌面项目列表版本：{}", store.version));
    }
    if store.items.len() > MAX_PROJECTS {
        return Err(format!("桌面项目列表超过安全上限（{MAX_PROJECTS}）"));
    }
    Ok(store)
}

fn save_store(storage: &Storage, store: &CodexProjectStore) -> Result<(), String> {
    let raw =
        serde_json::to_string(store).map_err(|err| format!("序列化桌面项目列表失败：{err}"))?;
    storage
        .set_app_setting(PROJECTS_SETTING_KEY, &raw, now_ts())
        .map_err(|err| format!("保存桌面项目列表失败：{err}"))
}

fn project_summary(project: &StoredCodexProject) -> CodexProjectSummary {
    CodexProjectSummary {
        path: portable_stored_path(&project.path),
        name: project.name.clone(),
        added_at: project.added_at,
        available: Path::new(&project.path).is_dir(),
    }
}

fn list_projects_from_storage(storage: &Storage) -> Result<CodexProjectListResult, String> {
    let store = load_store(storage)?;
    Ok(CodexProjectListResult {
        items: store.items.iter().map(project_summary).collect(),
    })
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("项目目录不存在或无法访问（{}）：{err}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!("所选路径不是目录：{}", path.display()));
    }
    path.canonicalize()
        .map_err(|err| format!("解析项目目录失败（{}）：{err}", path.display()))
}

fn utf8_path(path: &Path, label: &str) -> Result<String, String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{label}包含当前界面无法安全保存的字符：{}", path.display()))
}

fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(target_os = "windows")]
fn project_path_key(path: &str) -> String {
    windows_path_without_verbatim_prefix(path).to_lowercase()
}

#[cfg(not(target_os = "windows"))]
fn project_path_key(path: &str) -> String {
    path.to_string()
}

#[cfg(target_os = "windows")]
fn portable_stored_path(path: &str) -> String {
    windows_path_without_verbatim_prefix(path)
}

#[cfg(not(target_os = "windows"))]
fn portable_stored_path(path: &str) -> String {
    path.to_string()
}

fn add_project_to_storage(
    storage: &Storage,
    selected_path: &Path,
    added_at: i64,
) -> Result<CodexProjectAddResult, String> {
    let canonical = canonical_directory(selected_path)?;
    let path = portable_path_string(&canonical, "项目目录")?;
    let key = project_path_key(&path);
    let mut store = load_store(storage)?;

    if let Some(existing) = store
        .items
        .iter()
        .find(|item| project_path_key(&item.path) == key)
    {
        return Ok(CodexProjectAddResult {
            canceled: false,
            added: false,
            project: Some(project_summary(existing)),
        });
    }
    if store.items.len() >= MAX_PROJECTS {
        return Err(format!("最多可保存 {MAX_PROJECTS} 个项目目录"));
    }

    let project = StoredCodexProject {
        name: project_name(&canonical),
        path,
        added_at,
    };
    store.items.insert(0, project.clone());
    save_store(storage, &store)?;
    Ok(CodexProjectAddResult {
        canceled: false,
        added: true,
        project: Some(project_summary(&project)),
    })
}

fn remove_project_from_storage(
    storage: &Storage,
    path: &str,
) -> Result<CodexProjectRemoveResult, String> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return Err("缺少要移除的项目目录".to_string());
    }
    let key = project_path_key(normalized);
    let mut store = load_store(storage)?;
    let before = store.items.len();
    store
        .items
        .retain(|item| project_path_key(&item.path) != key);
    let removed = store.items.len() != before;
    if removed {
        save_store(storage, &store)?;
    }
    Ok(CodexProjectRemoveResult { removed })
}

fn registered_project_path(storage: &Storage, requested_path: &str) -> Result<PathBuf, String> {
    let normalized = requested_path.trim();
    if normalized.is_empty() {
        return Err("缺少要启动的项目目录".to_string());
    }
    let key = project_path_key(normalized);
    let store = load_store(storage)?;
    let stored = store
        .items
        .iter()
        .find(|item| project_path_key(&item.path) == key)
        .ok_or_else(|| "只能启动已添加到项目列表的目录".to_string())?;
    canonical_directory(Path::new(&stored.path))
}

fn expand_home_prefix(value: &str) -> PathBuf {
    if value == "~" || value.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            let suffix = value.strip_prefix("~/").unwrap_or_default();
            return PathBuf::from(home).join(suffix);
        }
    }
    PathBuf::from(value)
}

fn validate_codex_home(path: PathBuf, source: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(&path).map_err(|err| {
        format!(
            "{source}指向的 Codex profile 不存在或无法访问（{}）：{err}",
            path.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "{source}指向的 Codex profile 不是目录：{}",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|err| format!("解析 Codex profile 失败（{}）：{err}", path.display()))
}

fn resolve_codex_home(storage: &Storage) -> Result<Option<PathBuf>, String> {
    if let Some(configured) = storage
        .get_app_setting(CODEX_HOME_SETTING_KEY)
        .map_err(|err| format!("读取 Codex profile 设置失败：{err}"))?
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return validate_codex_home(expand_home_prefix(&configured), "本机平台模式设置").map(Some);
    }

    if let Ok(configured) = std::env::var("CODEX_HOME") {
        let configured = configured.trim();
        if !configured.is_empty() {
            return validate_codex_home(expand_home_prefix(configured), "CODEX_HOME").map(Some);
        }
    }
    Ok(None)
}

fn codex_arguments(action: CodexProjectLaunchAction) -> &'static [&'static str] {
    match action {
        CodexProjectLaunchAction::Start => &["-C", "."],
        CodexProjectLaunchAction::Resume => &["resume", "-C", "."],
    }
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_CODEX_EXTENSIONS: [&str; 4] = ["com", "exe", "bat", "cmd"];

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsCodexCommandSpec {
    program: PathBuf,
    args: Vec<String>,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsTerminalCommandSpec {
    program: PathBuf,
    args: Vec<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCodexCommand {
    executable: PathBuf,
    safe_path: OsString,
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnixCodexCommandSpec {
    program: PathBuf,
    args: Vec<OsString>,
}

#[cfg(any(target_os = "windows", test))]
fn is_supported_windows_codex_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|extension| {
            WINDOWS_CODEX_EXTENSIONS
                .iter()
                .any(|supported| extension == *supported)
        })
}

#[cfg(any(target_os = "windows", test))]
fn windows_path_key_is_same_or_descendant(path_key: &str, root_key: &str) -> bool {
    if path_key == root_key {
        return true;
    }
    path_key
        .strip_prefix(root_key)
        .is_some_and(|suffix| root_key.ends_with('\\') || suffix.starts_with('\\'))
}

fn path_is_same_or_descendant(path: &Path, root: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let path_key = project_path_key(path.to_string_lossy().as_ref());
        let root_key = project_path_key(root.to_string_lossy().as_ref());
        return windows_path_key_is_same_or_descendant(&path_key, &root_key);
    }

    #[cfg(not(target_os = "windows"))]
    path.starts_with(root)
}

fn safe_path_from_directories(
    directories: &[PathBuf],
    failure_label: &str,
) -> Result<OsString, String> {
    std::env::join_paths(directories)
        .map_err(|err| format!("无法构建安全的{failure_label} PATH：{err}"))
}

#[cfg(any(unix, test))]
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return metadata.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    true
}

#[cfg(any(unix, test))]
fn canonical_executable_outside_project(candidate: &Path, project_dir: &Path) -> Option<PathBuf> {
    if !candidate.is_absolute() || !is_executable_file(candidate) {
        return None;
    }
    let canonical_candidate = candidate.canonicalize().ok()?;
    let canonical_project = project_dir.canonicalize().ok()?;
    if path_is_same_or_descendant(&canonical_candidate, &canonical_project) {
        return None;
    }
    Some(canonical_candidate)
}

#[cfg(any(unix, test))]
fn safe_unix_search_directories<I>(directories: I, project_dir: &Path) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let Some(canonical_project) = project_dir.canonicalize().ok() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for directory in directories {
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            continue;
        }
        let Ok(canonical_directory) = directory.canonicalize() else {
            continue;
        };
        if !canonical_directory.is_dir()
            || path_is_same_or_descendant(&canonical_directory, &canonical_project)
            || result.contains(&canonical_directory)
        {
            continue;
        }
        result.push(canonical_directory);
    }
    result
}

#[cfg(any(unix, test))]
fn find_unix_executable_in_directories(
    directories: &[PathBuf],
    executable_name: &str,
    project_dir: &Path,
) -> Option<PathBuf> {
    let name = Path::new(executable_name);
    if name.is_absolute() || name.components().count() != 1 {
        return None;
    }
    directories.iter().find_map(|directory| {
        canonical_executable_outside_project(&directory.join(name), project_dir)
    })
}

#[cfg(any(unix, test))]
fn resolve_unix_codex_from_safe_directories(
    safe_directories: &[PathBuf],
    project_dir: &Path,
) -> Result<Option<ResolvedCodexCommand>, String> {
    let Some(executable) =
        find_unix_executable_in_directories(safe_directories, "codex", project_dir)
    else {
        return Ok(None);
    };
    let safe_path = safe_path_from_directories(safe_directories, " Codex CLI")?;
    Ok(Some(ResolvedCodexCommand {
        executable,
        safe_path,
    }))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn build_unix_codex_command_spec(
    env_executable: PathBuf,
    resolved: &ResolvedCodexCommand,
    action: CodexProjectLaunchAction,
    codex_home: Option<&Path>,
) -> Result<UnixCodexCommandSpec, String> {
    if !env_executable.is_absolute() || !is_executable_file(&env_executable) {
        return Err(format!(
            "env 必须是已解析的绝对可执行路径：{}",
            env_executable.display()
        ));
    }

    let mut path_assignment = OsString::from("PATH=");
    path_assignment.push(&resolved.safe_path);
    let mut args = vec![path_assignment];
    if let Some(codex_home) = codex_home {
        let mut codex_home_assignment = OsString::from("CODEX_HOME=");
        codex_home_assignment.push(codex_home.as_os_str());
        args.push(codex_home_assignment);
    }
    args.push(resolved.executable.as_os_str().to_os_string());
    args.extend(codex_arguments(action).iter().map(OsString::from));

    Ok(UnixCodexCommandSpec {
        program: env_executable,
        args,
    })
}

#[cfg(any(unix, test))]
const LOGIN_SHELL_PATH_MARKER: &[u8] = b"\0CODEXMANAGER_PATH\0";

#[cfg(unix)]
fn parse_login_shell_path(output: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt;

    let marker_index = output
        .windows(LOGIN_SHELL_PATH_MARKER.len())
        .rposition(|window| window == LOGIN_SHELL_PATH_MARKER)?;
    let value_start = marker_index + LOGIN_SHELL_PATH_MARKER.len();
    let value_end = output[value_start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|offset| value_start + offset)?;
    Some(OsString::from_vec(output[value_start..value_end].to_vec()))
}

#[cfg(unix)]
fn login_shell_search_directories(
    shell: &Path,
    initial_safe_path: &OsString,
    project_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let output = Command::new(shell)
        .current_dir("/")
        .args([
            "-lc",
            "printf '\\000CODEXMANAGER_PATH\\000%s\\000' \"$PATH\"",
        ])
        .env("PATH", initial_safe_path)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("无法读取登录 shell 的 PATH：{err}"))?;
    if !output.status.success() {
        return Err(format!(
            "登录 shell 无法提供 Codex CLI PATH，退出状态：{}",
            output.status
        ));
    }
    let shell_path = parse_login_shell_path(&output.stdout)
        .ok_or_else(|| "登录 shell 未返回可解析的 PATH".to_string())?;
    Ok(safe_unix_search_directories(
        std::env::split_paths(&shell_path).collect::<Vec<_>>(),
        project_dir,
    ))
}

#[cfg(any(target_os = "windows", test))]
fn safe_windows_search_directories<I>(directories: I, project_dir: &Path) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let Some(canonical_project) = project_dir.canonicalize().ok() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for directory in directories {
        // Empty and relative PATH entries resolve against the process cwd on Windows.
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            continue;
        }
        let Ok(canonical_directory) = directory.canonicalize() else {
            continue;
        };
        if !canonical_directory.is_dir()
            || path_is_same_or_descendant(&canonical_directory, &canonical_project)
            || result.contains(&canonical_directory)
        {
            continue;
        }
        result.push(canonical_directory);
    }
    result
}

#[cfg(any(target_os = "windows", test))]
fn find_windows_codex_in_directories<I>(
    directories: I,
    forbidden_root: Option<&Path>,
) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let forbidden_root = forbidden_root.and_then(|root| root.canonicalize().ok());
    for directory in directories {
        // Callers normally pass canonical safe directories. Keep this check so this
        // low-level resolver also fails closed when used independently.
        if directory.as_os_str().is_empty() || !directory.is_absolute() {
            continue;
        }
        for extension in WINDOWS_CODEX_EXTENSIONS {
            let candidate = directory.join(format!("codex.{extension}"));
            if !candidate.is_file() {
                continue;
            }
            let canonical_candidate = match candidate.canonicalize() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if forbidden_root
                .as_deref()
                .is_some_and(|root| path_is_same_or_descendant(&canonical_candidate, root))
            {
                continue;
            }
            return Some(canonical_candidate);
        }
    }
    None
}

#[cfg(any(target_os = "windows", test))]
fn resolve_windows_codex_from_directories<I>(
    directories: I,
    project_dir: &Path,
) -> Result<ResolvedCodexCommand, String>
where
    I: IntoIterator<Item = PathBuf>,
{
    let safe_directories = safe_windows_search_directories(directories, project_dir);
    let executable =
        find_windows_codex_in_directories(safe_directories.iter().cloned(), Some(project_dir))
            .ok_or_else(|| {
                "未找到项目目录之外的 Codex CLI，请先安装 Codex CLI 并加入绝对 PATH".to_string()
            })?;
    let safe_path = safe_path_from_directories(&safe_directories, " Windows")?;
    Ok(ResolvedCodexCommand {
        executable,
        safe_path,
    })
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_codex_command_spec(
    executable: &Path,
    action: CodexProjectLaunchAction,
) -> Result<WindowsCodexCommandSpec, String> {
    if !executable.is_absolute() || !is_supported_windows_codex_executable(executable) {
        return Err(format!(
            "Codex CLI 必须是已解析的绝对 Windows 可执行路径：{}",
            executable.display()
        ));
    }
    Ok(WindowsCodexCommandSpec {
        program: executable.to_path_buf(),
        args: codex_arguments(action)
            .iter()
            .map(|value| value.to_string())
            .collect(),
    })
}

#[cfg(any(target_os = "windows", test))]
fn windows_path_without_verbatim_prefix(value: &str) -> String {
    let normalized = value.replace('/', "\\");
    if let Some(path) = normalized.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{path}");
    }
    normalized
        .strip_prefix(r"\\?\")
        .unwrap_or(&normalized)
        .to_string()
}

#[cfg(target_os = "windows")]
fn portable_path_string(path: &Path, label: &str) -> Result<String, String> {
    utf8_path(path, label).map(|value| windows_path_without_verbatim_prefix(&value))
}

#[cfg(not(target_os = "windows"))]
fn portable_path_string(path: &Path, label: &str) -> Result<String, String> {
    utf8_path(path, label)
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn codex_app_deep_link(project_dir: &Path) -> Result<String, String> {
    let project = portable_path_string(project_dir, "项目目录")?;
    Ok(format!(
        "codex://new?path={}",
        percent_encode_query_value(&project)
    ))
}

#[cfg(target_os = "windows")]
fn launch_codex_app(project_dir: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: isize,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_command: i32,
        ) -> isize;
    }

    const SW_SHOWNORMAL: i32 = 1;
    let deep_link = codex_app_deep_link(project_dir)?;
    let operation = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = std::ffi::OsStr::new(&deep_link)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            0,
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result <= 32 {
        return Err(format!(
            "Windows 无法通过 codex:// 协议打开 ChatGPT Codex App（ShellExecuteW={result}）"
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_codex_app(project_dir: &Path) -> Result<(), String> {
    let deep_link = codex_app_deep_link(project_dir)?;
    webbrowser::open(&deep_link)
        .map(|_| ())
        .map_err(|err| format!("打开 ChatGPT Codex App 失败：{err}"))
}

#[cfg(any(target_os = "windows", test))]
fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_terminal_command_spec(
    powershell: &Path,
    codex: &WindowsCodexCommandSpec,
) -> Result<WindowsTerminalCommandSpec, String> {
    if !powershell.is_absolute()
        || !powershell.is_file()
        || !powershell
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("powershell.exe"))
    {
        return Err(format!(
            "PowerShell 必须是已解析的系统可执行路径：{}",
            powershell.display()
        ));
    }

    let codex_path = utf8_path(&codex.program, "Codex CLI 路径")?;
    let mut command_parts = vec![powershell_single_quoted(&codex_path)];
    command_parts.extend(
        codex
            .args
            .iter()
            .map(|value| powershell_single_quoted(value)),
    );
    let command_text = format!("& {}", command_parts.join(" "));

    Ok(WindowsTerminalCommandSpec {
        program: powershell.to_path_buf(),
        args: ["-NoLogo", "-NoProfile", "-NoExit", "-Command"]
            .into_iter()
            .map(OsString::from)
            .chain(std::iter::once(OsString::from(command_text)))
            .collect(),
    })
}

#[cfg(target_os = "windows")]
fn resolve_windows_powershell(project_dir: &Path) -> Result<PathBuf, String> {
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(|| "无法读取 Windows SystemRoot，不能安全启动 PowerShell".to_string())?;
    let candidate = system_root
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let powershell = candidate
        .canonicalize()
        .map_err(|err| format!("无法解析系统 PowerShell（{}）：{err}", candidate.display()))?;
    if !powershell.is_file() || path_is_same_or_descendant(&powershell, project_dir) {
        return Err(format!(
            "系统 PowerShell 不可用或位于项目目录中：{}",
            powershell.display()
        ));
    }
    Ok(powershell)
}

fn spawn_and_reap(mut command: Command, failure_message: &str) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|err| format!("{failure_message}：{err}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn resolve_codex_executable(
    action: CodexProjectLaunchAction,
    project_dir: &Path,
) -> Result<ResolvedCodexCommand, String> {
    let path = std::env::var_os("PATH")
        .ok_or_else(|| "未找到可用的 Codex CLI，请先安装 Codex CLI 并加入 PATH".to_string())?;
    let resolved = resolve_windows_codex_from_directories(
        std::env::split_paths(&path).collect::<Vec<_>>(),
        project_dir,
    )?;
    let spec = build_windows_codex_command_spec(&resolved.executable, action)?;
    let safe_cwd = spec
        .program
        .parent()
        .ok_or_else(|| format!("无法解析 Codex CLI 所在目录：{}", spec.program.display()))?;

    // Command handles absolute .cmd/.bat programs through Rust's hardened Windows
    // batch-script escaping. The probe cwd is the CLI directory, never the project.
    let mut command = Command::new(&spec.program);
    command
        .current_dir(safe_cwd)
        .args(match action {
            CodexProjectLaunchAction::Start => vec!["--version"],
            CodexProjectLaunchAction::Resume => vec!["resume", "--help"],
        })
        .env("PATH", &resolved.safe_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command
        .status()
        .map_err(|err| format!("无法检查 Codex CLI：{err}"))?;
    if status.success() {
        Ok(resolved)
    } else if action == CodexProjectLaunchAction::Resume {
        Err("当前 Codex CLI 不支持继续会话，请先升级 Codex CLI".to_string())
    } else {
        Err("Codex CLI 无法启动，请检查安装和 PATH".to_string())
    }
}

#[cfg(not(target_os = "windows"))]
fn resolve_codex_executable(
    action: CodexProjectLaunchAction,
    project_dir: &Path,
) -> Result<ResolvedCodexCommand, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let gui_safe_directories = safe_unix_search_directories(
        std::env::split_paths(&path).collect::<Vec<_>>(),
        project_dir,
    );
    let initial_safe_path = safe_path_from_directories(&gui_safe_directories, " GUI")?;
    let resolved = if let Some(resolved) =
        resolve_unix_codex_from_safe_directories(&gui_safe_directories, project_dir)?
    {
        resolved
    } else {
        let configured_shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .and_then(|shell| canonical_executable_outside_project(&shell, project_dir));
        let shell = configured_shell
            .or_else(|| canonical_executable_outside_project(Path::new("/bin/sh"), project_dir))
            .ok_or_else(|| "无法找到项目目录之外的安全登录 shell".to_string())?;
        let login_safe_directories =
            login_shell_search_directories(&shell, &initial_safe_path, project_dir)?;
        resolve_unix_codex_from_safe_directories(&login_safe_directories, project_dir)?.ok_or_else(
            || "未找到可用的 Codex CLI，请先安装 Codex CLI 并加入绝对 PATH".to_string(),
        )?
    };

    let mut command = Command::new(&resolved.executable);
    command
        .current_dir("/")
        .args(match action {
            CodexProjectLaunchAction::Start => vec!["--version"],
            CodexProjectLaunchAction::Resume => vec!["resume", "--help"],
        })
        .env("PATH", &resolved.safe_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command
        .status()
        .map_err(|err| format!("无法检查 Codex CLI：{err}"))?;
    if !status.success() {
        return if action == CodexProjectLaunchAction::Resume {
            Err("当前 Codex CLI 不支持继续会话，请先升级 Codex CLI".to_string())
        } else {
            Err("Codex CLI 无法启动，请检查安装和 PATH".to_string())
        };
    }
    Ok(resolved)
}

#[cfg(target_os = "windows")]
fn launch_codex_terminal(
    project_dir: &Path,
    action: CodexProjectLaunchAction,
    codex_home: Option<&Path>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let resolved = resolve_codex_executable(action, project_dir)?;
    let codex_spec = build_windows_codex_command_spec(&resolved.executable, action)?;
    let powershell = resolve_windows_powershell(project_dir)?;
    let terminal_spec = build_windows_terminal_command_spec(&powershell, &codex_spec)?;
    let mut command = Command::new(&terminal_spec.program);
    command
        .current_dir(project_dir)
        .args(terminal_spec.args)
        .env("PATH", &resolved.safe_path)
        .creation_flags(CREATE_NEW_CONSOLE);
    if let Some(codex_home) = codex_home {
        command.env("CODEX_HOME", codex_home);
    }
    spawn_and_reap(command, "打开 Codex 终端失败")
}

#[cfg(any(target_os = "macos", test))]
fn macos_terminal_script(action: CodexProjectLaunchAction) -> String {
    r#"
on run argv
  set projectPath to item 1 of argv
  set codexPath to item 2 of argv
  set safePath to item 3 of argv
  set commandText to "cd " & quoted form of projectPath & " && "
  set commandText to commandText & "export PATH=" & quoted form of safePath & " && "
  if (count of argv) is greater than 3 then
    set commandText to commandText & "export CODEX_HOME=" & quoted form of (item 4 of argv) & " && "
  end if
  set commandText to commandText & "exec " & quoted form of codexPath & " __CODEX_ARGS__"
  tell application "Terminal"
    activate
    do script commandText
  end tell
end run
"#
    .replace("__CODEX_ARGS__", &codex_arguments(action).join(" "))
}

#[cfg(target_os = "macos")]
fn launch_codex_terminal(
    project_dir: &Path,
    action: CodexProjectLaunchAction,
    codex_home: Option<&Path>,
) -> Result<(), String> {
    let resolved = resolve_codex_executable(action, project_dir)?;
    let script = macos_terminal_script(action);

    let project = utf8_path(project_dir, "项目目录")?;
    let executable = utf8_path(&resolved.executable, "Codex CLI 路径")?;
    let safe_path = resolved
        .safe_path
        .to_str()
        .ok_or_else(|| "安全的 Codex CLI PATH 包含 macOS Terminal 无法传递的字符".to_string())?;
    let osascript =
        canonical_executable_outside_project(Path::new("/usr/bin/osascript"), project_dir)
            .ok_or_else(|| "未找到安全的 /usr/bin/osascript".to_string())?;
    let mut command = Command::new(osascript);
    command
        .args(["-e", &script, "--", &project, &executable, safe_path])
        .env("PATH", &resolved.safe_path);
    if let Some(codex_home) = codex_home {
        command.arg(utf8_path(codex_home, "Codex profile")?);
    }
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("打开 macOS Terminal 失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("打开 macOS Terminal 失败，退出状态：{status}"))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch_codex_terminal(
    project_dir: &Path,
    action: CodexProjectLaunchAction,
    codex_home: Option<&Path>,
) -> Result<(), String> {
    use std::ffi::OsString;

    let resolved = resolve_codex_executable(action, project_dir)?;
    // GNOME Terminal and some other emulators reuse a long-running server. Environment
    // variables set on the terminal client process are not guaranteed to reach the new
    // tab, so an NVM-installed Codex script can be found while its `env node` shebang
    // still fails. Make the environment part of the child argv instead.
    let env_executable = [Path::new("/usr/bin/env"), Path::new("/bin/env")]
        .into_iter()
        .find_map(|candidate| canonical_executable_outside_project(candidate, project_dir))
        .ok_or_else(|| "未找到项目目录之外的安全 env 可执行文件".to_string())?;
    let spec = build_unix_codex_command_spec(env_executable, &resolved, action, codex_home)?;
    let project = project_dir.as_os_str().to_os_string();
    let program_arg = spec.program.as_os_str().to_os_string();
    let launch_args = spec.args;
    let safe_directories = safe_unix_search_directories(
        std::env::split_paths(&resolved.safe_path).collect::<Vec<_>>(),
        project_dir,
    );

    let candidates = [
        (
            "xdg-terminal-exec",
            [vec![program_arg.clone()], launch_args.clone()].concat(),
        ),
        (
            "x-terminal-emulator",
            [
                vec![OsString::from("-e"), program_arg.clone()],
                launch_args.clone(),
            ]
            .concat(),
        ),
        (
            "gnome-terminal",
            [
                vec![
                    OsString::from("--working-directory"),
                    project.clone(),
                    OsString::from("--"),
                    program_arg.clone(),
                ],
                launch_args.clone(),
            ]
            .concat(),
        ),
        (
            "konsole",
            [
                vec![
                    OsString::from("--workdir"),
                    project.clone(),
                    OsString::from("-e"),
                    program_arg.clone(),
                ],
                launch_args.clone(),
            ]
            .concat(),
        ),
        (
            "kitty",
            [
                vec![
                    OsString::from("--directory"),
                    project.clone(),
                    program_arg.clone(),
                ],
                launch_args.clone(),
            ]
            .concat(),
        ),
        (
            "wezterm",
            [
                vec![
                    OsString::from("start"),
                    OsString::from("--cwd"),
                    project.clone(),
                    OsString::from("--"),
                    program_arg.clone(),
                ],
                launch_args.clone(),
            ]
            .concat(),
        ),
        (
            "xfce4-terminal",
            [
                vec![
                    OsString::from("--disable-server"),
                    OsString::from("--working-directory"),
                    project,
                    OsString::from("-x"),
                    program_arg,
                ],
                launch_args,
            ]
            .concat(),
        ),
    ];

    let mut errors = Vec::new();
    for (terminal, args) in candidates {
        let Some(terminal_executable) =
            find_unix_executable_in_directories(&safe_directories, terminal, project_dir)
        else {
            errors.push(format!("{terminal}: 未在安全的绝对 PATH 中找到"));
            continue;
        };
        let mut command = Command::new(&terminal_executable);
        command
            .current_dir(project_dir)
            .args(args)
            .env("PATH", &resolved.safe_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(codex_home) = codex_home {
            command.env("CODEX_HOME", codex_home);
        }
        match command.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(err) => errors.push(format!("{}: {err}", terminal_executable.display())),
        }
    }

    Err(format!(
        "未找到可用的终端模拟器，请安装 xdg-terminal-exec、GNOME Terminal、Konsole、Kitty、WezTerm 或 XFCE Terminal。{}",
        if errors.is_empty() {
            String::new()
        } else {
            format!(" 尝试结果：{}", errors.join("；"))
        }
    ))
}

fn launch_project(
    project_dir: &Path,
    action: CodexProjectLaunchAction,
    codex_home: Option<&Path>,
) -> Result<CodexProjectLaunchTarget, String> {
    if action == CodexProjectLaunchAction::Start {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            launch_codex_app(project_dir)?;
            return Ok(CodexProjectLaunchTarget::App);
        }
    }

    launch_codex_terminal(project_dir, action, codex_home)?;
    Ok(CodexProjectLaunchTarget::Cli)
}

/// 列出保存在当前桌面客户端本地数据库中的项目目录。
#[tauri::command]
pub async fn app_codex_projects_list(
    app: tauri::AppHandle,
) -> Result<CodexProjectListResult, String> {
    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_project_store();
        let storage = open_storage(&db_path)?;
        list_projects_from_storage(&storage)
    })
    .await
    .map_err(|err| format!("app_codex_projects_list task failed: {err}"))?
}

/// 打开系统目录选择器，并将所选目录保存到当前桌面客户端。
#[tauri::command]
pub async fn app_codex_project_add(app: tauri::AppHandle) -> Result<CodexProjectAddResult, String> {
    let selected = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Select Codex project directory")
            .pick_folder()
    })
    .await
    .map_err(|err| format!("app_codex_project_add dialog task failed: {err}"))?;
    let Some(selected) = selected else {
        return Ok(CodexProjectAddResult {
            canceled: true,
            added: false,
            project: None,
        });
    };

    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_project_store();
        let storage = open_storage(&db_path)?;
        add_project_to_storage(&storage, &selected, now_ts())
    })
    .await
    .map_err(|err| format!("app_codex_project_add task failed: {err}"))?
}

/// 只移除项目收藏记录，不会删除目录或目录内的任何文件。
#[tauri::command]
pub async fn app_codex_project_remove(
    app: tauri::AppHandle,
    path: String,
) -> Result<CodexProjectRemoveResult, String> {
    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = lock_project_store();
        let storage = open_storage(&db_path)?;
        remove_project_from_storage(&storage, &path)
    })
    .await
    .map_err(|err| format!("app_codex_project_remove task failed: {err}"))?
}

/// Windows/macOS 上优先在 ChatGPT Codex App 中打开项目；会话选择器仍由本机 CLI 提供。
#[tauri::command]
pub async fn app_codex_project_launch(
    app: tauri::AppHandle,
    path: String,
    action: CodexProjectLaunchAction,
) -> Result<CodexProjectLaunchResult, String> {
    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (project_dir, codex_home) = {
            let _guard = lock_project_store();
            let storage = open_storage(&db_path)?;
            (
                registered_project_path(&storage, &path)?,
                resolve_codex_home(&storage)?,
            )
        };
        let target = launch_project(&project_dir, action, codex_home.as_deref())?;
        Ok(CodexProjectLaunchResult {
            path: portable_path_string(&project_dir, "项目目录")?,
            action,
            target,
            codex_home: codex_home
                .as_deref()
                .map(|value| portable_path_string(value, "Codex profile"))
                .transpose()?,
        })
    })
    .await
    .map_err(|err| format!("app_codex_project_launch task failed: {err}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "codexmanager-projects-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp directory");
        dir
    }

    fn write_test_executable(path: &Path, contents: &str) {
        std::fs::write(path, contents).expect("write test executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)
                .expect("read test executable metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).expect("make test executable executable");
        }
    }

    fn test_storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("initialize storage");
        storage
    }

    #[test]
    fn project_store_adds_canonical_directory_and_deduplicates() {
        let storage = test_storage();
        let dir = temp_dir("deduplicate");

        let added = add_project_to_storage(&storage, &dir, 10).expect("add project");
        assert!(added.added);
        assert!(!added.canceled);
        assert_eq!(
            added.project.as_ref().map(|item| item.available),
            Some(true)
        );

        let duplicate =
            add_project_to_storage(&storage, &dir.join("."), 20).expect("deduplicate project");
        assert!(!duplicate.added);
        let listed = list_projects_from_storage(&storage).expect("list projects");
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].added_at, 10);

        std::fs::remove_dir_all(dir).expect("remove temp directory");
    }

    #[test]
    fn remove_project_does_not_require_directory_to_still_exist() {
        let storage = test_storage();
        let dir = temp_dir("missing-remove");
        let added = add_project_to_storage(&storage, &dir, 10).expect("add project");
        let path = added.project.expect("project").path;
        std::fs::remove_dir_all(&dir).expect("remove project directory");

        let listed = list_projects_from_storage(&storage).expect("list projects");
        assert!(!listed.items[0].available);
        assert!(
            remove_project_from_storage(&storage, &path)
                .expect("remove stored project")
                .removed
        );
        assert!(list_projects_from_storage(&storage)
            .expect("list empty projects")
            .items
            .is_empty());
    }

    #[test]
    fn malformed_project_store_fails_closed_without_overwrite() {
        let storage = test_storage();
        storage
            .set_app_setting(PROJECTS_SETTING_KEY, "{broken", 1)
            .expect("seed malformed setting");
        let dir = temp_dir("malformed");

        let error = add_project_to_storage(&storage, &dir, 10).unwrap_err();
        assert!(error.contains("数据损坏"));
        assert_eq!(
            storage
                .get_app_setting(PROJECTS_SETTING_KEY)
                .expect("read setting")
                .as_deref(),
            Some("{broken")
        );

        std::fs::remove_dir_all(dir).expect("remove temp directory");
    }

    #[test]
    fn launch_requires_a_registered_available_directory() {
        let storage = test_storage();
        let dir = temp_dir("registered");
        let other = temp_dir("unregistered");
        let added = add_project_to_storage(&storage, &dir, 10).expect("add project");
        let saved_path = added.project.expect("project").path;

        assert_eq!(
            registered_project_path(&storage, &saved_path).expect("registered path"),
            dir.canonicalize().expect("canonical project")
        );
        assert!(
            registered_project_path(&storage, other.to_string_lossy().as_ref())
                .unwrap_err()
                .contains("只能启动已添加")
        );

        std::fs::remove_dir_all(dir).expect("remove project directory");
        std::fs::remove_dir_all(other).expect("remove other directory");
    }

    #[test]
    fn launch_action_rejects_arbitrary_commands() {
        assert_eq!(
            serde_json::from_str::<CodexProjectLaunchAction>("\"resume\"").expect("resume action"),
            CodexProjectLaunchAction::Resume
        );
        assert!(serde_json::from_str::<CodexProjectLaunchAction>("\"rm -rf\"").is_err());
        assert_eq!(
            codex_arguments(CodexProjectLaunchAction::Start),
            &["-C", "."]
        );
        assert_eq!(
            codex_arguments(CodexProjectLaunchAction::Resume),
            &["resume", "-C", "."]
        );
    }

    #[test]
    fn windows_resolution_skips_project_and_never_builds_a_bare_codex_command() {
        let root = temp_dir("windows-command-resolution");
        let project = root.join("project");
        let safe_bin = root.join("safe-bin");
        let node_bin = root.join("node-bin");
        std::fs::create_dir_all(&project).expect("create project directory");
        std::fs::create_dir_all(&safe_bin).expect("create safe bin directory");
        std::fs::create_dir_all(&node_bin).expect("create node bin directory");
        let project_cli = project.join("codex.cmd");
        let safe_cli = safe_bin.join("codex.cmd");
        std::fs::write(&project_cli, "@echo malicious\r\n").expect("write project shim");
        std::fs::write(&safe_cli, "@node safe-codex.js %*\r\n").expect("write safe shim");

        let resolved = resolve_windows_codex_from_directories(
            [
                PathBuf::new(),
                PathBuf::from("."),
                project.clone(),
                safe_bin.clone(),
                node_bin.clone(),
            ],
            &project,
        )
        .expect("resolve safe Codex CLI and PATH");
        assert_eq!(
            resolved.executable,
            safe_cli.canonicalize().expect("canonical safe shim")
        );
        assert_eq!(
            std::env::split_paths(&resolved.safe_path).collect::<Vec<_>>(),
            [
                safe_bin.canonicalize().expect("canonical safe bin"),
                node_bin.canonicalize().expect("canonical node bin"),
            ]
        );

        let spec = build_windows_codex_command_spec(
            &resolved.executable,
            CodexProjectLaunchAction::Resume,
        )
        .expect("build absolute Windows command");
        assert!(spec.program.is_absolute());
        assert_eq!(spec.program, resolved.executable);
        assert_eq!(spec.args, ["resume", "-C", "."]);
        assert_ne!(spec.program, PathBuf::from("codex"));
        assert!(build_windows_codex_command_spec(
            Path::new("codex.cmd"),
            CodexProjectLaunchAction::Start,
        )
        .is_err());

        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn windows_root_path_key_contains_all_descendants() {
        assert!(windows_path_key_is_same_or_descendant("c:\\", "c:\\"));
        assert!(windows_path_key_is_same_or_descendant(
            "c:\\program files\\nodejs\\codex.cmd",
            "c:\\",
        ));
        assert!(windows_path_key_is_same_or_descendant(
            "c:\\work\\project\\codex.cmd",
            "c:\\work\\project",
        ));
        assert!(!windows_path_key_is_same_or_descendant(
            "c:\\work\\project-other\\codex.cmd",
            "c:\\work\\project",
        ));
        assert!(!windows_path_key_is_same_or_descendant(
            "d:\\codex.cmd",
            "c:\\",
        ));
    }

    #[test]
    fn windows_paths_drop_verbatim_prefix_for_storage_and_ui() {
        assert_eq!(
            windows_path_without_verbatim_prefix(r"\\?\C:\Users\qxnm\Desktop\test"),
            r"C:\Users\qxnm\Desktop\test"
        );
        assert_eq!(
            windows_path_without_verbatim_prefix(r"\\?\UNC\server\share\project"),
            r"\\server\share\project"
        );
    }

    #[test]
    fn codex_app_link_encodes_the_absolute_workspace_path() {
        let link = codex_app_deep_link(Path::new(r"C:\Work Space\示例"))
            .expect("build Codex App deep link");
        assert_eq!(
            link,
            "codex://new?path=C%3A%5CWork%20Space%5C%E7%A4%BA%E4%BE%8B"
        );
    }

    #[test]
    fn windows_terminal_uses_powershell_as_the_interactive_host() {
        let root = temp_dir("windows-powershell-host");
        let powershell = root.join("powershell.exe");
        std::fs::write(&powershell, b"test").expect("write PowerShell fixture");
        let codex = WindowsCodexCommandSpec {
            program: root.join("Codex's Folder").join("codex.cmd"),
            args: vec!["resume".to_string(), "-C".to_string(), ".".to_string()],
        };

        let spec = build_windows_terminal_command_spec(&powershell, &codex)
            .expect("build PowerShell terminal command");
        assert_eq!(spec.program, powershell);
        assert_eq!(
            spec.args[..4],
            ["-NoLogo", "-NoProfile", "-NoExit", "-Command"].map(OsString::from)
        );
        let command_text = spec.args[4].to_string_lossy();
        assert!(command_text.starts_with("& '"));
        assert!(command_text.contains("Codex''s Folder"));
        assert!(command_text.ends_with("'resume' '-C' '.'"));

        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn unix_resolution_skips_relative_and_project_terminal_executables() {
        let root = temp_dir("unix-terminal-resolution");
        let project = root.join("project");
        let safe_bin = root.join("safe-bin");
        std::fs::create_dir_all(&project).expect("create project directory");
        std::fs::create_dir_all(&safe_bin).expect("create safe bin directory");
        let project_terminal = project.join("gnome-terminal");
        let safe_terminal = safe_bin.join("gnome-terminal");
        let project_codex = project.join("codex");
        let safe_codex = safe_bin.join("codex");
        write_test_executable(&project_terminal, "#!/bin/sh\nexit 99\n");
        write_test_executable(&safe_terminal, "#!/bin/sh\nexit 0\n");
        write_test_executable(&project_codex, "#!/bin/sh\nexit 99\n");
        write_test_executable(&safe_codex, "#!/bin/sh\nexit 0\n");

        let safe_directories =
            safe_unix_search_directories([PathBuf::from("."), project.clone(), safe_bin], &project);
        let resolved =
            find_unix_executable_in_directories(&safe_directories, "gnome-terminal", &project)
                .expect("resolve safe terminal");
        assert_eq!(
            resolved,
            safe_terminal
                .canonicalize()
                .expect("canonical safe terminal")
        );
        assert!(!path_is_same_or_descendant(
            &resolved,
            &project.canonicalize().expect("canonical project")
        ));
        assert_eq!(
            find_unix_executable_in_directories(&safe_directories, "codex", &project)
                .expect("resolve safe Codex CLI"),
            safe_codex.canonicalize().expect("canonical safe Codex CLI")
        );
        assert!(find_unix_executable_in_directories(
            &safe_directories,
            "../gnome-terminal",
            &project,
        )
        .is_none());

        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_codex_keeps_its_sanitized_nvm_path_for_probe_and_launch() {
        use std::os::unix::ffi::OsStrExt;

        let root = temp_dir("login-shell-path");
        let project = root.join("project");
        let nvm_bin = root.join("nvm-bin");
        std::fs::create_dir_all(&project).expect("create project directory");
        std::fs::create_dir_all(&nvm_bin).expect("create nvm bin directory");
        let codex = nvm_bin.join("codex");
        let node = nvm_bin.join("node");
        write_test_executable(&codex, "#!/usr/bin/env node\n");
        write_test_executable(&node, "#!/bin/sh\nexit 0\n");

        let shell_path = OsString::from(format!(
            "{}::relative:{}",
            project.display(),
            nvm_bin.display()
        ));
        let mut shell_output = b"profile noise\n".to_vec();
        shell_output.extend_from_slice(LOGIN_SHELL_PATH_MARKER);
        shell_output.extend_from_slice(shell_path.as_os_str().as_bytes());
        shell_output.push(0);
        let parsed = parse_login_shell_path(&shell_output).expect("parse marked shell PATH");
        let safe_directories = safe_unix_search_directories(
            std::env::split_paths(&parsed).collect::<Vec<_>>(),
            &project,
        );
        let resolved = resolve_unix_codex_from_safe_directories(&safe_directories, &project)
            .expect("build resolved Codex command")
            .expect("find login-shell Codex CLI");

        assert_eq!(
            resolved.executable,
            codex.canonicalize().expect("canonical Codex shim")
        );
        assert_eq!(
            std::env::split_paths(&resolved.safe_path).collect::<Vec<_>>(),
            [nvm_bin.canonicalize().expect("canonical NVM bin")]
        );
        assert!(nvm_bin.join("node").is_file());

        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn unix_child_argv_restores_nvm_path_profile_and_project_cwd() {
        let root = temp_dir("unix-child-environment");
        let project = root.join("project with spaces");
        let nvm_bin = root.join("nvm-bin");
        let package_bin = root.join("lib");
        let codex_home = root.join("profile with spaces-$()");
        std::fs::create_dir_all(&project).expect("create project directory");
        std::fs::create_dir_all(&nvm_bin).expect("create NVM bin directory");
        std::fs::create_dir_all(&package_bin).expect("create package bin directory");
        std::fs::create_dir_all(&codex_home).expect("create Codex profile directory");

        let codex_script = package_bin.join("codex.js");
        write_test_executable(&codex_script, "#!/usr/bin/env node\n");
        write_test_executable(
            &nvm_bin.join("node"),
            "#!/bin/sh\nprintf 'cwd='\npwd\nprintf 'path=%s\\nprofile=%s\\nargs=%s\\n' \"$PATH\" \"$CODEX_HOME\" \"$*\"\n",
        );
        std::os::unix::fs::symlink(&codex_script, nvm_bin.join("codex"))
            .expect("link NVM Codex CLI");

        let safe_directories = safe_unix_search_directories([nvm_bin.clone()], &project);
        let resolved = resolve_unix_codex_from_safe_directories(&safe_directories, &project)
            .expect("resolve NVM Codex CLI")
            .expect("find NVM Codex CLI");
        let env_executable =
            canonical_executable_outside_project(Path::new("/usr/bin/env"), &project)
                .expect("resolve system env executable");
        let spec = build_unix_codex_command_spec(
            env_executable,
            &resolved,
            CodexProjectLaunchAction::Resume,
            Some(&codex_home),
        )
        .expect("build Unix child command");

        // Clear the test process environment to model a terminal server that did not
        // inherit NVM or the selected Codex profile from CodexManager.
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(&project)
            .env_clear()
            .output()
            .expect("run wrapped NVM Codex CLI");
        assert!(
            output.status.success(),
            "wrapped Codex failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8(output.stdout).expect("UTF-8 probe output");
        assert!(stdout.contains(&format!(
            "cwd={}\n",
            project.canonicalize().expect("canonical project").display()
        )));
        assert!(stdout.contains(&format!(
            "path={}\n",
            nvm_bin.canonicalize().expect("canonical NVM bin").display()
        )));
        assert!(stdout.contains(&format!("profile={}\n", codex_home.display())));
        assert!(stdout.contains(" resume -C .\n"));

        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn macos_terminal_script_exports_safe_path_and_uses_argv_only() {
        let project = temp_dir("macos-special-'-$()");
        let script = macos_terminal_script(CodexProjectLaunchAction::Resume);
        assert!(script.contains("set safePath to item 3 of argv"));
        assert!(script.contains("export PATH="));
        assert!(script.contains("quoted form of safePath"));
        assert!(script.contains("exec \" & quoted form of codexPath & \" resume -C ."));
        assert!(!script.contains(project.to_string_lossy().as_ref()));
        std::fs::remove_dir_all(project).expect("remove temp directory");
    }
}
