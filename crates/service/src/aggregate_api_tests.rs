use codexmanager_core::storage::{AggregateApi, ManagedModelV2Upsert, ModelRouteV2, Storage};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server};

use super::{
    action_path_or_default, extract_custom_balance, extract_generic_balance,
    extract_new_api_balance, list_aggregate_apis, normalize_action_override,
    normalize_custom_balance_query_config, normalize_provider_type, normalize_provider_type_value,
    probe_claude_endpoint, probe_codex_endpoint, provider_default_url, read_aggregate_api_secret,
    CustomBalanceQueryConfig, AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_COMPATIBLE,
    AGGREGATE_API_PROVIDER_GEMINI,
};

static AGGREGATE_API_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn new_test_dir(prefix: &str) -> PathBuf {
    let seq = AGGREGATE_API_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn aggregate_api_with_action(action: Option<&str>) -> AggregateApi {
    AggregateApi {
        id: "agg-test".to_string(),
        provider_type: "claude".to_string(),
        supplier_name: Some("test".to_string()),
        sort: 0,
        url: "https://open.bigmodel.cn/api/anthropic".to_string(),
        auth_type: "apikey".to_string(),
        auth_params_json: None,
        action: action.map(str::to_string),
        model_override: None,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: false,
        balance_query_template: None,
        balance_query_base_url: None,
        balance_query_user_id: None,
        balance_query_config_json: None,
        last_balance_at: None,
        last_balance_status: None,
        last_balance_error: None,
        last_balance_json: None,
    }
}

#[test]
fn list_aggregate_apis_reads_model_assignments_from_v2_routes_only() {
    let _lock = crate::test_env_guard();
    let dir = new_test_dir("aggregate-api-list-assignments");
    let db_path = dir.join("codexmanager.db");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let mut api = aggregate_api_with_action(None);
    api.id = "agg-listed".to_string();
    storage.insert_aggregate_api(&api).expect("insert api");
    let mut model = storage
        .get_managed_model_v2("gpt-5.4")
        .expect("read V2 model")
        .expect("seeded model");
    model.routes.extend([
        ModelRouteV2 {
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-listed".to_string(),
            upstream_model: "provider-visible".to_string(),
            enabled: true,
            weight: 1,
            ..Default::default()
        },
        ModelRouteV2 {
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-unlisted".to_string(),
            upstream_model: "provider-unlisted".to_string(),
            enabled: true,
            weight: 1,
            ..Default::default()
        },
    ]);
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            model,
            ..Default::default()
        })
        .expect("save V2 routes");

    let items = list_aggregate_apis().expect("list aggregate APIs");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "agg-listed");
    assert_eq!(items[0].model_slugs, vec!["gpt-5.4".to_string()]);
}

