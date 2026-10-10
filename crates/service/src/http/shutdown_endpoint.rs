use axum::{extract::Extension, http::HeaderMap, http::StatusCode};
use std::sync::Arc;

/// Internal control route: the router applies the same token/origin checks as
/// RPC. Keep it outside admission slots so a busy listener can still drain.
pub(crate) async fn shutdown(
    Extension(state): Extension<Arc<super::state::AppState>>,
    headers: HeaderMap,
) -> Result<&'static str, StatusCode> {
    let actor = crate::RpcActor::from_parts(
        headers
            .get("X-CodexManager-Rpc-Actor-Role")
            .and_then(|value| value.to_str().ok()),
        headers
            .get("X-CodexManager-Rpc-Actor-User-Id")
            .and_then(|value| value.to_str().ok()),
    );
    if !actor.is_admin() {
        return Err(StatusCode::FORBIDDEN);
    }
    state.request_shutdown();
    // The empty address signals this process without sending another request.
    crate::request_shutdown("");
    Ok("shutdown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn shutdown_requires_rpc_admin_and_trusted_origin_before_signalling() {
        let _guard = crate::test_env_guard();
        crate::clear_shutdown_flag();
        struct ResetShutdown;
        impl Drop for ResetShutdown {
            fn drop(&mut self) {
                crate::clear_shutdown_flag();
            }
        }
        let _reset = ResetShutdown;
        let state = super::super::state::AppState::new();
        let app = super::super::router::build_router(state.clone());
        for (token, role, origin, site, expected) in [
            ("", "admin", "", "", StatusCode::UNAUTHORIZED),
            ("invalid-fixture", "admin", "", "", StatusCode::UNAUTHORIZED),
            (
                crate::rpc_auth_token(),
                "member",
                "",
                "",
                StatusCode::FORBIDDEN,
            ),
            (
                crate::rpc_auth_token(),
                "admin",
                "https://untrusted.example",
                "",
                StatusCode::FORBIDDEN,
            ),
            (
                crate::rpc_auth_token(),
                "admin",
                "",
                "cross-site",
                StatusCode::FORBIDDEN,
            ),
        ] {
            let mut request = Request::builder()
                .uri("/__shutdown")
                .header("X-CodexManager-Rpc-Token", token)
                .header("X-CodexManager-Rpc-Actor-Role", role);
            if !origin.is_empty() {
                request = request.header("Origin", origin);
            }
            if !site.is_empty() {
                request = request.header("Sec-Fetch-Site", site);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert!(!crate::shutdown_requested());
            assert!(!*state.shutdown.borrow());
        }
        // Shutdown does not compete with saturated ordinary request slots.
        let _slots = state.rpc_slots.acquire_many(64).await.unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/__shutdown")
                    .header("X-CodexManager-Rpc-Token", crate::rpc_auth_token())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(crate::shutdown_requested());
        assert!(*state.shutdown.borrow());
    }
}
