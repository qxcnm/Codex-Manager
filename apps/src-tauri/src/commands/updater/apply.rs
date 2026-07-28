use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::app_shell::prepare_for_forced_app_exit;

use super::model::UpdateActionResponse;
use super::runtime::current_exe_path;
#[cfg(target_os = "windows")]
use super::runtime::CREATE_NO_WINDOW;
use super::state::{
    clear_pending_update, pending_update_path, read_pending_update, script_dir_from_pending,
};

/// 函数 `append_apply_log`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - log_path: 参数 log_path
/// - message: 参数 message
///
/// # 返回
/// 无
fn append_apply_log(log_path: &Path, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let line = format!("[{timestamp}] {message}\n");

    if let Some(parent) = log_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!(
                "event=update_log_directory_create_failed phase=apply path={} error={err}",
                parent.display()
            );
            return;
        }
    }
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(mut file) => {
            if let Err(err) = file.write_all(line.as_bytes()).and_then(|()| file.flush()) {
                log::warn!(
                    "event=update_log_write_failed phase=apply path={} error={err}",
                    log_path.display()
                );
            }
        }
        Err(err) => log::warn!(
            "event=update_log_open_failed phase=apply path={} error={err}",
            log_path.display()
        ),
    }
}

/// 函数 `log_path_for_script_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - script_dir: 参数 script_dir
/// - file_name: 参数 file_name
///
/// # 返回
/// 返回函数执行结果
fn log_path_for_script_dir(script_dir: &Path, file_name: &str) -> PathBuf {
    script_dir.join("logs").join(file_name)
}

/// 函数 `write_windows_powershell_script`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - script_path: 参数 script_path
/// - script: 参数 script
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "windows")]
fn write_windows_powershell_script(script_path: &Path, script: &str) -> Result<(), String> {
    // Windows PowerShell 5.1 对无 BOM 的 UTF-8 脚本兼容较差，写入 BOM 避免脚本内容被误解析。
    let mut bytes = Vec::with_capacity(script.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(script.as_bytes());
    fs::write(script_path, bytes).map_err(|err| format!("写入更新应用脚本失败：{err}"))
}

/// 函数 `portable_executable_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn portable_executable_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["CodexManager-portable.exe", "CodexManager.exe"]
    } else if cfg!(target_os = "macos") {
        &[
            "CodexManager-portable.app",
            "CodexManager.app",
            "CodexManager",
        ]
    } else {
        &["CodexManager-portable", "CodexManager"]
    }
}

/// 函数 `resolve_portable_restart_exe`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn resolve_portable_restart_exe(
    staging_dir: &Path,
    current_exe_name: &str,
) -> Result<String, String> {
    if staging_dir.join(current_exe_name).is_file() {
        return Ok(current_exe_name.to_string());
    }

    for candidate in portable_executable_candidates() {
        if staging_dir.join(candidate).is_file() {
            return Ok((*candidate).to_string());
        }
    }

    Err(format!(
        "便携包无效：暂存目录中未找到可执行文件，期望名称之一为 [{}]",
        portable_executable_candidates().join(", ")
    ))
}

