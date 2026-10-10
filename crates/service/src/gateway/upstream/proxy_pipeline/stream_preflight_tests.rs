use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::time::Duration;
use tokio::sync::mpsc;

use super::*;
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse};

fn stream_response(body: &'static str) -> GatewayUpstreamResponse {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_bytes(Bytes::from_static(body.as_bytes())),
    ))
}

fn json_stream_response(body: &'static str) -> GatewayUpstreamResponse {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_bytes(Bytes::from_static(body.as_bytes())),
    ))
}

fn json_stream_response_with_status(
    status: reqwest::StatusCode,
    body: &'static str,
) -> GatewayUpstreamResponse {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        status,
        headers,
        GatewayByteStream::from_bytes(Bytes::from_static(body.as_bytes())),
    ))
}

fn stream_response_from_items(items: Vec<GatewayByteStreamItem>) -> GatewayUpstreamResponse {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let (tx, rx) = mpsc::channel(items.len().max(1));
    for item in items {
        tx.blocking_send(item).expect("queue upstream item");
    }
    drop(tx);
    GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver(rx),
    ))
}

#[test]
fn prefix_waits_through_metadata_only_frames() {
    let prefix = b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n";
    assert_eq!(classify_prefix(prefix, false), PrefixDecision::NeedMore);
}

#[test]
fn prefix_waits_for_terminal_confirmation_of_usage_notice_output() {
    let prefix = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"content\":[]}}\n\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::NeedMore
    );

    let terminated = format!("{prefix}data: [DONE]\n\n");
    assert!(matches!(
        classify_prefix(terminated.as_bytes(), false),
        PrefixDecision::RetryUsageNotice(message) if message.contains("usage limit")
    ));
}

#[test]
fn prefix_accumulates_usage_notice_split_across_deltas() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"usage limit. Try again later.\"}\n\n",
        "data: [DONE]\n\n"
    );
    assert!(matches!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::RetryUsageNotice(message) if message.contains("usage limit")
    ));
}

#[test]
fn prefix_retries_usage_notice_confirmed_by_incomplete_terminal() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_limited\",\"status\":\"incomplete\"}}\n\n"
    );
    assert!(matches!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::RetryUsageNotice(message) if message.contains("usage limit")
    ));
}

#[test]
fn prefix_retries_usage_notice_confirmed_by_bare_terminal_events() {
    for terminal_event in ["response.incomplete", "response.failed"] {
        let prefix = format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}}\n\ndata: {{\"type\":\"{terminal_event}\"}}\n\n"
        );
        assert!(matches!(
            classify_prefix(prefix.as_bytes(), false),
            PrefixDecision::RetryUsageNotice(message) if message.contains("usage limit")
        ));
    }
}

#[test]
fn prefix_delivers_when_a_possible_usage_notice_prefix_diverges() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"stride goal for today.\"}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Deliver
    );
}

#[test]
fn prefix_detects_usage_limit_in_explicit_error_fields() {
    let prefix = concat!(
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"usage_limit_reached\"}}}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Failover("usage_limit_reached".to_string())
    );
}

#[test]
fn prefix_detects_deactivation_in_explicit_error_event() {
    let prefix = concat!(
        "event: error\n",
        "data: {\"message\":\"workspace_deactivated\"}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Failover("workspace_deactivated".to_string())
    );
}

#[test]
fn terminal_prefix_detects_error_frame_without_trailing_separator() {
    let prefix = concat!(
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"You've hit your usage limit.\"}}"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::NeedMore
    );
    assert!(matches!(
        classify_prefix(prefix.as_bytes(), true),
        PrefixDecision::Failover(message) if message.contains("usage limit")
    ));
}

#[test]
fn prefix_ignores_actionable_words_in_metadata_strings() {
    let prefix = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"metadata\":{\"prompt\":\"Explain You've hit your usage limit and deactivated\"}}}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::NeedMore
    );
}

#[test]
fn prefix_does_not_scan_arbitrary_strings_inside_error_event() {
    let prefix = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"context\":{\"prompt\":\"Explain You've hit your usage limit and deactivated\"}}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Deliver
    );
}

