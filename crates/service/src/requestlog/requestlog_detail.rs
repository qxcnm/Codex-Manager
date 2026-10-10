use codexmanager_core::rpc::types::{
    RequestLogAttemptDetail, RequestLogDetailContextSegment, RequestLogDetailField,
    RequestLogDetailParams, RequestLogDetailResult,
};
use codexmanager_core::storage::{RequestLogPayloadFull, Storage};

/// Upper bound of earlier requests stitched in front of a
/// `previous_response_id` continuation.
const MAX_CONTEXT_SEGMENTS: usize = 32;

/// Load the stored request payload for one gateway trace.
///
/// `allowed_key_ids` enforces member scoping: `None` grants the
/// administrator view, while `Some(key_ids)` restricts access to payloads
/// whose request log row belongs to one of the member's API keys.
/// Both a missing log row and an out-of-scope key resolve to the same
/// "not found" error so trace ids cannot be probed for existence.
pub(crate) fn read_request_log_detail(
    storage: &Storage,
    params: &RequestLogDetailParams,
    allowed_key_ids: Option<&[String]>,
) -> Result<RequestLogDetailResult, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return Err(
            "request log detail requires the sqlite storage backend; remote SeaORM storage is not supported yet"
                .to_string(),
        );
    }
    let trace_id = params.trace_id.trim();
    if trace_id.is_empty() {
        return Err("trace_id must not be empty".to_string());
    }
    let log_key_id = storage
        .find_request_log_key_id_by_trace_id(trace_id)
        .map_err(|err| format!("read request log owner failed: {err}"))?
        .ok_or_else(|| "request log detail not found".to_string())?;
    if let Some(allowed_key_ids) = allowed_key_ids {
        let owned = log_key_id.as_ref().is_some_and(|key_id| {
            allowed_key_ids
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(key_id.trim()))
        });
        if !owned {
            return Err("request log detail not found".to_string());
        }
    }

    let stages = available_request_log_payload_stages(storage, trace_id)?;
    for stage in candidate_stages(&stages, params.stage.as_deref()) {
        let attempt = storage
            .find_request_log_upstream_attempt(trace_id, stage.as_str())
            .map_err(|err| format!("read request log attempt failed: {err}"))?;
        let attempt_meta = attempt.as_ref().map(|attempt| RequestLogAttemptDetail {
            method: attempt.method.clone(),
            url: attempt.url.clone(),
            transport: attempt.transport.clone(),
            content_encoding: attempt.content_encoding.clone(),
            wire_sha256: attempt.wire_sha256.clone(),
            identical_to_client: attempt.identical_to_client,
        });
        // When the outbound body is identical to the client body it has no
        // duplicate payload row, but it still has its own attempt metadata.
        let lookup_stages = if attempt
            .as_ref()
            .is_some_and(|attempt| attempt.identical_to_client)
        {
            vec![
                stage.as_str(),
                codexmanager_core::storage::PAYLOAD_STAGE_CLIENT,
            ]
        } else {
            vec![stage.as_str()]
        };
        for lookup_stage in lookup_stages {
            let full = storage
                .load_request_log_payload_full(trace_id, lookup_stage)
                .map_err(|err| format!("read request log payload failed: {err}"))?;
            if let Some(full) = full {
                let context =
                    load_previous_response_context(storage, &full, log_key_id.as_deref())?;
                let mut result = full_detail(full, context);
                result.stage = stage.clone();
                result.stages = stages.clone();
                result.attempt = attempt_meta.clone();
                return Ok(result);
            }
            let payload = storage
                .find_request_log_payload_by_trace_id(trace_id, lookup_stage)
                .map_err(|err| format!("read request log payload failed: {err}"))?;
            if let Some(payload) = payload {
                return Ok(RequestLogDetailResult {
                    trace_id: payload.trace_id,
                    stage: stage.clone(),
                    stages: stages.clone(),
                    attempt: attempt_meta.clone(),
                    storage_mode: "preview".to_string(),
                    payload: payload.payload,
                    payload_bytes: payload.payload_bytes,
                    payload_truncated: payload.payload_truncated,
                    redacted: payload.redacted,
                    created_at: payload.created_at,
                    complete: !payload.payload_truncated,
                    ..Default::default()
                });
            }
        }
    }
    Err("request log detail not found".to_string())
}

