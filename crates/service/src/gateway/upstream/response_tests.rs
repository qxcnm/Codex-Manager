use std::time::Duration;
use tokio::sync::mpsc;

use bytes::Bytes;

use super::*;
use crate::gateway::http_bridge::{
    OpenAIResponsesPassthroughSseReader, PassthroughSseCollector, SseKeepAliveFrame,
};

#[test]
fn prefetch_caps_the_classification_copy_and_replays_the_full_chunk() {
    let body = Bytes::from(vec![b'x'; 128 * 1024]);
    let stream = GatewayByteStream::from_bytes(body.clone());

    let (prefix, replayed, terminal) =
        stream.prefetch_until(64 * 1024, Some(Duration::from_secs(1)), None, |_| false);

    assert_eq!(prefix.len(), 64 * 1024);
    assert_eq!(terminal, GatewayStreamPrefetchTerminal::PrefixLimit);
    assert_eq!(replayed.read_all_bytes().expect("replayed body"), body);
}

#[test]
fn prefetch_reports_eof_without_losing_the_buffered_body() {
    let body = Bytes::from_static(b"metadata only");
    let stream = GatewayByteStream::from_bytes(body.clone());

    let (prefix, replayed, terminal) =
        stream.prefetch_until(1024, Some(Duration::from_secs(1)), None, |_| false);

    assert_eq!(prefix, body);
    assert_eq!(terminal, GatewayStreamPrefetchTerminal::Eof);
    assert_eq!(replayed.read_all_bytes().expect("replayed body"), body);
}

#[test]
fn prefetch_reports_and_replays_stream_errors() {
    let (tx, rx) = mpsc::channel(2);
    tx.blocking_send(GatewayByteStreamItem::Chunk(Bytes::from_static(
        b"metadata",
    )))
    .expect("send metadata");
    tx.blocking_send(GatewayByteStreamItem::Error("upstream reset".to_string()))
        .expect("send stream error");
    let stream = GatewayByteStream::from_receiver(rx);

    let (prefix, replayed, terminal) =
        stream.prefetch_until(1024, Some(Duration::from_secs(1)), None, |_| false);

    assert_eq!(prefix.as_ref(), b"metadata");
    assert_eq!(
        terminal,
        GatewayStreamPrefetchTerminal::Error("upstream reset".to_string())
    );
    assert_eq!(replayed.read_all_bytes(), Err("upstream reset".to_string()));
}

#[test]
fn prefetch_distinguishes_a_disconnected_producer_from_clean_eof() {
    let (tx, rx) = mpsc::channel(1);
    drop(tx);
    let stream = GatewayByteStream::from_receiver(rx);

    let (prefix, _replayed, terminal) =
        stream.prefetch_until(1024, Some(Duration::from_secs(1)), None, |_| false);

    assert!(prefix.is_empty());
    assert_eq!(terminal, GatewayStreamPrefetchTerminal::Disconnected);
}

#[test]
fn prefetch_wall_clock_timeout_is_not_reset_by_activity_and_replays_all_bytes() {
    let (tx, rx) = mpsc::channel(32);
    let producer = std::thread::spawn(move || {
        let mut expected = Vec::new();
        for index in 0..20 {
            let chunk = format!("chunk-{index};").into_bytes();
            expected.extend_from_slice(chunk.as_slice());
            tx.blocking_send(GatewayByteStreamItem::Chunk(Bytes::from(chunk)))
                .expect("send active stream chunk");
            std::thread::sleep(Duration::from_millis(5));
        }
        tx.blocking_send(GatewayByteStreamItem::Eof)
            .expect("send active stream EOF");
        expected
    });
    let stream = GatewayByteStream::from_receiver(rx);

    let started_at = std::time::Instant::now();
    let (_prefix, replayed, terminal) = stream.prefetch_until(
        1024,
        Some(Duration::from_secs(1)),
        Some(Duration::from_millis(30)),
        |_| false,
    );

    assert_eq!(terminal, GatewayStreamPrefetchTerminal::WallClockTimeout);
    assert!(started_at.elapsed() < Duration::from_millis(750));
    let expected = producer.join().expect("join active stream producer");
    assert_eq!(replayed.read_all_bytes().expect("replayed body"), expected);
}

#[test]
fn prefetch_idle_timeout_wins_before_later_wall_clock_timeout() {
    let (_tx, rx) = mpsc::channel(1);
    let stream = GatewayByteStream::from_receiver(rx);

    let (_prefix, _replayed, terminal) = stream.prefetch_until(
        1024,
        Some(Duration::from_millis(20)),
        Some(Duration::from_millis(200)),
        |_| false,
    );

    assert_eq!(terminal, GatewayStreamPrefetchTerminal::IdleTimeout);
}

#[test]
fn dropping_a_stream_signals_its_upstream_producer_to_cancel() {
    let (_tx, rx) = mpsc::channel(1);
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let stream = GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel_tx));

    drop(stream);

    assert_eq!(cancel_rx.try_recv(), Ok(()));
}

