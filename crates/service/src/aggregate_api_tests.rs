use codexmanager_core::rpc::types::AggregateApiSupplierModelImportParams;
use codexmanager_core::storage::{AggregateApi, AggregateApiSupplierModel, Storage};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server, StatusCode};

use super::{
    action_path_or_default, build_codex_models_probe_url, build_codex_probe_body,
    claude_probe_fallback_models_for_api,
    extract_custom_balance, extract_generic_balance, extract_model_ids_from_models_response,
    extract_new_api_balance, import_aggregate_api_supplier_models, list_aggregate_apis,
    normalize_action_override, normalize_custom_balance_query_config, normalize_provider_type,
    normalize_provider_type_value, probe_claude_endpoint, probe_codex_endpoint,
    provider_default_url, read_aggregate_api_secret, CustomBalanceQueryConfig,
    AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_GEMINI, ALIBABA_CODING_PLAN_PROBE_MODEL,
    CLAUDE_DEFAULT_PROBE_MODEL,
};

#[test]
fn codex_probe_uses_valid_responses_input_content_type() {
    let body = build_codex_probe_body();
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["max_output_tokens"], 16);
}

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
fn list_aggregate_apis_loads_model_assignments_for_existing_apis_only() {
    let _lock = crate::test_env_guard();
    let dir = new_test_dir("aggregate-api-list-assignments");
    let db_path = dir.join("codexmanager.db");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let mut storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let mut api = aggregate_api_with_action(None);
    api.id = "agg-listed".to_string();
    storage.insert_aggregate_api(&api).expect("insert api");
    storage
        .set_quota_source_model_assignments(
            "aggregate_api",
            "agg-listed",
            &["gpt-visible".to_string()],
        )
        .expect("assign listed api model");
    storage
        .set_quota_source_model_assignments(
            "aggregate_api",
            "agg-unlisted",
            &["gpt-hidden".to_string()],
        )
        .expect("assign unlisted api model");

    let items = list_aggregate_apis().expect("list aggregate APIs");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "agg-listed");
    assert_eq!(items[0].model_slugs, vec!["gpt-visible".to_string()]);
    assert!(!items[0].model_slugs.contains(&"gpt-hidden".to_string()));
}

#[test]
fn import_supplier_models_reads_supplier_identity_projection() {
    let _lock = crate::test_env_guard();
    let dir = new_test_dir("aggregate-api-import-supplier-models");
    let db_path = dir.join("codexmanager.db");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let mut api = aggregate_api_with_action(None);
    api.id = "api-import-template".to_string();
    api.provider_type = "openai-compatible".to_string();
    api.supplier_name = Some("Template Supplier".to_string());
    api.url = "https://ignored-fallback.example.test/v1".to_string();
    api.auth_params_json = Some(r#"{"ignored":"secret"}"#.to_string());
    api.last_balance_json = Some(r#"{"ignored":true}"#.to_string());
    storage.insert_aggregate_api(&api).expect("insert api");
    storage
        .upsert_aggregate_api_supplier_model(&AggregateApiSupplierModel {
            supplier_key: "Template Supplier".to_string(),
            provider_type: "codex".to_string(),
            upstream_model: "provider-model-a".to_string(),
            display_name: Some("Provider Model A".to_string()),
            status: "available".to_string(),
            created_at: 1,
            updated_at: 1,
        })
        .expect("insert available template");
    storage
        .upsert_aggregate_api_supplier_model(&AggregateApiSupplierModel {
            supplier_key: "Template Supplier".to_string(),
            provider_type: "codex".to_string(),
            upstream_model: "provider-model-disabled".to_string(),
            display_name: None,
            status: "disabled".to_string(),
            created_at: 1,
            updated_at: 1,
        })
        .expect("insert disabled template");

    let result = import_aggregate_api_supplier_models(AggregateApiSupplierModelImportParams {
        api_id: "api-import-template".to_string(),
        supplier_key: None,
        provider_type: None,
    })
    .expect("import supplier models");

    assert_eq!(result.imported, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].source_id, "api-import-template");
    assert_eq!(result.items[0].upstream_model, "provider-model-a");
    let imported = Storage::open(&db_path)
        .expect("reopen db")
        .list_model_source_models(Some("aggregate_api"), Some("api-import-template"))
        .expect("list imported source models");
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].upstream_model, "provider-model-a");
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
fn codex_models_probe_url_uses_standard_openai_path() {
    let mut api = aggregate_api_with_action(None);
    api.url = "https://api.openai.com/v1".to_string();

    let url = build_codex_models_probe_url(&api);

    assert_eq!(url, "https://api.openai.com/v1/models");
}

#[test]
fn codex_models_probe_url_preserves_custom_action_query() {
    let mut api = aggregate_api_with_action(Some("/models?limit=20"));
    api.url = "https://api.openai.com/v1".to_string();

    let url = build_codex_models_probe_url(&api);

    assert_eq!(url, "https://api.openai.com/v1/models?limit=20");
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
}