/// Capture stages stored for this trace, ordered `client` before `upstream`.
fn available_request_log_payload_stages(
    storage: &Storage,
    trace_id: &str,
) -> Result<Vec<String>, String> {
    let mut stages = storage
        .list_request_log_payload_manifest_stages(trace_id)
        .map_err(|err| format!("read request log payload stages failed: {err}"))?;
    for stage in storage
        .list_request_log_payload_stages(trace_id)
        .map_err(|err| format!("read request log payload stages failed: {err}"))?
    {
        if !stages.contains(&stage) {
            stages.push(stage);
        }
    }
    for stage in storage
        .list_request_log_upstream_attempt_stages(trace_id)
        .map_err(|err| format!("read request log attempt stages failed: {err}"))?
    {
        if !stages.contains(&stage) {
            stages.push(stage);
        }
    }
    stages.sort();
    Ok(stages)
}

/// Stage lookup order: an explicitly requested stage wins, otherwise the body
/// actually forwarded upstream, falling back to the body as received from the
/// client when the request never reached the upstream.
fn candidate_stages(stages: &[String], requested: Option<&str>) -> Vec<String> {
    if let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        return vec![requested.to_string()];
    }
    let mut candidates: Vec<String> = stages
        .iter()
        .rev()
        .filter(|stage| stage.starts_with(codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM))
        .cloned()
        .collect();
    candidates.push(codexmanager_core::storage::PAYLOAD_STAGE_CLIENT.to_string());
    candidates
}

fn full_detail(
    full: RequestLogPayloadFull,
    context: Vec<RequestLogDetailContextSegment>,
) -> RequestLogDetailResult {
    let manifest = full.manifest;
    RequestLogDetailResult {
        trace_id: manifest.trace_id,
        stage: manifest.stage.clone(),
        stages: Vec::new(),
        attempt: None,
        storage_mode: "full".to_string(),
        payload: String::new(),
        payload_bytes: manifest.payload_bytes,
        payload_truncated: false,
        redacted: manifest.redacted,
        created_at: manifest.created_at,
        body_kind: Some(manifest.body_kind),
        list_field: manifest.list_field,
        complete: full.complete,
        fields: full
            .fields
            .into_iter()
            .map(|(name, value)| RequestLogDetailField { name, value })
            .collect(),
        items: full.items,
        inherited_item_count: manifest.shared_prefix_len,
        parent_trace_id: manifest.parent_trace_id,
        previous_response_id: manifest.previous_response_id,
        context,
    }
}

