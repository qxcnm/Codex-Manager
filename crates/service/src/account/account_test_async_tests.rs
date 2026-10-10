use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn provider(
    reply: &'static str,
) -> (
    String,
    tokio::sync::oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    let (sent, received) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]);
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(reply.as_bytes()).await.unwrap();
        let _ = sent.send(());
        let count = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buffer))
            .await
            .expect("cancelled request must close its upstream socket")
            .unwrap();
        assert_eq!(count, 0);
    });
    (url, received, task)
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_text_and_image_tests_interrupts_silent_headers_and_body() {
    let _guard = crate::test_env_guard();
    for image in [false, true] {
        for reply in ["", "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"] {
            let (url, received, provider) = provider(reply).await;
            let flag = Arc::new(AtomicBool::new(false));
            let cancellation = Arc::clone(&flag);
            let cancel = tokio::spawn(async move {
                received.await.unwrap();
                cancellation.store(true, Ordering::Relaxed);
            });
            let client = Client::builder().no_proxy().build().unwrap();
            let headers = HeaderMap::new();
            let test_id = generate_account_test_id();
            let events = subscribe_account_test_events(&test_id);
            let operation = async {
                if image {
                    execute_image_test(&client, &url, &headers, &test_id, "image-model", "hi", &flag, &[]).await
                } else {
                    execute_text_test(&client, &url, &headers, &test_id, "text-model", "hi", &flag, &[]).await
                }
            };
            let outcome = tokio::time::timeout(Duration::from_secs(2), await_account_test(&test_id, &flag, operation))
                .await.expect("caller runtime stays responsive during network waits");
            assert!(matches!(outcome, AccountTestOutcome::Canceled));
            cancel.await.unwrap();
            provider.await.unwrap();
            let complete = events.receiver.try_iter().filter(|event| event.event_type == "test_complete").collect::<Vec<_>>();
            assert_eq!(complete.len(), 1);
            assert_eq!(complete[0].success, Some(false));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn async_text_and_image_tests_consume_terminal_sse() {
    let _guard = crate::test_env_guard();
    for (image, body) in [
        (false, "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\ndata: {\"type\":\"response.completed\"}\n\n"),
        (true, "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"image_generation_call\",\"result\":\"aW1hZ2U=\"}]}}\n\n"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/responses", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 8192];
            socket.read(&mut buffer).await.unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
        });
        let client = Client::builder().no_proxy().build().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let headers = HeaderMap::new();
        let test_id = generate_account_test_id();
        let events = subscribe_account_test_events(&test_id);
        let outcome = if image {
            execute_image_test(&client, &url, &headers, &test_id, "image-model", "hi", &flag, &[]).await
        } else {
            execute_text_test(&client, &url, &headers, &test_id, "text-model", "hi", &flag, &[]).await
        };
        assert!(matches!(outcome, AccountTestOutcome::Success));
        let events = events.receiver.try_iter().collect::<Vec<_>>();
        assert_eq!(events.iter().filter(|event| event.event_type == "test_complete" && event.success == Some(true)).count(), 1);
        assert!(events.iter().any(|event| event.event_type == if image { "image" } else { "content" }));
        task.await.unwrap();
    }
}

#[test]
fn active_test_guard_releases_only_its_own_registration() {
    let _guard = crate::test_env_guard();
    let account = "async-test-guard-account";
    let old = "test-old";
    register_active_test(account, old).unwrap();
    let cleanup = ActiveTestGuard {
        account_id: account.to_string(),
        test_id: old.to_string(),
    };
    remove_active_test(account, old);
    register_active_test(account, "test-new").unwrap();
    drop(cleanup);
    assert!(cancel_account_test(account, "test-new").unwrap());
    drop(ActiveTestGuard {
        account_id: account.to_string(),
        test_id: "test-new".to_string(),
    });
    assert!(!cancel_account_test(account, "test-new").unwrap());
}

#[test]
fn dropped_active_test_publishes_one_cancelled_terminal_event() {
    let _guard = crate::test_env_guard();
    let account_id = "shutdown-account-test";
    let test_id = "shutdown-account-test-id";
    let events = subscribe_account_test_events(test_id);
    register_active_test(account_id, test_id).unwrap();
    drop(ActiveTestGuard {
        account_id: account_id.into(),
        test_id: test_id.into(),
    });
    assert!(!cancel_account_test(account_id, test_id).unwrap());
    let terminal = events
        .receiver
        .try_iter()
        .filter(|event| event.event_type == "test_complete")
        .collect::<Vec<_>>();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].success, Some(false));
    drop(ActiveTestGuard {
        account_id: account_id.into(),
        test_id: test_id.into(),
    });
    assert!(
        events.receiver.try_recv().is_err(),
        "cleanup must be idempotent"
    );
}
