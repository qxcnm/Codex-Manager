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
    run_front_proxy(addr)
}
