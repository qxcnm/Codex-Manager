use super::{parse_images_response_body, ImagesResponseFormat};
use crate::gateway::http_bridge::body_conversion::convert_responses_body_to_images;
use crate::gateway::http_bridge::stream_readers::UpstreamSseFramePump;
use crate::gateway::http_bridge::{ImagesFromResponsesSseReader, PassthroughSseCollector};
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
use bytes::Bytes;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn image_response() -> Value {
    json!({
        "object": "response", "status": "completed", "created_at": 1775900000,
        "output": [{"type": "image_generation_call", "status": "completed",
            "result": "aGVsbG8=", "output_format": "webp", "revised_prompt": "red apple"}],
        "tool_usage": {"image_gen": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3}}
    })
}

fn event(kind: &str, value: Value) -> String {
    format!("event: {kind}\ndata: {value}\n\n")
}

fn completed_sse(response: Value) -> String {
    event(
        "response.completed",
        json!({"type": "response.completed", "response": response}),
    )
}

#[test]
fn non_stream_images_accepts_responses_sse_in_both_output_formats() {
    let body = completed_sse(image_response());
    for (format, field, expected) in [
        (ImagesResponseFormat::B64Json, "b64_json", "aGVsbG8="),
        (
            ImagesResponseFormat::Url,
            "url",
            "data:image/webp;base64,aGVsbG8=",
        ),
    ] {
        let mapped = convert_responses_body_to_images(body.as_bytes(), format).unwrap();
        let mapped: Value = serde_json::from_slice(&mapped).unwrap();
        assert_eq!(mapped["created"], 1775900000);
        assert_eq!(mapped["data"][0][field], expected);
        assert_eq!(mapped["data"][0]["revised_prompt"], "red apple");
        assert_eq!(mapped["usage"]["total_tokens"], 3);
        assert!(mapped.get("output").is_none());
    }
}

#[test]
fn non_stream_images_accepts_bom_and_sse_metadata_before_completion() {
    let response = image_response();
    let mut expected = response.clone();
    expected["created"] = response["created_at"].clone();
    let payload = json!({"type": "response.completed", "response": response});
    for prefix in [
        "\u{feff}",
        "id: image-1\n",
        "retry: 1000\n",
        "\u{feff}id: image-1\nretry: 1000\n\n",
    ] {
        for frame in [
            completed_sse(response.clone()),
            format!("data: {payload}\n\n"),
        ] {
            let body = format!("{prefix}{frame}");
            assert_eq!(
                parse_images_response_body(body.as_bytes()).unwrap(),
                expected
            );
            let mapped =
                convert_responses_body_to_images(body.as_bytes(), ImagesResponseFormat::B64Json)
                    .unwrap();
            let mapped: Value = serde_json::from_slice(&mapped).unwrap();
            assert_eq!(mapped["data"][0]["b64_json"], "aGVsbG8=");
            assert_eq!(mapped["usage"]["total_tokens"], 3);
        }
    }
}

#[test]
fn non_stream_images_preserves_prefixed_sse_failures_and_rejects_truncation() {
    let failed = json!({"type": "response.failed", "response": {"status": "failed", "error": {"message": "image generation rejected"}}});
    for prefix in [
        "\u{feff}",
        "id: image-1\n",
        "retry: 1000\n",
        "\u{feff}id: image-1\nretry: 1000\n\n",
    ] {
        for frame in [
            event("response.failed", failed.clone()),
            format!("data: {failed}\n\n"),
        ] {
            let error =
                parse_images_response_body(format!("{prefix}{frame}").as_bytes()).unwrap_err();
            assert!(error.contains("image generation rejected"), "{error}");
        }
        let incomplete =
            format!("{prefix}data: {{\"type\":\"response.created\"}}\n\ndata: [DONE]\n\n");
        assert!(parse_images_response_body(incomplete.as_bytes())
            .unwrap_err()
            .contains("without a completed response"));
    }
}

#[test]
fn sse_detection_keeps_json_and_invalid_payloads_distinct() {
    use super::looks_like_sse_payload;
    for body in [
        r#"{"id":"image-1","retry":1000,"data":"event: response.completed"}"#,
        "<html>upstream error</html>",
        "not an event stream",
    ] {
        assert!(!looks_like_sse_payload(body.as_bytes()), "{body}");
    }
    assert!(!looks_like_sse_payload(b"\xffdata: invalid utf8"));
}