#[test]
fn read_aggregate_api_secret_uses_auth_type_projection() {
    let _lock = crate::test_env_guard();
    let dir = new_test_dir("aggregate-api-read-secret");
    let db_path = dir.join("codexmanager.db");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let mut api = aggregate_api_with_action(None);
    api.id = "api-userpass-secret".to_string();
    api.auth_type = "userpass".to_string();
    api.auth_params_json = Some(r#"{"ignored":"secret"}"#.to_string());
    api.last_balance_json = Some(r#"{"ignored":true}"#.to_string());
    storage.insert_aggregate_api(&api).expect("insert api");
    storage
        .upsert_aggregate_api_secret(
            "api-userpass-secret",
            r#"{"username":"user-a","password":"pass-a"}"#,
        )
        .expect("insert secret");

    let result =
        read_aggregate_api_secret("api-userpass-secret").expect("read aggregate api secret");

    assert_eq!(result.id, "api-userpass-secret");
    assert_eq!(result.auth_type, "userpass");
    assert_eq!(result.key, "");
    assert_eq!(result.username.as_deref(), Some("user-a"));
    assert_eq!(result.password.as_deref(), Some("pass-a"));
    let err = read_aggregate_api_secret("api-missing").expect_err("missing api should error");
    assert_eq!(err, "aggregate api not found");
}

#[test]
fn action_override_disabled_stays_none() {
    let value = normalize_action_override(Some(false), Some("/v1/messages".to_string())).unwrap();
    assert_eq!(value, Some(None));
}

#[test]
fn action_override_enabled_and_empty_preserves_empty_string() {
    let value = normalize_action_override(Some(true), Some("   ".to_string())).unwrap();
    assert_eq!(value, Some(Some(String::new())));
}

#[test]
fn empty_action_uses_base_url_without_default_path() {
    let api = aggregate_api_with_action(Some(""));
    let path = action_path_or_default(&api, "/v1/messages?beta=true");
    assert_eq!(path, "");
}

#[test]
fn gemini_provider_type_is_normalized_independently() {
    assert_eq!(
        normalize_provider_type(Some("gemini_native".to_string())).unwrap(),
        AGGREGATE_API_PROVIDER_GEMINI
    );
    assert_eq!(
        normalize_provider_type_value("google_gemini"),
        AGGREGATE_API_PROVIDER_GEMINI
    );
    assert_eq!(
        provider_default_url(AGGREGATE_API_PROVIDER_GEMINI),
        "https://generativelanguage.googleapis.com"
    );
    assert_eq!(
        normalize_provider_type(Some("claude".to_string())).unwrap(),
        AGGREGATE_API_PROVIDER_CLAUDE
    );
    assert_eq!(
        normalize_provider_type(Some("compatible".to_string())).unwrap(),
        AGGREGATE_API_PROVIDER_COMPATIBLE
    );
    assert_eq!(
        normalize_provider_type_value("compatible"),
        AGGREGATE_API_PROVIDER_COMPATIBLE
    );
}

#[test]
fn generic_balance_extractor_accepts_common_balance_shape() {
    let body: Value =
        serde_json::from_str(r#"{"is_active":true,"balance":12.5,"currency":"USD","used":1.25}"#)
            .expect("parse balance body");

    let snapshot = extract_generic_balance(&body).expect("extract balance");

    assert!(snapshot.is_valid);
    assert_eq!(snapshot.remaining, Some(12.5));
    assert_eq!(snapshot.unit.as_deref(), Some("USD"));
    assert_eq!(snapshot.used, Some(1.25));
}

#[test]
fn new_api_balance_extractor_converts_quota_to_usd() {
    let body: Value = serde_json::from_str(
        r#"{"success":true,"data":{"group":"default","quota":1000000,"used_quota":500000}}"#,
    )
    .expect("parse new api balance body");

    let snapshot = extract_new_api_balance(&body).expect("extract balance");

    assert!(snapshot.is_valid);
    assert_eq!(snapshot.plan_name.as_deref(), Some("default"));
    assert_eq!(snapshot.remaining, Some(2.0));
    assert_eq!(snapshot.used, Some(1.0));
    assert_eq!(snapshot.total, Some(3.0));
}

#[test]
fn generic_balance_extractor_accepts_usage_balance_shape() {
    let body: Value = serde_json::from_str(
            r#"{"mode":"quota_limited","status":"active","quota":{"limit":20,"used":7.5,"remaining":12.5}}"#,
        )
        .expect("parse usage body");

    let snapshot = extract_generic_balance(&body).expect("extract usage balance");

    assert!(snapshot.is_valid);
    assert_eq!(snapshot.plan_name.as_deref(), Some("quota_limited"));
    assert_eq!(snapshot.remaining, Some(12.5));
    assert_eq!(snapshot.used, Some(7.5));
    assert_eq!(snapshot.total, Some(20.0));
    assert_eq!(snapshot.unit.as_deref(), Some("USD"));
}

#[test]
fn custom_balance_config_normalizes_and_extracts_paths() {
    let normalized = normalize_custom_balance_query_config(Some(
        r#"{
                "method":"get",
                "path":"v1/usage",
                "auth":"balance-bearer",
                "remainingPath":"data.available",
                "totalPath":"data.limit",
                "usedPath":"data.used",
                "planPath":"data.plan",
                "validPath":"data.valid",
                "unit":"credits",
                "multiplier":0.5
            }"#
        .to_string(),
    ))
    .expect("normalize custom balance config")
    .expect("custom config present");
    let config: CustomBalanceQueryConfig =
        serde_json::from_str(normalized.as_str()).expect("parse normalized custom config");

    assert_eq!(config.method.as_deref(), Some("GET"));
    assert_eq!(config.path, "/v1/usage");
    assert_eq!(config.auth.as_deref(), Some("balance_bearer"));

    let body: Value = serde_json::from_str(
        r#"{"data":{"available":4,"limit":10,"used":6,"plan":"pro","valid":true}}"#,
    )
    .expect("parse custom balance body");
    let snapshot = extract_custom_balance(&body, &config).expect("extract custom balance");

    assert!(snapshot.is_valid);
    assert_eq!(snapshot.remaining, Some(2.0));
    assert_eq!(snapshot.total, Some(5.0));
    assert_eq!(snapshot.used, Some(3.0));
    assert_eq!(snapshot.plan_name.as_deref(), Some("pro"));
    assert_eq!(snapshot.unit.as_deref(), Some("credits"));
}

#[test]
fn custom_balance_config_rejects_absolute_request_path() {
    let result = normalize_custom_balance_query_config(Some(
        r#"{"path":"https://example.com/v1/usage","remainingPath":"balance"}"#.to_string(),
    ));

    assert!(result.is_err());
}

#[test]
fn claude_probe_uses_configured_model_without_model_discovery() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive messages request")
            .expect("messages request present");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read request body");
        tx.send((
            request.method().as_str().to_string(),
            request.url().to_string(),
            body,
        ))
        .expect("send messages request");
        request
            .respond(Response::from_string(
                r#"{"id":"msg_probe","type":"message"}"#,
            ))
            .expect("respond messages");
    });

    let mut api = aggregate_api_with_action(None);
    api.url = base_url;
    api.model_override = Some("qwen3.5-plus".to_string());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status =
        probe_claude_endpoint(&client, &api, "secret", "qwen3.5-plus").expect("probe succeeds");

    assert_eq!(status, 200);
    let captured = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("captured request");
    join.join().expect("join mock server");
    assert_eq!(captured.0, "POST");
    assert_eq!(captured.1, "/v1/messages?beta=true");
    let body: Value = serde_json::from_str(captured.2.as_str()).expect("parse body");
    assert_eq!(body["model"], "qwen3.5-plus");
}

