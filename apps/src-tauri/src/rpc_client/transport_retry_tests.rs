use super::{rpc_call_with_sockets, rpc_call_with_sockets_and_timeout, rpc_io_timeout};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn read_rpc_request(stream: &mut TcpStream) -> serde_json::Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set mock read timeout");
    let mut request = Vec::new();
    loop {
        let mut bytes = [0_u8; 1024];
        let read = stream.read(&mut bytes).expect("read RPC request");
        assert!(read > 0, "request ended before its complete body");
        request.extend_from_slice(&bytes[..read]);
        let Some(split) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let headers = std::str::from_utf8(&request[..split]).expect("HTTP headers");
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
            .expect("content length header");
        if request.len() >= split + 4 + length {
            return serde_json::from_slice(&request[split + 4..split + 4 + length])
                .expect("RPC JSON request");
        }
    }
}

fn response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn assert_no_pending_connection(listener: &TcpListener) {
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        other => panic!("unexpected replay connection: {other:?}"),
    }
}

#[test]
fn requestlog_detail_forwards_desktop_paging_and_legacy_defaults() {
    use serde_json::json;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    for (offset, limit, field_limit) in [
        (Some(1), Some(2), Some(4)),
        (Some(1), Some(2), Some(0)),
        (None, None, None),
    ] {
        let socket = listener.try_clone().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            let request = read_rpc_request(&mut stream);
            assert_eq!(request["method"], "requestlog/detail");
            let reply = json!({"result": request["params"]});
            stream
                .write_all(response(&reply.to_string()).as_bytes())
                .unwrap();
        });
        let reply =
            tauri::async_runtime::block_on(crate::commands::requestlog::service_requestlog_detail(
                Some(addr.clone()),
                Some("trace".into()),
                Some("upstream".into()),
                offset,
                limit,
                field_limit,
            ))
            .unwrap();
        server.join().unwrap();
        assert_eq!(
            reply["result"],
            json!({
                "traceId": "trace", "stage": "upstream", "itemOffset": offset.unwrap_or(0),
                "itemLimit": limit.unwrap_or(0), "fieldLimit": field_limit.unwrap_or(0)
            })
        );
    }
    assert_eq!(
        rpc_io_timeout("requestlog/detail", None),
        Duration::from_secs(120)
    );
}

#[test]
fn managed_model_price_sync_uses_a_dedicated_timeout() {
    assert_eq!(
        rpc_io_timeout("apikey/managedModelPriceSyncV2", None),
        Duration::from_secs(60)
    );
    for method in [
        "apikey/managedModelListV2",
        "apikey/managedModelPriceSync",
        "apikey/managedModelPriceSyncV2Extra",
    ] {
        assert_eq!(
            rpc_io_timeout(method, None),
            Duration::from_secs(10),
            "unrelated method must retain the default timeout: {method}"
        );
    }
}

#[test]
fn managed_model_price_sync_waits_beyond_the_default_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind price sync mock");
    let addr = listener.local_addr().expect("price sync mock address");
    let server_listener = listener.try_clone().expect("clone price sync listener");
    let fallback = TcpListener::bind("127.0.0.1:0").expect("bind fallback");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = server_listener.accept().expect("accept price sync RPC");
        let request = read_rpc_request(&mut stream);
        assert_eq!(request["method"], "apikey/managedModelPriceSyncV2");
        assert_eq!(
            request["params"]["modelSlugs"],
            serde_json::json!(["gpt-6-sol"])
        );
        // Real socket regression: the service may still be fetching its price sources
        // after the previous ten-second desktop timeout has elapsed.
        std::thread::sleep(Duration::from_secs(11));
        let _ = stream.write_all(
            response(r#"{"result":{"updated":1,"updatedSlugs":["gpt-6-sol"]}}"#).as_bytes(),
        );
    });

    let started = Instant::now();
    let result = rpc_call_with_sockets(
        "apikey/managedModelPriceSyncV2",
        &addr.to_string(),
        &[addr, fallback.local_addr().expect("fallback address")],
        Some(serde_json::json!({ "modelSlugs": ["gpt-6-sol"] })),
    );
    server.join().expect("join delayed price sync mock");
    assert_no_pending_connection(&listener);
    assert_no_pending_connection(&fallback);
    let result = result.expect("price sync must wait beyond the default ten-second timeout");
    assert!(started.elapsed() >= Duration::from_secs(11));
    assert_eq!(result["result"]["updated"], 1);
    assert_eq!(
        result["result"]["updatedSlugs"],
        serde_json::json!(["gpt-6-sol"])
    );
}

