use crate::app_storage::apply_runtime_storage_env;
use crate::rpc_client::{normalize_addr, rpc_call};
use crate::service_runtime::{
    spawn_service_with_addr, stop_service, validate_initialize_response, wait_for_service_ready,
};
use std::io;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

const SERVICE_READY_RETRIES: usize = 40;
const SERVICE_READY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
const BIND_PROBE_RETRIES: usize = 10;
const BIND_PROBE_DELAY: Duration = Duration::from_millis(120);

fn is_addr_in_use(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::AddrInUse
}

fn probe_bind_target_available(bind_addr: &str, connect_addr: &str) -> Result<(), String> {
    let trimmed = bind_addr.trim();
    if trimmed.len() > "localhost:".len()
        && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &trimmed["localhost:".len()..];
        let v4 = TcpListener::bind(format!("127.0.0.1:{port}"));
        let v6 = TcpListener::bind(format!("[::1]:{port}"));
        if v4.as_ref().is_err_and(is_addr_in_use) || v6.as_ref().is_err_and(is_addr_in_use) {
            return Err(format!("端口已被占用: {connect_addr}"));
        }
        v4.map_err(|err| format!("检查服务端口失败 ({connect_addr}): {err}"))?;
        if let Err(err) = v6 {
            log::debug!(
                "IPv6 loopback bind probe skipped for {}: {}",
                connect_addr,
                err
            );
        }
        return Ok(());
    }

    TcpListener::bind(trimmed).map(|_| ()).map_err(|err| {
        if is_addr_in_use(&err) {
            format!("端口已被占用: {connect_addr}")
        } else {
            format!("检查服务端口失败 ({connect_addr}): {err}")
        }
    })
}

pub(crate) fn ensure_bind_target_available(
    bind_addr: &str,
    connect_addr: &str,
) -> Result<(), String> {
    let mut last_err = None;
    for attempt in 0..=BIND_PROBE_RETRIES {
        match probe_bind_target_available(bind_addr, connect_addr) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                if attempt < BIND_PROBE_RETRIES {
                    thread::sleep(BIND_PROBE_DELAY);
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| format!("检查服务端口失败 ({connect_addr})")))
}

/// 函数 `service_initialize`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_initialize(
    app: tauri::AppHandle,
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let v = tauri::async_runtime::spawn_blocking(move || rpc_call("initialize", addr, None))
        .await
        .map_err(|err| format!("initialize task failed: {err}"))??;
    validate_initialize_response(&v)?;
    Ok(v)
}

/// 函数 `service_start`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_start(app: tauri::AppHandle, addr: String) -> Result<(), String> {
    let connect_addr = normalize_addr(&addr)?;
    apply_runtime_storage_env(&app);
    let bind_mode = codexmanager_service::current_service_bind_mode();
    let bind_addr = codexmanager_service::listener_bind_addr_for_mode(&connect_addr, &bind_mode);
    tauri::async_runtime::spawn_blocking(move || {
        log::info!(
            "service_start requested connect_addr={} bind_addr={}",
            connect_addr,
            bind_addr
        );
        stop_service();
        ensure_bind_target_available(&bind_addr, &connect_addr)?;
        spawn_service_with_addr(&app, &bind_addr, &connect_addr)?;
        thread::sleep(SERVICE_READY_RETRY_DELAY);
        wait_for_service_ready(
            &connect_addr,
            SERVICE_READY_RETRIES,
            SERVICE_READY_RETRY_DELAY,
        )
        .map_err(|err| {
            log::error!(
                "service health check failed at {} (bind {}): {}",
                connect_addr,
                bind_addr,
                err
            );
            stop_service();
            format!("service not ready at {connect_addr}: {err}")
        })
    })
    .await
    .map_err(|err| format!("service_start task failed: {err}"))?
}

/// 函数 `service_stop`
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
#[tauri::command]
pub async fn service_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        stop_service();
        Ok(())
    })
    .await
    .map_err(|err| format!("service_stop task failed: {err}"))?
}

/// 函数 `service_rpc_token`
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
#[tauri::command]
pub async fn service_rpc_token() -> Result<String, String> {
    Ok(codexmanager_service::rpc_auth_token().to_string())
}
