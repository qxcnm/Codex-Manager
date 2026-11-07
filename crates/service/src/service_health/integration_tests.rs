use super::*;
use codexmanager_core::{
    rpc::types::{JsonRpcMessage, JsonRpcRequest},
    storage::{now_ts, RequestLog, Storage},
};

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn new(path: &std::path::Path) -> Self {
        let mut saved = Vec::new();
        for (key, value) in [
            ("CODEXMANAGER_DB_PATH", Some(path.as_os_str())),
            (
                "CODEXMANAGER_STORAGE_BACKEND",
                Some(std::ffi::OsStr::new("sqlite")),
            ),
            ("CODEXMANAGER_DATABASE_URL", None),
        ] {
            saved.push((key, std::env::var_os(key)));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self(saved)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
fn rpc(method: &str, params: Value, actor: &RpcActor) -> Value {
    let request: JsonRpcRequest = serde_json::from_value(
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )
    .unwrap();
    match crate::handle_request_with_actor(request, actor.clone()) {
        JsonRpcMessage::Response(response) => response.result,
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn shared_summary_keeps_report_ownership_cooldown_and_expiry() {
    let _guard = crate::test_env_guard();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "codexmanager-health-{}-{nonce}.sqlite",
        std::process::id()
    ));
    let _environment = Environment::new(&path);
    let storage = Storage::open(&path).unwrap();
    storage.init().unwrap();
    let now = now_ts();
    storage
        .insert_request_log(&RequestLog {
            request_path: "/v1/messages".into(),
            method: "POST".into(),
            actual_source_kind: Some("openai_account".into()),
            account_id: Some("PRIVATE_ACCOUNT".into()),
            key_id: Some("PRIVATE_KEY".into()),
            model: Some("PRIVATE_MODEL".into()),
            upstream_url: Some("https://PRIVATE_UPSTREAM.invalid?key=PRIVATE_CREDENTIAL".into()),
            error: Some("PRIVATE_ERROR".into()),
            status_code: Some(502),
            created_at: now,
            ..Default::default()
        })
        .unwrap();
    drop(storage);
    let alice = RpcActor::from_parts(Some("member"), Some("PRIVATE_ALICE"));
    let bob = RpcActor::from_parts(Some("member"), Some("PRIVATE_BOB"));
    let input = serde_json::json!({"provider":"codex","symptom":"errors","region":"unknown"});
    let first = rpc("serviceHealth/report", input.clone(), &alice);
    let submitted_at = first["report"]["updatedAt"]
        .as_i64()
        .expect("submission time");
    let report_id = first["report"]["id"]
        .as_str()
        .expect("report id")
        .to_owned();
    assert_eq!(
        rpc("serviceHealth/report", input, &alice)["report"]["id"],
        report_id
    );
    let second = rpc(
        "serviceHealth/report",
        serde_json::json!({"provider":"codex","symptom":"slow","region":"europe"}),
        &bob,
    );
    assert!(second["report"]["id"].is_string());
    let summary = rpc("serviceHealth/summary", serde_json::json!({}), &alice);
    assert_eq!(summary["providers"][0]["reporterCount"], 2);
    assert_eq!(summary["providers"][0]["unknownErrorCount"], 1);
    assert_eq!(summary["myReports"].as_array().unwrap().len(), 1);
    assert!(!serde_json::to_string(&summary)
        .unwrap()
        .contains("PRIVATE_"));
    assert!(rpc(
        "serviceHealth/resolve",
        serde_json::json!({"id":report_id}),
        &bob
    )
    .get("error")
    .is_some());
    assert_eq!(
        rpc(
            "serviceHealth/resolve",
            serde_json::json!({"id":report_id}),
            &alice
        )["ok"],
        true
    );
    let resolved_at = now_ts();
    assert!(reports::mine(&alice, resolved_at).unwrap().is_empty());
    let report_input = ReportInput {
        provider: Provider::Codex,
        symptom: Symptom::Errors,
        region: Region::Unknown,
    };
    assert_eq!(
        reports::report(&alice, report_input.clone(), resolved_at + 1).unwrap_err(),
        "health_report_rate_limited"
    );
    let renewed = reports::report(&alice, report_input, submitted_at + 601).unwrap();
    assert_eq!(renewed.expires_at, submitted_at + 601 + REPORT_TTL);
    assert!(reports::mine(&alice, renewed.expires_at)
        .unwrap()
        .is_empty());
    assert!(rpc(
        "serviceHealth/summary",
        serde_json::json!({"source":"shared"}),
        &alice
    )
    .get("error")
    .is_some());
}
