use codexmanager_core::rpc::types::ModelsResponse;
use codexmanager_core::storage::{Account, Storage};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, ETAG, USER_AGENT};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const OFFICIAL_CATALOG_CACHE_DIR: &str = "official-model-catalogs";
const OFFICIAL_CATALOG_CACHE_TTL_SECS: i64 = 300;
static OFFICIAL_CATALOG_SYNC_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayCatalogPolicy {
    OfficialAccountPool,
    Managed,
}

pub(crate) fn gateway_catalog_policy_for_api_key(
    storage: &Storage,
    api_key_id: &str,
) -> Result<GatewayCatalogPolicy, String> {
    let api_key = storage
        .find_api_key_by_id(api_key_id)
        .map_err(|err| format!("read api key routing config failed: {err}"))?
        .ok_or_else(|| "api key not found".to_string())?;
    Ok(gateway_catalog_policy_for_rotation_strategy(
        api_key.rotation_strategy.as_str(),
    ))
}

pub(crate) fn gateway_catalog_policy_for_rotation_strategy(
    rotation_strategy: &str,
) -> GatewayCatalogPolicy {
    if rotation_strategy == crate::apikey_profile::ROTATION_ACCOUNT {
        GatewayCatalogPolicy::OfficialAccountPool
    } else {
        GatewayCatalogPolicy::Managed
    }
}

pub(crate) fn models_response_for_gateway_key(
    storage: &Storage,
    api_key_id: &str,
) -> Result<(ModelsResponse, GatewayCatalogPolicy), String> {
    let policy = gateway_catalog_policy_for_api_key(storage, api_key_id)?;
    let response = match policy {
        GatewayCatalogPolicy::OfficialAccountPool => {
            let value = load_or_sync_official_model_catalog(storage, api_key_id)?;
            official_models_response_from_value(value)?
        }
        GatewayCatalogPolicy::Managed => crate::models_v2::models_response_with_storage(storage)?,
    };
    Ok((response, policy))
}

fn official_models_response_from_value(value: Value) -> Result<ModelsResponse, String> {
    official_model_catalog_from_value(&value)?;
    serde_json::from_value(value)
        .map_err(|err| format!("decode official Codex model cache failed: {err}"))
}

pub(crate) fn write_gateway_model_catalog(
    storage: &Storage,
    api_key_id: &str,
    catalog_path: &Path,
    policy: GatewayCatalogPolicy,
) -> Result<usize, String> {
    let (content, models_count) = match policy {
        GatewayCatalogPolicy::OfficialAccountPool => {
            let official_cache = load_or_sync_official_model_catalog(storage, api_key_id)?;
            let official_models = official_model_catalog_from_value(&official_cache)?;
            let models_count = official_models.len();
            (
                serialize_account_pool_model_catalog(&official_models)?,
                models_count,
            )
        }
        GatewayCatalogPolicy::Managed => {
            let catalog = crate::models_v2::text_generation_models_response_with_storage(storage)?;
            let models_count = catalog.models.len();
            (serialize_gateway_model_catalog(&catalog)?, models_count)
        }
    };
    write_atomic(catalog_path, &content)?;
    Ok(models_count)
}

