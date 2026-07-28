use crate::rpc_client::rpc_call;

/// 函数 `rpc_call_in_background`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) async fn rpc_call_in_background(
    method: &'static str,
    addr: Option<String>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let method_name = method.to_string();
    let method_for_task = method_name.clone();
    tauri::async_runtime::spawn_blocking(move || rpc_call(&method_for_task, addr, params))
        .await
        .map_err(|err| format!("{method_name} task failed: {err}"))?
}

/// 函数 `open_in_browser_blocking`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn open_url_with_shell(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = wide_null("open");
    let target = wide_null(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if result > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code: {result}"))
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn open_in_browser_blocking(url: &str) -> Result<(), String> {
    open_url_with_shell(url)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn open_in_browser_blocking(url: &str) -> Result<(), String> {
    webbrowser::open(url).map(|_| ()).map_err(|e| e.to_string())
}

fn validate_external_url(url: &str) -> Result<&str, String> {
    let normalized = url.trim();
    if normalized.is_empty() {
        return Err("缺少外部跳转地址".to_string());
    }

    let Some(separator_index) = normalized.find(':') else {
        return Err("外部跳转地址缺少协议".to_string());
    };
    if separator_index == 0 {
        return Err("外部跳转地址协议无效".to_string());
    }

    let scheme = &normalized[..separator_index];
    if !scheme
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
    {
        return Err("外部跳转地址协议无效".to_string());
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn validate_external_url_accepts_custom_protocols() {
        assert_eq!(
            validate_external_url(" ccswitch://v1/import?resource=provider ").unwrap(),
            "ccswitch://v1/import?resource=provider"
        );
    }

    #[test]
    fn validate_external_url_rejects_missing_or_invalid_protocols() {
        assert!(validate_external_url("").is_err());
        assert!(validate_external_url("example.com").is_err());
        assert!(validate_external_url("bad scheme://example.com").is_err());
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn open_external_url_blocking(url: &str) -> Result<(), String> {
    open_url_with_shell(validate_external_url(url)?)
}

#[cfg(target_os = "macos")]
pub(crate) fn open_external_url_blocking(url: &str) -> Result<(), String> {
    let normalized = validate_external_url(url)?;
    let status = std::process::Command::new("open")
        .arg(normalized)
        .status()
        .map_err(|err| format!("启动 open 失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open 退出状态异常：{status}"))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn open_external_url_blocking(url: &str) -> Result<(), String> {
    let normalized = validate_external_url(url)?;
    let status = std::process::Command::new("xdg-open")
        .arg(normalized)
        .status()
        .map_err(|err| format!("启动 xdg-open 失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-open 退出状态异常：{status}"))
    }
}

/// 函数 `spawn_background_command`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - command: 参数 command
/// - launch_failure_message: 参数 launch_failure_message
///
/// # 返回
/// 返回函数执行结果
fn spawn_background_command(
    mut command: std::process::Command,
    launch_failure_message: &str,
) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|err| format!("{launch_failure_message}：{err}"))?;
    if let Err(err) = std::thread::Builder::new()
        .name("background-command-reaper".to_string())
        .spawn(move || match child.wait() {
            Ok(status) => log::debug!("event=background_command_exited status={status}"),
            Err(err) => log::warn!("event=background_command_wait_failed error={err}"),
        })
    {
        log::warn!("event=background_command_reaper_spawn_failed error={err}");
    }
    Ok(())
}

/// 函数 `open_in_file_manager_blocking`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn open_in_file_manager_blocking(path: &str) -> Result<(), String> {
    let normalized = path.trim();
    if normalized.is_empty() {
        return Err("缺少要打开的目录".to_string());
    }

    let target = std::path::PathBuf::from(normalized);
    if !target.exists() {
        return Err(format!("目录不存在：{}", target.display()));
    }

    let dir = if target.is_dir() {
        target
    } else {
        target
            .parent()
            .map(|value| value.to_path_buf())
            .ok_or_else(|| format!("无法解析目录：{}", normalized))?
    };

    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(dir.as_os_str());
        spawn_background_command(command, "打开资源管理器失败")
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        command.arg(dir.as_os_str());
        spawn_background_command(command, "打开 Finder 失败")
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(dir.as_os_str());
        spawn_background_command(command, "打开目录失败")
    }
}
