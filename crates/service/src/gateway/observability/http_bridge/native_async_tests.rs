use super::*;
use axum::body::Body;
use bytes::Bytes;
use futures_util::StreamExt;
use std::time::{Duration, Instant};

const COMPLETED_USAGE: &[u8] = b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_async\",\"status\":\"completed\",\"usage\":{\"input_tokens\":11,\"output_tokens\":7,\"total_tokens\":18},\"output\":[]}}\n\n";

fn request() -> (
    Request,
    tokio::sync::oneshot::Receiver<axum::http::Response<Body>>,
) {
    let (parts, ()) = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(())
        .unwrap()
        .into_parts();
    Request::new(parts, Bytes::from_static(b"{}"))
}

fn upstream(body: crate::gateway::upstream::GatewayByteStream) -> GatewayUpstreamResponse {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("content-type", "text/event-stream".parse().unwrap());
    GatewayUpstreamResponse::Stream(crate::gateway::upstream::GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        body,
    ))
}

type NativeFixtureDelivery = tokio::task::JoinHandle<Result<UpstreamResponseBridgeResult, String>>;

async fn start_native_fixture(
    wire: Bytes,
    end_upstream: bool,
) -> (
    tokio::sync::mpsc::Sender<crate::gateway::upstream::GatewayByteStreamItem>,
    axum::http::Response<Body>,
    NativeFixtureDelivery,
) {
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    let (request, receiver) = request();
    let (provider, input) = tokio::sync::mpsc::channel(2);
    provider
        .try_send(GatewayByteStreamItem::Chunk(wire))
        .unwrap();
    if end_upstream {
        provider.try_send(GatewayByteStreamItem::Eof).unwrap();
    }
    let delivery = tokio::spawn(respond_with_upstream_async(
        request,
        upstream(GatewayByteStream::from_receiver(input)),
        crate::gateway::acquire_account_inflight("native-async-missing-item-done"),
        crate::gateway::ResponseAdapter::Passthrough,
        None,
        None,
        "/v1/responses",
        None,
        true,
        false,
        None,
        None,
        Instant::now(),
    ));
    let response = tokio::time::timeout(Duration::from_secs(2), receiver)
        .await
        .expect("native fixture response headers arrive")
        .unwrap();
    (provider, response, delivery)
}

async fn finish_native_fixture(delivery: NativeFixtureDelivery) -> UpstreamResponseBridgeResult {
    tokio::time::timeout(Duration::from_secs(2), delivery)
        .await
        .expect("native fixture delivery finishes")
        .unwrap()
        .unwrap()
}

async fn collect_native_fixture(wire: String) -> (Bytes, UpstreamResponseBridgeResult) {
    let (_provider, response, delivery) = start_native_fixture(Bytes::from(wire), true).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = tokio::time::timeout(
        Duration::from_secs(2),
        axum::body::to_bytes(response.into_body(), usize::MAX),
    )
    .await
    .expect("native fixture body finishes")
    .unwrap();
    (body, finish_native_fixture(delivery).await)
}

fn assistant_message(id: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "message", "id": id, "role": "assistant", "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}]
    })
}

fn completed_output_frame(output: Vec<serde_json::Value>) -> String {
    let event = serde_json::json!({
        "type": "response.completed", "sequence_number": 17,
        "response": {
            "id": "resp_missing_done", "status": "completed", "output": output,
            "usage": {"input_tokens": 11, "output_tokens": 7, "total_tokens": 18}
        }
    });
    format!("event: response.completed\ndata: {event}\n\n")
}

fn native_json_events(body: &[u8]) -> Vec<serde_json::Value> {
    std::str::from_utf8(body)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

#[test]
fn native_completed_only_assistant_message_is_delivered_before_completion() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let item = assistant_message("msg_completed_only", "这里是完整答复。");
            let terminal = completed_output_frame(vec![item.clone()]);
            let (body, bridge) = collect_native_fixture(terminal.clone()).await;
            let events = native_json_events(&body);
            assert_eq!(events.len(), 2, "only the missing item completion is added");
            assert_eq!(events[0]["type"], "response.output_item.done");
            assert_eq!(events[0]["output_index"], 0);
            assert_eq!(events[0]["item"], item);
            assert_eq!(events[1]["type"], "response.completed");
            assert!(
                body.ends_with(terminal.as_bytes()),
                "original completed bytes stay intact"
            );
            assert!(bridge.is_ok(true), "{bridge:?}");
            assert_eq!(
                bridge.usage.output_text.as_deref(),
                Some("这里是完整答复。")
            );
            assert_eq!(bridge.usage.total_tokens, Some(18));
        });
}

