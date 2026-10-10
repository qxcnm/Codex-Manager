use codexmanager_core::rpc::types::ModelsResponse;
use codexmanager_core::storage::{Account, Storage};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, ETAG, USER_AGENT};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const OFFICIAL_CATALOG_CACHE_DIR: &str = "official-model-catalogs";
const OFFICIAL_CATALOG_CACHE_TTL_SECS: i64 = 300;
static OFFICIAL_CATALOG_SYNC_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CATALOG_PHASE_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

async fn catalog_phase<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let permit = CATALOG_PHASE_WORKERS
        .acquire()
        .await
        .map_err(|_| "Codex catalog workers unavailable".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| format!("Codex catalog worker failed: {error}"))?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayCatalogPolicy {
    OfficialAccountPool,
    Managed,
}

pub(crate) fn gateway_catalog_policy_for_api_key(
    storage: &Storage,
    api_key_id: &str,
) -> Result<GatewayCatalogPolicy, String> {
    let api_key = crate::apikey::remote::find_by_id(storage, api_key_id)
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

// Legacy synchronous entry points are retained for desktop/Rhai callers; native routes await the async variants.
#[allow(dead_code)]
pub(crate) fn models_response_for_gateway_key(
    storage: &Storage,
    api_key_id: &str,
) -> Result<(ModelsResponse, GatewayCatalogPolicy), String> {
    crate::gateway::run_upstream_io(models_response_for_gateway_key_async(storage, api_key_id))?
}

pub(crate) async fn models_response_for_gateway_key_async(
    storage: &Storage,
    api_key_id: &str,
) -> Result<(ModelsResponse, GatewayCatalogPolicy), String> {
    let owned_storage = storage.shared_handle();
    let owned_key = api_key_id.to_owned();
    let policy =
        catalog_phase(move || gateway_catalog_policy_for_api_key(&owned_storage, &owned_key))
            .await?;
    let response = match policy {
        GatewayCatalogPolicy::OfficialAccountPool => {
            let value = load_or_sync_official_model_catalog(storage, api_key_id).await?;
            catalog_phase(move || official_models_response_from_value(value)).await?
        }
        GatewayCatalogPolicy::Managed => {
            let storage = storage.shared_handle();
            catalog_phase(move || crate::models_v2::models_response_with_storage(&storage)).await?
        }
    };
    Ok((response, policy))
}

fn official_models_response_from_value(value: Value) -> Result<ModelsResponse, String> {
    official_model_catalog_from_value(&value)?;
    serde_json::from_value(value)
        .map_err(|err| format!("decode official Codex model cache failed: {err}"))
}

#[allow(dead_code)]
pub(crate) fn write_gateway_model_catalog(
    storage: &Storage,
    api_key_id: &str,
    catalog_path: &Path,
    policy: GatewayCatalogPolicy,
) -> Result<usize, String> {
    crate::gateway::run_upstream_io(write_gateway_model_catalog_async(
        storage,
        api_key_id,
        catalog_path,
        policy,
    ))?
}

#[allow(dead_code)]
pub(crate) async fn write_gateway_model_catalog_async(
    storage: &Storage,
    api_key_id: &str,
    catalog_path: &Path,
    policy: GatewayCatalogPolicy,
) -> Result<usize, String> {
    let (content, models_count) =
        gateway_model_catalog_content_async(storage, api_key_id, policy).await?;
    let catalog_path = catalog_path.to_owned();
    catalog_phase(move || write_atomic(&catalog_path, &content)).await?;
    Ok(models_count)
}

pub(crate) async fn gateway_model_catalog_content_async(
    storage: &Storage,
    api_key_id: &str,
    policy: GatewayCatalogPolicy,
) -> Result<(String, usize), String> {
    match policy {
        GatewayCatalogPolicy::OfficialAccountPool => {
            let official_cache = load_or_sync_official_model_catalog(storage, api_key_id).await?;
            catalog_phase(move || {
                let official_models = official_model_catalog_from_value(&official_cache)?;
                let models_count = official_models.len();
                Ok((
                    serialize_account_pool_model_catalog(&official_models)?,
                    models_count,
                ))
            })
            .await
        }
        GatewayCatalogPolicy::Managed => managed_model_catalog_content_async(storage).await,
    }
}

pub(crate) async fn managed_model_catalog_content_async(
    storage: &Storage,
) -> Result<(String, usize), String> {
    let storage = storage.shared_handle();
    catalog_phase(move || {
        let catalog = crate::models_v2::text_generation_models_response_with_storage(&storage)?;
        let models_count = catalog.models.len();
        Ok((serialize_gateway_model_catalog(&catalog)?, models_count))
    })
    .await
}

pub(crate) async fn selected_managed_model_catalog_content_async(
    storage: &Storage,
    model_slugs: Vec<String>,
) -> Result<(String, Vec<String>), String> {
    let storage = storage.shared_handle();
    catalog_phase(move || {
        let mut requested = Vec::new();
        let mut seen = HashSet::new();
        for slug in model_slugs {
            let slug = slug.trim();
            if slug.is_empty() {
                continue;
            }
            let normalized = slug.to_ascii_lowercase();
            if seen.insert(normalized.clone()) {
                requested.push((slug.to_string(), normalized));
            }
        }
        if requested.is_empty() {
            return Err("no models selected".to_string());
        }

        let requested_keys = requested
            .iter()
            .map(|(_, normalized)| normalized.clone())
            .collect::<HashSet<_>>();
        let selected = crate::models_v2::list_with_storage(&storage, true)?
            .items
            .into_iter()
            .filter(|model| requested_keys.contains(&model.slug.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        let selected_keys = selected
            .iter()
            .map(|model| model.slug.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let unknown = requested
            .into_iter()
            .filter_map(|(requested_slug, normalized)| {
                (!selected_keys.contains(&normalized)).then_some(requested_slug)
            })
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown managed model slug(s): {}",
                unknown.join(", ")
            ));
        }

        let canonical_slugs = selected.iter().map(|model| model.slug.clone()).collect();
        let catalog = ModelsResponse {
            models: selected.iter().map(selected_codex_model_info).collect(),
            ..ModelsResponse::default()
        };
        Ok((serialize_gateway_model_catalog(&catalog)?, canonical_slugs))
    })
    .await
}

pub(crate) async fn reconciled_managed_model_catalog_content_async(
    storage: &Storage,
    model_slugs: Vec<String>,
) -> Result<(Option<String>, Vec<String>), String> {
    let storage = storage.shared_handle();
    catalog_phase(move || {
        let requested_keys = model_slugs
            .into_iter()
            .map(|slug| slug.trim().to_ascii_lowercase())
            .filter(|slug| !slug.is_empty())
            .collect::<HashSet<_>>();
        let selected = crate::models_v2::list_with_storage(&storage, true)?
            .items
            .into_iter()
            .filter(|model| requested_keys.contains(&model.slug.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Ok((None, Vec::new()));
        }

        let canonical_slugs = selected.iter().map(|model| model.slug.clone()).collect();
        let catalog = ModelsResponse {
            models: selected.iter().map(selected_codex_model_info).collect(),
            ..ModelsResponse::default()
        };
        Ok((
            Some(serialize_gateway_model_catalog(&catalog)?),
            canonical_slugs,
        ))
    })
    .await
}

fn selected_codex_model_info(
    model: &codexmanager_core::storage::ManagedModelV2,
) -> codexmanager_core::rpc::types::ModelInfo {
    let mut info = crate::models_v2::model_info(model);
    // Applying an explicit selection is the user's allowlist for Codex's picker.
    // Keep the stored flags unchanged for gateway/API behavior, but do not let
    // Codex hide an explicitly selected entry when it reads this dedicated file.
    info.visibility = Some("list".to_string());
    info.supported_in_api = true;
    info
}

async fn load_official_snapshot_async(
    cache_path: &Path,
    client_version: &str,
) -> Result<Option<Value>, String> {
    let cache_path = cache_path.to_owned();
    let client_version = client_version.to_owned();
    catalog_phase(move || {
        Ok(load_compatible_official_snapshot(
            &cache_path,
            &client_version,
        ))
    })
    .await
}

async fn load_or_sync_official_model_catalog(
    storage: &Storage,
    api_key_id: &str,
) -> Result<Value, String> {
    let client_version = crate::gateway::current_codex_user_agent_version();
    let cache_path = official_catalog_cache_path(api_key_id);
    let now = chrono::Utc::now().timestamp();
    let cached = load_official_snapshot_async(&cache_path, &client_version).await?;
    if cached
        .as_ref()
        .is_some_and(|value| official_snapshot_is_fresh(value, now))
    {
        return Ok(cached.expect("checked above"));
    }

    let sync_lock = OFFICIAL_CATALOG_SYNC_LOCK.get_or_init(|| Mutex::new(()));
    let lease = Arc::new(sync_lock.lock().await);

    // Another caller may have completed the refresh while this caller waited for the lock.
    let cached = load_official_snapshot_async(&cache_path, &client_version).await?;
    if cached
        .as_ref()
        .is_some_and(|value| official_snapshot_is_fresh(value, chrono::Utc::now().timestamp()))
    {
        return Ok(cached.expect("checked above"));
    }

    match fetch_official_model_catalog(storage, api_key_id, &client_version).await {
        Ok((response, etag)) => {
            // A cancelled request must not release the sync lock before its
            // already-started snapshot write commits on the disk worker.
            catalog_phase(move || {
            let _lease = lease;
            let snapshot = build_official_snapshot(response, &client_version, etag.as_deref())?;
            write_official_snapshot(&cache_path, &snapshot)?;
            Ok(snapshot)
            }).await
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

async fn fetch_official_model_catalog(
    storage: &Storage,
    api_key_id: &str,
    client_version: &str,
) -> Result<(Value, Option<String>), String> {
    let owned_storage = storage.shared_handle();
    let api_key_id = api_key_id.to_owned();
    let (upstream_base, routed) = catalog_phase(move || {
    let api_key = crate::apikey::remote::find_by_id(&owned_storage, &api_key_id)
        .map_err(|err| format!("read api key routing config failed: {err}"))?
        .ok_or_else(|| "api key not found".to_string())?;
    let upstream_base = crate::gateway::gateway_resolve_effective_upstream_base(&api_key);
    if !crate::gateway::gateway_should_send_chatgpt_account_header(&upstream_base) {
        return Err(format!(
            "account-pool model sync requires the official ChatGPT Codex backend, got {upstream_base}"
        ));
    }
    let routed = crate::gateway::gateway_collect_routed_candidates_with_log_source(
        &owned_storage, &api_key_id, None,
    )?;
    Ok((upstream_base, routed))
    }).await?;

    let (models_url, _) =
        crate::gateway::gateway_compute_upstream_url(&upstream_base, "/v1/models");
    let mut models_url = reqwest::Url::parse(&models_url)
        .map_err(|err| format!("build official Codex model endpoint failed: {err}"))?;
    models_url
        .query_pairs_mut()
        .append_pair("client_version", client_version);

    if routed.candidates.is_empty() {
        return Err("no available OpenAI account for official model sync".to_string());
    }

    let mut errors = Vec::new();
    for (account, mut token) in routed.candidates {
        let result = async {
            let bearer = crate::gateway::gateway_resolve_openai_bearer_token_async(
                storage, &account, &mut token,
            )
            .await?;
            let client = crate::gateway::async_upstream_client_for_account(account.id.as_str())?;
            let mut request = client
                .get(models_url.clone())
                .header(
                    AUTHORIZATION,
                    crate::agent_identity::format_upstream_authorization(&bearer),
                )
                .header(ACCEPT, "application/json")
                .header(ACCEPT_ENCODING, "identity")
                .header(USER_AGENT, crate::gateway::current_gateway_user_agent())
                .header("originator", crate::gateway::current_wire_originator());
            if let Some(account_id) = official_chatgpt_account_id(&account, &upstream_base) {
                request = request.header("ChatGPT-Account-ID", account_id);
            }
            let response = request
                .send()
                .await
                .map_err(|err| format!("request official Codex model endpoint failed: {err}"))?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
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
            let body = response
                .bytes()
                .await
                .map_err(|err| format!("read official Codex model response failed: {err}"))?;
            catalog_phase(move || {
                let value = serde_json::from_slice::<Value>(&body)
                    .map_err(|err| format!("decode official Codex model response failed: {err}"))?;
                official_model_catalog_from_value(&value)?;
                Ok((value, response_etag))
            })
            .await
        }
        .await;
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
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            account
                .workspace_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
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
    model.visibility = Some("list".to_string());
    model.base_instructions.get_or_insert_with(String::new);
    model
        .availability_nux
        .get_or_insert(serde_json::Value::Null);
    model.upgrade.get_or_insert(serde_json::Value::Null);
    if let Some(serde_json::Value::Object(upgrade)) = model.upgrade.as_mut() {
        let migration_markdown = upgrade
            .get("migration_markdown")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                upgrade
                    .get("upgrade_copy")
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or_default()
            .to_string();
        upgrade.insert(
            "migration_markdown".to_string(),
            serde_json::Value::String(migration_markdown),
        );
        upgrade
            .entry("retirement_at".to_string())
            .or_insert(serde_json::Value::Null);
    }
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
    use codexmanager_core::storage::{
        ManagedModelV2, ManagedModelV2Upsert, ModelPriceV2, ModelRouteV2,
    };

    #[tokio::test(flavor = "current_thread")]
    async fn aggregate_model_reasoning_levels_survive_persistence_and_catalog_generation() {
        let temp_root = std::env::temp_dir().join(format!(
            "codexmanager-aggregate-reasoning-catalog-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_root).expect("create catalog temp dir");
        let storage = Storage::open(&temp_root.join("codexmanager.db")).expect("open storage");
        storage.init().expect("initialize storage");

        let inputs = [
            (
                "deepseek-flash",
                vec!["low", "medium", "high"],
                Some("medium"),
            ),
            ("plain-upstream", Vec::new(), None),
        ]
        .into_iter()
        .map(|(slug, efforts, default)| ManagedModelV2Upsert {
            model: ManagedModelV2 {
                slug: slug.to_string(),
                display_name: slug.to_string(),
                origin: "custom".to_string(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".to_string(),
                default_reasoning_effort: default.map(str::to_string),
                capabilities: serde_json::json!({
                    "supports_text_generation": true,
                    "reasoningEfforts": efforts,
                }),
                instructions_mode: "passthrough".to_string(),
                price: ModelPriceV2 {
                    price_status: "missing".to_string(),
                    ..Default::default()
                },
                routes: vec![ModelRouteV2 {
                    source_kind: "aggregate_api".to_string(),
                    source_id: "agg-deepseek".to_string(),
                    upstream_model: slug.to_string(),
                    enabled: true,
                    weight: 1,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        })
        .collect::<Vec<_>>();
        storage
            .upsert_managed_models_v2(&inputs)
            .expect("save aggregate models");

        let stored = storage
            .get_managed_model_v2("deepseek-flash")
            .expect("read configured model")
            .expect("configured model exists");
        assert_eq!(
            stored.capabilities["reasoningEfforts"],
            serde_json::json!(["low", "medium", "high"])
        );
        assert_eq!(stored.default_reasoning_effort.as_deref(), Some("medium"));

        let (full_content, _) = managed_model_catalog_content_async(&storage)
            .await
            .expect("generate managed catalog");
        let (selected_content, selected_slugs) = selected_managed_model_catalog_content_async(
            &storage,
            vec!["deepseek-flash".to_string(), "plain-upstream".to_string()],
        )
        .await
        .expect("generate selected catalog");
        assert_eq!(selected_slugs, ["deepseek-flash", "plain-upstream"]);

        for content in [full_content, selected_content] {
            let catalog: Value = serde_json::from_str(&content).expect("parse generated catalog");
            let models = catalog["models"].as_array().expect("models array");
            let reasoning = models
                .iter()
                .find(|model| model["slug"] == "deepseek-flash")
                .expect("configured aggregate model");
            assert_eq!(reasoning["default_reasoning_level"], "medium");
            assert_eq!(
                reasoning["supported_reasoning_levels"],
                serde_json::json!([
                    {"effort": "low", "description": ""},
                    {"effort": "medium", "description": ""},
                    {"effort": "high", "description": ""}
                ])
            );
            let unsupported = models
                .iter()
                .find(|model| model["slug"] == "plain-upstream")
                .expect("unconfigured aggregate model");
            assert_eq!(
                unsupported["supported_reasoning_levels"],
                serde_json::json!([])
            );
            assert!(unsupported["default_reasoning_level"].is_null());
        }

        drop(storage);
        let _ = fs::remove_dir_all(temp_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn selected_catalog_uses_catalog_order_and_canonical_slugs() {
        let temp_root = std::env::temp_dir().join(format!(
            "codexmanager-selected-models-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_root).expect("create catalog temp dir");
        let db_path = temp_root.join("codexmanager.db");
        let storage = Storage::open(&db_path).expect("open catalog storage");
        storage.init().expect("initialize catalog storage");
        let mut image_model = storage
            .get_managed_model_v2("gpt-image-2")
            .expect("read image model")
            .expect("seeded image model");
        image_model.enabled = false;
        image_model.supported_in_api = false;
        image_model.visibility = "hide".to_string();
        storage
            .upsert_managed_model_v2(&codexmanager_core::storage::ManagedModelV2Upsert {
                previous_slug: Some(image_model.slug.clone()),
                model: image_model,
            })
            .expect("make image model hidden and unavailable");
        let requested_keys = HashSet::from(["gpt-6-sol", "gpt-image-2"]);
        let expected = crate::models_v2::list_with_storage(&storage, true)
            .expect("list full catalog")
            .items
            .into_iter()
            .filter(|model| requested_keys.contains(model.slug.as_str()))
            .map(|model| model.slug)
            .collect::<Vec<_>>();
        assert_eq!(expected.len(), 2);

        let (content, canonical) = selected_managed_model_catalog_content_async(
            &storage,
            vec![
                " GPT-IMAGE-2 ".to_string(),
                "gpt-6-sol".to_string(),
                "GPT-6-SOL".to_string(),
            ],
        )
        .await
        .expect("build selected catalog");
        assert_eq!(canonical, expected);
        let catalog: serde_json::Value = serde_json::from_str(&content).expect("parse catalog");
        let models = catalog["models"].as_array().expect("models array");
        assert_eq!(
            models
                .iter()
                .map(|model| model["slug"].as_str().expect("model slug").to_string())
                .collect::<Vec<_>>(),
            canonical
        );
        assert!(models
            .iter()
            .all(|model| model["visibility"].as_str() == Some("list")));
        assert!(!models.iter().any(|model| model["slug"] == "gpt-5.6-sol"));
        let image = models
            .iter()
            .find(|model| model["slug"] == "gpt-image-2")
            .expect("selected image model");
        assert_eq!(image["visibility"], "list");
        assert_eq!(image["supported_in_api"], true);
        assert_eq!(image["supports_text_generation"], false);
        assert_eq!(image["output_modalities"], serde_json::json!(["image"]));
        assert_eq!(
            image["supported_endpoints"],
            serde_json::json!(["/v1/images/generations", "/v1/images/edits"])
        );
        let stored_image = storage
            .get_managed_model_v2("gpt-image-2")
            .expect("read persisted image model")
            .expect("persisted image model");
        assert!(!stored_image.enabled);
        assert!(!stored_image.supported_in_api);
        assert_eq!(stored_image.visibility, "hide");

        drop(storage);
        let _ = fs::remove_dir_all(temp_root);
    }

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
    fn gateway_catalog_normalizes_legacy_upgrade_metadata_for_current_codex() {
        let catalog = ModelsResponse {
            models: vec![ModelInfo {
                slug: "gpt-old".to_string(),
                display_name: "GPT Old".to_string(),
                upgrade: Some(serde_json::json!({
                    "model": "gpt-new",
                    "upgrade_copy": "Use GPT New"
                })),
                ..ModelInfo::default()
            }],
            ..ModelsResponse::default()
        };

        let content = serialize_gateway_model_catalog(&catalog).expect("serialize catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");
        let upgrade = &value["models"][0]["upgrade"];

        assert_eq!(upgrade["model"].as_str(), Some("gpt-new"));
        assert_eq!(upgrade["upgrade_copy"].as_str(), Some("Use GPT New"));
        assert_eq!(upgrade["migration_markdown"].as_str(), Some("Use GPT New"));
        assert!(upgrade["retirement_at"].is_null());
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

    #[test]
    fn official_model_sync_omits_empty_account_identity_header_value() {
        let account = Account {
            id: "account-1".to_string(),
            label: "Account 1".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("  ".to_string()),
            workspace_id: Some("\t".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(
            official_chatgpt_account_id(&account, "https://chatgpt.com/backend-api/codex"),
            None
        );

        let account = Account {
            id: "account-1b".to_string(),
            label: "Account 1b".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("  ".to_string()),
            workspace_id: Some(" workspace-456 ".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(
            official_chatgpt_account_id(&account, "https://chatgpt.com/backend-api/codex"),
            Some("workspace-456")
        );

        let account = Account {
            id: "account-2".to_string(),
            label: "Account 2".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("  account-123  ".to_string()),
            workspace_id: Some("workspace-456".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(
            official_chatgpt_account_id(&account, "https://chatgpt.com/backend-api/codex"),
            Some("account-123")
        );
    }
}
