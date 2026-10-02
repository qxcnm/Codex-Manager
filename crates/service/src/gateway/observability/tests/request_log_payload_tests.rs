use super::*;
use codexmanager_core::storage::Storage;

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
}

fn job(trace_id: &str, body: &[u8], redact: bool, preview: bool) -> RequestLogPayloadJob {
    stage_job(trace_id, body, redact, preview, PAYLOAD_STAGE_UPSTREAM)
}

fn stage_job(
    trace_id: &str,
    body: &[u8],
    redact: bool,
    preview: bool,
    stage: &str,
) -> RequestLogPayloadJob {
    RequestLogPayloadJob {
        trace_id: trace_id.to_string(),
        stage: stage.to_string(),
        body: Bytes::copy_from_slice(body),
        conversation_key: Some("gk_test|conv-1".to_string()),
        redact,
        preview,
        created_at: 1_700_000_000,
    }
}

#[test]
fn payload_sanitizer_redacts_credential_like_keys() {
    let body = br#"{"model":"gpt-6-astra","api_key":"sk-secret-value","Authorization":"Bearer abc","nested":{"client_secret":"hidden","max_tokens":128},"refreshToken":"r-tok"}"#;
    let sanitized = sanitize_request_payload(body);
    let parsed: serde_json::Value = serde_json::from_str(&sanitized).expect("sanitized json");
    assert_eq!(parsed["model"], "gpt-6-astra");
    assert_eq!(parsed["api_key"], "[REDACTED]");
    assert_eq!(parsed["Authorization"], "[REDACTED]");
    assert_eq!(parsed["nested"]["client_secret"], "[REDACTED]");
    assert_eq!(parsed["nested"]["max_tokens"], 128);
    assert_eq!(parsed["refreshToken"], "[REDACTED]");
}

#[test]
fn payload_sanitizer_keeps_non_json_text() {
    assert_eq!(
        sanitize_request_payload(b"plain text body"),
        "plain text body"
    );
}

#[test]
fn payload_sanitizer_marks_non_utf8_body() {
    assert_eq!(
        sanitize_request_payload(&[0xff, 0xfe]),
        "<non-utf8 body omitted>"
    );
}

#[test]
fn payload_truncation_respects_utf8_boundary_and_reports_flag() {
    let text = "纯中文内容".repeat(64);
    let (kept, truncated) = truncate_utf8_payload(&text, 16);
    assert!(truncated);
    assert!(kept.len() <= 16);
    assert!(text.starts_with(&kept));

    let (kept_full, truncated_full) = truncate_utf8_payload("short", 16);
    assert_eq!(kept_full, "short");
    assert!(!truncated_full);
}

#[test]
fn preview_mode_stores_redacted_capped_preview() {
    let storage = storage();
    persist_request_log_payload(
        &storage,
        job(
            "trc_preview",
            br#"{"model":"gpt-6-astra","api_key":"sk-secret"}"#,
            true,
            true,
        ),
    );
    let stored = storage
        .find_request_log_payload_by_trace_id("trc_preview", PAYLOAD_STAGE_UPSTREAM)
        .expect("read payload")
        .expect("payload row exists");
    assert_eq!(stored.payload_bytes, 45);
    assert!(!stored.payload_truncated);
    assert!(stored.redacted);
    let parsed: serde_json::Value = serde_json::from_str(&stored.payload).expect("json");
    assert_eq!(parsed["api_key"], "[REDACTED]");
    assert_eq!(parsed["model"], "gpt-6-astra");
}

#[test]
fn preview_mode_without_redaction_keeps_original_bytes() {
    let storage = storage();
    let body = br#"{"model":"gpt-6-astra", "api_key":"sk-secret"}"#;
    persist_request_log_payload(&storage, job("trc_raw_preview", body, false, true));
    let stored = storage
        .find_request_log_payload_by_trace_id("trc_raw_preview", PAYLOAD_STAGE_UPSTREAM)
        .expect("read payload")
        .expect("payload row exists");
    assert!(!stored.redacted);
    assert_eq!(stored.payload.as_bytes(), body);
}