#[test]
fn native_completed_only_repair_omits_bom_for_sse_clients() {
    use eventsource_stream::Eventsource;
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for with_event_header in [true, false] {
                let item = assistant_message("msg_bom", "BOM 答复。");
                let mut terminal = completed_output_frame(vec![item.clone()]);
                if !with_event_header {
                    terminal = terminal.replacen("event: response.completed\n", "", 1);
                }
                let (body, bridge) = collect_native_fixture(format!("\u{feff}{terminal}")).await;
                assert!(!body.starts_with("\u{feff}".as_bytes()));
                assert!(body.ends_with(terminal.as_bytes()));
                assert_eq!(
                    std::str::from_utf8(&body)
                        .unwrap()
                        .matches('\u{feff}')
                        .count(),
                    0
                );
                // Decode the actual repaired bytes, including split UTF-8 text.
                // The optional upstream BOM must not move between the events or
                // trigger the downstream parser's leading-BOM handling bug.
                let chunks = body
                    .chunks(1)
                    .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)));
                let events = futures_util::stream::iter(chunks)
                    .eventsource()
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .map(Result::unwrap)
                    .collect::<Vec<_>>();
                assert_eq!(events.len(), 2, "done and completed must both decode");
                assert_eq!(events[0].event, "response.output_item.done");
                let repaired: serde_json::Value = serde_json::from_str(&events[0].data).unwrap();
                assert_eq!(repaired["item"], item);
                assert_eq!(repaired["output_index"], 0);
                let completed: serde_json::Value = serde_json::from_str(&events[1].data).unwrap();
                assert_eq!(completed["type"], "response.completed");
                assert_eq!(completed["response"]["output"][0], item);
                assert!(bridge.is_ok(true), "{bridge:?}");
            }
        });
}

#[test]
fn native_existing_output_item_done_preserves_wire_through_completion() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let item = assistant_message("msg_already_done", "Already delivered.");
            let done = serde_json::json!({
                "type": "response.output_item.done", "output_index": 0,
                "sequence_number": 16, "item": item
            });
            let expected = format!(
            ": upstream comment\r\n\r\nevent: response.output_item.done\r\ndata: {done}\r\n\r\n{}",
            completed_output_frame(vec![item]),
        );
            let (body, bridge) =
                collect_native_fixture(format!("{expected}data: [DONE]\n\n")).await;
            assert_eq!(
                body.as_ref(),
                expected.as_bytes(),
                "normal SSE stays intact through its terminal frame"
            );
            assert!(bridge.is_ok(true), "{bridge:?}");
            assert_eq!(
                bridge.usage.output_text.as_deref(),
                Some("Already delivered.")
            );
        });
}

#[test]
fn native_added_and_delta_without_item_done_gets_one_completion() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let item = assistant_message("msg_streamed", "Streamed answer.");
            let added = serde_json::json!({
                "type": "response.output_item.added", "output_index": 0,
                "item": {"type": "message", "id": "msg_streamed", "role": "assistant",
                    "status": "in_progress", "content": []}
            });
            let delta = serde_json::json!({
                "type": "response.output_text.delta", "item_id": "msg_streamed",
                "output_index": 0, "content_index": 0, "delta": "Streamed answer."
            });
            let prefix = format!("data: {added}\n\ndata: {delta}\n\n");
            let terminal = completed_output_frame(vec![item.clone()]);
            let (body, bridge) = collect_native_fixture(format!("{prefix}{terminal}")).await;
            assert!(body.starts_with(prefix.as_bytes()));
            assert!(body.ends_with(terminal.as_bytes()));
            let events = native_json_events(&body);
            let done = events
                .iter()
                .filter(|event| event["type"] == "response.output_item.done")
                .collect::<Vec<_>>();
            assert_eq!(done.len(), 1);
            assert_eq!(done[0]["output_index"], 0);
            assert_eq!(done[0]["item"], item);
            assert_eq!(
                events[events.len() - 2]["type"],
                "response.output_item.done"
            );
            assert_eq!(events.last().unwrap()["type"], "response.completed");
            assert_eq!(
                bridge.usage.output_text.as_deref(),
                Some("Streamed answer.")
            );
            assert!(bridge.is_ok(true), "{bridge:?}");
        });
}