fn load_or_sync_official_model_catalog(
    storage: &Storage,
    api_key_id: &str,
) -> Result<Value, String> {
    let client_version = crate::gateway::current_codex_user_agent_version();
    let cache_path = official_catalog_cache_path(api_key_id);
    let now = chrono::Utc::now().timestamp();
    let cached = load_compatible_official_snapshot(&cache_path, &client_version);
    if cached
        .as_ref()
        .is_some_and(|value| official_snapshot_is_fresh(value, now))
    {
        return Ok(cached.expect("checked above"));
    }

    let sync_lock = OFFICIAL_CATALOG_SYNC_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = crate::lock_utils::lock_recover(sync_lock, "official_catalog_sync");

    // Another caller may have completed the refresh while this caller waited for the lock.
    let cached = load_compatible_official_snapshot(&cache_path, &client_version);
    if cached
        .as_ref()
        .is_some_and(|value| official_snapshot_is_fresh(value, chrono::Utc::now().timestamp()))
    {
        return Ok(cached.expect("checked above"));
    }

    match fetch_official_model_catalog(storage, api_key_id, &client_version) {
        Ok((response, etag)) => {
            let snapshot = build_official_snapshot(response, &client_version, etag.as_deref())?;
            write_official_snapshot(&cache_path, &snapshot)?;
            Ok(snapshot)
        }
        Err(err) => match cached {
            Some(snapshot) => {
                log::warn!(
                    "refresh official Codex model catalog failed; using stale snapshot for client_version {client_version}: {err}"
                );
                Ok(snapshot)
            }
            None => Err(format!(
                "refresh official Codex model catalog failed and no snapshot exists for client_version {client_version}: {err}"
            )),
        },
    }
}

fn fetch_official_model_catalog(
    storage: &Storage,
    api_key_id: &str,
    client_version: &str,
) -> Result<(Value, Option<String>), String> {
    let api_key = storage
        .find_api_key_by_id(api_key_id)
        .map_err(|err| format!("read api key routing config failed: {err}"))?
        .ok_or_else(|| "api key not found".to_string())?;
    let upstream_base = crate::gateway::gateway_resolve_effective_upstream_base(&api_key);
    if !crate::gateway::gateway_should_send_chatgpt_account_header(&upstream_base) {
        return Err(format!(
            "account-pool model sync requires the official ChatGPT Codex backend, got {upstream_base}"
        ));
    }

    let (models_url, _) =
        crate::gateway::gateway_compute_upstream_url(&upstream_base, "/v1/models");
    let mut models_url = reqwest::Url::parse(&models_url)
        .map_err(|err| format!("build official Codex model endpoint failed: {err}"))?;
    models_url
        .query_pairs_mut()
        .append_pair("client_version", client_version);

    let routed = crate::gateway::gateway_collect_routed_candidates_with_log_source(
        storage, api_key_id, None,
    )?;
    if routed.candidates.is_empty() {
        return Err("no available OpenAI account for official model sync".to_string());
    }

    let mut errors = Vec::new();
    for (account, mut token) in routed.candidates {
        let result = (|| {
            let bearer =
                crate::gateway::gateway_resolve_openai_bearer_token(storage, &account, &mut token)?;
            let client = crate::gateway::upstream_client_for_account(account.id.as_str())?;
            let mut request = client
                .get(models_url.clone())
                .header(
                    AUTHORIZATION,
                    crate::agent_identity::format_upstream_authorization(&bearer),
                )
                .header(ACCEPT, "application/json")
                .header(ACCEPT_ENCODING, "identity")
                .header(USER_AGENT, crate::gateway::current_codex_user_agent())
                .header("originator", crate::gateway::current_wire_originator());
            if let Some(account_id) = official_chatgpt_account_id(&account, &upstream_base) {
                request = request.header("ChatGPT-Account-ID", account_id);
            }
            let response = request
                .send()
                .map_err(|err| format!("request official Codex model endpoint failed: {err}"))?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().unwrap_or_default();
                return Err(format!(
                    "official Codex model endpoint returned {status}: {}",
                    truncate_error_body(&body)
                ));
            }
            let response_etag = response
                .headers()
                .get(ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let value = response
                .json::<Value>()
                .map_err(|err| format!("decode official Codex model response failed: {err}"))?;
            official_model_catalog_from_value(&value)?;
            Ok((value, response_etag))
        })();
        match result {
            Ok(value) => return Ok(value),
            Err(err) => errors.push(format!("account {}: {err}", account.id)),
        }
    }
    Err(errors.join("; "))
}

fn official_chatgpt_account_id<'a>(account: &'a Account, upstream_base: &str) -> Option<&'a str> {
    if !crate::gateway::gateway_should_send_chatgpt_account_header(upstream_base) {
        return None;
    }
    account
        .chatgpt_account_id
        .as_deref()
        .or(account.workspace_id.as_deref())
}

