//! Narrow wire normalization for ordinary Responses SDKs. Native Codex clients
//! keep provider-specific events; future unknown event kinds are not filtered.
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompatibleResponsesEvent {
    Preserve,
    Rewrite,
    Omit,
}

pub(crate) fn normalize_compatible_responses_event(event: &mut Value) -> CompatibleResponsesEvent {
    let kind = event.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("response.metadata" | "response.done")) {
        return CompatibleResponsesEvent::Preserve;
    }
    let error = [
        "/response/error",
        "/response/status_details/error",
        "/error",
    ]
    .iter()
    .find_map(|path| {
        event
            .pointer(path)
            .filter(|value| !value.is_null())
            .cloned()
    });
    let status = event.pointer("/response/status").and_then(Value::as_str);
    let failed = error.is_some()
        || status.is_some_and(|status| {
            !matches!(
                status,
                "completed" | "incomplete" | "in_progress" | "queued"
            )
        });
    if kind == Some("response.metadata") && !failed {
        // Observed on the official Codex backend, but rejected by the public
        // Responses SDK accumulator. HTTP observers consume its usage first.
        return CompatibleResponsesEvent::Omit;
    }
    let status = if failed || matches!(status, Some("in_progress" | "queued")) {
        "failed"
    } else if status == Some("incomplete") {
        "incomplete"
    } else {
        "completed"
    };
    event["type"] = json!(format!("response.{status}"));
    if let Some(response) = event.get_mut("response").and_then(Value::as_object_mut) {
        response.insert("status".into(), json!(status));
        if status == "failed" {
            response.insert("error".into(), error.unwrap_or_else(|| json!({"code":"upstream_response_failed","message":"upstream response did not complete successfully"})));
        }
    }
    CompatibleResponsesEvent::Rewrite
}

pub(crate) fn responses_event_text_for_client(
    text: &str,
    native_codex_client: bool,
) -> Option<String> {
    if native_codex_client {
        return Some(text.to_owned());
    }
    let Ok(mut event) = serde_json::from_str::<Value>(text) else {
        return Some(text.to_owned());
    };
    match normalize_compatible_responses_event(&mut event) {
        CompatibleResponsesEvent::Preserve => Some(text.to_owned()),
        CompatibleResponsesEvent::Rewrite => Some(event.to_string()),
        CompatibleResponsesEvent::Omit => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_responses_filters_only_confirmed_metadata_extension() {
        assert!(responses_event_text_for_client(
            r#"{"type":"response.metadata","metadata":{"fixture":true}}"#,
            false
        )
        .is_none());
        let future = r#"{"type":"response.future_extension","data":"unchanged"}"#;
        assert_eq!(
            responses_event_text_for_client(future, false).as_deref(),
            Some(future)
        );
    }

    #[test]
    fn native_codex_keeps_metadata_and_done_wire_bytes() {
        for text in [
            r#"{ "type": "response.metadata", "metadata": {"fixture":true} }"#,
            r#"{"type":"response.done","response":{"status":"completed"}}"#,
        ] {
            assert_eq!(
                responses_event_text_for_client(text, true).as_deref(),
                Some(text)
            );
        }
    }

    #[test]
    fn compatible_done_keeps_failed_and_incomplete_semantics() {
        for (status, expected) in [
            ("completed", "response.completed"),
            ("failed", "response.failed"),
            ("cancelled", "response.failed"),
            ("incomplete", "response.incomplete"),
            ("in_progress", "response.failed"),
        ] {
            let text = json!({"type":"response.done","response":{"status":status,"output":[]}})
                .to_string();
            let event: Value =
                serde_json::from_str(&responses_event_text_for_client(&text, false).unwrap())
                    .unwrap();
            assert_eq!(event["type"], expected);
        }
        let text = json!({"type":"response.done","response":{"status":"completed","error":{"code":"rate_limit","message":"quota"}}}).to_string();
        let event: Value =
            serde_json::from_str(&responses_event_text_for_client(&text, false).unwrap()).unwrap();
        assert_eq!(event["type"], "response.failed");
        assert_eq!(event["response"]["error"]["code"], "rate_limit");
    }

    #[test]
    fn compatible_websocket_frames_export_real_normalized_sdk_fixture() {
        let response = json!({"id":"resp_compat_ws","object":"response","created_at":1,"model":"test","status":"completed","output":[{"id":"msg_compat_ws","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"compatibility ok","annotations":[]}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5},"error":null,"incomplete_details":null});
        let mut created = response.clone();
        created["status"] = json!("in_progress");
        created["output"] = json!([]);
        let frames = [
            json!({"type":"response.created","sequence_number":0,"response":created}),
            json!({"type":"response.metadata","sequence_number":1,"metadata":{"fixture":true}}),
            json!({"type":"response.done","sequence_number":2,"response":response}),
        ];
        let output = frames
            .iter()
            .filter_map(|event| responses_event_text_for_client(&event.to_string(), false))
            .collect::<Vec<_>>();
        assert_eq!(output.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(&output[1]).unwrap()["type"],
            "response.completed"
        );
        if let Some(directory) = std::env::var_os("CM_REPAIR_FIXTURE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join("responses-sdk-compat-ws.jsonl"),
                output.join("\n") + "\n",
            )
            .unwrap();
        }
    }
}
