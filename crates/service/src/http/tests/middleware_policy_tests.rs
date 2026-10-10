use super::*;
use axum::{http::Request as HttpRequest, middleware, routing::get, Router};
use bytes::Bytes;
use futures_util::{stream, StreamExt};
use std::collections::VecDeque;
use std::convert::Infallible;
use tower::ServiceExt;

#[test]
fn origins_require_exact_http_loopback_hosts() {
    for origin in [
        "http://localhost.evil.example",
        "http://127.0.0.1.evil.example",
        "https://localhost@evil.example",
        "null",
        "file://localhost",
        "http://evil.example@localhost",
        "http://localhost/path",
        "http://localhost?token=value",
    ] {
        assert!(
            !is_loopback_origin(origin),
            "accepted untrusted origin {origin}"
        );
    }
    for origin in [
        "http://localhost:8080",
        "https://127.0.0.1:8000",
        "http://[::1]:80",
    ] {
        assert!(
            is_loopback_origin(origin),
            "rejected loopback origin {origin}"
        );
    }
}

#[tokio::test]
async fn spoofed_rpc_origins_are_rejected_by_route_middleware() {
    let app = Router::new()
        .route("/rpc", get(|| async { "ok" }))
        .layer(middleware::from_fn(require_rpc_auth));
    for origin in [
        "http://localhost.evil.example",
        "http://127.0.0.1.evil.example",
        "null",
    ] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .header("X-CodexManager-Rpc-Token", crate::rpc_auth_token())
                    .header("Origin", origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn rpc_auth_preserves_json_media_type_parameters() {
    let app = Router::new()
        .route("/rpc", get(|| async { "ok" }))
        .layer(middleware::from_fn(require_rpc_auth));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/rpc")
                .header("content-type", "application/json; charset=utf-8")
                .header("X-CodexManager-Rpc-Token", crate::rpc_auth_token())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

struct Frames(VecDeque<http_body::Frame<Bytes>>);
impl HttpBody for Frames {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        Poll::Ready(self.0.pop_front().map(Ok))
    }
    fn is_end_stream(&self) -> bool {
        self.0.is_empty()
    }
}

#[tokio::test]
async fn body_permit_preserves_trailers_and_releases_at_eos() {
    let limits = RequestLimits::new(1, 1);
    let mut trailers = axum::http::HeaderMap::new();
    trailers.insert("x-checksum", HeaderValue::from_static("complete"));
    let body = Body::new(Frames(VecDeque::from([
        http_body::Frame::data(Bytes::from_static(b"data")),
        http_body::Frame::trailers(trailers),
    ])));
    let mut body =
        hold_response_permit(Response::new(body), limits.try_acquire(true).unwrap()).into_body();
    assert!(limits.try_acquire(true).is_err());
    let data = futures_util::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(data.into_data().unwrap(), Bytes::from_static(b"data"));
    assert!(limits.try_acquire(true).is_err());
    let trailers = futures_util::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .unwrap()
        .unwrap()
        .into_trailers()
        .unwrap();
    assert_eq!(trailers.get("x-checksum").unwrap(), "complete");
    assert!(limits.try_acquire(true).is_ok());
}

#[tokio::test]
async fn body_permit_releases_on_error_and_drop() {
    let limits = RequestLimits::new(1, 1);
    let body = Body::from_stream(stream::once(async {
        Err::<Bytes, _>(std::io::Error::other("disconnected"))
    }));
    let response = hold_response_permit(Response::new(body), limits.try_acquire(true).unwrap());
    assert!(axum::body::to_bytes(response.into_body(), 1024)
        .await
        .is_err());
    assert!(limits.try_acquire(true).is_ok());
    let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let response = hold_response_permit(Response::new(body), limits.try_acquire(true).unwrap());
    assert!(limits.try_acquire(true).is_err());
    drop(response);
    assert!(limits.try_acquire(true).is_ok());
}

#[tokio::test]
async fn real_sse_socket_holds_stream_slot_until_disconnect() {
    let limits = RequestLimits::new(1, 1);
    let app = Router::new()
        .route(
            "/stream",
            get(|| async {
                Response::new(Body::from_stream(
                    stream::once(async {
                        Ok::<_, Infallible>(Bytes::from_static(b"data: connected\n\n"))
                    })
                    .chain(stream::pending::<Result<Bytes, Infallible>>()),
                ))
            }),
        )
        .route("/normal", get(|| async { "ok" }))
        .layer(middleware::from_fn(concurrency_gate))
        .layer(axum::Extension(limits));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut active = client
        .get(format!("http://{address}/stream"))
        .header("accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(active.status(), StatusCode::OK);
    active.chunk().await.unwrap().unwrap();
    let rejected = client
        .get(format!("http://{address}/stream"))
        .header("accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        client
            .get(format!("http://{address}/normal"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    drop(active);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = client
                .get(format!("http://{address}/stream"))
                .header("accept", "text/event-stream")
                .send()
                .await
                .unwrap();
            if response.status() == StatusCode::OK {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("disconnect releases stream capacity");
    server.abort();
    let _ = server.await;
}