#[test]
fn prefix_delivers_usage_limit_words_in_normal_output() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"The usage limit has been reached is an English error message. An account may also be deactivated.\"}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Deliver
    );
}

#[test]
fn prefix_delivers_deactivation_notice_in_normal_output() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Your account has been deactivated.\"}\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Deliver
    );
}

#[test]
fn prefix_delivers_exact_usage_notice_when_response_completes_normally() {
    let prefix = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
        "data: {\"type\":\"response.output_text.done\",\"text\":\"You've hit your usage limit. Try again later.\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_normal\",\"status\":\"completed\"}}\n\n",
        "data: [DONE]\n\n"
    );
    assert_eq!(
        classify_prefix(prefix.as_bytes(), false),
        PrefixDecision::Deliver
    );
}

#[test]
fn prefix_commits_normal_output() {
    let prefix = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
    assert_eq!(classify_prefix(prefix, false), PrefixDecision::Deliver);
}

#[test]
fn preflight_replays_normal_prefix_without_loss() {
    let body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "data: [DONE]\n\n"
    );
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        stream_response(body),
        "/v1/responses",
        true,
        true,
    ))
    .expect("gateway async test runtime");
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("normal output must be delivered");
    };
    let (replayed, _) = response.into_buffered().expect("buffer replayed response");
    assert_eq!(replayed.as_ref(), body.as_bytes());
}

#[test]
fn preflight_leaves_successful_json_response_untouched() {
    let body = r#"{"id":"resp_json","status":"completed","output":[]}"#;
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        json_stream_response(body),
        "/v1/responses",
        true,
        true,
    ))
    .expect("gateway async test runtime");
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("successful JSON must bypass SSE preflight");
    };
    let (replayed, _) = response.into_buffered().expect("buffer JSON response");
    assert_eq!(replayed.as_ref(), body.as_bytes());
}

#[test]
fn preflight_fails_over_on_json_usage_limit_error_response() {
    let body = r#"{"error":{"message":"The usage limit has been reached.","type":"usage_limit_reached","code":"usage_limit_reached"}}"#;
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        json_stream_response_with_status(reqwest::StatusCode::TOO_MANY_REQUESTS, body),
        "/v1/responses",
        false,
        true,
    ))
    .expect("gateway async test runtime");
    assert!(matches!(
        outcome,
        StreamPreflightOutcome::Failover(message) if message.contains("usage limit")
            || message == "usage_limit_reached"
    ));
}

#[test]
fn preflight_fails_over_on_any_non_2xx_when_more_candidates_exist() {
    let body = r#"{"error":{"message":"upstream temporarily unavailable"}}"#;
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        json_stream_response_with_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR, body),
        "/v1/responses",
        false,
        true,
    ))
    .expect("gateway async test runtime");
    assert!(matches!(
        outcome,
        StreamPreflightOutcome::StatusFailover {
            status_code: 500,
            message,
        } if message.contains("status=500")
    ));
}

#[test]
fn preflight_delivers_2xx_success_status_when_more_candidates_exist() {
    let body = r#"{"id":"created_elsewhere"}"#;
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        json_stream_response_with_status(reqwest::StatusCode::CREATED, body),
        "/v1/responses",
        false,
        true,
    ))
    .expect("gateway async test runtime");
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("2xx response must be delivered");
    };
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let (replayed, _) = response.into_buffered().expect("buffer JSON response");
    assert_eq!(replayed.as_ref(), body.as_bytes());
}

#[test]
fn preflight_delivers_json_usage_limit_error_when_no_more_candidates() {
    let body =
        r#"{"error":{"message":"The usage limit has been reached.","type":"usage_limit_reached"}}"#;
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        json_stream_response_with_status(reqwest::StatusCode::TOO_MANY_REQUESTS, body),
        "/v1/responses",
        false,
        false,
    ))
    .expect("gateway async test runtime");
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("last candidate error must be delivered");
    };
    assert_eq!(response.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let (replayed, _) = response.into_buffered().expect("buffer JSON response");
    assert_eq!(replayed.as_ref(), body.as_bytes());
}

