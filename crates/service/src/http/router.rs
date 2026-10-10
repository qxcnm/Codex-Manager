use axum::Router;
use std::future::Future;
use std::sync::Arc;

use super::proxy_runtime::{build_front_proxy_app_with_limits, ProxyState};
pub use super::state::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    build_front_proxy_app_with_limits(ProxyState::default(), state.request_limits.clone())
        .layer(axum::Extension(state))
}

/// Serve the shared router until the caller's cancellation future resolves.
/// This is used by Service-mode launchers and keeps runtime ownership outside
/// request handlers.
#[allow(dead_code)]
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    state: Arc<AppState>,
    shutdown: F,
) -> Result<(), std::io::Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    let shutdown_state = state.clone();
    let result = axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        shutdown_state.request_shutdown();
    })
    .await
    .map_err(std::io::Error::other);
    crate::gateway::drain_deferred_responses().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[test]
    fn app_state_builds_native_router() {
        let state = AppState::new();
        let _router = build_router(state);
    }

    #[tokio::test]
    async fn router_adds_request_id_before_backend_dispatch() {
        let state = AppState::new();
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn serve_binds_real_port_and_honors_graceful_shutdown() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let state = AppState::new();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(serve(listener, state, async move {
            let _ = shutdown_rx.await;
        }));

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("health response");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.expect("health body"), "ok");
        let response = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .expect("metrics");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(response.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/plain"));
        assert!(!response.text().await.unwrap().is_empty());
        let response = client
            .post(format!("http://{addr}/rpc"))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .expect("unauthorized rpc");
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        let response = client
            .post(format!("http://{addr}/rpc"))
            .header("content-type", "application/json")
            .header("x-codexmanager-rpc-token", crate::rpc_auth_token())
            .body(r#"{"jsonrpc":"2.0","id":23,"method":"not/a/method"}"#)
            .send()
            .await
            .expect("rpc");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let payload: serde_json::Value = response.json().await.expect("json rpc");
        assert_eq!(payload["id"], 23);
        assert_eq!(payload["error"]["code"], -32601);
        let response = client
            .get(format!("http://{addr}/auth/callback?code=invalid"))
            .send()
            .await
            .expect("callback");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("Missing login state"));
        shutdown_tx.send(()).expect("shutdown signal");
        task.await.expect("serve task").expect("serve result");
    }
}
