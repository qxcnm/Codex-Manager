use super::chat_non_stream_tools::{seed_account_route, MODEL};
use super::*;
use serde_json::json;

#[test]
fn converted_messages_preserve_canonical_bad_request_without_legacy_path_retry() {
    let _lock = test_env_guard();
    let dir = new_test_dir("messages-canonical-error");
    let db_path = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated db");
    storage.init().expect("initialize db");
    seed_account_route(&storage);
    let original_error = json!({"error": {
        "message": "invalid_content_type_fixture: invalid assistant history content type",
        "type": "invalid_request_error", "code": "invalid_content_type_fixture"
    }})
    .to_string();
    // A second response reproduces the old masking behavior if the gateway
    // incorrectly tries the nonexistent Codex /v1/responses URL.
    let (addr, requests, join) = start_mock_upstream_sequence_lenient_with_content_types(
        vec![
            (400, original_error, "application/json".into()),
            (404, "Not Found".into(), "text/plain".into()),
        ],
        Duration::from_secs(2),
    );
    let _upstream = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{addr}/backend-api/codex"),
    );
    let server = codexmanager_service::start_one_shot_server().expect("start gateway");
    let (status, body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &json!({"model": MODEL, "max_tokens": 128, "stream": true,
            "messages": [{"role": "user", "content": "synthetic error check"}]
        })
        .to_string(),
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer pk_non_stream_tools"),
            ("anthropic-version", "2023-06-01"),
        ],
    );
    server.join();
    join.join().expect("join mock upstream");
    assert_eq!(status, 400, "canonical status must survive: {body}");
    assert!(body.contains("invalid_content_type_fixture"), "{body}");
    let attempts: Vec<_> = requests.try_iter().collect();
    assert_eq!(attempts.len(), 1, "must not retry a nonexistent endpoint");
    assert_eq!(attempts[0].path, "/backend-api/codex/responses");
    let logs = storage.list_request_logs(None, 10).expect("request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status_code, Some(400));
}
