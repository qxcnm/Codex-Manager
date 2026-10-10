use super::*;

#[tokio::test(flavor = "current_thread")]
async fn async_models_reader_enforces_body_limit_and_preserves_status_errors() {
    use axum::{body::Body, response::Response, routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/ok",
            get(|| async { axum::Json(json!({"data":[{"id":"gpt-test"}]})) }),
        )
        .route(
            "/denied",
            get(|| async { (axum::http::StatusCode::FORBIDDEN, "private-provider-error") }),
        )
        .route(
            "/large",
            get(|| async {
                let parts =
                    futures_util::stream::iter((0..3).map(|_| {
                        Ok::<_, std::io::Error>(bytes::Bytes::from(vec![b'x'; 1024 * 1024]))
                    }));
                Response::new(Body::from_stream(parts))
            }),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let ok = read_models_response(client.get(format!("{base}/ok")).send().await.unwrap())
        .await
        .unwrap();
    assert_eq!(parse_account_models(&ok)[0].0, "gpt-test");
    let denied = read_models_response(client.get(format!("{base}/denied")).send().await.unwrap())
        .await
        .unwrap_err();
    assert_eq!(denied, "account models http_status=403");
    let too_large = read_models_response(client.get(format!("{base}/large")).send().await.unwrap())
        .await
        .unwrap_err();
    assert_eq!(too_large, "account models response is too large");
    server.abort();
}
