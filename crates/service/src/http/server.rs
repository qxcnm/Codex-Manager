use crate::http::backend_runtime::{start_backend_server, wake_backend_shutdown};
use crate::http::proxy_runtime::run_front_proxy;

/// 函数 `start_http`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
pub fn start_http(addr: &str) -> std::io::Result<()> {
    let backend = start_backend_server()?;
    let result = run_front_proxy(addr, &backend.addr);
    wake_backend_shutdown(&backend.addr);
    if let Err(_payload) = backend.join.join() {
        log::error!("event=http_backend_join_failed status=panicked");
    }
    result
}