#[test]
fn preview_mode_truncates_large_bodies() {
    let storage = storage();
    let body = format!(
        "{{\"input\":\"{}\"}}",
        "x".repeat(REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES * 2)
    );
    persist_request_log_payload(&storage, job("trc_big", body.as_bytes(), true, true));
    let stored = storage
        .find_request_log_payload_by_trace_id("trc_big", PAYLOAD_STAGE_UPSTREAM)
        .expect("read payload")
        .expect("payload row exists");
    assert!(stored.payload_truncated);
    assert_eq!(stored.payload.len(), REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES);
    assert_eq!(stored.payload_bytes, body.len() as i64);
}

#[test]
fn full_mode_stores_untruncated_body_and_shares_conversation_prefix() {
    let storage = storage();
    let big_text = "y".repeat(REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES * 3);
    let first = format!(
        r#"{{"model":"gpt-6-astra","api_key":"sk-secret","tools":[{{"type":"function","name":"shell"}}],"input":[{{"role":"user","content":"{big_text}"}}]}}"#
    );
    let second = format!(
        r#"{{"model":"gpt-6-astra","api_key":"sk-secret","tools":[{{"type":"function","name":"shell"}}],"input":[{{"role":"user","content":"{big_text}"}},{{"role":"assistant","content":"ok"}},{{"role":"user","content":"next"}}]}}"#
    );
    let mut cache = ParentCache::default();
    persist_request_log_payload_with_cache(
        &storage,
        job("trc_full_1", first.as_bytes(), false, false),
        &mut cache,
    );
    persist_request_log_payload_with_cache(
        &storage,
        job("trc_full_2", second.as_bytes(), false, false),
        &mut cache,
    );

    assert!(storage
        .find_request_log_payload_by_trace_id("trc_full_2", PAYLOAD_STAGE_CLIENT)
        .expect("read preview")
        .is_none());
    let manifest = storage
        .find_request_log_payload_manifest("trc_full_2", PAYLOAD_STAGE_UPSTREAM)
        .expect("read manifest")
        .expect("manifest exists");
    assert_eq!(manifest.parent_trace_id.as_deref(), Some("trc_full_1"));
    assert_eq!(manifest.shared_prefix_len, 1);
    assert_eq!(manifest.item_count, 3);
    assert_eq!(manifest.list_field.as_deref(), Some("input"));
    assert!(!manifest.redacted);

    let full = storage
        .load_request_log_payload_full("trc_full_2", PAYLOAD_STAGE_UPSTREAM)
        .expect("load")
        .expect("exists");
    assert!(full.complete);
    assert_eq!(full.items.len(), 3);
    let first_item: serde_json::Value = serde_json::from_str(&full.items[0]).expect("item json");
    assert_eq!(
        first_item["content"].as_str().map(str::len),
        Some(big_text.len())
    );
    let api_key = full
        .fields
        .iter()
        .find(|(name, _)| name == "api_key")
        .map(|(_, value)| value.as_str());
    assert_eq!(api_key, Some("\"sk-secret\""));
}

#[test]
fn full_mode_redaction_masks_fields_and_items() {
    let storage = storage();
    let body = br#"{"model":"m","api_key":"sk-secret","messages":[{"role":"user","content":"hi","password":"p"}]}"#;
    persist_request_log_payload(&storage, job("trc_full_redacted", body, true, false));
    let full = storage
        .load_request_log_payload_full("trc_full_redacted", PAYLOAD_STAGE_UPSTREAM)
        .expect("load")
        .expect("exists");
    assert!(full.manifest.redacted);
    assert_eq!(full.manifest.list_field.as_deref(), Some("messages"));
    let api_key = full
        .fields
        .iter()
        .find(|(name, _)| name == "api_key")
        .map(|(_, value)| value.clone());
    assert_eq!(api_key.as_deref(), Some("\"[REDACTED]\""));
    let item: serde_json::Value = serde_json::from_str(&full.items[0]).expect("item");
    assert_eq!(item["password"], "[REDACTED]");
    assert_eq!(item["content"], "hi");
}