#[test]
fn non_stream_images_honors_declared_sse_with_ignored_extension_fields() {
    let response = image_response();
    let mut expected = response.clone();
    expected["created"] = response["created_at"].clone();
    let body = format!(
        "vendor-metadata: ignored\n{}",
        completed_sse(response.clone())
    );
    assert_eq!(
        super::parse_images_response_body_with_sse(body.as_bytes(), true).unwrap(),
        expected
    );
    assert!(
        super::parse_images_response_body_with_sse(b"vendor-metadata: ignored\n\n", true).is_err()
    );
    assert_eq!(
        super::parse_images_response_body_with_sse(response.to_string().as_bytes(), true).unwrap(),
        response,
        "valid JSON stays accepted even when its Content-Type advertises SSE"
    );
}

#[test]
fn non_stream_images_retains_output_item_done_when_completion_output_is_empty() {
    let response = image_response();
    let item = response["output"][0].clone();
    let body = event(
        "response.output_item.done",
        json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
    ) + completed_sse(json!({"status": "completed", "output": []})).as_str();
    let mapped = convert_responses_body_to_images(
        body.replace('\n', "\r\n").as_bytes(),
        ImagesResponseFormat::B64Json,
    )
    .unwrap();
    let mapped: Value = serde_json::from_slice(&mapped).unwrap();
    assert_eq!(mapped["data"][0]["b64_json"], "aGVsbG8=");
}

#[test]
fn non_stream_images_rejects_failed_and_incomplete_responses_with_upstream_reason() {
    for kind in ["response.failed", "response.incomplete", "error"] {
        let failure = event(
            kind,
            json!({"type": kind, "response": {"status": "failed", "error": {
                "type": "image_generation_user_error", "code": "invalid_value",
                "message": "Complexity 'medium' is not supported for this image model.",
                "param": "complexity"
            }}}),
        );
        // Even an earlier successful-looking snapshot must not hide a failure.
        for prefix in [String::new(), completed_sse(image_response())] {
            let body = prefix + failure.as_str();
            let error = parse_images_response_body(body.as_bytes()).unwrap_err();
            assert!(error.contains("Complexity 'medium'"), "{error}");
            assert!(error.contains("invalid_value"), "{error}");
            assert!(error.contains("complexity"), "{error}");
            assert!(convert_responses_body_to_images(
                body.as_bytes(),
                ImagesResponseFormat::B64Json
            )
            .is_none());
        }
    }
}

#[test]
fn non_stream_images_rejects_nonterminal_or_empty_payloads() {
    let partial = event(
        "response.image_generation_call.partial_image",
        json!({"type": "response.image_generation_call.partial_image", "partial_image_b64": "partial"}),
    );
    for body in [
        partial + "data: [DONE]\n\n",
        completed_sse(json!({"status": "completed", "output": []})),
        completed_sse(json!({"status": "failed", "output": image_response()["output"]})),
        json!({"status": "in_progress", "output": image_response()["output"]}).to_string(),
        "not a response".to_string(),
    ] {
        assert!(
            parse_images_response_body(body.as_bytes()).is_err(),
            "{body}"
        );
    }
    let body = json!({"error": {"message": "invalid image model"}}).to_string();
    assert!(parse_images_response_body(body.as_bytes())
        .unwrap_err()
        .contains("invalid image model"));
}

#[test]
fn non_stream_images_rejects_failed_json_envelopes_before_unwrapping_response() {
    for kind in ["error", "response.failed", "response.incomplete"] {
        let value = json!({"type": kind, "response": image_response()});
        let error = parse_images_response_body(value.to_string().as_bytes()).unwrap_err();
        assert!(error.contains(kind), "{error}");
    }
    let value = json!({
        "type": "response.failed", "response": image_response(),
        "error": {"message": "invalid image model", "param": "model", "code": "invalid_value"}
    });
    let error = parse_images_response_body(value.to_string().as_bytes()).unwrap_err();
    assert!(error.contains("invalid image model"), "{error}");
    assert!(error.contains("param=model"), "{error}");
    assert!(error.contains("code=invalid_value"), "{error}");
}

