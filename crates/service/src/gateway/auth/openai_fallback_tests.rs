use super::*;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn chatgpt_account_header_falls_back_when_stored_chatgpt_id_is_blank() {
    let account = Account {
        id: "header-fallback".to_string(),
        label: "Header fallback".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some(" \t".to_string()),
        workspace_id: Some(" workspace-fallback ".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
    };

    assert_eq!(
        super::resolve_chatgpt_account_header(&account, "https://chatgpt.com/backend-api/codex"),
        Some("workspace-fallback")
    );
}

fn spawn_silent_sse_upstream() -> (String, mpsc::Receiver<bool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock OpenAI upstream");
    let addr = listener.local_addr().expect("mock OpenAI upstream addr");
    let (disconnect_tx, disconnect_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock OpenAI request");
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("set mock request read timeout");

        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
                Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }

        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n",
            )
            .expect("write mock SSE headers");
        stream.flush().expect("flush mock SSE headers");

        let deadline = Instant::now() + Duration::from_secs(2);
        let disconnected = loop {
            match stream.read(&mut buffer) {
                Ok(0) => break true,
                Ok(_) => continue,
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::ConnectionAborted
                            | ErrorKind::ConnectionReset
                            | ErrorKind::BrokenPipe
                    ) =>
                {
                    break true;
                }
                Err(err)
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                        && Instant::now() < deadline =>
                {
                    continue;
                }
                Err(_) => break false,
            }
        };
        let _ = disconnect_tx.send(disconnected);
    });

    (format!("http://{addr}/v1/responses"), disconnect_rx, handle)
}

fn assert_official_openai_request_is_cancellable(is_stream: bool) {
    let (url, disconnected, handle) = spawn_silent_sse_upstream();
    let upstream_base = url
        .strip_suffix("/v1/responses")
        .expect("mock upstream base");
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    let now = codexmanager_core::storage::now_ts();
    let account = Account {
        id: "official-openai-stream-cancel-test".to_string(),
        label: "official OpenAI stream cancel test".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    };
    let mut token = Token {
        account_id: account.id.clone(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: String::new(),
        api_key_access_token: Some("api-key-access-token".to_string()),
        last_refresh: now,
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .build()
        .expect("build async OpenAI client");

    let response = crate::gateway::run_upstream_io(try_openai_fallback(
        &client,
        &storage,
        &Method::GET,
        "/v1/responses",
        &super::super::IncomingHeaderSnapshot::default(),
        &Bytes::new(),
        is_stream,
        upstream_base,
        &account,
        &mut token,
        false,
        false,
        None,
    ))
    .expect("gateway async test runtime")
    .expect("send official OpenAI request")
    .expect("official OpenAI response");
    assert!(matches!(response, GatewayUpstreamResponse::Stream(_)));

    drop(response);

    assert_eq!(
        disconnected.recv_timeout(Duration::from_secs(3)),
        Ok(true),
        "dropping the official OpenAI stream must close a silent upstream"
    );
    handle.join().expect("join mock OpenAI upstream");
}

#[test]
fn official_openai_stream_uses_cancellable_async_transport() {
    assert_official_openai_request_is_cancellable(true);
}

#[test]
fn official_openai_json_uses_cancellable_async_transport() {
    assert_official_openai_request_is_cancellable(false);
}
