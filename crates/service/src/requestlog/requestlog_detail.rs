use codexmanager_core::rpc::types::{RequestLogDetailParams, RequestLogDetailResult};
use codexmanager_core::storage::Storage;

/// Load the sanitized request payload preview for one gateway trace.
///
/// `allowed_key_ids` enforces member scoping: `None` grants the
/// administrator view, while `Some(key_ids)` restricts access to payload
/// previews whose request log row belongs to one of the member's API keys.
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
    if let Some(allowed_key_ids) = allowed_key_ids {
        let log_key_id = storage
            .find_request_log_key_id_by_trace_id(trace_id)
            .map_err(|err| format!("read request log owner failed: {err}"))?;
        let owned = log_key_id.flatten().is_some_and(|key_id| {
            allowed_key_ids
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(key_id.trim()))
        });
        if !owned {
            return Err("request log detail not found".to_string());
        }
    }
    let payload = storage
        .find_request_log_payload_by_trace_id(trace_id)
        .map_err(|err| format!("read request log payload failed: {err}"))?;
    let Some(payload) = payload else {
        return Err("request log detail not found".to_string());
    };
    Ok(RequestLogDetailResult {
        trace_id: payload.trace_id,
        payload: payload.payload,
        payload_bytes: payload.payload_bytes,
        payload_truncated: payload.payload_truncated,
        created_at: payload.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{RequestLog, RequestLogPayload, RequestTokenStat};

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
                payload: "{\"model\":\"gpt-6-astra\"}".to_string(),
                payload_bytes: 26,
                payload_truncated: false,
                created_at: 1_700_000_000,
            })
            .expect("insert payload");
    }

    fn params(trace_id: &str) -> RequestLogDetailParams {
        RequestLogDetailParams {
            trace_id: trace_id.to_string(),
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
        assert_eq!(detail.payload, "{\"model\":\"gpt-6-astra\"}");
        assert!(!detail.payload_truncated);
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
    fn empty_trace_id_is_rejected() {
        let storage = storage();
        let err = read_request_log_detail(&storage, &params("  "), None)
            .expect_err("blank trace id must fail");
        assert_eq!(err, "trace_id must not be empty");
    }
}