#[test]
fn split_detects_protocol_list_fields_and_previous_response_id() {
    let gemini = job(
        "trc_gemini",
        br#"{"systemInstruction":{"parts":[{"text":"sys"}]},"contents":[{"role":"user","parts":[{"text":"a"}]}]}"#,
        false,
        false,
    );
    let split = split_request_payload(&gemini);
    assert_eq!(split.list_field.as_deref(), Some("contents"));
    assert_eq!(split.body_kind, "json_list");
    assert_eq!(split.items.len(), 1);
    assert_eq!(split.fields.len(), 1);

    let follow_up = job(
        "trc_follow",
        br#"{"previous_response_id":"resp_123","input":[{"role":"user","content":"more"}]}"#,
        false,
        false,
    );
    let split = split_request_payload(&follow_up);
    assert_eq!(split.previous_response_id.as_deref(), Some("resp_123"));
    assert_eq!(split.list_field.as_deref(), Some("input"));

    let text = job("trc_text", b"not json", false, false);
    let split = split_request_payload(&text);
    assert_eq!(split.body_kind, "text");
    assert_eq!(split.fields[0].0, RAW_BODY_FIELD);
    assert_eq!(split.fields[0].1.content, "not json");

    let binary = job("trc_bin", &[0xff, 0x00, 0x01], false, false);
    let split = split_request_payload(&binary);
    assert_eq!(split.body_kind, "base64");
    assert_eq!(split.fields[0].1.content, "/wAB");
}

#[test]
fn conversation_key_is_scoped_by_api_key() {
    assert_eq!(
        request_log_payload_conversation_key("gk_1", Some(" conv ")),
        "gk_1|conv"
    );
    assert_eq!(request_log_payload_conversation_key("gk_1", None), "gk_1|~");
    assert_eq!(
        request_log_payload_conversation_key("gk_1", Some("")),
        "gk_1|~"
    );
}

#[test]
fn upstream_capture_is_skipped_when_client_body_is_unchanged() {
    let storage = storage();
    let body = br#"{"model":"m","input":[{"role":"user","content":"hi"}]}"#;
    persist_request_log_payload(
        &storage,
        stage_job("trc_same", body, false, false, PAYLOAD_STAGE_CLIENT),
    );
    persist_request_log_payload(
        &storage,
        stage_job("trc_same", body, false, false, PAYLOAD_STAGE_UPSTREAM),
    );
    assert_eq!(
        storage
            .list_request_log_payload_manifest_stages("trc_same")
            .expect("stages"),
        vec![PAYLOAD_STAGE_CLIENT.to_string()]
    );

    let rewritten = br#"{"model":"m","input":[{"role":"user","content":"hi"},{"role":"user","content":"more"}]}"#;
    persist_request_log_payload(
        &storage,
        stage_job("trc_same", rewritten, false, false, PAYLOAD_STAGE_UPSTREAM),
    );
    assert_eq!(
        storage
            .list_request_log_payload_manifest_stages("trc_same")
            .expect("stages"),
        vec![
            PAYLOAD_STAGE_CLIENT.to_string(),
            PAYLOAD_STAGE_UPSTREAM.to_string()
        ]
    );
    let upstream = storage
        .load_request_log_payload_full("trc_same", PAYLOAD_STAGE_UPSTREAM)
        .expect("load upstream")
        .expect("upstream exists");
    assert_eq!(upstream.items.len(), 2);
}

#[test]
fn preview_upstream_capture_is_skipped_when_unchanged() {
    let storage = storage();
    let body = br#"{"model":"m","input":"hi"}"#;
    persist_request_log_payload(
        &storage,
        stage_job("trc_pv", body, false, true, PAYLOAD_STAGE_CLIENT),
    );
    persist_request_log_payload(
        &storage,
        stage_job("trc_pv", body, false, true, PAYLOAD_STAGE_UPSTREAM),
    );
    assert_eq!(
        storage
            .list_request_log_payload_stages("trc_pv")
            .expect("stages"),
        vec![PAYLOAD_STAGE_CLIENT.to_string()]
    );

    let rewritten = br#"{"model":"m","input":"hi there"}"#;
    persist_request_log_payload(
        &storage,
        stage_job("trc_pv", rewritten, false, true, PAYLOAD_STAGE_UPSTREAM),
    );
    assert_eq!(
        storage
            .list_request_log_payload_stages("trc_pv")
            .expect("stages"),
        vec![
            PAYLOAD_STAGE_CLIENT.to_string(),
            PAYLOAD_STAGE_UPSTREAM.to_string()
        ]
    );
}