#[test]
fn managed_model_price_sync_does_not_replay_lost_or_timed_out_responses() {
    for delayed_response in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind price sync mock");
        let addr = listener.local_addr().expect("price sync mock address");
        let server_listener = listener.try_clone().expect("clone price sync listener");
        let fallback = TcpListener::bind("127.0.0.1:0").expect("bind fallback");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = server_listener.accept().expect("accept price sync RPC");
            let request = read_rpc_request(&mut stream);
            assert_eq!(request["method"], "apikey/managedModelPriceSyncV2");
            assert_eq!(
                request["params"]["modelSlugs"],
                serde_json::json!(["gpt-6-sol"])
            );
            if delayed_response {
                std::thread::sleep(Duration::from_millis(150));
                let _ = stream.write_all(response(r#"{"result":{"updated":1}}"#).as_bytes());
            }
        });

        let error = rpc_call_with_sockets_and_timeout(
            "apikey/managedModelPriceSyncV2",
            &addr.to_string(),
            &[addr, fallback.local_addr().expect("fallback address")],
            Some(serde_json::json!({ "modelSlugs": ["gpt-6-sol"] })),
            Duration::from_millis(30),
        )
        .expect_err("an unconfirmed price update must not replay");
        server.join().expect("join price sync mock");
        assert!(error.starts_with("rpc_result_unknown:"), "{error}");
        assert!(error.contains("apikey/managedModelPriceSyncV2"), "{error}");
        assert_no_pending_connection(&listener);
        assert_no_pending_connection(&fallback);
    }
}

#[test]
fn create_key_commits_once_when_response_is_lost_even_with_retry_parameters() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock address");
    let fallback = TcpListener::bind("127.0.0.1:0").expect("bind fallback");
    let fallback_addr = fallback.local_addr().expect("fallback address");
    let commits = Arc::new(AtomicUsize::new(0));
    let server_commits = commits.clone();
    let server = std::thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request = read_rpc_request(&mut stream);
                    assert_eq!(request["method"], "apikey/create");
                    // Record the mock commit before intentionally losing its response.
                    server_commits.fetch_add(1, Ordering::SeqCst);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept mock request: {error}"),
            }
        }
    });

    let error = rpc_call_with_sockets_and_timeout(
        "apikey/create",
        &addr.to_string(),
        &[addr, fallback_addr],
        Some(serde_json::json!({
            "name": "temporary-mock-key",
            "retries": 99,
            "retrySafety": "readOnly"
        })),
        Duration::from_millis(60),
    )
    .expect_err("lost write response must have unknown outcome");
    server.join().expect("join mock");
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    assert!(error.starts_with("rpc_result_unknown:"), "{error}");
    assert!(error.contains("结果未知，请先刷新核实"), "{error}");
    assert_no_pending_connection(&fallback);
}

#[test]
fn non_idempotent_and_unknown_writes_do_not_replay_invalid_responses() {
    for method in ["apikey/create", "future/get"] {
        for reply in [
            response("not JSON"),
            response(r#"{"unexpected":true}"#),
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\n\r\n{}".to_owned(),
            "invalid HTTP response".to_owned(),
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ninvalid\r\n".to_owned(),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
            let addr = listener.local_addr().expect("mock address");
            let server_listener = listener.try_clone().expect("clone mock listener");
            let fallback = TcpListener::bind("127.0.0.1:0").expect("bind fallback");
            let server_method = method.to_owned();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = server_listener.accept().expect("accept request");
                assert_eq!(read_rpc_request(&mut stream)["method"], server_method);
                stream
                    .write_all(reply.as_bytes())
                    .expect("write mock reply");
            });
            let error = rpc_call_with_sockets_and_timeout(
                method,
                &addr.to_string(),
                &[addr, fallback.local_addr().expect("fallback address")],
                None,
                Duration::from_millis(30),
            )
            .expect_err("unconfirmed write result must stop replay");
            server.join().expect("join mock");
            assert!(error.starts_with("rpc_result_unknown:"), "{error}");
            assert_no_pending_connection(&listener);
            assert_no_pending_connection(&fallback);
        }
    }
}

#[test]
fn create_key_does_not_replay_after_response_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock address");
    let server_listener = listener.try_clone().expect("clone mock listener");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = server_listener.accept().expect("accept request");
        assert_eq!(read_rpc_request(&mut stream)["method"], "apikey/create");
        std::thread::sleep(Duration::from_millis(100));
        let _ = stream.write_all(response(r#"{"result":{"id":"mock-key"}}"#).as_bytes());
    });
    let error = rpc_call_with_sockets_and_timeout(
        "apikey/create",
        &addr.to_string(),
        &[addr],
        None,
        Duration::from_millis(20),
    )
    .expect_err("timed out write must stop replay");
    server.join().expect("join mock");
    assert!(error.starts_with("rpc_result_unknown:"), "{error}");
    assert_no_pending_connection(&listener);
}