#[test]
fn preflight_suppresses_actionable_usage_limit() {
    let body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
        "data: [DONE]\n\n"
    );
    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response(stream_response(body), "/v1/responses", true, true)).expect("gateway async test runtime"),
        StreamPreflightOutcome::RetryUsageNotice(message) if message.contains("usage limit")
    ));
}

#[test]
fn preflight_retry_cancels_the_discarded_upstream_producer() {
    let body = Bytes::from_static(
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
            "data: [DONE]\n\n"
        )
        .as_bytes(),
    );
    let (tx, rx) = mpsc::channel(2);
    tx.blocking_send(GatewayByteStreamItem::Chunk(body))
        .expect("queue quota notice");
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel_tx)),
    ));

    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response(
            response,
            "/v1/responses",
            true,
            true
        ))
        .expect("gateway async test runtime"),
        StreamPreflightOutcome::RetryUsageNotice(_)
    ));
    assert_eq!(cancel_rx.try_recv(), Ok(()));
}

#[test]
fn preflight_waits_beyond_legacy_two_second_window_for_quota_notice() {
    let metadata = Bytes::from_static(
        b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_delayed\"}}\n\n",
    );
    let quota_notice = Bytes::from_static(
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
            "data: [DONE]\n\n"
        )
        .as_bytes(),
    );
    let (tx, rx) = mpsc::channel(2);
    tx.blocking_send(GatewayByteStreamItem::Chunk(metadata))
        .expect("queue response metadata");
    let producer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2_100));
        tx.blocking_send(GatewayByteStreamItem::Chunk(quota_notice))
            .expect("queue delayed quota notice");
    });
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver(rx),
    ));

    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response_with_idle_timeout(
            response,
            "/v1/responses",
            true,
            true,
            Some(Duration::from_secs(5)),
        )).expect("gateway async test runtime"),
        StreamPreflightOutcome::RetryUsageNotice(message) if message.contains("usage limit")
    ));
    producer.join().expect("join delayed quota producer");
}

#[test]
fn preflight_idle_before_deliverable_content_fails_over_and_cancels_upstream() {
    let metadata = Bytes::from_static(
        b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_idle\"}}\n\n",
    );
    let (tx, rx) = mpsc::channel(1);
    tx.blocking_send(GatewayByteStreamItem::Chunk(metadata))
        .expect("queue response metadata");
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel_tx)),
    ));

    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response_with_idle_timeout(
            response,
            "/v1/responses",
            true,
            true,
            Some(Duration::from_millis(25)),
        )).expect("gateway async test runtime"),
        StreamPreflightOutcome::TransportFailover(message) if message.contains("idle timeout")
    ));
    assert_eq!(cancel_rx.try_recv(), Ok(()));
    drop(tx);
}

