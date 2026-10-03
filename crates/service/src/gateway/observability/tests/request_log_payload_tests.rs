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
        generation: 0,
        attempt: None,
    }
}

#[test]
fn paused_real_writer_cannot_restore_cleared_payloads() {
    use codexmanager_core::rpc::types::RequestLogDetailParams;
    use codexmanager_core::storage::{RequestLog, RequestTokenStat};
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    for preview in [true, false] {
        let storage = storage();
        let trace_id = if preview {
            "trc_stale_preview"
        } else {
            "trc_stale_full"
        };
        storage
            .insert_request_log_with_token_stat(
                &RequestLog {
                    trace_id: Some(trace_id.to_string()),
                    key_id: Some("gk_test".to_string()),
                    request_path: "/v1/responses".to_string(),
                    method: "POST".to_string(),
                    created_at: now_ts(),
                    ..Default::default()
                },
                &RequestTokenStat::default(),
            )
            .expect("seed log");
        let (tx, rx) = sync_channel(2);
        let (ready_tx, ready_rx) = sync_channel(0);
        let (release_tx, release_rx) = sync_channel(0);
        let writer_storage = storage.shared_handle();
        let writer = std::thread::spawn(move || {
            run_payload_writer(
                rx,
                || Some(Box::new(writer_storage.shared_handle())),
                || {
                    ready_tx.send(()).expect("report pause");
                    release_rx.recv().expect("resume writer");
                },
            );
        });
        let mut queued = stage_job(
            trace_id,
            br#"{"input":[{"role":"user","content":"before clear"}]}"#,
            false,
            preview,
            PAYLOAD_STAGE_CLIENT,
        );
        queued.created_at = now_ts();
        queued.generation = storage.request_log_payload_generation().unwrap();
        tx.send(queued).unwrap();
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer paused");
        storage
            .clear_request_logs()
            .expect("clear while job is paused");
        release_tx.send(()).unwrap();
        drop(tx);
        writer.join().expect("writer finished");
        assert!(storage
            .find_request_log_payload_by_trace_id(trace_id, PAYLOAD_STAGE_CLIENT)
            .unwrap()
            .is_none());
        assert!(storage
            .find_request_log_payload_manifest(trace_id, PAYLOAD_STAGE_CLIENT)
            .unwrap()
            .is_none());
        assert_eq!(storage.request_log_payload_generation().unwrap(), 1);
        assert_eq!(
            crate::requestlog::detail::read_request_log_detail(
                &storage,
                &RequestLogDetailParams {
                    trace_id: trace_id.to_string(),
                    stage: None
                },
                None,
            )
            .expect_err("cleared trace not visible"),
            "request log detail not found"
        );
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
fn preview_redacts_root_array_before_rejected_request_can_be_logged() {
    let storage = storage();
    let body = br#"[{"api_key":"sk-dummy-secret","nested":{"password":"dummy-password"}}]"#;
    persist_request_log_payload(
        &storage,
        stage_job("trc_rejected_array", body, true, true, PAYLOAD_STAGE_CLIENT),
    );
    let stored = storage
        .find_request_log_payload_by_trace_id("trc_rejected_array", PAYLOAD_STAGE_CLIENT)
        .expect("load preview")
        .expect("capture before request validation");
    let parsed: Value = serde_json::from_str(&stored.payload).expect("valid array");
    assert_eq!(parsed[0]["api_key"], "[REDACTED]");
    assert_eq!(parsed[0]["nested"]["password"], "[REDACTED]");
    assert!(!stored.payload.contains("sk-dummy-secret"));
    assert!(!stored.payload.contains("dummy-password"));
    assert!(stored.redacted);
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
fn actual_upstream_attempt_inherits_authenticated_client_session() {
    let storage = storage();
    let mut cache = ParentCache::default();
    let mut client = stage_job(
        "trc_session",
        br#"{"input":[{"role":"user","content":"hello"}]}"#,
        false,
        false,
        PAYLOAD_STAGE_CLIENT,
    );
    client.conversation_key = Some("gk_test|session_A".to_string());
    persist_request_log_payload_with_cache(&storage, client, &mut cache);
    let mut upstream = stage_job(
        "trc_session",
        br#"{"input":[{"role":"user","content":"hello"}],"model":"different"}"#,
        false,
        false,
        PAYLOAD_STAGE_UPSTREAM,
    );
    upstream.conversation_key = Some("gk_test|~".to_string());
    upstream.attempt = Some(OutboundAttemptCapture {
        method: "POST".to_string(),
        url: "http://127.0.0.1/v1/responses".to_string(),
        transport: "http".to_string(),
        content_encoding: None,
        wire_body: upstream.body.clone(),
    });
    persist_request_log_payload_with_cache(&storage, upstream, &mut cache);
    let stored = storage
        .find_request_log_payload_manifest("trc_session", PAYLOAD_STAGE_UPSTREAM)
        .expect("upstream manifest")
        .expect("rewritten body stored");
    assert_eq!(
        stored.conversation_key.as_deref(),
        Some("gk_test|session_A")
    );
}

#[test]
fn cached_parent_is_not_reused_after_retention_pruning() {
    let storage = storage();
    let mut cache = ParentCache::default();
    let mut earlier = job(
        "trc_earlier",
        br#"{"input":[{"role":"user","content":"old"}]}"#,
        false,
        false,
    );
    let cutoff = now_ts() - 30;
    earlier.created_at = cutoff - 1;
    persist_request_log_payload_with_cache(&storage, earlier, &mut cache);
    storage
        .prune_request_logs_before(cutoff)
        .expect("retention prune");
    let mut later = job(
        "trc_later",
        br#"{"input":[{"role":"user","content":"old"},{"role":"user","content":"new"}]}"#,
        false,
        false,
    );
    later.created_at = cutoff + 1;
    persist_request_log_payload_with_cache(&storage, later, &mut cache);
    let full = storage
        .load_request_log_payload_full("trc_later", PAYLOAD_STAGE_UPSTREAM)
        .expect("read payload")
        .expect("later request kept");
    assert!(full.complete);
    assert!(full.manifest.parent_trace_id.is_none());
    assert_eq!(full.items.len(), 2);
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
