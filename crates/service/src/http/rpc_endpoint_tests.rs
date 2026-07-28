use super::{handle_parsed_rpc_request, handle_rpc_http};
use axum::body::{to_bytes, Body, Bytes};
use axum::http::{HeaderValue, Request, StatusCode};
use codexmanager_core::rpc::types::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 函数 `panicking_rpc_handler_returns_structured_json_error`
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
#[test]
fn panicking_rpc_handler_returns_structured_json_error() {
    let request = JsonRpcRequest {
        id: 7.into(),
        method: "account/usage/refresh".to_string(),
        params: None,
        trace: None,
    };

    let (body, success) = handle_parsed_rpc_request(request, |_req| {
        panic!("usage refresh boom");
    });

    assert!(!success);

    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(parsed.get("id").and_then(|value| value.as_u64()), Some(7));
    assert_eq!(
        parsed
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str()),
        Some("internal_error: rpc handler panicked")
    );
    assert_eq!(
        parsed
            .get("error")
            .and_then(|value| value.get("code"))
            .and_then(|value| value.as_i64()),
        Some(-32603)
    );
}

/// 函数 `normal_rpc_handler_keeps_success_shape`
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
#[test]
fn normal_rpc_handler_keeps_success_shape() {
    let request = JsonRpcRequest {
        id: 9.into(),
        method: "noop".to_string(),
        params: None,
        trace: None,
    };

    let (body, success) = handle_parsed_rpc_request(request, |req| {
        JsonRpcMessage::Response(JsonRpcResponse {
            id: req.id,
            result: serde_json::json!({ "ok": true }),
        })
    });

    assert!(success);
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(parsed.get("id").and_then(|value| value.as_u64()), Some(9));
    assert_eq!(
        parsed
            .get("result")
            .and_then(|value| value.get("ok"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

/// 函数 `notification_handler_returns_empty_body`
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
#[test]
fn notification_handler_returns_empty_body() {
    let request = JsonRpcRequest {
        id: 11.into(),
        method: "noop".to_string(),
        params: None,
        trace: None,
    };

    let (body, success) = handle_parsed_rpc_request(request, |_req| {
        JsonRpcMessage::Notification(JsonRpcNotification {
            method: "initialized".to_string(),
            params: None,
        })
    });

    assert!(success);
    assert!(body.is_empty());
}

#[tokio::test]
async fn axum_rpc_rejects_body_over_the_bounded_upload_limit() {
    let body = "x".repeat(crate::RPC_BODY_LIMIT_BYTES + 1);
    let request = Request::builder()
        .method("POST")
        .uri("/rpc")
        .header("content-type", HeaderValue::from_static("application/json"))
        .header(
            "x-codexmanager-rpc-token",
            HeaderValue::from_str(crate::rpc_auth_token()).expect("rpc token header"),
        )
        .body(Body::from(body))
        .expect("RPC request");

    let response = handle_rpc_http(request).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn axum_rpc_accepts_authenticated_body_within_the_limit() {
    let request = Request::builder()
        .method("POST")
        .uri("/rpc")
        .header("content-type", HeaderValue::from_static("application/json"))
        .header(
            "x-codexmanager-rpc-token",
            HeaderValue::from_str(crate::rpc_auth_token()).expect("rpc token header"),
        )
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":23,"method":"not/a/method"}"#,
        ))
        .expect("RPC request");

    let response = handle_rpc_http(request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("RPC response body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("RPC response JSON");
    assert_eq!(payload["id"], 23);
    assert_eq!(payload["error"]["code"], -32601);
}

#[tokio::test]
async fn axum_rpc_rejects_unauthenticated_large_body_without_reading_it() {
    let body_polled = Arc::new(AtomicBool::new(false));
    let body_polled_for_stream = body_polled.clone();
    let stream = futures_util::stream::once(async move {
        body_polled_for_stream.store(true, Ordering::SeqCst);
        Ok::<Bytes, Infallible>(Bytes::from(vec![b'x'; crate::RPC_BODY_LIMIT_BYTES + 1]))
    });
    let request = Request::builder()
        .method("POST")
        .uri("/rpc")
        .header("content-type", HeaderValue::from_static("application/json"))
        .header(
            "content-length",
            (crate::RPC_BODY_LIMIT_BYTES + 1).to_string(),
        )
        .body(Body::from_stream(stream))
        .expect("RPC request");

    let response = handle_rpc_http(request).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!body_polled.load(Ordering::SeqCst));
}
