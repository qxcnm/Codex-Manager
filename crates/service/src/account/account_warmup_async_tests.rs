use super::*;

#[tokio::test(flavor = "current_thread")]
async fn warmup_request_consumes_live_async_sse_on_the_callers_runtime() {
    let _guard = crate::test_env_guard();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/responses",
        axum::routing::post(
            |axum::Json(body): axum::Json<serde_json::Value>| async move {
                assert_eq!(body["stream"], true);
                assert_eq!(body["input"][0]["content"][0]["text"], "hi");
                (
                    [("content-type", "text/event-stream")],
                    "event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
                )
            },
        ),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let account = Account {
        id: "async-warmup".into(),
        label: "async-warmup".into(),
        issuer: "https://auth.openai.com".into(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".into(),
        created_at: 0,
        updated_at: 0,
    };
    let authorization = WarmupAuthorization {
        value: "test-token".into(),
        task_id: None,
        is_fedramp: false,
        uses_agent_identity: false,
        account_scope_id: None,
    };
    let client = Client::builder().no_proxy().build().unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        send_warmup_request_at_url(
            &client,
            &account,
            &authorization,
            "test-model",
            "hi",
            Some(Duration::from_secs(1)),
            &url,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
}