fn official_catalog_cache_path(api_key_id: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(api_key_id.trim().as_bytes());
    crate::process_env::db_dir()
        .join(OFFICIAL_CATALOG_CACHE_DIR)
        .join(format!("{:x}.json", hasher.finalize()))
}

fn load_compatible_official_snapshot(cache_path: &Path, client_version: &str) -> Option<Value> {
    if !cache_path.is_file() {
        return None;
    }
    let content = match fs::read_to_string(cache_path) {
        Ok(content) => content,
        Err(err) => {
            log::warn!(
                "read official Codex model snapshot failed ({}): {err}",
                cache_path.display()
            );
            return None;
        }
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "parse official Codex model snapshot failed ({}): {err}",
                cache_path.display()
            );
            return None;
        }
    };
    if !official_snapshot_matches_client_version(&value, client_version) {
        return None;
    }
    if let Err(err) = official_model_catalog_from_value(&value) {
        log::warn!(
            "validate official Codex model snapshot failed ({}): {err}",
            cache_path.display()
        );
        return None;
    }
    Some(value)
}

fn official_snapshot_matches_client_version(value: &Value, client_version: &str) -> bool {
    value.get("client_version").and_then(Value::as_str) == Some(client_version)
}

fn official_snapshot_is_fresh(value: &Value, now: i64) -> bool {
    let Some(fetched_at) = value.get("fetched_at").and_then(Value::as_str) else {
        return false;
    };
    let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(fetched_at) else {
        return false;
    };
    now.saturating_sub(fetched_at.timestamp()) <= OFFICIAL_CATALOG_CACHE_TTL_SECS
}

fn build_official_snapshot(
    mut response: Value,
    client_version: &str,
    etag: Option<&str>,
) -> Result<Value, String> {
    official_model_catalog_from_value(&response)?;
    set_snapshot_fetch_metadata(&mut response, client_version, etag)?;
    Ok(response)
}

fn set_snapshot_fetch_metadata(
    snapshot: &mut Value,
    client_version: &str,
    etag: Option<&str>,
) -> Result<(), String> {
    let object = snapshot
        .as_object_mut()
        .ok_or_else(|| "official Codex model response is not a JSON object".to_string())?;
    object.insert(
        "fetched_at".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    );
    object.insert(
        "client_version".to_string(),
        Value::String(client_version.to_string()),
    );
    match etag.map(str::trim).filter(|value| !value.is_empty()) {
        Some(etag) => {
            object.insert("etag".to_string(), Value::String(etag.to_string()));
        }
        None => {
            object.remove("etag");
        }
    }
    Ok(())
}

fn write_official_snapshot(cache_path: &Path, snapshot: &Value) -> Result<(), String> {
    let mut content = serde_json::to_string_pretty(snapshot)
        .map_err(|err| format!("serialize official Codex model snapshot failed: {err}"))?;
    content.push('\n');
    write_atomic(cache_path, &content)
}

fn truncate_error_body(body: &str) -> String {
    const MAX_CHARS: usize = 2_048;
    let mut chars = body.chars();
    let preview: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn official_model_catalog_from_value(value: &Value) -> Result<Vec<Value>, String> {
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "official Codex model cache is missing models array".to_string())?;
    if models.is_empty() {
        return Err("official Codex model cache is empty".to_string());
    }
    for model in models {
        let slug = model
            .get("slug")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if slug.is_empty() {
            return Err("official Codex model cache contains a model without slug".to_string());
        }
    }
    Ok(models.clone())
}

fn serialize_gateway_model_catalog(catalog: &ModelsResponse) -> Result<String, String> {
    if catalog.models.is_empty() {
        return Err(
            "managed model catalog is empty; refusing to replace the Codex catalog".to_string(),
        );
    }
    let mut catalog = catalog.clone();
    for model in &mut catalog.models {
        prepare_managed_model(model);
    }
    let mut content = serde_json::to_string_pretty(&catalog)
        .map_err(|err| format!("serialize managed model catalog failed: {err}"))?;
    content.push('\n');
    Ok(content)
}

