use super::*;
use crate::gateway::ResponseAdapter;
use serde_json::json;

fn credentials() -> BpsCredentials {
    BpsCredentials {
        access_token: "mock-bps-secret".to_owned(),
        account_id: "upstream-bps-account".to_owned(),
    }
}

fn request_context() -> UpstreamRequestContext<'static> {
    UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: "openai_compat",
        is_fedramp: false,
        original_body: None,
        capture: None,
        replay_guard: None,
    }
}

fn completed(output: Value) -> Value {
    json!({"type":"response.completed","response":{"id":"resp_bps_fixture","object":"response","status":"completed","model":"test-model","created_at":1,"output":output,"usage":{"input_tokens":3,"output_tokens":4,"total_tokens":7}}})
}

fn text_output() -> Value {
    json!([{"id":"msg_bps_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"BPS OK","annotations":[]}]}])
}

async fn mock_upstream(events: Vec<Value>) -> (String, tokio::task::JoinHandle<Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}/basispoints/api/responses",
        listener.local_addr().unwrap()
    );
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut extensions =
            tokio_tungstenite::tungstenite::extensions::ExtensionsConfig::default();
        extensions.permessage_deflate = Some(tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig::default());
        let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        config.extensions = extensions;
        let mut socket = tokio_tungstenite::accept_hdr_async_with_config(stream, |request: &tokio_tungstenite::tungstenite::handshake::server::Request, mut response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            assert_eq!(request.headers()["authorization"], "Bearer mock-bps-secret");
            assert_eq!(request.headers()["chatgpt-account-id"], "upstream-bps-account");
            assert_eq!(request.headers()["x-basispoints-auth-mode"], "chatgpt");
            assert!(request.headers()["sec-websocket-protocol"].to_str().unwrap().contains("responses"));
            response.headers_mut().insert("sec-websocket-protocol", "responses".parse().unwrap());
            Ok(response)
        }, Some(config)).await.unwrap();
        let frame = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let frame: Value = serde_json::from_str(&frame).unwrap();
        for event in events {
            socket
                .send(Message::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        frame
    });
    (endpoint, handle)
}

async fn fixture(
    events: Vec<Value>,
    input: Value,
) -> (GatewayUpstreamResponse, tokio::task::JoinHandle<Value>) {
    let (endpoint, server) = mock_upstream(events).await;
    let (body, bridge) = bps_protocol::prepare(
        input.to_string().as_bytes(),
        "fixture-account:fixture-session",
    )
    .unwrap();
    let response = send_prepared(
        &endpoint,
        None,
        &credentials(),
        &body,
        bridge,
        Some(Instant::now() + Duration::from_secs(5)),
        request_context(),
    )
    .await
    .unwrap();
    (response, server)
}

#[tokio::test]
async fn bps_websocket_preserves_requested_model_and_uses_independent_headers() {
    let (response, server) = fixture(
        vec![
            json!({"type":"basispoints.response.heartbeat"}),
            completed(text_output()),
        ],
        json!({"model":"test-model","input":"hello","reasoning":{"effort":"high"}}),
    )
    .await;
    assert!(response.isolates_account_state());
    let (wire, rebuilt) = response.into_buffered_async().await.unwrap();
    assert!(rebuilt.isolates_account_state());
    let wire = String::from_utf8(wire.to_vec()).unwrap();
    assert!(wire.contains("response.completed"));
    assert!(wire.contains("BPS OK"));
    assert!(!wire.contains("mock-bps-secret"));
    assert!(!wire.contains("heartbeat"));
    let frame = server.await.unwrap();
    assert_eq!(frame["model"], "test-model");
    assert_eq!(frame["reasoning_effort"], "high");
    assert_eq!(frame["type"], "response.create");
    assert_eq!(frame["model_selection"], "explicit");
    assert!(frame["basispoints_request_id"].as_str().unwrap().len() == 36);
    assert!(frame["basispoints_resume_token"]
        .as_str()
        .unwrap()
        .starts_with("resume_"));
}

async fn deliver(
    response: GatewayUpstreamResponse,
    adapter: ResponseAdapter,
    stream: bool,
) -> Bytes {
    let (parts, ()) = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(())
        .unwrap()
        .into_parts();
    let (request, receiver) =
        crate::http::gateway_request::GatewayRequest::new(parts, Bytes::from_static(b"{}"));
    let delivery = tokio::spawn(crate::gateway::http_bridge::respond_with_upstream_async(
        request,
        response,
        crate::gateway::acquire_account_inflight("bps-fixture-delivery"),
        adapter,
        None,
        None,
        "/v1/responses",
        None,
        stream,
        false,
        None,
        Some("test-model"),
        Instant::now(),
    ));
    let response = receiver.await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result = delivery.await.unwrap().unwrap();
    assert!(
        result.is_ok(stream),
        "BPS must satisfy existing downstream protocol delivery"
    );
    assert_eq!(result.usage.total_tokens, Some(7));
    body
}