#[test]
fn native_completed_output_repairs_only_assistant_messages_at_original_indices() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime().unwrap().block_on(async {
        let first = assistant_message("msg_first", "First answer.");
        let second = assistant_message("msg_second", "Second answer.");
        let terminal = completed_output_frame(vec![
            serde_json::json!({"type":"function_call", "id":"fc_preserve", "call_id":"call_preserve",
                "name":"weather", "arguments":"{}", "status":"completed"}),
            first.clone(),
            serde_json::json!({"type":"image_generation_call", "id":"ig_preserve",
                "status":"completed", "result":"QUJD"}),
            second.clone(),
        ]);
        let (body, bridge) = collect_native_fixture(terminal.clone()).await;
        let events = native_json_events(&body);
        assert_eq!(events.len(), 3, "tool and image items must not be synthesized");
        for (event, index, item) in [(&events[0], 1, first), (&events[1], 3, second)] {
            assert_eq!(event["type"], "response.output_item.done");
            assert_eq!(event["output_index"], index);
            assert_eq!(event["item"], item);
        }
        assert_eq!(events[2]["type"], "response.completed");
        assert!(body.ends_with(terminal.as_bytes()));
        assert!(bridge.is_ok(true), "{bridge:?}");
        assert_eq!(bridge.usage.total_tokens, Some(18));
    });
}

#[test]
fn native_disconnect_cleanup_observes_completed_usage_without_delivering_repair() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            // All bytes enter the pump together. More comments than the bounded
            // downstream queue guarantees completed is read only during cleanup.
            let mut wire = ": queued provider frame\n\n".repeat(384);
            wire.push_str(&completed_output_frame(vec![assistant_message(
                "msg_cleanup_only",
                "Must not be marked delivered.",
            )]));
            let (provider, response, delivery) =
                start_native_fixture(Bytes::from(wire), false).await;
            let mut body = response.into_body().into_data_stream();
            let first = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(first, ": queued provider frame\n\n".as_bytes());
            drop(body);
            let bridge = finish_native_fixture(delivery).await;
            assert!(bridge
                .delivery_error
                .as_deref()
                .is_some_and(|error| error.contains("broken pipe")));
            assert!(!bridge.is_ok(true));
            assert!(bridge.stream_terminal_seen);
            assert!(
                !bridge.stream_terminal_delivered,
                "cleanup cannot manufacture delivery"
            );
            assert_eq!(
                bridge.usage.output_text.as_deref(),
                Some("Must not be marked delivered.")
            );
            assert_eq!(bridge.usage.total_tokens, Some(18));
            assert!(provider.is_closed());
        });
}

#[test]
fn native_disconnect_preserves_usage_from_forwarded_responses_frame() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let (request, receiver) = request();
            let (provider, body) = tokio::sync::mpsc::channel(2);
            provider
                .send(crate::gateway::upstream::GatewayByteStreamItem::Chunk(
                    Bytes::from_static(COMPLETED_USAGE),
                ))
                .await
                .unwrap();
            let delivery = tokio::spawn(respond_with_upstream_async(
                request,
                upstream(crate::gateway::upstream::GatewayByteStream::from_receiver(
                    body,
                )),
                crate::gateway::acquire_account_inflight("native-async-usage"),
                crate::gateway::ResponseAdapter::Passthrough,
                None,
                None,
                "/v1/responses",
                None,
                true,
                false,
                None,
                None,
                Instant::now(),
            ));
            let response = receiver.await.unwrap();
            let mut body = response.into_body().into_data_stream();
            assert_eq!(body.next().await.unwrap().unwrap(), COMPLETED_USAGE);
            // Codex closes as soon as it consumes response.completed. No EOF or
            // optional [DONE] from the still-open provider is required afterward.
            drop(body);
            let bridge = tokio::time::timeout(Duration::from_secs(2), delivery)
                .await
                .expect("downstream disconnect cancels the still-open provider")
                .unwrap()
                .unwrap();
            assert!(bridge.delivery_error.is_none(), "{bridge:?}");
            assert!(bridge.is_ok(true), "{bridge:?}");
            assert!(bridge.stream_terminal_delivered);
            assert_eq!(bridge.usage.input_tokens, Some(11));
            assert_eq!(bridge.usage.output_tokens, Some(7));
            assert_eq!(bridge.usage.total_tokens, Some(18));
            assert!(
                provider.is_closed(),
                "semantic completion cancels provider input"
            );
        });
}