/// 函数 `spawn_portable_apply_worker`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - script_dir: 参数 script_dir
/// - target_dir: 参数 target_dir
/// - staging_dir: 参数 staging_dir
/// - exe_name: 参数 exe_name
/// - pending_path: 参数 pending_path
/// - pid_to_wait: 参数 pid_to_wait
///
/// # 返回
/// 返回函数执行结果
fn spawn_portable_apply_worker(
    script_dir: &Path,
    target_dir: &Path,
    staging_dir: &Path,
    exe_name: &str,
    pending_path: &Path,
    pid_to_wait: u32,
) -> Result<(), String> {
    let log_path = log_path_for_script_dir(script_dir, "apply-portable-update.log");
    append_apply_log(
        &log_path,
        &format!(
            "准备启动便携更新脚本，target={}, staging={}, exe={}",
            target_dir.display(),
            staging_dir.display(),
            exe_name
        ),
    );
    #[cfg(target_os = "windows")]
    {
        let script_path = script_dir.join("apply-portable-update.ps1");
        let script = r#"
param(
  [Parameter(Mandatory=$true)][string]$TargetDir,
  [Parameter(Mandatory=$true)][string]$StagingDir,
  [Parameter(Mandatory=$true)][string]$ExeName,
  [Parameter(Mandatory=$true)][string]$PendingFile,
  [Parameter(Mandatory=$true)][string]$LogFile,
  [Parameter(Mandatory=$true)][int]$PidToWait
)
$ErrorActionPreference = "Stop"
function Write-Log([string]$Message) {
  Add-Content -LiteralPath $LogFile -Value ("[{0}] {1}" -f (Get-Date -Format o), $Message)
}
Write-Log "portable update worker started"
for ($i = 0; $i -lt 240; $i++) {
  if (-not (Get-Process -Id $PidToWait -ErrorAction SilentlyContinue)) { break }
  Start-Sleep -Milliseconds 500
}
Write-Log "copying staged files"
Get-ChildItem -LiteralPath $StagingDir -Force | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $TargetDir $_.Name) -Recurse -Force
}
Write-Log "staged files copied"
if (Test-Path -LiteralPath $PendingFile) {
  Remove-Item -LiteralPath $PendingFile -Force -ErrorAction SilentlyContinue
}
Write-Log "pending update marker cleared"
$RestartPath = Join-Path -Path $TargetDir -ChildPath $ExeName
Write-Log ("relaunching: {0}" -f $RestartPath)
Start-Process -FilePath $RestartPath -WorkingDirectory $TargetDir | Out-Null
Write-Log "app relaunched"
"#;
        write_windows_powershell_script(&script_path, script)?;

        let args = vec![
            "-TargetDir".to_string(),
            target_dir.display().to_string(),
            "-StagingDir".to_string(),
            staging_dir.display().to_string(),
            "-ExeName".to_string(),
            exe_name.to_string(),
            "-PendingFile".to_string(),
            pending_path.display().to_string(),
            "-LogFile".to_string(),
            log_path.display().to_string(),
            "-PidToWait".to_string(),
            pid_to_wait.to_string(),
        ];

        let try_spawn = |shell: &str| -> Result<(), String> {
            let mut cmd = Command::new(shell);
            cmd.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&script_path)
                .args(&args);
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.spawn()
                .map(|_| ())
                .map_err(|err| format!("启动 {shell} 失败：{err}"))
        };

        if try_spawn("pwsh.exe").is_ok() {
            return Ok(());
        }
        return try_spawn("powershell.exe");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let script_path = script_dir.join("apply-portable-update.sh");
        let script = r#"#!/usr/bin/env sh
TARGET_DIR="$1"
STAGING_DIR="$2"
EXE_NAME="$3"
PENDING_FILE="$4"
PID_TO_WAIT="$5"
LOG_FILE="$6"

log() {
  printf '[%s] %s\n' "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" "$1" >> "$LOG_FILE"
}

log "便携更新脚本已启动"
i=0
while kill -0 "$PID_TO_WAIT" 2>/dev/null && [ "$i" -lt 240 ]; do
  i=$((i + 1))
  sleep 0.5
done

log "开始复制暂存文件"
cp -Rf "$STAGING_DIR"/. "$TARGET_DIR"/ || {
  log "复制暂存文件失败"
  exit 1
}
rm -f "$PENDING_FILE"
log "pending-update.json 已清理"
chmod +x "$TARGET_DIR/$EXE_NAME" 2>/dev/null || true
"$TARGET_DIR/$EXE_NAME" >/dev/null 2>&1 &
log "应用已重新拉起"
"#;
        fs::write(&script_path, script).map_err(|err| format!("写入更新应用脚本失败：{err}"))?;

        #[cfg(unix)]
        {
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
                .map_err(|err| format!("设置更新应用脚本权限失败：{err}"))?;
        }

        Command::new("sh")
            .arg(&script_path)
            .arg(target_dir)
            .arg(staging_dir)
            .arg(exe_name)
            .arg(pending_path)
            .arg(pid_to_wait.to_string())
            .arg(&log_path)
            .spawn()
            .map_err(|err| format!("启动更新应用脚本失败：{err}"))?;
        Ok(())
    }
}

