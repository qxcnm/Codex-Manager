use tiny_http::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendRoute {
    Rpc,
    AuthCallback,
    UsageRefreshEvents,
    AccountTestEvents,
    Metrics,
    Gateway,
}

/// 函数 `resolve_backend_route`
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
pub(crate) fn resolve_backend_route(method: &str, path: &str) -> BackendRoute {
    if method == "POST" && path == "/rpc" {
        return BackendRoute::Rpc;
    }
    if method == "GET" && path.starts_with("/auth/callback") {
        return BackendRoute::AuthCallback;
    }
    if method == "GET" && path == "/events/usage-refresh" {
        return BackendRoute::UsageRefreshEvents;
    }
    if method == "GET" && path == "/events/account-test" {
        return BackendRoute::AccountTestEvents;
    }
    if method == "GET" && path == "/metrics" {
        return BackendRoute::Metrics;
    }
    BackendRoute::Gateway
}

/// 函数 `handle_backend_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn handle_backend_request(request: Request) {
    let route = resolve_backend_route(request.method().as_str(), request.url());
    match route {
        BackendRoute::Rpc => crate::http::rpc_endpoint::handle_rpc(request),
        BackendRoute::AuthCallback => crate::http::callback_endpoint::handle_callback(request),
        BackendRoute::UsageRefreshEvents => {
            crate::http::usage_events::handle_usage_refresh_events(request)
        }
        BackendRoute::AccountTestEvents => {
            crate::http::account_test_events::handle_account_test_events(request)
        }
        BackendRoute::Metrics => crate::http::gateway_endpoint::handle_metrics(request),
        BackendRoute::Gateway => crate::http::gateway_endpoint::handle_gateway(request),
    }
}

#[cfg(test)]
#[path = "tests/backend_router_tests.rs"]
mod tests;
