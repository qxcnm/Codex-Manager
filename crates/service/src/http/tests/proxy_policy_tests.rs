use super::*;
use futures_util::{stream, StreamExt};
use std::convert::Infallible;
use std::time::Duration;

#[tokio::test]
async fn unfinished_gateway_upload_times_out_before_domain_dispatch() {
    use tower::ServiceExt;
    let _env_lock = crate::test_env_guard();
    let previous = std::env::var_os(crate::http::middleware::ENV_HTTP_TIMEOUT_MS);
    std::env::set_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS, "20");
    let response = crate::http::router::build_router(crate::http::router::AppState::new())
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/v1/responses")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::from_stream(stream::pending::<
                    Result<Bytes, Infallible>,
                >()))
                .unwrap(),
        )
        .await
        .unwrap();
    match previous {
        Some(value) => std::env::set_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS, value),
        None => std::env::remove_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS),
    }
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(response.headers().contains_key("x-request-id"));
}

#[tokio::test]
async fn real_json_stream_and_zstd_use_stream_pool_and_skip_ordinary_deadline() {
    let _env_lock = crate::test_env_guard();
    let previous = std::env::var_os(crate::http::middleware::ENV_HTTP_TIMEOUT_MS);
    std::env::set_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS, "20");
    let backend = Router::new().fallback(any(|| async {
        tokio::time::sleep(Duration::from_millis(75)).await;
        Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(
                stream::once(async {
                    Ok::<_, Infallible>(Bytes::from_static(b"data: connected\n\n"))
                })
                .chain(stream::pending::<Result<Bytes, Infallible>>()),
            ))
            .unwrap()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_address = listener.local_addr().unwrap();
    let backend_task = tokio::spawn(async move { axum::serve(listener, backend).await.unwrap() });
    let client = Client::builder().no_proxy().build().unwrap();
    let app = build_front_proxy_app_with_limits(
        ProxyState {
            backend_base_url: format!("http://{backend_address}"),
            client: client.clone(),
        },
        crate::http::middleware::RequestLimits::new(1, 1),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let proxy_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let compressed =
        zstd::stream::encode_all(br#"{"stream":true,"model":"test"}"#.as_slice(), 0).unwrap();
    let mut active = client
        .post(format!("http://{address}/v1/responses"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_ENCODING, "zstd")
        .body(compressed)
        .send()
        .await
        .unwrap();
    assert_eq!(active.status(), StatusCode::OK);
    active.chunk().await.unwrap().unwrap();
    let rejected = client
        .post(format!("http://{address}/v1/chat/completions"))
        .json(&serde_json::json!({"stream": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ordinary = client
        .post(format!("http://{address}/v1/responses"))
        .json(&serde_json::json!({"stream": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(ordinary.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(ordinary.headers().contains_key("x-request-id"));
    for endpoint in ["health", "metrics"] {
        assert_eq!(
            client
                .get(format!("http://{address}/{endpoint}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    drop(active);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = client
                .post(format!("http://{address}/v1/chat/completions"))
                .json(&serde_json::json!({"stream": true}))
                .send()
                .await
                .unwrap();
            if response.status() == StatusCode::OK {
                break;
            }
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("disconnect releases proxy stream slot");
    proxy_task.abort();
    backend_task.abort();
    let _ = proxy_task.await;
    let _ = backend_task.await;
    match previous {
        Some(value) => std::env::set_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS, value),
        None => std::env::remove_var(crate::http::middleware::ENV_HTTP_TIMEOUT_MS),
    }
}

#[derive(Clone)]
struct SpanCapture(Arc<std::sync::Mutex<Vec<String>>>);

impl tracing::field::Visit for SpanCapture {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .unwrap()
            .push(format!("{}={value:?}", field.name()));
    }
}

impl tracing::Subscriber for SpanCapture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        attributes.record(&mut self.clone());
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut self.clone());
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        event.record(&mut self.clone());
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn tower_trace_excludes_query_credentials_headers_and_body() {
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let request = HttpRequest::builder()
        .uri("/auth/callback?code=private-code&state=private-state")
        .header("authorization", "Bearer private-token")
        .header("cookie", "session=private-cookie")
        .header("x-request-id", "correlation-123")
        .body(Body::from("private-body"))
        .unwrap();
    tracing::subscriber::with_default(SpanCapture(captured.clone()), || {
        let _span = make_http_span(&request);
    });
    let captured = captured.lock().unwrap().join(" ");
    assert!(captured.contains("/auth/callback"));
    assert!(captured.contains("correlation-123"));
    for secret in [
        "private-code",
        "private-state",
        "private-token",
        "private-cookie",
        "private-body",
        "authorization",
        "cookie",
    ] {
        assert!(!captured.contains(secret), "trace leaked {secret}");
    }
}
