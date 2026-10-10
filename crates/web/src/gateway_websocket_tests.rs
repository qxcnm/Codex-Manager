use super::{is_upgrade_request, target_url};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::routing::{any, get};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;

const TEST_LARGE_RESPONSES_WS_FRAME_BYTES: usize = 17 * 1024 * 1024;

#[test]
fn websocket_target_url_preserves_path_and_query() {
    let uri: Uri = "/v1/responses?stream=true".parse().expect("valid uri");
    assert_eq!(
        target_url("localhost:48760", &uri),
        "ws://localhost:48760/v1/responses?stream=true"
    );
    assert_eq!(
        target_url("https://service.example", &uri),
        "wss://service.example/v1/responses?stream=true"
    );
}

#[test]
fn upgrade_detection_requires_connection_and_upgrade_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    assert!(!is_upgrade_request(&headers));
    headers.insert(
        header::CONNECTION,
        HeaderValue::from_static("keep-alive, Upgrade"),
    );
    assert!(is_upgrade_request(&headers));
}

fn test_app_state(service_addr: String) -> Arc<crate::AppState> {
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    Arc::new(crate::AppState {
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("build test proxy client"),
        service_rpc_url: format!("http://{service_addr}/rpc"),
        service_addr,
        rpc_token: "rpc_test_token".to_string(),
        web_auth_session_key: "web_test_session".to_string(),
        shutdown_tx,
        spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: Arc::new(String::new()),
    })
}

async fn start_test_gateway(
    service_addr: String,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test web gateway");
    let addr = listener.local_addr().expect("read test web gateway addr");
    let app = Router::new()
        .route("/v1/{*path}", any(crate::service_gateway::gateway_proxy))
        .with_state(test_app_state(service_addr));
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve test web gateway");
    });
    (addr, handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::result_large_err)]
async fn tunnels_responses_websocket_to_service() {
    let service_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock websocket service");
    let service_addr = service_listener
        .local_addr()
        .expect("read mock websocket service addr");
    let (capture_tx, capture_rx) = oneshot::channel();
    let service_handle = tokio::spawn(async move {
        let (stream, _) = service_listener
            .accept()
            .await
            .expect("accept web gateway websocket");
        let upstream_config = WebSocketConfig::default()
            .max_message_size(Some(TEST_LARGE_RESPONSES_WS_FRAME_BYTES * 2))
            .max_frame_size(Some(TEST_LARGE_RESPONSES_WS_FRAME_BYTES * 2));
        let mut websocket = tokio_tungstenite::accept_hdr_async_with_config(
            stream,
            move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                  response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                let capture = (
                    request.uri().path().to_string(),
                    request
                        .headers()
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string),
                    request
                        .headers()
                        .get("openai-beta")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string),
                );
                let _ = capture_tx.send(capture);
                Ok(response)
            },
            Some(upstream_config),
        )
        .await
        .expect("accept websocket handshake");
        let request = websocket
            .next()
            .await
            .expect("receive proxied websocket frame")
            .expect("read proxied websocket frame");
        assert!(matches!(request, WsMessage::Text(ref text)
            if text.contains("response.create")
                && text.contains("data:image/png;base64,")
                && text.len() > 16 * 1024 * 1024));
        websocket
            .send(WsMessage::Text(
                r#"{"type":"response.completed","response":{"id":"resp_web_proxy"}}"#.into(),
            ))
            .await
            .expect("send proxied websocket response");
        let _ = websocket.close(None).await;
    });

    let (gateway_addr, gateway_handle) = start_test_gateway(service_addr.to_string()).await;
    let mut request = format!("ws://{gateway_addr}/v1/responses")
        .into_client_request()
        .expect("build client websocket request");
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer platform_ws_key"),
    );
    request.headers_mut().insert(
        "openai-beta",
        HeaderValue::from_static("responses_websockets=2026-02-06"),
    );
    let (mut client, response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("connect through web gateway");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    let image_data = "A".repeat(TEST_LARGE_RESPONSES_WS_FRAME_BYTES);
    let payload = serde_json::json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_image",
                "image_url": format!("data:image/png;base64,{image_data}")
            }]
        }]
    })
    .to_string();
    assert!(payload.len() > 16 * 1024 * 1024);
    client
        .send(WsMessage::Text(payload.into()))
        .await
        .expect("send client websocket frame");
    let response = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("proxied response timeout")
        .expect("proxied response missing")
        .expect("read proxied response");
    assert!(matches!(response, WsMessage::Text(ref text) if text.contains("resp_web_proxy")));

    let (path, authorization, beta) = capture_rx.await.expect("receive handshake capture");
    assert_eq!(path, "/v1/responses");
    assert_eq!(authorization.as_deref(), Some("Bearer platform_ws_key"));
    assert_eq!(beta.as_deref(), Some("responses_websockets=2026-02-06"));

    let _ = client.close(None).await;
    service_handle.await.expect("join mock websocket service");
    gateway_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_handshake_rejection_status() {
    let service_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind rejecting service");
    let service_addr = service_listener
        .local_addr()
        .expect("read rejecting service addr");
    let service_app = Router::new().route(
        "/v1/responses",
        get(|| async {
            (
                StatusCode::UPGRADE_REQUIRED,
                [("x-codexmanager-error-code", "upgrade_required")],
                "responses websocket unsupported",
            )
        }),
    );
    let service_handle = tokio::spawn(async move {
        axum::serve(service_listener, service_app)
            .await
            .expect("serve rejecting service");
    });
    let (gateway_addr, gateway_handle) = start_test_gateway(service_addr.to_string()).await;

    let err = tokio_tungstenite::connect_async(format!("ws://{gateway_addr}/v1/responses"))
        .await
        .expect_err("websocket handshake should be rejected");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
            assert_eq!(
                response
                    .headers()
                    .get("x-codexmanager-error-code")
                    .and_then(|value| value.to_str().ok()),
                Some("upgrade_required")
            );
        }
        other => panic!("unexpected websocket rejection: {other}"),
    }

    gateway_handle.abort();
    service_handle.abort();
}

async fn close_handshake_through_gateway(upstream_initiates: bool) {
    use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

    let frame = CloseFrame {
        code: CloseCode::Normal,
        reason: "fixture finished".into(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service_addr = listener.local_addr().unwrap();
    let upstream_frame = frame.clone();
    let service_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut upstream = tokio_tungstenite::accept_async(stream).await.unwrap();
        if upstream_initiates {
            upstream.close(Some(upstream_frame.clone())).await.unwrap();
        }
        let message = tokio::time::timeout(Duration::from_secs(5), upstream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(message, WsMessage::Close(Some(upstream_frame)));
        if !upstream_initiates {
            // Complete the forwarded close handshake as a real service would.
            let _ = upstream.flush().await;
        }
    });
    let (gateway_addr, gateway_handle) = start_test_gateway(service_addr.to_string()).await;
    let mut request = format!("ws://{gateway_addr}/v1/responses")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer close_fixture_key"),
    );
    let (mut client, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    if !upstream_initiates {
        client.close(Some(frame.clone())).await.unwrap();
    }
    let message = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(message, WsMessage::Close(Some(frame)));
    if upstream_initiates {
        let _ = client.flush().await;
    }
    service_handle.await.unwrap();
    gateway_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledges_client_close_and_forwards_code_and_reason() {
    close_handshake_through_gateway(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledges_upstream_close_and_forwards_code_and_reason() {
    close_handshake_through_gateway(true).await;
}