#[test]
fn native_unconsumed_terminal_frame_is_still_a_delivery_failure() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let (provider, response, delivery) =
                start_native_fixture(Bytes::from_static(COMPLETED_USAGE), false).await;
            // Let the producer parse and enqueue its small final frame without
            // ever polling the HTTP body. Enqueueing is not downstream receipt.
            tokio::task::yield_now().await;
            drop(response);
            let bridge = finish_native_fixture(delivery).await;
            assert!(
                bridge
                    .delivery_error
                    .as_deref()
                    .is_some_and(|error| error.contains("broken pipe")),
                "{bridge:?}"
            );
            assert!(
                !bridge.is_ok(true),
                "unconsumed completion cannot turn into success"
            );
            assert!(provider.is_closed());
        });
}

#[test]
fn native_partial_large_completion_or_repair_is_still_a_delivery_failure() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for repair in [false, true] {
                let padding = "x".repeat(20 * 1024);
                let wire = if repair {
                    completed_output_frame(vec![assistant_message("msg_partial_repair", &padding)])
                } else {
                    let payload = serde_json::json!({
                        "type": "response.completed",
                        "response": {"id":"resp_partial_large", "status":"completed", "output":[],
                            "metadata":{"padding":padding},
                            "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18}}
                    });
                    format!("event: response.completed\ndata: {payload}\n\n")
                };
                let (provider, response, delivery) =
                    start_native_fixture(Bytes::from(wire), false).await;
                let mut body = response.into_body().into_data_stream();
                let first = tokio::time::timeout(Duration::from_secs(2), body.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    first.len(),
                    16 * 1024,
                    "fixture spans native delivery chunks"
                );
                assert!(
                    !first.ends_with(b"\n\n"),
                    "only part of the JSON event was consumed"
                );
                // A final chunk can already be queued here. It must be ACKed by
                // the body consumer, not merely parsed or read by the producer.
                tokio::task::yield_now().await;
                drop(body);
                let bridge = finish_native_fixture(delivery).await;
                assert!(
                    bridge
                        .delivery_error
                        .as_deref()
                        .is_some_and(|error| error.contains("broken pipe")),
                    "repair={repair}: {bridge:?}"
                );
                assert!(
                    !bridge.is_ok(true),
                    "repair={repair}: partial frame is not delivered"
                );
                assert_eq!(bridge.usage.total_tokens, Some(18));
                assert!(provider.is_closed());
            }
        });
}

#[test]
fn native_consumed_failure_terminal_finishes_transport_but_preserves_failure() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for (event_type, status) in [
                ("response.failed", "failed"),
                ("response.incomplete", "incomplete"),
                ("response.completed", "failed"),
            ] {
                let payload = serde_json::json!({
                    "type":event_type,
                    "response":{"id":"resp_semantic_failure","status":status,"output":[],
                        "error":{"code":"upstream_failure","message":"provider rejected completion"},
                        "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18}}
                });
                let wire = format!("event: {event_type}\ndata: {payload}\n\n");
                let (provider, response, delivery) =
                    start_native_fixture(Bytes::from(wire.clone()), false).await;
                let mut body = response.into_body().into_data_stream();
                assert_eq!(body.next().await.unwrap().unwrap().as_ref(), wire.as_bytes());
                drop(body);
                let bridge = finish_native_fixture(delivery).await;
                assert!(bridge.delivery_error.is_none(), "{event_type}: {bridge:?}");
                assert!(bridge.stream_terminal_seen);
                assert!(bridge.stream_terminal_delivered);
                assert!(bridge.stream_terminal_error.as_deref().is_some_and(|error|
                    error.contains("provider rejected completion")), "{bridge:?}");
                assert!(!bridge.is_ok(true), "failure terminal must not become a successful response");
                assert_eq!(bridge.usage.total_tokens, Some(18));
                assert!(provider.is_closed());
            }
        });
}