fn serialize_account_pool_model_catalog(official_models: &[Value]) -> Result<String, String> {
    if official_models.is_empty() {
        return Err(
            "official Codex model cache is empty; refusing to replace the Codex catalog"
                .to_string(),
        );
    }

    // Preserve every official object verbatim. Account-pool mode must not depend on Manager's
    // built-in model list, enabled flags, or schema knowledge.
    let mut content =
        serde_json::to_string_pretty(&serde_json::json!({ "models": official_models }))
            .map_err(|err| format!("serialize account-pool model catalog failed: {err}"))?;
    content.push('\n');
    Ok(content)
}

fn prepare_managed_model(model: &mut codexmanager_core::rpc::types::ModelInfo) {
    if model
        .shell_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        model.shell_type = Some("shell_command".to_string());
    }
    model.visibility.get_or_insert_with(|| "list".to_string());
    model.base_instructions.get_or_insert_with(String::new);
    model
        .availability_nux
        .get_or_insert(serde_json::Value::Null);
    model.upgrade.get_or_insert(serde_json::Value::Null);
    model.model_messages.get_or_insert_with(|| {
        serde_json::json!({
            "instructions_template": "",
            "instructions_variables": null,
            "approvals": null,
        })
    });
    model
        .default_reasoning_summary
        .get_or_insert_with(|| "auto".to_string());
    model.support_verbosity.get_or_insert(false);
    model
        .web_search_tool_type
        .get_or_insert_with(|| "text".to_string());
    model.truncation_policy.get_or_insert_with(|| {
        codexmanager_core::rpc::types::ModelTruncationPolicy {
            mode: "tokens".to_string(),
            limit: 10_000,
            ..Default::default()
        }
    });
    model.supports_parallel_tool_calls.get_or_insert(false);
    model.effective_context_window_percent.get_or_insert(95);

    let max_context_window = model.context_window.unwrap_or(200_000);
    model
        .extra
        .entry("max_context_window".to_string())
        .or_insert_with(|| serde_json::json!(max_context_window));
    for key in ["comp_hash", "tool_mode", "multi_agent_version"] {
        model
            .extra
            .entry(key.to_string())
            .or_insert(serde_json::Value::Null);
    }
    // Managed aggregate and hybrid catalogs keep the established full Responses transport.
    // Responses Lite requires official prompt metadata and must remain exclusive to the raw
    // official account-pool catalog.
    model.extra.insert(
        "use_responses_lite".to_string(),
        serde_json::Value::Bool(false),
    );
    model.extra.insert(
        "supports_reasoning_summary_parameter".to_string(),
        serde_json::Value::Bool(model.supports_reasoning_summaries.unwrap_or(false)),
    );
    model
        .extra
        .entry("include_skills_usage_instructions".to_string())
        .or_insert(serde_json::Value::Bool(false));
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("unable to resolve parent for {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "create catalog directory failed ({}): {err}",
            parent.display()
        )
    })?;
    let temp_path = temp_file_path(parent, path);
    fs::write(&temp_path, content).map_err(|err| {
        format!(
            "write catalog temp file failed ({}): {err}",
            temp_path.display()
        )
    })?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(_) if cfg!(windows) && path.exists() => {
            fs::remove_file(path).map_err(|err| {
                let _ = fs::remove_file(&temp_path);
                format!(
                    "remove previous model catalog failed ({}): {err}",
                    path.display()
                )
            })?;
            fs::rename(&temp_path, path).map_err(|err| {
                let _ = fs::remove_file(&temp_path);
                format!("replace model catalog failed ({}): {err}", path.display())
            })
        }
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Err(format!(
                "replace model catalog failed ({}): {err}",
                path.display()
            ))
        }
    }
}

