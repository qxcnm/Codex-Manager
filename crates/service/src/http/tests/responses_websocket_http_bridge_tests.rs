use super::*;
use axum::extract::State as AxumState;
use codexmanager_core::storage::{AggregateApi, ModelRouteV2};
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

type ClientSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
const PLATFORM_KEY: &str = "platform_key_ws_http_bridge";

struct Capture {
    method: String,
    path: String,
    body: Value,
}

struct MockState {
    protocol: &'static str,
    mode: &'static str,
    calls: AtomicUsize,
    dropped: Arc<AtomicUsize>,
    capture: tokio::sync::mpsc::UnboundedSender<Capture>,
}

fn sse(events: Vec<Value>) -> String {
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\rdata: {event}\r\r",
                event["type"].as_str().unwrap()
            )
        })
        .collect()
}

fn response_events(round: usize, tools: bool) -> Vec<Value> {
    let response_id = format!("resp_bridge_{round}");
    let item = if tools {
        json!({"id":"fc_bridge", "type":"function_call", "status":"completed", "call_id":"call_ws_lookup", "name":"lookup", "arguments":"{\"city\":\"上海\"}"})
    } else {
        json!({"id":format!("msg_bridge_{round}"), "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text","text":"桥接成功","annotations":[]}]})
    };
    let mut events = vec![
        json!({"type":"response.created", "response":{"id":response_id,"status":"in_progress"}}),
    ];
    if !tools {
        events.push(json!({"type":"response.output_text.delta", "item_id":format!("msg_bridge_{round}"), "output_index":0, "content_index":0, "delta":"桥接成功"}));
    }
    events.push(json!({"type":"response.output_item.done", "output_index":0,"item":item}));
    events.push(json!({"type":"response.completed", "response":{"id":response_id,"status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}));
    events
}

fn chat_sse(round: usize, tools: bool) -> String {
    let delta = if tools {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_ws_lookup","type":"function","function":{"name":"lookup","arguments":"{\"city\":\"上海\"}"}}]})
    } else {
        json!({"role":"assistant","content":"桥接成功"})
    };
    let mut output = String::new();
    for event in [
        json!({"id":format!("chatcmpl_bridge_{round}"),"object":"chat.completion.chunk","model":"mock-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":format!("chatcmpl_bridge_{round}"),"object":"chat.completion.chunk","model":"mock-model","choices":[{"index":0,"delta":{},"finish_reason":if tools {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}),
    ] {
        output.push_str(&format!("data: {event}\n\n"));
    }
    output.push_str("data: [DONE]\n\n");
    output
}

fn messages_sse(round: usize, tools: bool) -> String {
    let block = if tools {
        json!({"type":"tool_use","id":"call_ws_lookup","name":"lookup","input":{}})
    } else {
        json!({"type":"text","text":""})
    };
    let delta = if tools {
        json!({"type":"input_json_delta","partial_json":"{\"city\":\"上海\"}"})
    } else {
        json!({"type":"text_delta","text":"桥接成功"})
    };
    sse(vec![
        json!({"type":"message_start","message":{"id":format!("msg_anthropic_{round}"),"type":"message","role":"assistant","model":"mock-model","content":[],"usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":block}),
        json!({"type":"content_block_delta","index":0,"delta":delta}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":if tools {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ])
}

struct DropSignal(Arc<AtomicUsize>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async fn mock_http(
    AxumState(state): AxumState<Arc<MockState>>,
    request: HttpRequest<Body>,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 1024 * 1024).await.unwrap();
    let _ = state.capture.send(Capture {
        method: parts.method.to_string(),
        path: parts.uri.path().to_string(),
        body: serde_json::from_slice(&body).unwrap(),
    });
    let round = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    let tools = state.mode == "tools" && round == 1;
    let payload = if state.mode == "failed" {
        sse(vec![
            json!({"type":"response.failed","response":{"id":"resp_failed","status":"failed","error":{"code":"mock_failure","message":"mock failed"},"output":[]}}),
        ])
    } else if state.mode == "stall" || state.mode == "truncated" {
        sse(vec![
            json!({"type":"response.created","response":{"id":"resp_stall","status":"in_progress"}}),
            json!({"type":"response.output_text.delta","delta":"partial","output_index":0,"content_index":0}),
        ])
    } else {
        match state.protocol {
            "chat" => chat_sse(round, tools),
            "messages" => messages_sse(round, tools),
            _ => sse(response_events(round, tools)),
        }
    };
    let body = if state.mode == "stall" {
        let state = (
            Some(Bytes::from(payload)),
            DropSignal(state.dropped.clone()),
        );
        Body::from_stream(futures_util::stream::unfold(
            state,
            |(mut bytes, guard)| async move {
                match bytes.take() {
                    Some(bytes) => Some((Ok::<_, std::io::Error>(bytes), (None, guard))),
                    None => std::future::pending().await,
                }
            },
        ))
    } else {
        // Split UTF-8 and CR separators across transport chunks.
        let chunks = payload
            .as_bytes()
            .chunks(7)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>();
        Body::from_stream(futures_util::stream::iter(chunks))
    };
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(body)
        .unwrap()
}

async fn start_mock(
    protocol: &'static str,
    mode: &'static str,
) -> (
    String,
    Arc<MockState>,
    tokio::sync::mpsc::UnboundedReceiver<Capture>,
    tokio::task::JoinHandle<()>,
) {
    let (capture, captures) = tokio::sync::mpsc::unbounded_channel();
    let state = Arc::new(MockState {
        protocol,
        mode,
        calls: AtomicUsize::new(0),
        dropped: Arc::new(AtomicUsize::new(0)),
        capture,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let app = axum::Router::new()
        .fallback(mock_http)
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, state, captures, task)
}

fn seed_aggregate(storage: &Storage, addr: &str, protocol: &str, rotation: &str) {
    insert_api_key_record(storage, PLATFORM_KEY, rotation, None);
    let now = chrono::Utc::now().timestamp();
    storage
        .insert_aggregate_api(&AggregateApi {
            id: "agg_ws_bridge".into(),
            provider_type: if protocol == "messages" {
                "claude"
            } else {
                "codex"
            }
            .into(),
            supplier_name: Some("WS bridge test".into()),
            sort: 0,
            url: format!("http://{addr}"),
            auth_type: "apikey".into(),
            auth_params_json: None,
            action: Some(
                match protocol {
                    "chat" => "/v1/chat/completions",
                    "messages" => "/v1/messages",
                    _ => "/v1/responses",
                }
                .into(),
            ),
            model_override: None,
            user_agent: None,
            status: "active".into(),
            created_at: now,
            updated_at: now,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
            balance_query_enabled: false,
            balance_query_template: None,
            balance_query_base_url: None,
            balance_query_user_id: None,
            balance_query_config_json: None,
            last_balance_at: None,
            last_balance_status: None,
            last_balance_error: None,
            last_balance_json: None,
        })
        .unwrap();
    storage
        .upsert_aggregate_api_secret("agg_ws_bridge", "mock-source-secret")
        .unwrap();
    let mut model = storage.get_managed_model_v2("gpt-6-luna").unwrap().unwrap();
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "aggregate_api".into(),
        source_id: "agg_ws_bridge".into(),
        upstream_model: "mock-model".into(),
        enabled: true,
        priority: 0,
        weight: 1,
    }];
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .unwrap();
}

async fn connect_bridge(
    storage: &Storage,
    addr: &str,
    protocol: &str,
    rotation: &str,
) -> (
    ClientSocket,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    connect_bridge_with_limits(storage, addr, protocol, rotation, None).await
}

async fn connect_bridge_with_limits(
    storage: &Storage,
    addr: &str,
    protocol: &str,
    rotation: &str,
    limits: Option<crate::http::middleware::RequestLimits>,
) -> (
    ClientSocket,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    seed_aggregate(storage, addr, protocol, rotation);
    tokio::task::spawn_blocking(crate::gateway::reload_runtime_config_from_env)
        .await
        .unwrap();
    let state = ProxyState {
        backend_base_url: String::new(),
        client: Client::new(),
    };
    let (front, shutdown, server) = match limits {
        None => start_front_proxy_test_server(state).await,
        Some(limits) => {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let front = listener.local_addr().unwrap().to_string();
            let app = crate::http::proxy_runtime::build_front_proxy_app_with_limits(state, limits);
            let (shutdown, receiver) = oneshot::channel();
            let server = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = receiver.await;
                    })
                    .await
                    .unwrap();
            });
            (front, shutdown, server)
        }
    };
    let (socket, response) = connect_async(build_ws_request(
        &format!("ws://{front}/v1/responses"),
        PLATFORM_KEY,
        &[],
    ))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    (socket, shutdown, server)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_obeys_shared_stream_admission() {
    let _guard = crate::test_env_guard();
    let db = new_test_db_path("codexmanager-ws-http-admission");
    let storage = init_test_storage(&db);
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, mock, _, upstream) = start_mock("responses", "text").await;
    let limits = crate::http::middleware::RequestLimits::new(1, 1);
    let held = limits.try_acquire(true).unwrap();
    let (mut socket, signal, server) = connect_bridge_with_limits(
        &storage,
        &addr,
        "responses",
        crate::apikey_profile::ROTATION_AGGREGATE_API,
        Some(limits),
    )
    .await;
    for generate in [false, true] {
        send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","input":"capacity test","generate":generate})).await;
        let event = next_event(&mut socket).await;
        assert_eq!(event["type"], "error");
        assert_eq!(event["status"], 503);
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    }
    drop(held);
    send(
        &mut socket,
        json!({"type":"response.create","model":"gpt-6-luna","input":"capacity available"}),
    )
    .await;
    assert_eq!(terminal(&mut socket).await["type"], "response.completed");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    shutdown(socket, signal, server, upstream).await;
}

async fn send(socket: &mut ClientSocket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn next_event(socket: &mut ClientSocket) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("WS event timeout")
            .expect("WS open")
            .expect("WS frame");
        match message {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(bytes) => {
                socket.send(Message::Pong(bytes)).await.unwrap();
            }
            Message::Pong(_) => {}
            other => panic!("unexpected WS event {other:?}"),
        }
    }
}