#[test]
fn codex_probe_uses_configured_model_without_model_discovery() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive chat completions request")
            .expect("chat completions request present");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read request body");
        tx.send((
            request.method().as_str().to_string(),
            request.url().to_string(),
            body,
        ))
        .expect("send chat completions request");
        request
            .respond(Response::from_string(r#"{"id":"chatcmpl_probe"}"#))
            .expect("respond chat completions");
    });

    let mut api = aggregate_api_with_action(Some("/chat/completions"));
    api.url = base_url;
    api.model_override = Some("qwen3.5-plus".to_string());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status =
        probe_codex_endpoint(&client, &api, "secret", "qwen3.5-plus").expect("probe succeeds");

    assert_eq!(status, 200);
    let captured = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("captured request");
    join.join().expect("join mock server");
    assert_eq!(captured.0, "POST");
    assert_eq!(captured.1, "/v1/chat/completions");
    let body: Value = serde_json::from_str(captured.2.as_str()).expect("parse body");
    assert_eq!(body["model"], "qwen3.5-plus");
}

#[test]
fn codex_responses_probe_uses_valid_input_text_content() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive responses request")
            .expect("responses request present");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read request body");
        tx.send((request.url().to_string(), body))
            .expect("send responses request");
        request
            .respond(Response::from_string(r#"{"id":"resp_probe"}"#))
            .expect("respond responses");
    });

    let mut api = aggregate_api_with_action(None);
    api.provider_type = "codex".to_string();
    api.url = base_url;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status =
        probe_codex_endpoint(&client, &api, "secret", "gpt-5.6-sol").expect("probe succeeds");

    assert_eq!(status, 200);
    let captured = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("captured request");
    join.join().expect("join mock server");
    assert_eq!(captured.0, "/v1/responses");
    let body: Value = serde_json::from_str(captured.1.as_str()).expect("parse body");
    assert_eq!(body["model"], "gpt-5.6-sol");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["store"], false);
}

#[test]
fn codex_probe_failure_includes_upstream_error_detail() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let join = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive responses request")
            .expect("responses request present");
        request
            .respond(
                Response::from_string(r#"{"error":{"message":"invalid input content type"}}"#)
                    .with_status_code(400),
            )
            .expect("respond with probe error");
    });

    let mut api = aggregate_api_with_action(None);
    api.provider_type = "codex".to_string();
    api.url = base_url;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let error = probe_codex_endpoint(&client, &api, "secret", "gpt-5.6-sol")
        .expect_err("probe should fail");

    join.join().expect("join mock server");
    assert!(error.contains("http_status=400"));
    assert!(error.contains("invalid input content type"));
}

#[test]
fn minimax_codex_probe_uses_responses_string_input() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive responses request")
            .expect("responses request present");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read request body");
        tx.send((
            request.method().as_str().to_string(),
            request.url().to_string(),
            body,
        ))
        .expect("send responses request");
        request
            .respond(Response::from_string(
                r#"{"id":"resp_probe","output_text":"ok"}"#,
            ))
            .expect("respond responses");
    });

    let mut api = aggregate_api_with_action(None);
    api.provider_type = "codex".to_string();
    api.supplier_name = Some("MiniMax".to_string());
    api.url = format!("{base_url}/v1");
    api.model_override = Some("MiniMax-M3".to_string());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status =
        probe_codex_endpoint(&client, &api, "secret", "MiniMax-M3").expect("probe succeeds");

    assert_eq!(status, 200);
    let captured = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("captured request");
    join.join().expect("join mock server");
    assert_eq!(captured.0, "POST");
    assert_eq!(captured.1, "/v1/responses");
    let body: Value = serde_json::from_str(captured.2.as_str()).expect("parse body");
    assert_eq!(body["model"], "MiniMax-M3");
    assert_eq!(body["input"], "Who are you?");
}
