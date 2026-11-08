//! Responses WebSocket frontend for routes whose upstream uses HTTP/SSE.
//!
//! Every generated turn enters the ordinary HTTP gateway. This module never
//! selects an upstream, replays a submitted request, or performs billing itself.
use super::*;
use axum::extract::ConnectInfo;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use std::net::SocketAddr;
use tokio::sync::mpsc;

const BRIDGE_EVENT_BUFFER: usize = 1;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_RETAINED_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_RETAINED_OUTPUT_ITEMS: usize = 256;

#[derive(Clone)]
pub(super) struct BridgeContext {
    headers: HeaderMap,
    peer: Option<ConnectInfo<SocketAddr>>,
    limits: crate::http::middleware::RequestLimits,
}

impl BridgeContext {
    pub(super) fn from_parts(parts: &axum::http::request::Parts) -> Self {
        let headers = parts
            .headers
            .iter()
            .filter(|(name, value)| {
                !crate::http::header_filter::should_skip_request_header(name, value)
                    && !name.as_str().starts_with("sec-websocket-")
                    && *name != header::CONTENT_ENCODING
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Self {
            headers,
            peer: parts.extensions.get::<ConnectInfo<SocketAddr>>().copied(),
            limits: crate::http::middleware::RequestLimits::from_extensions(&parts.extensions),
        }
    }

    fn request_parts(
        &self,
        delivery_gate: crate::http::gateway_request::WebSocketDeliveryGate,
    ) -> axum::http::request::Parts {
        let mut request = HttpRequest::builder()
            .method("POST")
            .uri(RESPONSES_ENDPOINT)
            .body(())
            .expect("constant Responses bridge request");
        *request.headers_mut() = self.headers.clone();
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        request.headers_mut().insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        if let Some(peer) = self.peer {
            request.extensions_mut().insert(peer);
        }
        request
            .extensions_mut()
            .insert(crate::http::gateway_request::WebSocketHttpBridge);
        request.extensions_mut().insert(delivery_gate);
        request.into_parts().0
    }
}

struct PreparedBridgeTurn {
    body: Bytes,
    input: Value,
    previous_response_id: Option<String>,
    warmup: bool,
    model: Option<String>,
}

fn prepare_turn(
    text: &str,
    payload: Value,
    completed: &CompletedWsResponseCache,
) -> Result<PreparedBridgeTurn, WsSessionError> {
    for name in ["generate", "stream"] {
        if payload.get(name).is_some_and(|value| !value.is_boolean()) {
            return Err(WsSessionError::bad_request(format!(
                "{name} must be a boolean"
            )));
        }
    }
    if payload
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err(WsSessionError::bad_request(
            "previous_response_id must be a string or null",
        ));
    }
    let input = payload
        .get("input")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let previous_response_id = payload
        .get("previous_response_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let expanded = expand_response_create_previous_response(text, completed)
        .map_err(|error| WsSessionError::context_rebase_failed(error))?;
    let mut request = match expanded {
        Some(expanded) => serde_json::from_str::<Value>(&expanded)
            .map_err(|error| WsSessionError::bad_request(error.to_string()))?,
        None => payload,
    };
    let object = request
        .as_object_mut()
        .ok_or_else(|| WsSessionError::bad_request("response.create must be an object"))?;
    let warmup = object.remove("generate") == Some(Value::Bool(false));
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    object.remove("type");
    // A WebSocket always delivers response events, including when a client sends
    // stream:false. HTTP adapters retain their ordinary streaming conversion.
    object.insert("stream".to_string(), Value::Bool(true));
    if let Some(input) = object.get_mut("input") {
        normalize_ws_tool_call_outputs(input);
    }
    let body = serde_json::to_vec(&request)
        .map_err(|error| WsSessionError::bad_request(error.to_string()))?;
    let limit = crate::gateway::front_proxy_max_body_bytes();
    if body.len() > RESPONSES_WS_MAX_MESSAGE_BYTES || (limit > 0 && body.len() > limit) {
        return Err(WsSessionError::new(
            413,
            "request_body_too_large",
            "Responses WebSocket request exceeds the gateway request body limit",
        ));
    }
    Ok(PreparedBridgeTurn {
        body: body.into(),
        input,
        previous_response_id,
        warmup,
        model,
    })
}

struct ActiveTurn {
    events: mpsc::Receiver<Result<Value, WsSessionError>>,
    worker: tokio::task::JoinHandle<()>,
    delivery_confirmation: tokio::sync::watch::Sender<bool>,
    input: Value,
    previous_response_id: Option<String>,
    response_id: String,
    completed_output: Vec<Value>,
    completed_output_bytes: usize,
    output_cache_overflow: bool,
    sequence_number: u64,
}

impl Drop for ActiveTurn {
    fn drop(&mut self) {
        // Aborting drops either the header wait (cancellation guard) or the HTTP
        // body. Both paths cancel routing/delivery through the existing gateway.
        self.worker.abort();
    }
}

impl ActiveTurn {
    fn start(turn: PreparedBridgeTurn, context: BridgeContext) -> Self {
        let (events_tx, events) = mpsc::channel(BRIDGE_EVENT_BUFFER);
        let (delivery_confirmation, delivery_gate) =
            crate::http::gateway_request::WebSocketDeliveryGate::new();
        let worker = tokio::spawn(async move {
            if let Err(error) =
                stream_http_response(turn.body, context, delivery_gate, &events_tx).await
            {
                let _ = events_tx.send(Err(error)).await;
            }
        });
        Self {
            events,
            worker,
            delivery_confirmation,
            input: turn.input,
            previous_response_id: turn.previous_response_id,
            response_id: format!("resp_cm_{}", crate::gateway::next_trace_id()),
            completed_output: Vec::new(),
            completed_output_bytes: 0,
            output_cache_overflow: false,
            sequence_number: 0,
        }
    }

    fn observe(&mut self, event: &Value) {
        if let Some(id) = event.pointer("/response/id").and_then(Value::as_str) {
            self.response_id = id.to_string();
        }
        if let Some(sequence) = event.get("sequence_number").and_then(Value::as_u64) {
            self.sequence_number = self.sequence_number.max(sequence.saturating_add(1));
        }
        if event.get("type").and_then(Value::as_str) == Some("response.output_item.done") {
            if let Some(item) = event.get("item") {
                if responses_websocket_control::is_retained_interrupted_output(item) {
                    if !self.output_cache_overflow {
                        let bytes = serde_json::to_vec(item).map_or(usize::MAX, |body| body.len());
                        if self.completed_output.len() >= MAX_RETAINED_OUTPUT_ITEMS
                            || self.completed_output_bytes.saturating_add(bytes)
                                > MAX_RETAINED_OUTPUT_BYTES
                        {
                            self.output_cache_overflow = true;
                            self.completed_output.clear();
                            self.completed_output_bytes = 0;
                        } else {
                            self.completed_output_bytes += bytes;
                            self.completed_output.push(item.clone());
                        }
                    }
                }
            }
        }
    }
}

async fn stream_http_response(
    body: Bytes,
    context: BridgeContext,
    delivery_gate: crate::http::gateway_request::WebSocketDeliveryGate,
    events: &mpsc::Sender<Result<Value, WsSessionError>>,
) -> Result<(), WsSessionError> {
    let _permit = context.limits.try_acquire(true).map_err(|response| {
        WsSessionError::new(response.status().as_u16(), "service_busy", "service busy")
    })?;
    let response = crate::http::gateway_endpoint::handle_gateway_http(
        context.request_parts(delivery_gate),
        body,
    )
    .await;
    let status = response.status();
    if !status.is_success() {
        let body = axum::body::to_bytes(response.into_body(), MAX_ERROR_BODY_BYTES)
            .await
            .map_err(|error| {
                WsSessionError::new(status.as_u16(), "gateway_error", error.to_string())
            })?;
        let value = serde_json::from_slice::<Value>(&body).ok();
        let message = value
            .as_ref()
            .and_then(|value| {
                value
                    .pointer("/error/message")
                    .or_else(|| value.get("message"))
            })
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        let code = value
            .as_ref()
            .and_then(|value| value.pointer("/error/code").or_else(|| value.get("code")))
            .and_then(Value::as_str)
            .unwrap_or("gateway_error");
        return Err(WsSessionError::new(status.as_u16(), code, message));
    }
    let is_sse = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"));
    if !is_sse {
        return Err(WsSessionError::bad_gateway(
            "Responses HTTP gateway returned a non-streaming response to a streaming request",
        ));
    }
    // Reuse the SSE decoder used by the gateway's protocol regression tests;
    // splitting UTF-8, CRLF, lone CR and multiple data lines is not WS framing.
    let mut after_cr = false;
    let bytes = response
        .into_body()
        .into_data_stream()
        .map(move |chunk| chunk.map(|bytes| normalize_sse_line_endings(bytes, &mut after_cr)));
    let mut stream = bytes.eventsource();
    while let Some(event) = stream.next().await {
        let event = event.map_err(|error| WsSessionError::bad_gateway(error.to_string()))?;
        if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
            continue;
        }
        let mut value = serde_json::from_str::<Value>(&event.data).map_err(|error| {
            WsSessionError::bad_gateway(format!("invalid Responses stream event: {error}"))
        })?;
        let object = value.as_object_mut().ok_or_else(|| {
            WsSessionError::bad_gateway("Responses stream event must be a JSON object")
        })?;
        if !object.contains_key("type") && !event.event.is_empty() && event.event != "message" {
            object.insert("type".to_string(), Value::String(event.event));
        }
        normalize_terminal_event(&mut value);
        let terminal = is_terminal_event(&value);
        if events.send(Ok(value)).await.is_err() {
            return Ok(());
        }
        if terminal {
            return Ok(());
        }
    }
    Err(WsSessionError::bad_gateway(
        "Responses stream closed before a terminal event; the request was not replayed",
    ))
}

fn normalize_sse_line_endings(bytes: Bytes, after_cr: &mut bool) -> Bytes {
    if !*after_cr && !bytes.contains(&b'\r') {
        return bytes;
    }
    // eventsource-stream uses streaming CRLF lookahead and otherwise waits for
    // another byte after a lone final CR. Normalize line endings before decoding
    // so a terminal frame is delivered while the HTTP connection remains open.
    let mut normalized = Vec::with_capacity(bytes.len());
    for byte in bytes {
        let skip_lf = *after_cr && byte == b'\n';
        *after_cr = byte == b'\r';
        if !skip_lf {
            normalized.push(if *after_cr { b'\n' } else { byte });
        }
    }
    normalized.into()
}

fn is_terminal_event(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some("response.completed" | "response.failed" | "response.incomplete" | "error")
    )
}