async fn terminal(socket: &mut ClientSocket) -> Value {
    loop {
        let event = next_event(socket).await;
        assert_ne!(event["type"], "error", "bridge error: {event}");
        if matches!(
            event["type"].as_str(),
            Some("response.completed" | "response.failed" | "response.incomplete")
        ) {
            return event;
        }
    }
}

async fn shutdown(
    mut socket: ClientSocket,
    signal: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
    upstream: tokio::task::JoinHandle<()>,
) {
    socket.close(None).await.unwrap();
    let _ = signal.send(());
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
    crate::http::gateway_endpoint::drain_domain_workers().await;
    upstream.abort();
    let _ = upstream.await;
}

async fn tools_and_history_case(protocol: &'static str, rotation: &str) {
    let _guard = crate::test_env_guard();
    eprintln!("WS bridge matrix: protocol={protocol}, rotation={rotation}");
    let db = new_test_db_path(&format!("codexmanager-ws-http-tools-{protocol}"));
    let storage = init_test_storage(&db);
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, mock, mut captures, upstream) = start_mock(protocol, "tools").await;
    let (mut socket, signal, server) = connect_bridge(&storage, &addr, protocol, rotation).await;
    send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","generate":false,"input":"warmup context"})).await;
    let warmup = terminal(&mut socket).await;
    assert_eq!(warmup["response"]["usage"]["total_tokens"], 0);
    assert_eq!(
        mock.calls.load(Ordering::SeqCst),
        0,
        "warmup must never call upstream"
    );
    send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","stream":false,"previous_response_id":warmup["response"]["id"],"input":"look up the city","tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}]})).await;
    let first = terminal(&mut socket).await;
    assert_eq!(first["type"], "response.completed");
    let tool = first["response"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("tool call survives conversion");
    assert_eq!(tool["call_id"], "call_ws_lookup");
    assert_eq!(
        serde_json::from_str::<Value>(tool["arguments"].as_str().unwrap()).unwrap(),
        json!({"city":"上海"})
    );
    let first_capture = captures.recv().await.unwrap();
    assert_eq!(first_capture.method, "POST");
    assert!(first_capture.body.to_string().contains("warmup context"));
    assert_eq!(first_capture.body["stream"], true);
    assert!(first_capture.body.get("previous_response_id").is_none());
    assert_eq!(
        first_capture.path,
        match protocol {
            "chat" => "/v1/chat/completions",
            "messages" => "/v1/messages",
            _ => "/v1/responses",
        }
    );
    send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","previous_response_id":first["response"]["id"],"input":[{"type":"function_call_output","call_id":"call_ws_lookup","output":"上海天气晴朗"}]})).await;
    let second = terminal(&mut socket).await;
    assert_eq!(second["type"], "response.completed");
    assert!(second.to_string().contains("桥接成功"));
    let followup = captures.recv().await.unwrap();
    assert_eq!(followup.method, "POST");
    for value in [
        "warmup context",
        "look up the city",
        "call_ws_lookup",
        "上海天气晴朗",
    ] {
        assert!(
            followup.body.to_string().contains(value),
            "missing {value} in {protocol} follow-up"
        );
    }
    assert_eq!(mock.calls.load(Ordering::SeqCst), 2);
    shutdown(socket, signal, server, upstream).await;
    for _ in 0..100 {
        if storage.list_request_logs(None, 10).unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let logs = storage.list_request_logs(None, 10).unwrap();
    assert_eq!(logs.len(), 2, "warmup creates no upstream charge");
    assert!(logs.iter().all(|log| log.status_code == Some(200)
        && log.request_type.as_deref() == Some("ws")
        && log.method == "POST"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_responses_preserves_warmup_tools_and_history() {
    tools_and_history_case("responses", crate::apikey_profile::ROTATION_AGGREGATE_API).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_chat_preserves_warmup_tools_and_history() {
    tools_and_history_case("chat", crate::apikey_profile::ROTATION_HYBRID).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_messages_preserves_warmup_tools_and_history() {
    tools_and_history_case(
        "messages",
        crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_cancel_drops_upstream_and_does_not_replay() {
    let _guard = crate::test_env_guard();
    let db = new_test_db_path("codexmanager-ws-http-cancel");
    let storage = init_test_storage(&db);
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, mock, mut captures, upstream) = start_mock("responses", "stall").await;
    let (mut socket, signal, server) = connect_bridge(
        &storage,
        &addr,
        "responses",
        crate::apikey_profile::ROTATION_AGGREGATE_API,
    )
    .await;
    send(
        &mut socket,
        json!({"type":"response.create","model":"gpt-6-luna","input":"cancel this turn"}),
    )
    .await;
    while next_event(&mut socket).await["type"] != "response.output_text.delta" {}
    send(
        &mut socket,
        json!({"type":"response.create","model":"gpt-6-luna","input":"overlap"}),
    )
    .await;
    let error = next_event(&mut socket).await;
    assert_eq!(error["error"]["code"], "response_in_flight");
    send(&mut socket, json!({"type":"response.interrupt"})).await;
    assert_eq!(
        next_event(&mut socket).await["type"],
        "response.interrupt.accepted"
    );
    let interrupted = terminal(&mut socket).await;
    assert_eq!(interrupted["type"], "response.incomplete");
    assert_eq!(
        interrupted["response"]["incomplete_details"]["reason"],
        "interrupted"
    );
    for _ in 0..100 {
        if mock.dropped.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        mock.dropped.load(Ordering::SeqCst) > 0,
        "cancel must release upstream HTTP body"
    );
    assert_eq!(
        mock.calls.load(Ordering::SeqCst),
        1,
        "cancellation cannot replay"
    );
    assert!(captures.try_recv().is_ok());
    send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","generate":false,"previous_response_id":interrupted["response"]["id"],"input":"safe next turn"})).await;
    assert_eq!(terminal(&mut socket).await["type"], "response.completed");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    shutdown(socket, signal, server, upstream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_revalidates_key_and_rejects_unknown_history() {
    let _guard = crate::test_env_guard();
    let db = new_test_db_path("codexmanager-ws-http-key");
    let storage = init_test_storage(&db);
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, mock, _, upstream) = start_mock("responses", "text").await;
    let (mut socket, signal, server) = connect_bridge(
        &storage,
        &addr,
        "responses",
        crate::apikey_profile::ROTATION_AGGREGATE_API,
    )
    .await;
    send(&mut socket,json!({"type":"response.create","model":"gpt-6-luna","previous_response_id":"not-owned-by-this-session","input":"hi"})).await;
    assert_eq!(
        next_event(&mut socket).await["error"]["code"],
        "responses_websocket_context_rebase_failed"
    );
    storage
        .update_api_key_status("gk_proxy_runtime_ws", "disabled")
        .unwrap();
    send(
        &mut socket,
        json!({"type":"response.create","model":"gpt-6-luna","generate":false,"input":"hi"}),
    )
    .await;
    assert_eq!(next_event(&mut socket).await["status"], 403);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    shutdown(socket, signal, server, upstream).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_http_bridge_reports_failed_and_truncated_streams() {
    let _guard = crate::test_env_guard();
    for mode in ["failed", "truncated"] {
        let db = new_test_db_path("codexmanager-ws-http-error");
        let storage = init_test_storage(&db);
        let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
        let (addr, mock, _, upstream) = start_mock("responses", mode).await;
        let (mut socket, signal, server) = connect_bridge(
            &storage,
            &addr,
            "responses",
            crate::apikey_profile::ROTATION_AGGREGATE_API,
        )
        .await;
        send(
            &mut socket,
            json!({"type":"response.create","model":"gpt-6-luna","input":"trigger failure"}),
        )
        .await;
        loop {
            let event = next_event(&mut socket).await;
            assert_ne!(
                event["type"], "response.completed",
                "failure cannot become success"
            );
            if event["type"] == "error" || event["type"] == "response.failed" {
                break;
            }
        }
        assert_eq!(
            mock.calls.load(Ordering::SeqCst),
            1,
            "visible output/failure must not replay"
        );
        shutdown(socket, signal, server, upstream).await;
    }
}
