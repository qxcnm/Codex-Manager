use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tauri::Manager;

use super::model::{PendingUpdate, UpdateCheckResponse};

const PENDING_UPDATE_FILE: &str = "pending-update.json";

#[derive(Debug, Default)]
struct UpdaterState {
    last_check: Option<UpdateCheckResponse>,
    last_error: Option<String>,
}

static UPDATER_STATE: OnceLock<Mutex<UpdaterState>> = OnceLock::new();

/// 函数 `updater_state`
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
fn updater_state() -> &'static Mutex<UpdaterState> {
    UPDATER_STATE.get_or_init(|| Mutex::new(UpdaterState::default()))
}

fn lock_updater_state() -> std::sync::MutexGuard<'static, UpdaterState> {
    updater_state().lock().unwrap_or_else(|poisoned| {
        log::error!("event=updater_state_lock_poisoned recovery=use_inner_state");
        poisoned.into_inner()
    })
}

/// 函数 `set_last_check`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn set_last_check(check: UpdateCheckResponse) {
    let mut guard = lock_updater_state();
    guard.last_check = Some(check);
    guard.last_error = None;
}

/// 函数 `set_last_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn set_last_error(message: String) {
    lock_updater_state().last_error = Some(message);
}

/// 函数 `clear_last_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn clear_last_error() {
    lock_updater_state().last_error = None;
}

/// 函数 `snapshot_last_state`
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
pub(super) fn snapshot_last_state() -> (Option<UpdateCheckResponse>, Option<String>) {
    let guard = lock_updater_state();
    (guard.last_check.clone(), guard.last_error.clone())
}

/// 函数 `updates_root_dir`
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
pub(super) fn updates_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut root = app
        .path()
        .app_data_dir()
        .map_err(|_| "未找到应用数据目录".to_string())?;
    root.push("updates");
    fs::create_dir_all(&root).map_err(|err| format!("创建更新目录失败：{err}"))?;
    Ok(root)
}

/// 函数 `pending_update_path`
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
pub(super) fn pending_update_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(updates_root_dir(app)?.join(PENDING_UPDATE_FILE))
}

/// 函数 `read_pending_update`
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
pub(super) fn read_pending_update(app: &tauri::AppHandle) -> Result<Option<PendingUpdate>, String> {
    let path = pending_update_path(app)?;
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|err| format!("读取待安装更新信息失败：{err}"))?;
    let parsed = serde_json::from_slice::<PendingUpdate>(&bytes)
        .map_err(|err| format!("解析待安装更新信息失败：{err}"))?;
    Ok(Some(parsed))
}

/// 函数 `write_pending_update`
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
pub(super) fn write_pending_update(
    app: &tauri::AppHandle,
    pending: &PendingUpdate,
) -> Result<(), String> {
    let path = pending_update_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建待安装信息目录失败：{err}"))?;
    }
    let bytes = serde_json::to_vec_pretty(pending)
        .map_err(|err| format!("序列化待安装更新信息失败：{err}"))?;
    fs::write(&path, bytes).map_err(|err| format!("写入待安装更新信息失败：{err}"))
}

/// 函数 `clear_pending_update`
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
pub(super) fn clear_pending_update(app: &tauri::AppHandle) -> Result<(), String> {
    let path = pending_update_path(app)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|err| format!("删除待安装更新信息失败：{err}"))?;
    }
    Ok(())
}

/// 函数 `script_dir_from_pending`
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
pub(super) fn script_dir_from_pending(
    pending: &PendingUpdate,
    app: &tauri::AppHandle,
) -> Result<PathBuf, String> {
    let asset_path = PathBuf::from(&pending.asset_path);
    if let Some(parent) = asset_path.parent() {
        return Ok(parent.to_path_buf());
    }
    updates_root_dir(app)
}