#[test]
fn preflight_wall_clock_cap_commits_slow_stream_without_losing_prefix() {
    let metadata = Bytes::from_static(
        b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_slow\"}}\n\n",
    );
    let output = Bytes::from_static(
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\ndata: [DONE]\n\n",
    );
    let expected = [metadata.as_ref(), output.as_ref()].concat();
    let (tx, rx) = mpsc::channel(2);
    tx.blocking_send(GatewayByteStreamItem::Chunk(metadata))
        .expect("queue response metadata");
    let producer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        tx.blocking_send(GatewayByteStreamItem::Chunk(output))
            .expect("queue delayed output");
        tx.blocking_send(GatewayByteStreamItem::Eof)
            .expect("queue delayed output EOF");
    });
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel_tx)),
    ));

    let started_at = std::time::Instant::now();
    let outcome = crate::gateway::run_upstream_io(preflight_stream_response_with_timeouts(
        response,
        "/v1/responses",
        true,
        true,
        Some(Duration::from_secs(1)),
        Some(Duration::from_millis(25)),
    ))
    .expect("gateway async test runtime");
    assert!(started_at.elapsed() < Duration::from_millis(500));
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("wall-clock cap must commit a normal slow stream");
    };
    assert!(matches!(
        cancel_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    producer.join().expect("join delayed output producer");
    let (replayed, _) = response.into_buffered().expect("buffer replayed response");
    assert_eq!(replayed.as_ref(), expected.as_slice());
}

#[test]
fn preflight_prefix_limit_commits_large_metadata_without_losing_bytes() {
    let body = format!(
        "data: {{\"type\":\"response.created\",\"response\":{{\"id\":\"resp_large\",\"metadata\":{{\"padding\":\"{}\"}}}}}}\n\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}}\n\ndata: [DONE]\n\n",
        "x".repeat(STREAM_PREFLIGHT_MAX_BYTES + 1024),
    );
    let response = stream_response_from_items(vec![
        GatewayByteStreamItem::Chunk(Bytes::copy_from_slice(body.as_bytes())),
        GatewayByteStreamItem::Eof,
    ]);

    let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
        response,
        "/v1/responses",
        true,
        true,
    ))
    .expect("gateway async test runtime");
    let StreamPreflightOutcome::Ready(response) = outcome else {
        panic!("classification prefix limit must commit the original stream");
    };
    let (replayed, _) = response.into_buffered().expect("buffer replayed response");
    assert_eq!(replayed.as_ref(), body.as_bytes());
}

#[test]
fn preflight_fails_over_on_read_error_before_deliverable_content() {
    let metadata = Bytes::from_static(
        b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
    );
    let truncated_event =
        Bytes::from_static(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial");
    let response = stream_response_from_items(vec![
        GatewayByteStreamItem::Chunk(metadata),
        GatewayByteStreamItem::Chunk(truncated_event),
        GatewayByteStreamItem::Error("connection reset".to_string()),
    ]);

    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response(response, "/v1/responses", true, true)).expect("gateway async test runtime"),
        StreamPreflightOutcome::TransportFailover(message)
            if message.contains("connection reset")
    ));
}

#[test]
fn preflight_fails_over_when_producer_disconnects_after_metadata() {
    let metadata = Bytes::from_static(
        b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
    );
    let response = stream_response_from_items(vec![GatewayByteStreamItem::Chunk(metadata)]);

    assert!(matches!(
        crate::gateway::run_upstream_io(preflight_stream_response(response, "/v1/responses", true, true)).expect("gateway async test runtime"),
        StreamPreflightOutcome::TransportFailover(message)
            if message.contains("disconnected")
    ));
}

#[test]
fn model_rejection_classification_and_last_response_replay_cover_both_endpoints_and_modes() {
    let body = r#"{"error":{"message":"The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account."}}"#;
    for path in ["/v1/responses", "/v1/chat/completions"] {
        for is_stream in [false, true] {
            for has_more in [false, true] {
                let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
                    json_stream_response_with_status(reqwest::StatusCode::BAD_REQUEST, body),
                    path,
                    is_stream,
                    has_more,
                ))
                .unwrap();
                let StreamPreflightOutcome::ModelUnsupported { message, response } = outcome else {
                    panic!("complete explicit rejection must be learned even for last candidate");
                };
                assert!(message.contains("gpt-6-luna"));
                assert_eq!(response.status().as_u16(), 400);
                let (replayed, _) = response.into_buffered().unwrap();
                assert_eq!(replayed.as_ref(), body.as_bytes());
            }
        }
    }
}

#[test]
fn oversized_model_rejection_is_not_learned_and_last_body_replays_losslessly() {
    let mut body = serde_json::to_vec(&serde_json::json!({
        "error": {"message": "The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account."},
        "padding": "x".repeat(STREAM_PREFLIGHT_MAX_BYTES),
    })).unwrap();
    body.extend_from_slice(b" ");
    for has_more in [false, true] {
        let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
            reqwest::StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            GatewayByteStream::from_bytes(Bytes::from(body.clone())),
        ));
        let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
            response,
            "/v1/responses",
            true,
            has_more,
        ))
        .unwrap();
        if has_more {
            assert!(matches!(
                outcome,
                StreamPreflightOutcome::StatusFailover {
                    status_code: 400,
                    ..
                }
            ));
        } else {
            let StreamPreflightOutcome::Ready(response) = outcome else {
                panic!("last candidate must remain deliverable");
            };
            let (replayed, _) = response.into_buffered().unwrap();
            assert_eq!(replayed.as_ref(), body.as_slice());
        }
    }
}