fn temp_file_path(parent: &Path, target: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = target
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("gateway-models.json");
    parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        unique
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::rpc::types::ModelInfo;

    #[test]
    fn gateway_catalog_serializes_models_response_shape() {
        let catalog = ModelsResponse {
            models: vec![ModelInfo {
                slug: "gpt-test".to_string(),
                display_name: "GPT Test".to_string(),
                ..ModelInfo::default()
            }],
            ..ModelsResponse::default()
        };

        let content = serialize_gateway_model_catalog(&catalog).expect("serialize catalog");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse catalog");

        assert_eq!(value["models"][0]["slug"].as_str(), Some("gpt-test"));
        assert_eq!(
            value["models"][0]["shell_type"].as_str(),
            Some("shell_command")
        );
        assert_eq!(value["models"][0]["base_instructions"].as_str(), Some(""));
        assert_eq!(value["models"][0]["visibility"].as_str(), Some("list"));
        assert_eq!(
            value["models"][0]["default_reasoning_summary"].as_str(),
            Some("auto")
        );
        assert_eq!(
            value["models"][0]["support_verbosity"].as_bool(),
            Some(false)
        );
        assert_eq!(
            value["models"][0]["truncation_policy"]["mode"].as_str(),
            Some("tokens")
        );
        assert_eq!(
            value["models"][0]["truncation_policy"]["limit"].as_i64(),
            Some(10_000)
        );
        assert_eq!(
            value["models"][0]["supports_parallel_tool_calls"].as_bool(),
            Some(false)
        );
        assert_eq!(
            value["models"][0]["supports_reasoning_summary_parameter"].as_bool(),
            Some(false)
        );
        assert!(value["models"][0]["availability_nux"].is_null());
        assert!(value["models"][0]["upgrade"].is_null());
        assert_eq!(
            value["models"][0]["model_messages"]["instructions_template"].as_str(),
            Some("")
        );
        assert_eq!(
            value["models"][0]["effective_context_window_percent"].as_i64(),
            Some(95)
        );
        assert_eq!(
            value["models"][0]["max_context_window"].as_i64(),
            Some(200_000)
        );
        assert!(value["models"][0]["comp_hash"].is_null());
        assert!(value["models"][0]["tool_mode"].is_null());
        assert!(value["models"][0]["multi_agent_version"].is_null());
        assert_eq!(
            value["models"][0]["use_responses_lite"].as_bool(),
            Some(false)
        );
        assert_eq!(
            value["models"][0]["include_skills_usage_instructions"].as_bool(),
            Some(false)
        );
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn gateway_catalog_preserves_explicit_shell_type() {
        let catalog = ModelsResponse {
            models: vec![ModelInfo {
                slug: "gpt-test".to_string(),
                display_name: "GPT Test".to_string(),
                shell_type: Some("custom_shell".to_string()),
                ..ModelInfo::default()
            }],
            ..ModelsResponse::default()
        };

        let content = serialize_gateway_model_catalog(&catalog).expect("serialize catalog");
        let value: serde_json::Value = serde_json::from_str(&content).expect("parse catalog");

        assert_eq!(
            value["models"][0]["shell_type"].as_str(),
            Some("custom_shell")
        );
    }

    #[test]
    fn managed_catalog_disables_responses_lite_without_official_metadata() {
        let mut model = ModelInfo {
            slug: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            ..ModelInfo::default()
        };
        model
            .extra
            .insert("use_responses_lite".to_string(), Value::Bool(true));
        let catalog = ModelsResponse {
            models: vec![model],
            ..ModelsResponse::default()
        };

        let content = serialize_gateway_model_catalog(&catalog).expect("serialize catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");
        let model = &value["models"][0];

        assert_eq!(model["base_instructions"].as_str(), Some(""));
        assert_eq!(model["use_responses_lite"].as_bool(), Some(false));
    }

    #[test]
    fn account_pool_catalog_preserves_complete_official_catalog() {
        let official = official_model_catalog_from_value(&serde_json::json!({
            "models": [{
                "slug": "gpt-test",
                "display_name": "Official Name",
                "shell_type": "official_shell",
                "base_instructions": "official instructions",
                "future_codex_field": {"revision": 7}
            }, {
                "slug": "future-model-manager-does-not-know",
                "display_name": "Future Model",
                "future_codex_field": true
            }]
        }))
        .expect("parse official catalog");

        let content = serialize_account_pool_model_catalog(&official)
            .expect("serialize account-pool catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");
        let model = &value["models"][0];

        assert_eq!(value["models"].as_array().map(Vec::len), Some(2));
        assert_eq!(model["display_name"].as_str(), Some("Official Name"));
        assert_eq!(model["shell_type"].as_str(), Some("official_shell"));
        assert_eq!(model["future_codex_field"]["revision"].as_i64(), Some(7));
        assert!(model.get("max_context_window").is_none());
        assert_eq!(
            value["models"][1]["slug"].as_str(),
            Some("future-model-manager-does-not-know")
        );
    }

    #[test]
    fn official_catalog_rejects_missing_slug() {
        let err = official_model_catalog_from_value(&serde_json::json!({
            "models": [{"display_name": "Broken"}]
        }))
        .expect_err("missing slug must fail");
        assert!(err.contains("without slug"));
    }

    #[test]
    fn official_models_response_preserves_cache_metadata_and_future_fields() {
        let response = official_models_response_from_value(serde_json::json!({
            "fetched_at": "2026-07-24T00:00:00Z",
            "etag": "W/\"future\"",
            "client_version": "0.145.0",
            "models": [{
                "slug": "future-model",
                "display_name": "Future Model",
                "future_codex_field": {"revision": 9}
            }]
        }))
        .expect("decode official response");

        assert_eq!(response.models.len(), 1);
        assert_eq!(response.extra["etag"], "W/\"future\"");
        assert_eq!(
            response.models[0].extra["future_codex_field"]["revision"],
            9
        );
    }

    #[test]
    fn official_snapshot_is_scoped_to_exact_client_version() {
        let snapshot = serde_json::json!({
            "client_version": "0.145.0",
            "models": [{"slug": "gpt-test"}]
        });

        assert!(official_snapshot_matches_client_version(
            &snapshot, "0.145.0"
        ));
        assert!(!official_snapshot_matches_client_version(
            &snapshot, "0.146.0"
        ));
    }

    #[test]
    fn official_snapshot_uses_five_minute_ttl() {
        let snapshot = serde_json::json!({
            "fetched_at": "2026-07-24T00:00:00Z",
            "models": [{"slug": "gpt-test"}]
        });
        let fetched_at = chrono::DateTime::parse_from_rfc3339("2026-07-24T00:00:00Z")
            .expect("timestamp")
            .timestamp();

        assert!(official_snapshot_is_fresh(&snapshot, fetched_at + 299));
        assert!(official_snapshot_is_fresh(&snapshot, fetched_at + 300));
        assert!(!official_snapshot_is_fresh(&snapshot, fetched_at + 301));
    }

    #[test]
    fn official_snapshot_preserves_unknown_response_fields() {
        let snapshot = build_official_snapshot(
            serde_json::json!({
                "models": [{
                    "slug": "future-model",
                    "future_codex_field": {"revision": 11}
                }],
                "future_top_level_field": true
            }),
            "0.145.0",
            Some("W/\"catalog\""),
        )
        .expect("build snapshot");

        assert_eq!(snapshot["client_version"], "0.145.0");
        assert_eq!(snapshot["etag"], "W/\"catalog\"");
        assert_eq!(snapshot["future_top_level_field"], true);
        assert_eq!(snapshot["models"][0]["future_codex_field"]["revision"], 11);
        assert!(snapshot["fetched_at"].as_str().is_some());
    }

    #[test]
    fn gateway_catalog_rejects_empty_models() {
        let err = serialize_gateway_model_catalog(&ModelsResponse::default())
            .expect_err("empty catalog must fail");
        assert!(err.contains("empty"));
    }
}