#[tokio::test]
async fn bps_text_reuses_responses_and_chat_stream_and_json_delivery() {
    for (adapter, stream) in [
        (ResponseAdapter::Passthrough, true),
        (ResponseAdapter::Passthrough, false),
        (ResponseAdapter::ChatCompletionsFromResponses, true),
        (ResponseAdapter::ChatCompletionsFromResponses, false),
    ] {
        let (response, server) = fixture(
            vec![completed(text_output())],
            json!({"model":"test-model","input":"hello","stream":stream}),
        )
        .await;
        let body = deliver(response, adapter, stream).await;
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.contains("BPS OK"));
        if stream {
            assert!(text.contains("data:"));
        } else {
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert!(value.is_object());
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn bps_client_tools_reuse_responses_and_chat_stream_and_json_delivery() {
    for (adapter, stream) in [
        (ResponseAdapter::Passthrough, true),
        (ResponseAdapter::Passthrough, false),
        (ResponseAdapter::ChatCompletionsFromResponses, true),
        (ResponseAdapter::ChatCompletionsFromResponses, false),
    ] {
        let arguments = json!({"code":json!({"tool":"lookup","arguments":{"q":"hello"}}).to_string(),"summary":"lookup","extended_summary":"lookup","destructive":false,"references":[]});
        let output = json!([{"type":"function_call","id":"fc_bps_fixture","call_id":"call_bps_fixture","name":"run_officejs","arguments":arguments.to_string(),"status":"completed"}]);
        let (response, server) = fixture(vec![completed(output)], json!({"model":"test-model","input":"lookup hello","tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{"q":{"type":"string"}},"required":["q"]}}]})).await;
        let body = deliver(response, adapter, stream).await;
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.contains("lookup"));
        assert!(text.contains("call_bps_fixture"));
        assert!(text.contains(if adapter == ResponseAdapter::Passthrough {
            "function_call"
        } else {
            "tool_calls"
        }));
        assert!(!text.contains("run_officejs"));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn bps_truncated_websocket_fails_instead_of_succeeding_or_replaying() {
    let (response, server) = fixture(vec![json!({"type":"response.created","response":{"id":"resp_bps_fixture","status":"in_progress","output":[]}})], json!({"model":"test-model","input":"hello"})).await;
    let (wire, response) = response.into_buffered_async().await.unwrap();
    assert!(response.isolates_account_state());
    let text = std::str::from_utf8(&wire).unwrap();
    assert!(text.contains("response.failed"));
    assert!(!text.contains("response.completed"));
    server.await.unwrap();
}

#[tokio::test]
async fn bps_unsupported_operation_fails_before_connecting() {
    let response = execute(
        &reqwest::Method::POST,
        "/v1/responses/compact",
        request_context(),
        &crate::gateway::IncomingHeaderSnapshot::default(),
        &Bytes::from_static(b"{}"),
        "local",
        credentials(),
        None,
    )
    .await;
    assert_eq!(response.status().as_u16(), 400);
    assert!(response.isolates_account_state());
}

#[tokio::test]
async fn bps_http_proxy_tunnel_and_committed_send_are_preserved() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (endpoint, server) = mock_upstream(vec![completed(text_output())]).await;
    let target = url::Url::parse(&endpoint).unwrap();
    let target_addr = format!("{}:{}", target.host_str().unwrap(), target.port().unwrap());
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let proxy_task = tokio::spawn(async move {
        let (mut client, _) = proxy.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            client.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            assert!(request.len() < 8192);
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with(&format!("CONNECT {target_addr} HTTP/1.1")));
        assert!(!request.contains("mock-bps-secret"));
        let mut upstream = tokio::net::TcpStream::connect(&target_addr).await.unwrap();
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    });
    let (body, bridge) =
        bps_protocol::prepare(br#"{"model":"test-model","input":"hi"}"#, "proxy-test").unwrap();
    let guard = crate::http::gateway_replay_guard::UpstreamReplayGuard::default();
    let context = UpstreamRequestContext {
        replay_guard: Some(&guard),
        ..request_context()
    };
    let response = send_prepared(
        &endpoint,
        Some(&proxy_url),
        &credentials(),
        &body,
        bridge,
        Some(Instant::now() + Duration::from_secs(5)),
        context,
    )
    .await
    .unwrap();
    assert!(
        guard.is_blocked(),
        "a submitted BPS request cannot be replayed"
    );
    let (wire, _) = response.into_buffered_async().await.unwrap();
    assert!(std::str::from_utf8(&wire).unwrap().contains("BPS OK"));
    server.await.unwrap();
    proxy_task.await.unwrap();
}