#[test]
fn hanging_error_body_obeys_idle_and_wall_clock_caps_without_learning_prefix() {
    for has_more in [false, true] {
        for wall_cap in [false, true] {
            let (tx, rx) = mpsc::channel(2);
            let body = Bytes::from_static(b"{\"error\":{\"message\":\"The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account.\"}}");
            tx.blocking_send(GatewayByteStreamItem::Chunk(body.clone()))
                .unwrap();
            let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
                reqwest::StatusCode::BAD_REQUEST,
                HeaderMap::new(),
                GatewayByteStream::from_receiver(rx),
            ));
            let started = std::time::Instant::now();
            let outcome = crate::gateway::run_upstream_io(preflight_stream_response_with_timeouts(
                response,
                "/v1/chat/completions",
                false,
                has_more,
                Some(Duration::from_millis(if wall_cap { 500 } else { 20 })),
                Some(Duration::from_millis(if wall_cap { 20 } else { 500 })),
            ))
            .unwrap();
            assert!(started.elapsed() < Duration::from_millis(300));
            if has_more {
                assert!(matches!(
                    outcome,
                    StreamPreflightOutcome::StatusFailover {
                        status_code: 400,
                        ..
                    }
                ));
            } else {
                tx.blocking_send(GatewayByteStreamItem::Eof).unwrap();
                let StreamPreflightOutcome::Ready(response) = outcome else {
                    panic!("timeout must preserve final response");
                };
                let (replayed, _) = response.into_buffered().unwrap();
                assert_eq!(replayed, body);
            }
        }
    }
}

#[test]
fn broken_error_body_preserves_last_candidate_and_never_learns_partial_json() {
    for has_more in [false, true] {
        let (tx, rx) = mpsc::channel(3);
        tx.blocking_send(GatewayByteStreamItem::Chunk(Bytes::from_static(
            b"{\"error\":",
        )))
        .unwrap();
        tx.blocking_send(GatewayByteStreamItem::Error(
            "upstream read failed".to_owned(),
        ))
        .unwrap();
        drop(tx);
        let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
            reqwest::StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            GatewayByteStream::from_receiver(rx),
        ));
        let outcome = crate::gateway::run_upstream_io(preflight_stream_response(
            response,
            "/v1/responses",
            false,
            has_more,
        ))
        .unwrap();
        if has_more {
            assert!(matches!(
                outcome,
                StreamPreflightOutcome::StatusFailover {
                    status_code: 400,
                    ..
                }
            ));
        } else {
            let StreamPreflightOutcome::Ready(response) = outcome else {
                panic!("last response must preserve its read error");
            };
            assert!(response
                .into_buffered()
                .unwrap_err()
                .contains("upstream read failed"));
        }
    }
}

#[tokio::test]
async fn continuously_dripping_error_body_stops_at_wall_clock_limit() {
    let (tx, rx) = mpsc::channel(2);
    let producer = tokio::spawn(async move {
        loop {
            if tx
                .send(GatewayByteStreamItem::Chunk(Bytes::from_static(b" ")))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    let response = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(
        reqwest::StatusCode::BAD_REQUEST,
        HeaderMap::new(),
        GatewayByteStream::from_receiver(rx),
    ));
    let started = std::time::Instant::now();
    let outcome = preflight_stream_response_with_timeouts(
        response,
        "/v1/responses",
        true,
        true,
        Some(Duration::from_secs(1)),
        Some(Duration::from_millis(25)),
    )
    .await;
    assert!(started.elapsed() < Duration::from_millis(300));
    assert!(matches!(
        outcome,
        StreamPreflightOutcome::StatusFailover {
            status_code: 400,
            ..
        }
    ));
    producer.await.unwrap();
}
