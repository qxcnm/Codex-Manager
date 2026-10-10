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
            // Keep the provider open, then disconnect immediately after the usage
            // bytes arrive. The observer may still be processing that same frame.
            drop(body);
            let bridge = tokio::time::timeout(Duration::from_secs(2), delivery)
                .await
                .expect("downstream disconnect cancels the still-open provider")
                .unwrap()
                .unwrap();
            assert!(bridge
                .delivery_error
                .as_deref()
                .unwrap()
                .contains("broken pipe"));
            assert!(bridge.stream_terminal_delivered);
            assert_eq!(bridge.usage.input_tokens, Some(11));
            assert_eq!(bridge.usage.output_tokens, Some(7));
            assert_eq!(bridge.usage.total_tokens, Some(18));
            drop(provider);
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
            accounting_started.await.unwrap();
            assert!(gate.try_acquire().unwrap().is_none());
            // A closed HTTP body cannot keep Hyper's graceful shutdown alive.
            drop(response);
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