#[test]
fn dropping_both_tee_consumers_cancels_a_silent_upstream() {
    let (_source_tx, source_rx) = mpsc::channel(1);
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let source = GatewayByteStream::from_receiver_with_cancel(source_rx, Some(cancel_tx));
    let (left, right) = source.tee();

    drop(left);
    drop(right);

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match cancel_rx.try_recv() {
            Ok(()) => break,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                if std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            other => panic!("tee relay did not cancel silent upstream: {other:?}"),
        }
    }
}

#[test]
fn dropping_openai_responses_reader_cancels_sidecar_and_silent_upstream() {
    let (_source_tx, source_rx) = mpsc::channel(1);
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
    let source = GatewayByteStream::from_receiver_with_cancel(source_rx, Some(cancel_tx));
    let response = GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        reqwest::header::HeaderMap::new(),
        source,
    );
    let reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
        response,
        std::sync::Arc::new(std::sync::Mutex::new(PassthroughSseCollector::default())),
        SseKeepAliveFrame::Comment,
        std::time::Instant::now(),
    );

    drop(reader);

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match cancel_rx.try_recv() {
            Ok(()) => break,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                if std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            other => panic!("responses reader did not cancel silent upstream: {other:?}"),
        }
    }
}

#[tokio::test]
async fn downstream_disconnect_cancels_silent_upstream_and_observer_promptly() {
    use futures_util::StreamExt;
    let (parts, ()) = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .body(())
        .unwrap()
        .into_parts();
    let (request, response_rx) =
        crate::http::gateway_request::GatewayRequest::new(parts, Bytes::new());
    let (source_tx, source_rx) = mpsc::channel(2);
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    source_tx.send(GatewayByteStreamItem::Chunk(Bytes::from_static(
        b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
    ))).await.unwrap();
    let upstream = GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        reqwest::header::HeaderMap::new(),
        GatewayByteStream::from_receiver_with_cancel(source_rx, Some(cancel_tx)),
    );
    let worker = tokio::spawn(async move {
        let reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
            upstream,
            std::sync::Arc::new(std::sync::Mutex::new(PassthroughSseCollector::default())),
            SseKeepAliveFrame::Comment,
            std::time::Instant::now(),
        );
        request
            .respond_async(crate::http::gateway_response::Response::new(
                crate::http::gateway_response::StatusCode(200),
                vec![],
                reader,
                None,
            ))
            .await
    });
    let response = response_rx.await.unwrap();
    let mut body = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(2), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(first.windows(5).any(|bytes| bytes == b"hello"));
    // The producer stays alive and sends no terminal frame. Cancellation must
    // wake all readers, without waiting for the upstream idle timeout.
    drop(body);
    let result = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .expect("silent stream worker must finish on downstream disconnect")
        .unwrap();
    assert!(result.is_err());
    tokio::time::timeout(Duration::from_secs(2), cancel_rx)
        .await
        .expect("tee and observer release the upstream")
        .unwrap();
    drop(source_tx);
}

#[tokio::test]
async fn closing_primary_flushes_already_received_bytes_to_observer_eof() {
    let (provider, input) = mpsc::channel(256);
    let mut expected = Vec::new();
    for value in 0..150_u8 {
        provider
            .try_send(GatewayByteStreamItem::Chunk(Bytes::from(vec![value])))
            .unwrap();
        expected.push(value);
    }
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let (mut primary, observer) =
        GatewayByteStream::from_receiver_with_cancel(input, Some(cancel)).tee();
    primary.close();
    cancelled.await.unwrap();
    let received = tokio::time::timeout(Duration::from_secs(2), observer.read_all_bytes_async())
        .await
        .expect("closed input drains queued bytes without provider EOF")
        .unwrap();
    assert_eq!(received.as_ref(), expected.as_slice());
    assert!(provider.is_closed());
}

#[tokio::test]
async fn chat_tool_call_delta_is_delivered_before_upstream_terminal() {
    use crate::gateway::http_bridge::ChatCompletionsFromResponsesSseReader;
    use std::io::Read;
    let (source_tx, source_rx) = mpsc::channel(2);
    let first = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"weather\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"{\\\"city\\\":\"}\n\n"
    );
    source_tx
        .send(GatewayByteStreamItem::Chunk(Bytes::from_static(
            first.as_bytes(),
        )))
        .await
        .unwrap();
    let upstream = GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        reqwest::header::HeaderMap::new(),
        GatewayByteStream::from_receiver(source_rx),
    );
    let worker = tokio::task::spawn_blocking(move || {
        let mut reader = ChatCompletionsFromResponsesSseReader::from_stream_response(
            upstream,
            std::sync::Arc::new(std::sync::Mutex::new(PassthroughSseCollector::default())),
            std::time::Instant::now(),
        );
        let mut output = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = reader.read(&mut buffer).unwrap();
            assert!(read > 0);
            output.extend_from_slice(&buffer[..read]);
            if output.windows(10).any(|bytes| bytes == b"tool_calls") {
                return output;
            }
        }
    });
    let output = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .expect("tool call streams before a terminal event")
        .unwrap();
    assert!(String::from_utf8(output).unwrap().contains("weather"));
    drop(source_tx);
}