#[test]
fn non_idempotent_writes_do_not_replay_or_keep_socket_after_headers_only_timeout() {
    for method in ["apikey/create", "account/usage/resetCredit/consume"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind headers-only mock");
        let addr = listener.local_addr().expect("headers-only mock address");
        let server_listener = listener.try_clone().expect("clone headers-only listener");
        let fallback = TcpListener::bind("127.0.0.1:0").expect("bind fallback");
        let commits = Arc::new(AtomicUsize::new(0));
        let server_commits = commits.clone();
        let (headers_tx, headers_rx) = std::sync::mpsc::channel();
        let server_method = method.to_owned();
        let server = std::thread::spawn(move || {
            server_listener
                .set_nonblocking(true)
                .expect("nonblocking headers-only listener");
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match server_listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "RPC did not reach the mock");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept headers-only RPC: {error}"),
                }
            };
            // Windows accepted sockets inherit the listener's nonblocking mode.
            // Observe EOF with the bounded blocking read configured below.
            stream
                .set_nonblocking(false)
                .expect("use blocking accepted mock socket");
            assert_eq!(read_rpc_request(&mut stream)["method"], server_method);
            server_commits.fetch_add(1, Ordering::SeqCst);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n")
                .expect("send response headers without a body");
            stream.flush().expect("flush response headers");
            headers_tx.send(()).expect("report sent headers");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("bound socket-release observation");
            let mut byte = [0_u8; 1];
            match stream.read(&mut byte) {
                Ok(0) => {}
                Err(error) if error.kind() == ErrorKind::ConnectionReset => {}
                other => panic!("timed-out RPC must release its upstream socket: {other:?}"),
            }
        });

        let error = rpc_call_with_sockets_and_timeout(
            method,
            &addr.to_string(),
            &[addr, fallback.local_addr().expect("fallback address")],
            Some(serde_json::json!({ "retries": 99 })),
            Duration::from_millis(250),
        )
        .expect_err("a write with an incomplete response has an unknown result");
        headers_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("mock emitted headers before the body stalled");
        server.join().expect("join headers-only mock");

        assert!(
            error.starts_with("rpc_result_unknown:"),
            "{method}: {error}"
        );
        assert_eq!(commits.load(Ordering::SeqCst), 1, "{method}");
        assert_no_pending_connection(&listener);
        assert_no_pending_connection(&fallback);
        if method == "account/usage/resetCredit/consume" {
            assert!(error.contains("reset-credit status"), "{error}");
        }
    }
}

#[test]
fn readonly_and_reviewed_idempotent_calls_retry_lost_responses() {
    for method in ["account/list", "apikey/disable"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().expect("mock address");
        let server_method = method.to_owned();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept request");
                assert_eq!(read_rpc_request(&mut stream)["method"], server_method);
                if attempt == 1 {
                    stream
                        .write_all(response(r#"{"result":{"ok":true}}"#).as_bytes())
                        .expect("write retry response");
                }
            }
        });
        let result = rpc_call_with_sockets_and_timeout(
            method,
            &addr.to_string(),
            &[addr],
            None,
            Duration::from_millis(50),
        )
        .expect("safe RPC should retry");
        server.join().expect("join mock");
        assert_eq!(result["result"]["ok"], true);
    }
}

#[test]
fn readonly_retries_are_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock address");
    let server_listener = listener.try_clone().expect("clone mock listener");
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = server_listener.accept().expect("accept request");
            assert_eq!(read_rpc_request(&mut stream)["method"], "account/list");
        }
    });
    let error = rpc_call_with_sockets_and_timeout(
        "account/list",
        &addr.to_string(),
        &[addr],
        None,
        Duration::from_millis(50),
    )
    .expect_err("two lost responses exhaust safe retries");
    server.join().expect("join mock");
    assert!(!error.contains("rpc_result_unknown:"), "{error}");
    assert_no_pending_connection(&listener);
}

#[test]
fn create_key_can_fall_back_after_connect_failure_before_send() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        assert_eq!(read_rpc_request(&mut stream)["method"], "apikey/create");
        stream
            .write_all(response(r#"{"result":{"id":"mock-key"}}"#).as_bytes())
            .expect("write response");
    });
    let result = rpc_call_with_sockets_and_timeout(
        "apikey/create",
        &addr.to_string(),
        &[SocketAddr::from(([127, 0, 0, 1], 0)), addr],
        None,
        Duration::from_millis(50),
    )
    .expect("connect failure before sending may fall back");
    server.join().expect("join mock");
    assert_eq!(result["result"]["id"], "mock-key");
}

#[test]
fn confirmed_rpc_business_error_is_not_an_unknown_result_or_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let addr = listener.local_addr().expect("mock address");
    let server_listener = listener.try_clone().expect("clone mock listener");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = server_listener.accept().expect("accept request");
        assert_eq!(read_rpc_request(&mut stream)["method"], "apikey/create");
        stream
            .write_all(response(r#"{"result":{"error":"missing name"}}"#).as_bytes())
            .expect("write response");
    });
    let result = rpc_call_with_sockets_and_timeout(
        "apikey/create",
        &addr.to_string(),
        &[addr],
        None,
        Duration::from_millis(50),
    )
    .expect("confirmed business response stays available to the caller");
    server.join().expect("join mock");
    assert_eq!(result["result"]["error"], "missing name");
    assert_no_pending_connection(&listener);
}
