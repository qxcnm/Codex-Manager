mod apply;
mod github;
mod model;
mod prepare;
mod runtime;
mod state;

pub use model::{
    UpdateActionResponse, UpdateCheckResponse, UpdatePrepareResponse, UpdateStatusResponse,
};

use crate::commands::shared::open_in_file_manager_blocking;
use apply::{apply_portable_impl, launch_installer_impl};
use prepare::{prepare_update_impl, resolve_update_context};
use runtime::{current_mode_and_marker, resolve_update_repo};
use state::{
    clear_last_error, read_pending_update, set_last_check, set_last_error, snapshot_last_state,
    updates_root_dir,
};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 函数 `append_update_runtime_log`
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
fn append_update_runtime_log(log_path: &std::path::Path, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let line = format!("[{timestamp}] {message}\n");

    if let Some(parent) = log_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!(
                "event=update_log_directory_create_failed phase=runtime path={} error={err}",
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
                    "event=update_log_write_failed phase=runtime path={} error={err}",
                    log_path.display()
                );
            }
        }
        Err(err) => log::warn!(
            "event=update_log_open_failed phase=runtime path={} error={err}",
            log_path.display()
        ),
    }
}

/// 函数 `updater_root_logs_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
fn updater_root_logs_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let logs_dir = updates_root_dir(app)?.join("logs");
    fs::create_dir_all(&logs_dir).map_err(|err| format!("创建更新日志目录失败：{err}"))?;
    Ok(logs_dir)
}

/// 函数 `app_update_check`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_update_check(app: tauri::AppHandle) -> Result<UpdateCheckResponse, String> {
    let check_log_path = updater_root_logs_dir(&app)?.join("check-update.log");
    append_update_runtime_log(&check_log_path, "开始检查更新");
    let task = tauri::async_runtime::spawn_blocking(resolve_update_context);
    match task.await {
        Ok(Ok(context)) => {
            set_last_check(context.check.clone());
            append_update_runtime_log(
                &check_log_path,
                &format!(
                    "检查更新完成，has_update={}, latest_version={}, reason={}",
                    context.check.has_update,
                    context.check.latest_version,
                    context.check.reason.clone().unwrap_or_default()
                ),
            );
            Ok(context.check)
        }
        Ok(Err(err)) => {
            set_last_error(err.clone());
            append_update_runtime_log(&check_log_path, &format!("检查更新失败：{err}"));
            Err(err)
        }
        Err(err) => {
            let message = format!("app_update_check 任务失败：{err}");
            set_last_error(message.clone());
            append_update_runtime_log(&check_log_path, &format!("检查更新任务失败：{message}"));
            Err(message)
        }
    }
}

/// 函数 `app_update_prepare`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_update_prepare(app: tauri::AppHandle) -> Result<UpdatePrepareResponse, String> {
    let app_handle = app.clone();
    let task = tauri::async_runtime::spawn_blocking(move || prepare_update_impl(&app_handle));
    match task.await {
        Ok(Ok(result)) => {
            clear_last_error();
            Ok(result)
        }
        Ok(Err(err)) => {
            set_last_error(err.clone());
            Err(err)
        }
        Err(err) => {
            let message = format!("app_update_prepare 任务失败：{err}");
            set_last_error(message.clone());
            Err(message)
        }
    }
}

/// 函数 `app_update_apply_portable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_update_apply_portable(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    apply_portable_impl(app)
}

/// 函数 `app_update_launch_installer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_update_launch_installer(app: tauri::AppHandle) -> Result<UpdateActionResponse, String> {
    launch_installer_impl(app)
}

/// 函数 `app_update_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_update_status(app: tauri::AppHandle) -> Result<UpdateStatusResponse, String> {
    let repo = resolve_update_repo();
    let (mode, is_portable, exe_path, marker_path) = current_mode_and_marker()?;
    let pending = read_pending_update(&app)?;
    let (last_check, last_error) = snapshot_last_state();

    Ok(UpdateStatusResponse {
        repo,
        mode,
        is_portable,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        current_exe_path: exe_path.display().to_string(),
        portable_marker_path: marker_path.display().to_string(),
        pending,
        last_check,
        last_error,
    })
}

/// 函数 `app_update_open_logs_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - asset_path: 参数 asset_path
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_update_open_logs_dir(
    app: tauri::AppHandle,
    asset_path: Option<String>,
) -> Result<(), String> {
    let target_dir = asset_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(|parent| parent.join("logs")))
        .unwrap_or(updater_root_logs_dir(&app)?);

    if !target_dir.exists() {
        fs::create_dir_all(&target_dir).map_err(|err| format!("创建更新日志目录失败：{err}"))?;
    }
    open_in_file_manager_blocking(&target_dir.display().to_string())
}
