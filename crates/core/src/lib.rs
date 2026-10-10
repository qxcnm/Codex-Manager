pub mod auth;
pub mod request_log_spill;
pub mod rpc;
pub mod storage;
pub mod usage;

/// 函数 `core_version`
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
pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
