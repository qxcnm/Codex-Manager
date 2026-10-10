use super::{plugin_http_client, plugin_http_client_build_count_for_test};

#[test]
fn plugin_http_client_reuses_cached_client() {
    let before = plugin_http_client_build_count_for_test();
    let first = plugin_http_client().expect("first plugin http client");
    let after_first = plugin_http_client_build_count_for_test();
    let second = plugin_http_client().expect("second plugin http client");
    let after_second = plugin_http_client_build_count_for_test();

    assert!(
        after_first == before || after_first == before + 1,
        "expected first call to reuse an existing client or build one client, before={before}, after_first={after_first}"
    );
    assert_eq!(after_second, after_first);
    drop(first);
    drop(second);
}

#[tokio::test(flavor = "current_thread")]
async fn plugin_async_network_waits_leave_executor_and_blocking_workers_available() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    let (entered, mut started) = tokio::sync::mpsc::channel(32);
    let app = axum::Router::new()
        .route(
            "/slow",
            axum::routing::get({
                let gate = gate.clone();
                move || {
                    let gate = gate.clone();
                    let entered = entered.clone();
                    async move {
                        entered.send(()).await.unwrap();
                        gate.acquire().await.unwrap().forget();
                        "plugin-body"
                    }
                }
            }),
        )
        .route("/fast", axum::routing::get(|| async { "fast-body" }))
        .route(
            "/error",
            axum::routing::get(|| async { axum::http::StatusCode::BAD_GATEWAY }),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut requests = Vec::new();
    for _ in 0..32 {
        let url = format!("{base}/slow");
        requests.push(tokio::spawn(async move { super::fetch_text(&url).await }));
    }
    for _ in 0..32 {
        started.recv().await.unwrap();
    }
    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            super::fetch_text(&format!("{base}/fast"))
        )
        .await
        .unwrap()
        .unwrap(),
        "fast-body"
    );
    assert_eq!(tokio::task::spawn_blocking(|| 17).await.unwrap(), 17);
    gate.add_permits(32);
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap(), "plugin-body");
    }
    assert!(super::fetch_text(&format!("{base}/error"))
        .await
        .unwrap_err()
        .contains("502"));
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn rhai_network_host_uses_shared_async_client_and_enforces_permission() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = axum::Router::new().fallback({
        let calls = calls.clone();
        move |method: axum::http::Method, body: String| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move { format!("{method}:{body}") }
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let script = format!(
        r#"fn run(context) {{
        let first = http_get("{base}/get");
        let second = http_post("{base}/post", "payload");
        #{{ first: first["body"], second: second["body"] }}
    }}"#
    );
    let result = tokio::task::spawn_blocking(move || {
        let plugin = codexmanager_core::storage::PluginRuntimeInstall {
            plugin_id: "async-network".to_string(),
            source_url: None,
            name: "Async network".to_string(),
            version: "1".to_string(),
            script_body: script,
            permissions_json: "[\"network\"]".to_string(),
            status: "enabled".to_string(),
        };
        let task = codexmanager_core::storage::PluginTaskExecutionRow {
            id: "async-network::run".to_string(),
            plugin_id: plugin.plugin_id.clone(),
            name: "Run".to_string(),
            description: None,
            entrypoint: "run".to_string(),
            schedule_kind: "manual".to_string(),
            interval_seconds: None,
            enabled: true,
        };
        let granted = super::parse_permissions(&plugin.permissions_json);
        let result = super::execute_plugin_script(&plugin, &task, None, &granted, 1).unwrap();
        assert!(
            super::execute_plugin_script(&plugin, &task, None, &Default::default(), 1).is_err()
        );
        result
    })
    .await
    .unwrap();
    assert_eq!(result["first"], "GET:");
    assert_eq!(result["second"], "POST:payload");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    server.abort();
}