/// 函数 `schedule_app_exit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 无
fn schedule_app_exit(app: tauri::AppHandle) {
    let app_for_worker = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("update-app-exit".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(280));
            app_for_worker.exit(0);
        })
    {
        log::error!("event=update_exit_worker_spawn_failed error={err}");
        app.exit(0);
    }
}

/// 函数 `resolve_current_macos_app_bundle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - exe_path: 参数 exe_path
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn resolve_current_macos_app_bundle(exe_path: &Path) -> Result<PathBuf, String> {
    for ancestor in exe_path.ancestors() {
        if ancestor
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
            && ancestor.is_dir()
        {
            return Ok(ancestor.to_path_buf());
        }
    }

    Err(format!(
        "无法从当前可执行文件路径解析 macOS app 包：{}",
        exe_path.display()
    ))
}

/// 函数 `resolve_staged_macos_app_bundle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - staging_dir: 参数 staging_dir
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn resolve_staged_macos_app_bundle(staging_dir: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(staging_dir).map_err(|err| format!("读取暂存目录失败：{err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("读取暂存目录项失败：{err}"))?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
            && path.is_dir()
        {
            return Ok(path);
        }
    }

    Err(format!(
        "暂存目录中未找到 macOS app 包：{}",
        staging_dir.display()
    ))
}

/// 函数 `spawn_macos_bundle_replace_worker`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - script_dir: 参数 script_dir
/// - target_app: 参数 target_app
/// - staged_app: 参数 staged_app
/// - pending_path: 参数 pending_path
/// - pid_to_wait: 参数 pid_to_wait
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn spawn_macos_bundle_replace_worker(
    script_dir: &Path,
    target_app: &Path,
    staged_app: &Path,
    pending_path: &Path,
    pid_to_wait: u32,
) -> Result<(), String> {
    let log_path = log_path_for_script_dir(script_dir, "apply-macos-bundle-update.log");
    append_apply_log(
        &log_path,
        &format!(
            "准备启动 macOS 替换脚本，target_app={}, staged_app={}",
            target_app.display(),
            staged_app.display()
        ),
    );
    let script_path = script_dir.join("apply-macos-bundle-update.sh");
    let script = r#"#!/usr/bin/env sh
TARGET_APP="$1"
STAGED_APP="$2"
PENDING_FILE="$3"
PID_TO_WAIT="$4"
LOG_FILE="$5"

log() {
  printf '[%s] %s\n' "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" "$1" >> "$LOG_FILE"
}

log "macOS 替换脚本已启动"
i=0
while kill -0 "$PID_TO_WAIT" 2>/dev/null && [ "$i" -lt 240 ]; do
  i=$((i + 1))
  sleep 0.5
done

TARGET_PARENT="$(dirname "$TARGET_APP")"
TARGET_NAME="$(basename "$TARGET_APP")"
TEMP_APP="$TARGET_PARENT/.${TARGET_NAME}.update"
BACKUP_APP="$TARGET_PARENT/.${TARGET_NAME}.backup"

rm -rf "$TEMP_APP"
rm -rf "$BACKUP_APP"
log "开始复制新 app 到临时目录"
ditto "$STAGED_APP" "$TEMP_APP" || {
  log "复制新 app 到临时目录失败"
  exit 1
}
xattr -dr com.apple.quarantine "$TEMP_APP" 2>/dev/null || true
if [ -e "$TARGET_APP" ]; then
  log "备份当前 app"
  mv "$TARGET_APP" "$BACKUP_APP"
fi
if ! mv "$TEMP_APP" "$TARGET_APP"; then
  log "切换新 app 失败，尝试回滚"
  rm -rf "$TARGET_APP"
  if [ -e "$BACKUP_APP" ]; then
    mv "$BACKUP_APP" "$TARGET_APP"
  fi
  exit 1
fi
rm -rf "$BACKUP_APP"
rm -f "$PENDING_FILE"
log "替换完成，准备重新拉起应用"
open "$TARGET_APP" >/dev/null 2>&1 &
log "应用已重新拉起"
"#;
    fs::write(&script_path, script).map_err(|err| format!("写入 macOS 更新脚本失败：{err}"))?;

    #[cfg(unix)]
    {
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .map_err(|err| format!("设置 macOS 更新脚本权限失败：{err}"))?;
    }

    Command::new("sh")
        .arg(&script_path)
        .arg(target_app)
        .arg(staged_app)
        .arg(pending_path)
        .arg(pid_to_wait.to_string())
        .arg(&log_path)
        .spawn()
        .map_err(|err| format!("启动 macOS 更新脚本失败：{err}"))?;
    Ok(())
}

/// 函数 `apply_macos_bundle_update`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - pending: 参数 pending
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "macos")]
fn apply_macos_bundle_update(
    app: tauri::AppHandle,
    pending: &super::model::PendingUpdate,
) -> Result<UpdateActionResponse, String> {
    let staging_dir = PathBuf::from(
        pending
            .staging_dir
            .as_ref()
            .ok_or_else(|| "macOS 更新缺少暂存目录".to_string())?,
    );
    if !staging_dir.is_dir() {
        return Err(format!("未找到暂存目录：{}", staging_dir.display()));
    }

    let staged_app = resolve_staged_macos_app_bundle(&staging_dir)?;
    let current_app = resolve_current_macos_app_bundle(&current_exe_path()?)?;
    let pending_path = pending_update_path(&app)?;
    let script_dir = script_dir_from_pending(pending, &app)?;
    let log_path = log_path_for_script_dir(&script_dir, "apply-macos-bundle-update.log");
    append_apply_log(
        &log_path,
        &format!(
            "开始应用 macOS bundle 更新，current_app={}, staged_app={}",
            current_app.display(),
            staged_app.display()
        ),
    );
    let pid = std::process::id();

    spawn_macos_bundle_replace_worker(&script_dir, &current_app, &staged_app, &pending_path, pid)?;

    schedule_app_exit(app);
    Ok(UpdateActionResponse {
        ok: true,
        message: format!(
            "macOS 更新已就绪，应用将退出并替换 {}。日志：{}",
            current_app.display(),
            log_path.display()
        ),
    })
}

