use super::*;

#[test]
fn usage_refresh_sse_frame_uses_named_event() {
    let event = crate::UsageRefreshCompletedEvent {
        source: "single",
        processed: 1,
        total: 1,
        completed_at: 1775900000,
    };

    let frame = String::from_utf8(usage_refresh_sse_frame(&event)).expect("utf8 frame");

    assert!(frame.starts_with("event: usage-refresh-completed\n"));
    assert!(frame.contains("\"source\":\"single\""));
    assert!(frame.ends_with("\n\n"));
}

#[test]
fn async_usage_event_receive_survives_lag_and_cancellation() {
    use futures_util::FutureExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build test runtime");
    runtime.block_on(async {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(2);
        let event = |processed| crate::UsageRefreshCompletedEvent {
            source: "async-receive",
            processed,
            total: 3,
            completed_at: 1775900000,
        };
        // An unready receive may be dropped when its HTTP body is cancelled.
        assert!(
            next_usage_refresh_event_chunk(&mut receiver, Duration::from_secs(1))
                .now_or_never()
                .is_none()
        );
        for processed in 0..3 {
            sender.send(event(processed)).unwrap();
        }
        let frame = next_usage_refresh_event_chunk(&mut receiver, Duration::from_secs(1))
            .await
            .expect("lagged subscriber resumes at retained event");
        let frame = String::from_utf8(frame).unwrap();
        assert!(frame.starts_with("event: usage-refresh-completed\n"));
        assert!(frame.contains("\"processed\":1"));
        assert!(
            next_usage_refresh_event_chunk(&mut receiver, Duration::from_secs(1))
                .await
                .is_some()
        );
        assert_eq!(
            next_usage_refresh_event_chunk(&mut receiver, Duration::from_millis(1)).await,
            Some(b": keep-alive\n\n".to_vec())
        );
        drop(sender);
        assert!(
            next_usage_refresh_event_chunk(&mut receiver, Duration::from_secs(1))
                .await
                .is_none()
        );
    });
}

#[test]
fn async_sse_listener_delivers_events_filters_account_tests_and_cancels_subscriptions() {
    let _guard = crate::test_env_guard();
    let token = crate::rpc_auth_token();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build listener runtime");
    runtime.block_on(async {
        let usage_before = crate::usage_refresh::usage_refresh_async_subscriber_count();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route(
                "/usage",
                axum::routing::get(handle_usage_refresh_events_http),
            )
            .route(
                "/account",
                axum::routing::get(
                    crate::http::account_test_events::handle_account_test_events_http,
                ),
            );
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::new();
        let mut usage = client
            .get(format!("http://{addr}/usage"))
            .header("X-CodexManager-Rpc-Token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(usage.status(), reqwest::StatusCode::OK);
        assert_eq!(usage.headers()["content-type"], "text/event-stream");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), usage.chunk())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Bytes::from_static(b": connected\n\n")
        );
        let test_id = "sse-listener-account-first";
        let other_id = "sse-listener-account-second";
        let mut account = client
            .get(format!("http://{addr}/account?testId={test_id}"))
            .header("X-CodexManager-Rpc-Token", token)
            .send()
            .await
            .unwrap();
        let mut other = client
            .get(format!("http://{addr}/account?testId={other_id}"))
            .header("X-CodexManager-Rpc-Token", token)
            .send()
            .await
            .unwrap();
        for response in [&mut account, &mut other] {
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), response.chunk())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
                Bytes::from_static(b": connected\n\n")
            );
        }
        crate::usage_refresh::notify_usage_refresh_completed("listener-event", 2, 3);
        crate::account_test::notify_account_test_event(crate::AccountTestEvent {
            test_id: test_id.to_owned(),
            event_type: "status".to_owned(),
            text: None,
            model: None,
            status: Some("running".to_owned()),
            image_url: None,
            mime_type: None,
            success: None,
            error: None,
        });
        let usage_frame = tokio::time::timeout(Duration::from_secs(2), usage.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(std::str::from_utf8(&usage_frame)
            .unwrap()
            .contains("event: usage-refresh-completed"));
        assert!(std::str::from_utf8(&usage_frame)
            .unwrap()
            .contains("listener-event"));
        let account_frame = tokio::time::timeout(Duration::from_secs(2), account.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let account_frame = std::str::from_utf8(&account_frame).unwrap();
        assert!(account_frame.contains("event: account-test-event"));
        assert!(account_frame.contains(test_id));
        assert!(
            tokio::time::timeout(Duration::from_millis(30), other.chunk())
                .await
                .is_err()
        );
        drop((usage, account, other));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if crate::usage_refresh::usage_refresh_async_subscriber_count() == usage_before
                    && crate::account_test::account_test_async_subscriber_count(test_id) == 0
                    && crate::account_test::account_test_async_subscriber_count(other_id) == 0
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("disconnected SSE bodies release async receivers promptly");
        shutdown.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    });
}