fn read_images_stream(
    body: &str,
    response_format: ImagesResponseFormat,
) -> (String, PassthroughSseCollector) {
    let chunks = body.as_bytes().chunks(17).collect::<Vec<_>>();
    let (tx, rx) = tokio::sync::mpsc::channel(chunks.len() + 1);
    for chunk in chunks {
        tx.try_send(GatewayByteStreamItem::Chunk(Bytes::copy_from_slice(chunk)))
            .unwrap();
    }
    tx.try_send(GatewayByteStreamItem::Eof).unwrap();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ImagesFromResponsesSseReader::from_pump(
        UpstreamSseFramePump::from_stream(GatewayByteStream::from_receiver(rx)),
        Arc::clone(&collector),
        Instant::now(),
        response_format,
    );
    let mut mapped = String::new();
    reader.read_to_string(&mut mapped).unwrap();
    let snapshot = collector.lock().unwrap().clone();
    (mapped, snapshot)
}

#[test]
fn images_accepts_message_wrapped_completion_for_json_and_streaming() {
    let payload = json!({"type": "response.completed", "response": image_response()});
    for body in [
        event("message", payload.clone()),
        format!("data: {payload}\n\n"),
    ] {
        let mapped =
            convert_responses_body_to_images(body.as_bytes(), ImagesResponseFormat::B64Json)
                .expect("completed Responses payload is valid without a specific SSE event header");
        let mapped: Value = serde_json::from_slice(&mapped).unwrap();
        assert_eq!(mapped["data"][0]["b64_json"], "aGVsbG8=");

        let (mapped, collector) = read_images_stream(&body, ImagesResponseFormat::B64Json);
        assert!(
            mapped.contains("event: image_generation.completed\n"),
            "{mapped}"
        );
        assert!(!mapped.contains("event: error\n"), "{mapped}");
        assert!(collector.saw_terminal);
        assert!(collector.usage.completed_successfully);
    }
}

#[test]
fn images_stream_keeps_partial_and_completed_events() {
    let body = event(
        "response.image_generation_call.partial_image",
        json!({"type": "response.image_generation_call.partial_image",
            "partial_image_b64": "partial", "partial_image_index": 1, "output_format": "webp"}),
    ) + completed_sse(image_response()).as_str();
    for format in [ImagesResponseFormat::B64Json, ImagesResponseFormat::Url] {
        let (mapped, collector) = read_images_stream(&body, format);
        assert!(mapped.contains("event: image_generation.partial_image\n"));
        assert!(mapped.contains("event: image_generation.completed\n"));
        let values = mapped
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str::<Value>(data).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        match format {
            ImagesResponseFormat::B64Json => {
                assert_eq!(values[0]["b64_json"], "partial");
                assert_eq!(values[1]["b64_json"], "aGVsbG8=");
            }
            ImagesResponseFormat::Url => {
                assert_eq!(values[0]["url"], "data:image/webp;base64,partial");
                assert_eq!(values[1]["url"], "data:image/webp;base64,aGVsbG8=");
            }
        }
        assert!(collector.saw_terminal);
        assert!(collector.usage.completed_successfully);
        assert!(collector.terminal_error.is_none());
    }
}

#[test]
fn images_stream_emits_error_instead_of_completed_for_failed_or_empty_response() {
    for body in [
        event(
            "response.failed",
            json!({"type": "response.failed", "response": {
                "status": "failed", "error": {"message": "unsupported complexity"}
            }}),
        ),
        completed_sse(json!({"status": "completed", "output": []})),
    ] {
        let (mapped, collector) = read_images_stream(&body, ImagesResponseFormat::B64Json);
        assert!(mapped.contains("event: error\n"), "{mapped}");
        assert!(!mapped.contains("image_generation.completed"), "{mapped}");
        assert!(!mapped.contains("response.failed"), "{mapped}");
        assert!(collector.saw_terminal);
        assert!(collector.usage.explicit_failure);
        assert!(!collector.usage.completed_successfully);
        assert!(collector.terminal_error.is_some());
    }
}