/// 函数 `launch_installer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - installer_path: 参数 installer_path
///
/// # 返回
/// 返回函数执行结果
fn launch_installer(installer_path: &Path) -> Result<(), String> {
    if !installer_path.is_file() {
        return Err(format!("未找到安装包：{}", installer_path.display()));
    }

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new(installer_path);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.spawn()
            .map_err(|err| format!("启动安装包失败：{err}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(installer_path)
            .spawn()
            .map_err(|err| format!("打开安装包失败：{err}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let ext = installer_path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "appimage" {
            #[cfg(unix)]
            {
                let _ = fs::set_permissions(installer_path, fs::Permissions::from_mode(0o755));
            }
            Command::new(installer_path)
                .spawn()
                .map_err(|err| format!("启动 AppImage 失败：{err}"))?;
            return Ok(());
        }

        Command::new("xdg-open")
            .arg(installer_path)
            .spawn()
            .map_err(|err| format!("打开安装包失败：{err}"))?;
        Ok(())
    }
}

/// 函数 `apply_portable_impl`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn apply_portable_impl(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    let pending = read_pending_update(&app)?
        .ok_or_else(|| "未找到已准备更新，请先调用 app_update_prepare".to_string())?;

    if pending.mode != "portable" {
        return Err("已准备更新并非便携模式".to_string());
    }

    let staging_dir = PathBuf::from(
        pending
            .staging_dir
            .as_ref()
            .ok_or_else(|| "便携更新缺少暂存目录".to_string())?,
    );
    if !staging_dir.is_dir() {
        return Err(format!("未找到暂存目录：{}", staging_dir.display()));
    }

    let exe_path = current_exe_path()?;
    let target_dir = exe_path
        .parent()
        .ok_or_else(|| "解析目标应用目录失败".to_string())?
        .to_path_buf();
    let exe_name = exe_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "解析当前可执行文件名失败".to_string())?
        .to_string();
    let restart_exe_name = resolve_portable_restart_exe(&staging_dir, &exe_name)?;
    let pending_path = pending_update_path(&app)?;
    let script_dir = script_dir_from_pending(&pending, &app)?;
    let log_path = log_path_for_script_dir(&script_dir, "apply-portable-update.log");
    append_apply_log(
        &log_path,
        &format!(
            "开始应用便携更新，target_dir={}, staging_dir={}",
            target_dir.display(),
            staging_dir.display()
        ),
    );
    let pid = std::process::id();

    spawn_portable_apply_worker(
        &script_dir,
        &target_dir,
        &staging_dir,
        &restart_exe_name,
        &pending_path,
        pid,
    )?;

    prepare_for_forced_app_exit();
    schedule_app_exit(app);
    Ok(UpdateActionResponse {
        ok: true,
        message: format!(
            "便携更新已就绪，应用将重启以完成替换。日志：{}",
            log_path.display()
        ),
    })
}