#[test]
fn claude_probe_models_prefer_alibaba_coding_plan_model_for_dashscope() {
    let mut api = aggregate_api_with_action(None);
    api.url = "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic".to_string();

    assert_eq!(
        claude_probe_fallback_models_for_api(&api),
        vec![ALIBABA_CODING_PLAN_PROBE_MODEL, CLAUDE_DEFAULT_PROBE_MODEL]
    );
}

#[test]
fn claude_probe_models_keep_anthropic_default_first_for_generic_urls() {
    let mut api = aggregate_api_with_action(None);
    api.url = "https://api.anthropic.com/v1".to_string();

    assert_eq!(
        claude_probe_fallback_models_for_api(&api),
        vec![CLAUDE_DEFAULT_PROBE_MODEL, ALIBABA_CODING_PLAN_PROBE_MODEL]
    );
}

#[test]
fn extract_model_ids_from_models_response_accepts_common_shapes() {
    let body = r#"{
            "data": [
                {"id":"provider-model-a"},
                {"model":"provider-model-b"},
                "provider-model-c"
            ],
            "models": [{"slug":"ignored-because-data-wins"}]
        }"#;

    assert_eq!(
        extract_model_ids_from_models_response(body),
        vec![
            "provider-model-a".to_string(),
            "provider-model-b".to_string(),
            "provider-model-c".to_string()
        ]
    );
}

#[test]
fn extract_model_ids_from_models_response_keeps_full_provider_catalog() {
    let mut data = (0..13)
        .map(|index| serde_json::json!({ "id": format!("provider-model-{index}") }))
        .collect::<Vec<Value>>();
    data.push(serde_json::json!({ "id": "gpt-5.5" }));
    let body = serde_json::json!({ "data": data }).to_string();

    let models = extract_model_ids_from_models_response(body.as_str());

    assert!(models.contains(&"gpt-5.5".to_string()));
    assert_eq!(models.len(), 14);
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

    let status = probe_claude_endpoint(&client, &api, "secret").expect("probe succeeds");

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

    let status = probe_codex_endpoint(&client, &api, "secret").expect("probe succeeds");

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

    let status = probe_codex_endpoint(&client, &api, "secret").expect("probe succeeds");

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

#[test]
fn claude_probe_uses_discovered_model_before_fallbacks() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive model list request")
            .expect("model list request present");
        tx.send((
            request.method().as_str().to_string(),
            request.url().to_string(),
            None,
        ))
        .expect("send model list request");
        request
            .respond(Response::from_string(
                r#"{"data":[{"id":"provider-model"}]}"#,
            ))
            .expect("respond model list");

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
            Some(body),
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
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status = probe_claude_endpoint(&client, &api, "secret").expect("probe succeeds");

    assert_eq!(status, 200);
    let first = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first captured request");
    let second = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second captured request");
    join.join().expect("join mock server");
    assert_eq!(first.0, "GET");
    assert_eq!(first.1, "/v1/models");
    assert_eq!(second.0, "POST");
    assert_eq!(second.1, "/v1/messages?beta=true");
    let second_body: Value =
        serde_json::from_str(second.2.as_deref().expect("second body")).expect("parse body");
    assert_eq!(second_body["model"], "provider-model");
}

#[test]
fn claude_probe_retries_with_alibaba_model_after_default_model_bad_request() {
    let server = Server::http("127.0.0.1:0").expect("start mock server");
    let base_url = format!("http://{}/apps/anthropic", server.server_addr());
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive model list request")
            .expect("model list request present");
        tx.send((request.url().to_string(), String::new()))
            .expect("send captured model list request");
        request
            .respond(Response::from_string("").with_status_code(StatusCode(404)))
            .expect("respond model list request");

        for (index, status) in [400u16, 200u16].into_iter().enumerate() {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive mock request")
                .expect("mock request present");
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read request body");
            tx.send((request.url().to_string(), body))
                .expect("send captured request");
            let response_body = if index == 0 {
                r#"{"error":{"message":"model not found"}}"#
            } else {
                r#"{"id":"msg_probe","type":"message","content":[]}"#
            };
            request
                .respond(Response::from_string(response_body).with_status_code(StatusCode(status)))
                .expect("respond mock request");
        }
    });

    let mut api = aggregate_api_with_action(None);
    api.url = base_url;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let status = probe_claude_endpoint(&client, &api, "secret").expect("probe succeeds");

    assert_eq!(status, 200);
    let models_request = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("captured model list request");
    let first = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first captured request");
    let second = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second captured request");
    join.join().expect("join mock server");
    assert_eq!(models_request.0, "/apps/anthropic/v1/models");
    assert_eq!(first.0, "/apps/anthropic/v1/messages?beta=true");
    assert_eq!(second.0, "/apps/anthropic/v1/messages?beta=true");
    let first_body: Value = serde_json::from_str(first.1.as_str()).expect("parse first body");
    let second_body: Value = serde_json::from_str(second.1.as_str()).expect("parse second body");
    assert_eq!(first_body["model"], CLAUDE_DEFAULT_PROBE_MODEL);
    assert_eq!(second_body["model"], ALIBABA_CODING_PLAN_PROBE_MODEL);
}
