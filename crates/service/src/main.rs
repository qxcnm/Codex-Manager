/// 函数 `main`
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
fn main() {
    codexmanager_service::portable::bootstrap_current_process();
    codexmanager_service::init_logging();
    let configured_addr = std::env::var("CODEXMANAGER_SERVICE_ADDR")
        .unwrap_or_else(|_| codexmanager_service::default_listener_bind_addr());
    let addr = codexmanager_service::listener_bind_addr(&configured_addr);
    log::info!("event=service_listening addr={addr}");
    if let Err(err) = codexmanager_service::start_server(&addr) {
        log::error!("event=service_stopped error={err}");
        std::process::exit(1);
    }
}