/// 函数 `launch_installer_impl`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn launch_installer_impl(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    let pending = read_pending_update(&app)?
        .ok_or_else(|| "未找到已准备更新，请先调用 app_update_prepare".to_string())?;
    if pending.mode != "installer" {
        return Err("已准备更新并非安装包模式".to_string());
    }

    let script_dir = script_dir_from_pending(&pending, &app)?;
    let installer_log_path = log_path_for_script_dir(&script_dir, "launch-installer.log");

    #[cfg(target_os = "macos")]
    if pending.staging_dir.is_some() {
        append_apply_log(
            &installer_log_path,
            "检测到 macOS app bundle 暂存目录，改走替换脚本",
        );
        return apply_macos_bundle_update(app, &pending);
    }

    let installer_path = PathBuf::from(
        pending
            .installer_path
            .as_ref()
            .ok_or_else(|| "待安装更新中缺少安装包路径".to_string())?,
    );

    append_apply_log(
        &installer_log_path,
        &format!("准备启动安装包：{}", installer_path.display()),
    );
    if let Err(err) = launch_installer(&installer_path) {
        append_apply_log(
            &installer_log_path,
            &format!("启动安装包失败：{}", installer_path.display()),
        );
        return Err(err);
    }
    append_apply_log(
        &installer_log_path,
        &format!("安装包已启动：{}", installer_path.display()),
    );
    clear_pending_update(&app)?;

    Ok(UpdateActionResponse {
        ok: true,
        message: format!(
            "已启动安装包：{}。日志：{}",
            installer_path.display(),
            installer_log_path.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::resolve_portable_restart_exe;

    /// 函数 `resolve_portable_restart_exe_prefers_existing_current_name`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn resolve_portable_restart_exe_prefers_existing_current_name() {
        let staging = std::env::temp_dir().join(format!(
            "codexmanager-updater-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("duration")
                .as_nanos()
        ));
        fs::create_dir_all(&staging).expect("create staging");
        let exe_name = if cfg!(target_os = "windows") {
            "current.exe"
        } else {
            "current"
        };
        fs::write(staging.join(exe_name), b"bin").expect("write exe");

        let resolved =
            resolve_portable_restart_exe(&staging, exe_name).expect("resolved executable");
        assert_eq!(resolved, exe_name);
        let _ = fs::remove_dir_all(&staging);
    }
}