fn normalize_terminal_event(event: &mut Value) {
    if !matches!(
        event.get("type").and_then(Value::as_str),
        Some("response.done" | "response.completed")
    ) {
        return;
    }
    let status = event.pointer("/response/status").and_then(Value::as_str);
    let has_error = [
        "/error",
        "/response/error",
        "/response/status_details/error",
    ]
    .iter()
    .any(|path| event.pointer(path).is_some_and(|error| !error.is_null()));
    let kind = if has_error
        || status.is_some_and(|status| !matches!(status, "completed" | "incomplete"))
    {
        "response.failed"
    } else if status == Some("incomplete") {
        "response.incomplete"
    } else {
        "response.completed"
    };
    event["type"] = Value::String(kind.to_string());
}

fn can_remember_terminal(event: &Value) -> bool {
    is_interrupted_response(event)
        || (event.get("type").and_then(Value::as_str) == Some("response.completed")
            && event
                .pointer("/response/status")
                .is_none_or(|status| status.is_null() || status.as_str() == Some("completed"))
            && [
                "/error",
                "/response/error",
                "/response/status_details/error",
            ]
            .iter()
            .all(|path| event.pointer(path).is_none_or(Value::is_null)))
}

fn remember_terminal(
    completed: &mut CompletedWsResponseCache,
    event: &Value,
    input: &Value,
    previous_response_id: Option<&str>,
) {
    if !can_remember_terminal(event) {
        return;
    }
    if let Err(error) =
        completed.observe_completed_response(&event.to_string(), previous_response_id, input)
    {
        log::warn!("event=responses_ws_http_bridge_history_unavailable error={error}");
    }
}