#[test]
fn anthropic_reader_failure_terminal_preserves_bridge_failure_after_delivery() {
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for started in [false, true] {
                let (request, receiver) = request();
                let (provider, input) = tokio::sync::mpsc::channel(2);
                if started {
                    provider.try_send(GatewayByteStreamItem::Chunk(Bytes::from_static(
                        b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_failed\",\"model\":\"test\",\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n",
                    ))).unwrap();
                }
                provider
                    .try_send(GatewayByteStreamItem::Error("provider connection reset".to_string()))
                    .unwrap();
                let delivery = tokio::spawn(respond_with_upstream_async(
                    request,
                    upstream(GatewayByteStream::from_receiver(input)),
                    crate::gateway::acquire_account_inflight("anthropic-reader-failed-terminal"),
                    crate::gateway::ResponseAdapter::ResponsesFromAnthropicMessages,
                    None,
                    None,
                    "/v1/responses",
                    None,
                    true,
                    false,
                    None,
                    None,
                    Instant::now(),
                ));
                let response = tokio::time::timeout(Duration::from_secs(2), receiver)
                    .await
                    .unwrap()
                    .unwrap();
                let body = tokio::time::timeout(
                    Duration::from_secs(2),
                    axum::body::to_bytes(response.into_body(), usize::MAX),
                )
                .await
                .expect("failed terminal must finish without upstream EOF")
                .unwrap();
                let events = native_json_events(&body);
                let terminal = events.last().unwrap();
                assert_eq!(terminal["type"], "response.failed");
                assert_eq!(terminal["response"]["error"]["message"], "provider connection reset");
                assert!(!events.iter().any(|event| event["type"] == "response.completed"));
                let bridge = finish_native_fixture(delivery).await;
                assert!(bridge.delivery_error.is_none(), "{bridge:?}");
                assert!(bridge.usage.explicit_failure, "{bridge:?}");
                assert!(!bridge.is_ok(true), "a delivered failure cannot be logged as a successful 200: {bridge:?}");
                assert!(bridge.error_message(true).is_some());
                if started {
                    assert_eq!(bridge.usage.input_tokens, Some(11));
                }
                assert!(provider.is_closed());
            }
        });
}

#[test]
fn native_completion_discards_trailing_frames_and_closes_without_provider_eof() {
    let _guard = crate::test_env_guard();
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            for tail in [
                "",
                "data: [DONE]\n\n: trailing provider heartbeat\n\n",
                "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"invalid post-terminal failure\"}}}\n\n",
            ] {
                let mut wire = COMPLETED_USAGE.to_vec();
                wire.extend_from_slice(tail.as_bytes());
                let (provider, response, delivery) =
                    start_native_fixture(Bytes::from(wire), false).await;
                let body = tokio::time::timeout(Duration::from_secs(2),
                    axum::body::to_bytes(response.into_body(), usize::MAX))
                    .await.expect("completion does not wait for provider EOF or keepalive").unwrap();
                assert_eq!(body.as_ref(), COMPLETED_USAGE, "post-terminal bytes are outside this response");
                let bridge = finish_native_fixture(delivery).await;
                assert!(bridge.is_ok(true), "tail={tail:?}: {bridge:?}");
                assert!(bridge.stream_terminal_error.is_none());
                assert_eq!(bridge.last_sse_event_type.as_deref(), Some("response.completed"));
                assert_eq!(bridge.usage.total_tokens, Some(18));
                assert!(provider.is_closed());
            }
        });
}

#[test]
fn deferred_disconnect_drain_keeps_request_gate_until_accounting_completes() {
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let (mut request, receiver) = request();
            let gate =
                crate::gateway::request_gate_lock("native-async-gate", "/v1/responses", None);
            request.hold_until_complete(gate.try_acquire().unwrap().unwrap());
            let (started, accounting_started) = tokio::sync::oneshot::channel();
            let (finish, accounting_finish) = std::sync::mpsc::channel();
            defer_upstream_response(
                request,
                upstream(crate::gateway::upstream::GatewayByteStream::from_bytes(
                    Bytes::from_static(COMPLETED_USAGE),
                )),
                crate::gateway::acquire_account_inflight("native-async-gate"),
                crate::gateway::ResponseAdapter::Passthrough,
                None,
                None,
                "/v1/responses",
                None,
                true,
                None,
                None,
                Instant::now(),
                move |bridge| {
                    assert_eq!(bridge.usage.output_tokens, Some(7));
                    started.send(()).unwrap();
                    accounting_finish
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap();
                },
            )
            .unwrap();
            let response = receiver.await.unwrap();
            let mut body = response.into_body().into_data_stream();
            assert_eq!(body.next().await.unwrap().unwrap(), COMPLETED_USAGE);
            accounting_started.await.unwrap();
            assert!(gate.try_acquire().unwrap().is_none());
            // A closed HTTP body cannot keep Hyper's graceful shutdown alive.
            drop(body);
            let mut draining = Box::pin(drain_deferred_responses());
            assert!(
                tokio::time::timeout(Duration::from_millis(30), &mut draining)
                    .await
                    .is_err()
            );
            finish.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), draining)
                .await
                .expect("shutdown drains detached accounting");
            assert!(gate.try_acquire().unwrap().is_some());
        });
}