/// A request carrying `previous_response_id` only contains the new turn; the
/// earlier turns live in previous requests of the same conversation. Walk
/// back through them (stopping at the first request that carries its full
/// history) and return them oldest first. The model outputs referenced by
/// `previous_response_id` are not part of any request body and are therefore
/// not available here.
fn load_previous_response_context(
    storage: &Storage,
    full: &RequestLogPayloadFull,
    key_id: Option<&str>,
) -> Result<Vec<RequestLogDetailContextSegment>, String> {
    let mut segments = Vec::new();
    let Some(key_id) = key_id else {
        return Ok(segments);
    };
    let mut cursor = full.manifest.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(cursor.trace_id.clone());
    for _ in 0..MAX_CONTEXT_SEGMENTS {
        let Some(response_id) = cursor.previous_response_id.as_deref() else {
            break;
        };
        let predecessor_trace = storage
            .find_request_log_trace_for_response_id(key_id, response_id)
            .map_err(|err| format!("read request log context failed: {err}"))?;
        let Some(predecessor_trace) = predecessor_trace else {
            break;
        };
        if !seen.insert(predecessor_trace.clone()) {
            break;
        }
        let previous_stages = if cursor
            .stage
            .starts_with(codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM)
        {
            let mut stages = storage
                .list_request_log_upstream_attempt_stages(&predecessor_trace)
                .map_err(|err| format!("read request log context failed: {err}"))?;
            stages.sort();
            stages.reverse();
            stages.push(codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM.to_string());
            stages.push(codexmanager_core::storage::PAYLOAD_STAGE_CLIENT.to_string());
            stages
        } else {
            vec![codexmanager_core::storage::PAYLOAD_STAGE_CLIENT.to_string()]
        };
        let mut previous = None;
        for stage in previous_stages {
            previous = storage
                .load_request_log_payload_full(&predecessor_trace, stage.as_str())
                .map_err(|err| format!("read request log context failed: {err}"))?;
            if previous.is_some() {
                break;
            }
        }
        let Some(previous) = previous else {
            break;
        };
        // A concrete conversation ID must not cross to a different concrete
        // session, even when a response ID happens to match.
        match (
            cursor.conversation_key.as_deref(),
            previous.manifest.conversation_key.as_deref(),
        ) {
            (Some(current), Some(parent))
                if current != parent && (!current.ends_with("|~") || !parent.ends_with("|~")) =>
            {
                break;
            }
            (None, Some(_)) | (Some(_), None) => break,
            _ => {}
        }
        let continues = previous.manifest.previous_response_id.is_some();
        segments.push(RequestLogDetailContextSegment {
            trace_id: previous.manifest.trace_id.clone(),
            created_at: previous.manifest.created_at,
            previous_response_id: previous.manifest.previous_response_id.clone(),
            list_field: previous.manifest.list_field.clone(),
            complete: previous.complete,
            items: previous.items,
        });
        if !continues {
            break;
        }
        cursor = previous.manifest;
    }
    segments.reverse();
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{
        RequestLog, RequestLogPayload, RequestLogPayloadManifestInput, RequestLogPayloadPart,
        RequestTokenStat,
    };

    fn storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        storage.init().expect("run storage migrations");
        storage
    }

    fn seed_trace(storage: &Storage, trace_id: &str, key_id: Option<&str>) {
        storage
            .insert_request_log_with_token_stat(
                &RequestLog {
                    trace_id: Some(trace_id.to_string()),
                    key_id: key_id.map(str::to_string),
                    request_path: "/v1/responses".to_string(),
                    method: "POST".to_string(),
                    created_at: 1_700_000_000,
                    ..Default::default()
                },
                &RequestTokenStat::default(),
            )
            .expect("insert request log");
    }

    fn seed_payload(storage: &Storage, trace_id: &str) {
        storage
            .insert_request_log_payload(&RequestLogPayload {
                trace_id: trace_id.to_string(),
                stage: codexmanager_core::storage::PAYLOAD_STAGE_CLIENT.to_string(),
                body_hash: format!("hash-{trace_id}"),
                payload: "{\"model\":\"gpt-6-astra\"}".to_string(),
                payload_bytes: 26,
                payload_truncated: false,
                redacted: true,
                created_at: 1_700_000_000,
            })
            .expect("insert payload");
    }

    fn part(content: &str) -> RequestLogPayloadPart {
        RequestLogPayloadPart {
            hash: format!("h:{content}"),
            content: content.to_string(),
        }
    }

    fn seed_manifest(
        storage: &Storage,
        trace_id: &str,
        created_at: i64,
        previous_response_id: Option<&str>,
        items: &[&str],
    ) {
        storage
            .insert_request_log_payload_manifest(
                &RequestLogPayloadManifestInput {
                    trace_id: trace_id.to_string(),
                    stage: codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM.to_string(),
                    body_hash: format!("hash-{trace_id}"),
                    body_kind: "json_list".to_string(),
                    list_field: Some("input".to_string()),
                    conversation_key: Some("gk_admin|conv".to_string()),
                    previous_response_id: previous_response_id.map(str::to_string),
                    payload_bytes: 64,
                    redacted: false,
                    created_at,
                    fields: vec![("model".to_string(), part("\"gpt-6-astra\""))],
                    items: items.iter().map(|item| part(item)).collect(),
                },
                None,
            )
            .expect("insert manifest");
    }

    fn params(trace_id: &str) -> RequestLogDetailParams {
        RequestLogDetailParams {
            trace_id: trace_id.to_string(),
            stage: None,
        }
    }

    fn params_with_stage(trace_id: &str, stage: &str) -> RequestLogDetailParams {
        RequestLogDetailParams {
            trace_id: trace_id.to_string(),
            stage: Some(stage.to_string()),
        }
    }

    #[test]
    fn admin_reads_stored_payload() {
        let storage = storage();
        seed_trace(&storage, "trc_admin", Some("gk_admin"));
        seed_payload(&storage, "trc_admin");
        let detail = read_request_log_detail(&storage, &params("trc_admin"), None)
            .expect("admin detail succeeds");
        assert_eq!(detail.trace_id, "trc_admin");
        assert_eq!(detail.storage_mode, "preview");
        assert_eq!(detail.payload, "{\"model\":\"gpt-6-astra\"}");
        assert!(detail.redacted);
        assert!(!detail.payload_truncated);
    }

    #[test]
    fn full_mode_detail_returns_fields_items_and_inheritance() {
        let storage = storage();
        seed_trace(&storage, "trc_full_2", Some("gk_admin"));
        seed_manifest(&storage, "trc_full_1", 100, None, &["\"u1\""]);
        seed_manifest(&storage, "trc_full_2", 101, None, &["\"u1\"", "\"a1\""]);
        let detail = read_request_log_detail(&storage, &params("trc_full_2"), None)
            .expect("full detail succeeds");
        assert_eq!(detail.storage_mode, "full");
        assert!(detail.complete);
        assert!(!detail.redacted);
        assert_eq!(detail.list_field.as_deref(), Some("input"));
        assert_eq!(detail.items, vec!["\"u1\"", "\"a1\""]);
        assert_eq!(detail.inherited_item_count, 1);
        assert_eq!(detail.parent_trace_id.as_deref(), Some("trc_full_1"));
        assert_eq!(detail.fields.len(), 1);
        assert_eq!(detail.fields[0].name, "model");
        assert!(detail.context.is_empty());
    }

    #[test]
    fn previous_response_id_continuation_stitches_conversation_context() {
        let storage = storage();
        for trace_id in ["trc_root", "trc_cont_1", "trc_cont_2"] {
            seed_trace(&storage, trace_id, Some("gk_admin"));
        }
        seed_manifest(&storage, "trc_root", 100, None, &["\"u1\""]);
        seed_manifest(&storage, "trc_cont_1", 101, Some("resp_1"), &["\"u2\""]);
        seed_manifest(&storage, "trc_cont_2", 102, Some("resp_2"), &["\"u3\""]);
        storage
            .record_request_log_response_id("gk_admin", "resp_1", "trc_root")
            .unwrap();
        storage
            .record_request_log_response_id("gk_admin", "resp_2", "trc_cont_1")
            .unwrap();
        let detail = read_request_log_detail(&storage, &params("trc_cont_2"), None)
            .expect("continuation detail succeeds");
        assert_eq!(detail.previous_response_id.as_deref(), Some("resp_2"));
        let traces = detail
            .context
            .iter()
            .map(|segment| segment.trace_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(traces, vec!["trc_root", "trc_cont_1"]);
        assert_eq!(detail.context[0].items, vec!["\"u1\""]);
        assert_eq!(detail.items, vec!["\"u3\""]);
    }

    #[test]
    fn continuation_ignores_more_recent_unrelated_trace() {
        let storage = storage();
        for trace in ["a_root", "b_unrelated", "a_continue"] {
            seed_trace(&storage, trace, Some("gk_admin"));
        }
        seed_manifest(&storage, "a_root", 100, None, &["\"A\""]);
        seed_manifest(&storage, "b_unrelated", 101, None, &["\"B\""]);
        seed_manifest(
            &storage,
            "a_continue",
            102,
            Some("resp_from_a"),
            &["\"A-next\""],
        );
        storage
            .record_request_log_response_id("gk_admin", "resp_from_a", "a_root")
            .unwrap();
        storage
            .record_request_log_response_id("gk_admin", "resp_from_b", "b_unrelated")
            .unwrap();
        let detail = read_request_log_detail(&storage, &params("a_continue"), None).unwrap();
        assert_eq!(detail.context.len(), 1);
        assert_eq!(detail.context[0].trace_id, "a_root");
        assert_eq!(detail.context[0].items, vec!["\"A\""]);

        // Unknown response IDs have no guessed predecessor, even though B
        // happens to be the nearest row for this key.
        seed_trace(&storage, "unknown", Some("gk_admin"));
        seed_manifest(&storage, "unknown", 103, Some("resp_unknown"), &["\"new\""]);
        assert!(read_request_log_detail(&storage, &params("unknown"), None)
            .unwrap()
            .context
            .is_empty());
    }

    #[test]
    fn response_link_does_not_cross_concrete_sessions() {
        let storage = storage();
        seed_trace(&storage, "trc_session_a", Some("gk_admin"));
        seed_trace(&storage, "trc_session_b", Some("gk_admin"));
        seed_manifest(&storage, "trc_session_a", 100, None, &["\"session A\""]);
        storage
            .record_request_log_response_id("gk_admin", "resp_from_session_a", "trc_session_a")
            .unwrap();
        let mut current = RequestLogPayloadManifestInput {
            trace_id: "trc_session_b".to_string(),
            stage: codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM.to_string(),
            body_hash: "hash-session-b".to_string(),
            body_kind: "json_list".to_string(),
            list_field: Some("input".to_string()),
            conversation_key: Some("gk_admin|different-session".to_string()),
            previous_response_id: Some("resp_from_session_a".to_string()),
            payload_bytes: 42,
            redacted: false,
            created_at: 101,
            fields: Vec::new(),
            items: vec![part("\"session B\"")],
        };
        storage
            .insert_request_log_payload_manifest(&current, None)
            .unwrap();
        assert!(
            read_request_log_detail(&storage, &params("trc_session_b"), None)
                .unwrap()
                .context
                .is_empty()
        );
        current.trace_id = "trc_session_other_key".to_string();
        current.conversation_key = Some("gk_other|different-session".to_string());
        seed_trace(&storage, &current.trace_id, Some("gk_other"));
        storage
            .insert_request_log_payload_manifest(&current, None)
            .unwrap();
        assert!(
            read_request_log_detail(&storage, &params("trc_session_other_key"), None)
                .unwrap()
                .context
                .is_empty()
        );
    }

    #[test]
    fn detail_defaults_to_last_attempt_and_only_reuses_verified_client_body() {
        use codexmanager_core::storage::RequestLogUpstreamAttempt;
        let storage = storage();
        seed_trace(&storage, "trc_attempts", Some("gk_admin"));
        seed_payload(&storage, "trc_attempts");
        let old_stage = "upstream";
        let latest_stage = "upstream:00000000000000000002";
        for (stage, identical) in [(old_stage, false), (latest_stage, true)] {
            let record = RequestLogUpstreamAttempt {
                trace_id: "trc_attempts".into(),
                stage: stage.into(),
                method: "POST".into(),
                url: "http://127.0.0.1/v1/responses".into(),
                transport: "http".into(),
                content_encoding: None,
                wire_sha256: "mock-wire-hash".into(),
                identical_to_client: identical,
                created_at: codexmanager_core::storage::now_ts(),
            };
            storage
                .record_request_log_upstream_attempt_if_current(&record, 0)
                .unwrap();
        }
        let detail = read_request_log_detail(&storage, &params("trc_attempts"), None).unwrap();
        assert_eq!(detail.stage, latest_stage);
        assert_eq!(detail.payload, "{\"model\":\"gpt-6-astra\"}");
        assert!(detail.attempt.as_ref().unwrap().identical_to_client);
        assert_eq!(detail.stages.len(), 3);
        let not_same = read_request_log_detail(
            &storage,
            &params_with_stage("trc_attempts", old_stage),
            None,
        )
        .expect_err("unverified upstream body must not be represented by client body");
        assert_eq!(not_same, "request log detail not found");
    }

    #[test]
    fn member_scope_authorized_for_own_key() {
        let storage = storage();
        seed_trace(&storage, "trc_member", Some("gk_member"));
        seed_payload(&storage, "trc_member");
        let allowed = vec!["gk_member".to_string()];
        let detail =
            read_request_log_detail(&storage, &params("trc_member"), Some(allowed.as_slice()))
                .expect("member own key detail succeeds");
        assert_eq!(detail.trace_id, "trc_member");
    }

    #[test]
    fn member_scope_rejected_for_foreign_key_with_not_found() {
        let storage = storage();
        seed_trace(&storage, "trc_other", Some("gk_other"));
        seed_payload(&storage, "trc_other");
        let allowed = vec!["gk_member".to_string()];
        let err = read_request_log_detail(&storage, &params("trc_other"), Some(allowed.as_slice()))
            .expect_err("foreign key must be rejected");
        assert_eq!(err, "request log detail not found");
    }

    #[test]
    fn missing_payload_reports_not_found() {
        let storage = storage();
        seed_trace(&storage, "trc_missing", Some("gk_admin"));
        let err = read_request_log_detail(&storage, &params("trc_missing"), None)
            .expect_err("missing payload must fail");
        assert_eq!(err, "request log detail not found");
    }

    #[test]
    fn stage_selection_prefers_upstream_and_allows_explicit_choice() {
        let storage = storage();
        seed_trace(&storage, "trc_stages", Some("gk_admin"));
        let manifest = |stage: &str, content: &str| RequestLogPayloadManifestInput {
            trace_id: "trc_stages".to_string(),
            stage: stage.to_string(),
            body_hash: format!("hash-{stage}"),
            body_kind: "json_list".to_string(),
            list_field: Some("input".to_string()),
            conversation_key: Some("gk_admin|conv".to_string()),
            previous_response_id: None,
            payload_bytes: 32,
            redacted: false,
            created_at: 1_700_000_000,
            fields: Vec::new(),
            items: vec![part(content)],
        };
        storage
            .insert_request_log_payload_manifest(
                &manifest(
                    codexmanager_core::storage::PAYLOAD_STAGE_CLIENT,
                    "\"client-body\"",
                ),
                None,
            )
            .expect("insert client");
        storage
            .insert_request_log_payload_manifest(
                &manifest(
                    codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM,
                    "\"upstream-body\"",
                ),
                None,
            )
            .expect("insert upstream");

        let default =
            read_request_log_detail(&storage, &params("trc_stages"), None).expect("default detail");
        assert_eq!(
            default.stage,
            codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM
        );
        assert_eq!(
            default.stages,
            vec![
                codexmanager_core::storage::PAYLOAD_STAGE_CLIENT.to_string(),
                codexmanager_core::storage::PAYLOAD_STAGE_UPSTREAM.to_string()
            ]
        );
        assert_eq!(default.items, vec!["\"upstream-body\""]);

        let client = read_request_log_detail(
            &storage,
            &params_with_stage(
                "trc_stages",
                codexmanager_core::storage::PAYLOAD_STAGE_CLIENT,
            ),
            None,
        )
        .expect("client detail");
        assert_eq!(
            client.stage,
            codexmanager_core::storage::PAYLOAD_STAGE_CLIENT
        );
        assert_eq!(client.items, vec!["\"client-body\""]);
    }

    #[test]
    fn empty_trace_id_is_rejected() {
        let storage = storage();
        let err = read_request_log_detail(&storage, &params("  "), None)
            .expect_err("blank trace id must fail");
        assert_eq!(err, "trace_id must not be empty");
    }
}