async fn send_event(socket: &mut WebSocket, event: &Value) -> bool {
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .is_ok()
}

enum SessionEvent {
    Client(Option<Result<Message, axum::Error>>),
    Upstream(Option<Result<Value, WsSessionError>>),
    Heartbeat,
}

pub(super) async fn run_session(
    mut socket: WebSocket,
    context: WsRequestContext,
    bridge: BridgeContext,
) {
    let mut completed = CompletedWsResponseCache::default();
    let mut active: Option<ActiveTurn> = None;
    let mut heartbeat = responses_ws_heartbeat_interval();
    loop {
        let event = tokio::select! {
            // Client cancellation wins over a buffered upstream event. Dropping
            // ActiveTurn also discards queued output and prevents any late replay.
            biased;
            message = socket.recv() => SessionEvent::Client(message),
            event = async {
                match active.as_mut() {
                    Some(turn) => turn.events.recv().await,
                    None => std::future::pending().await,
                }
            } => SessionEvent::Upstream(event),
            _ = heartbeat.tick() => SessionEvent::Heartbeat,
        };
        match event {
            SessionEvent::Heartbeat => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            SessionEvent::Client(Some(Ok(Message::Text(text)))) => {
                let message = match parse_client_message(text.as_str()) {
                    Ok(message) => message,
                    Err(error) => {
                        send_ws_error(&mut socket, error, context.prefer_raw_errors).await;
                        continue;
                    }
                };
                match message {
                    ClientMessage::Interrupt => {
                        let Some(turn) = active.take() else { continue };
                        turn.worker.abort();
                        let accepted = json!({"type":"response.interrupt.accepted", "response_id":turn.response_id, "sequence_number":turn.sequence_number});
                        let terminal = json!({
                            "type":"response.incomplete", "sequence_number":turn.sequence_number.saturating_add(1),
                            "response":{"id":turn.response_id, "object":"response", "status":"incomplete",
                                "output":turn.completed_output, "incomplete_details":{"reason":"interrupted"}}
                        });
                        // Only completed items already delivered to this socket
                        // survive cancellation; unobserved/partial calls never do.
                        if !turn.output_cache_overflow {
                            remember_terminal(
                                &mut completed,
                                &terminal,
                                &turn.input,
                                turn.previous_response_id.as_deref(),
                            );
                        }
                        if !send_event(&mut socket, &accepted).await
                            || !send_event(&mut socket, &terminal).await
                        {
                            return;
                        }
                        if turn.output_cache_overflow {
                            send_ws_error(
                                &mut socket,
                                WsSessionError::context_rebase_failed(
                                    "interruption succeeded, but completed output exceeded the session recovery cache limit; send the full context on the next turn",
                                ),
                                context.prefer_raw_errors,
                            ).await;
                        }
                    }
                    ClientMessage::Create(payload) => {
                        if active.is_some() {
                            send_ws_error(
                                &mut socket,
                                WsSessionError::new(
                                    409,
                                    RESPONSES_WS_REQUEST_IN_FLIGHT_CODE,
                                    "a response is already in flight",
                                ),
                                context.prefer_raw_errors,
                            )
                            .await;
                            continue;
                        }
                        if let Err(error) = validate_ws_api_key_for_new_request(&context).await {
                            send_ws_error(&mut socket, error, context.prefer_raw_errors).await;
                            continue;
                        }
                        let turn = match prepare_turn(text.as_str(), payload, &completed) {
                            Ok(turn) => turn,
                            Err(error) => {
                                send_ws_error(&mut socket, error, context.prefer_raw_errors).await;
                                continue;
                            }
                        };
                        if turn.warmup {
                            let _permit = match bridge.limits.try_acquire(true) {
                                Ok(permit) => permit,
                                Err(response) => {
                                    send_ws_error(
                                        &mut socket,
                                        WsSessionError::new(
                                            response.status().as_u16(),
                                            "service_busy",
                                            "service busy",
                                        ),
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    continue;
                                }
                            };
                            let response_id =
                                format!("resp_cm_warmup_{}", crate::gateway::next_trace_id());
                            let response = json!({"id":response_id, "object":"response", "model":turn.model, "status":"completed", "output":[], "usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}});
                            let mut created_response = response.clone();
                            created_response["status"] = Value::String("in_progress".to_string());
                            let created = json!({"type":"response.created", "sequence_number":0, "response":created_response});
                            let terminal = json!({"type":"response.completed", "sequence_number":1, "response":response});
                            remember_terminal(
                                &mut completed,
                                &terminal,
                                &turn.input,
                                turn.previous_response_id.as_deref(),
                            );
                            if !send_event(&mut socket, &created).await
                                || !send_event(&mut socket, &terminal).await
                            {
                                return;
                            }
                        } else {
                            active = Some(ActiveTurn::start(turn, bridge.clone()));
                        }
                    }
                }
            }
            SessionEvent::Client(Some(Ok(Message::Ping(payload)))) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    return;
                }
            }
            SessionEvent::Client(Some(Ok(Message::Pong(_)))) => {}
            SessionEvent::Client(Some(Ok(Message::Close(_)))) => {
                acknowledge_client_close(&mut socket).await;
                return;
            }
            SessionEvent::Client(Some(Ok(Message::Binary(_)))) => {
                send_ws_error(
                    &mut socket,
                    WsSessionError::bad_request("expected a response.create text frame"),
                    context.prefer_raw_errors,
                )
                .await;
            }
            SessionEvent::Client(None | Some(Err(_))) => return,
            SessionEvent::Upstream(Some(Ok(mut event))) => {
                let Some(turn) = active.as_mut() else {
                    continue;
                };
                turn.observe(&event);
                let terminal = is_terminal_event(&event);
                if terminal {
                    if let Some(response) = event.get_mut("response").and_then(Value::as_object_mut)
                    {
                        response
                            .entry("output")
                            .or_insert_with(|| Value::Array(turn.completed_output.clone()));
                    }
                }
                if !send_event(&mut socket, &event).await {
                    return;
                }
                if terminal {
                    let turn = active.take().unwrap();
                    // Only this successful terminal socket send confirms the
                    // HTTP bridge delivery. Drop/interrupt never confirm it.
                    turn.delivery_confirmation.send_replace(true);
                    if !turn.output_cache_overflow {
                        remember_terminal(
                            &mut completed,
                            &event,
                            &turn.input,
                            turn.previous_response_id.as_deref(),
                        );
                    }
                }
            }
            SessionEvent::Upstream(Some(Err(error))) => {
                active.take();
                send_ws_error(&mut socket, error, context.prefer_raw_errors).await;
            }
            SessionEvent::Upstream(None) => {
                active.take();
                send_ws_error(
                    &mut socket,
                    WsSessionError::bad_gateway(
                        "Responses gateway worker stopped before completing the turn",
                    ),
                    context.prefer_raw_errors,
                )
                .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn responses_websocket_http_bridge_decodes_split_cr_and_utf8_without_eof() {
        for ending in ["\r", "\r\n", "\n"] {
            let payload = format!("event: response.completed{ending}data: {{\"type\":\"response.completed\",\"text\":\"完成\"}}{ending}{ending}");
            let chunks = payload
                .bytes()
                .map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte])))
                .collect::<Vec<_>>();
            let mut after_cr = false;
            let mut events = futures_util::stream::iter(chunks)
                .chain(futures_util::stream::pending())
                .map(move |chunk| {
                    chunk.map(|bytes| normalize_sse_line_endings(bytes, &mut after_cr))
                })
                .eventsource();
            let event = tokio::time::timeout(Duration::from_secs(1), events.next())
                .await
                .expect("terminal must not wait for EOF")
                .unwrap()
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&event.data).unwrap()["text"],
                "完成"
            );
        }
    }

    #[test]
    fn responses_websocket_http_bridge_never_caches_failed_completions() {
        for field in [
            json!({"status":"failed"}),
            json!({"status":"incomplete"}),
            json!({"status":"completed","error":{"code":"bad"}}),
        ] {
            let mut event =
                json!({"type":"response.completed","response":{"id":"resp_bad","output":[]}});
            event["response"]
                .as_object_mut()
                .unwrap()
                .extend(field.as_object().unwrap().clone());
            assert!(!can_remember_terminal(&event));
            let mut cache = CompletedWsResponseCache::default();
            remember_terminal(&mut cache, &event, &json!("input"), None);
            assert!(!cache.contains("resp_bad"));
            normalize_terminal_event(&mut event);
            assert_ne!(event["type"], "response.completed");
        }
        let mut done = json!({"type":"response.done","response":{"id":"resp_done","status":"completed","output":[]}});
        normalize_terminal_event(&mut done);
        assert_eq!(done["type"], "response.completed");
        assert!(can_remember_terminal(&done));
    }

    #[tokio::test]
    async fn responses_websocket_http_bridge_bounds_interrupted_output_history() {
        let turn = PreparedBridgeTurn {
            body: Bytes::new(),
            input: Value::Null,
            previous_response_id: None,
            warmup: false,
            model: None,
        };
        let (_, events) = mpsc::channel(BRIDGE_EVENT_BUFFER);
        let mut active = ActiveTurn {
            events,
            worker: tokio::spawn(std::future::pending()),
            delivery_confirmation: crate::http::gateway_request::WebSocketDeliveryGate::new().0,
            input: turn.input,
            previous_response_id: None,
            response_id: "resp_limit".into(),
            completed_output: vec![],
            completed_output_bytes: 0,
            output_cache_overflow: false,
            sequence_number: 0,
        };
        for index in 0..=MAX_RETAINED_OUTPUT_ITEMS {
            active.observe(&json!({"type":"response.output_item.done","sequence_number":u64::MAX,"item":{"id":format!("fc_{index}"),"type":"function_call","status":"completed","call_id":format!("call_{index}"),"name":"lookup","arguments":"{}"}}));
        }
        assert!(active.output_cache_overflow);
        assert!(active.completed_output.is_empty());
        assert_eq!(active.sequence_number.saturating_add(1), u64::MAX);
    }
}
