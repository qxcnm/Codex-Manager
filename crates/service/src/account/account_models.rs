use codexmanager_core::rpc::types::AggregateApiAssociateModelsResult;
use codexmanager_core::storage::{
    now_ts, ManagedModelRouteEnsureV2, ManagedModelV2, ManagedModelV2Upsert, ModelFastPolicyV2,
    ModelPriceV2, ModelRouteV2, Storage,
};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, USER_AGENT};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use crate::storage_helpers::open_storage;

const ACCOUNT_POOL_SOURCE_KIND: &str = "account_pool";
const ACCOUNT_POOL_SOURCE_ID: &str = "default";
const MAX_FETCHED_MODELS: usize = 500;
const MAX_MODELS_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const ACCOUNT_MODELS_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const OFFICIAL_CODEX_BACKEND_PATH: &str = "/backend-api/codex";
const OFFICIAL_CODEX_BACKEND_REQUIRED: &str =
    "account model discovery requires the official ChatGPT Codex backend";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountFetchedModel {
    pub(crate) upstream_model: String,
    pub(crate) display_name: Option<String>,
    pub(crate) existing_model_slug: Option<String>,
    pub(crate) already_linked: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountFetchModelsResult {
    pub(crate) account_id: String,
    pub(crate) fetched_at: i64,
    pub(crate) items: Vec<AccountFetchedModel>,
}

fn normalize_model_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 200
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(value.to_string())
}

fn model_id_from_value(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return normalize_model_id(value);
    }
    ["slug", "id", "model"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .and_then(normalize_model_id)
}

fn model_display_name_from_value(value: &Value) -> Option<String> {
    ["display_name", "displayName", "name"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .filter(|ch| !ch.is_control())
                .take(200)
                .collect()
        })
        .filter(|value: &String| !value.is_empty())
}

fn parse_account_models(body: &Value) -> Vec<(String, Option<String>)> {
    let candidates = body
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| body.get("data").and_then(Value::as_array))
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for value in candidates {
        let Some(upstream_model) = model_id_from_value(&value) else {
            continue;
        };
        if !seen.insert(upstream_model.to_ascii_lowercase()) {
            continue;
        }
        parsed.push((upstream_model, model_display_name_from_value(&value)));
        if parsed.len() >= MAX_FETCHED_MODELS {
            break;
        }
    }
    parsed
}

fn account_fetched_model(
    existing: &[ManagedModelV2],
    upstream_model: String,
    display_name: Option<String>,
) -> AccountFetchedModel {
    let catalog_slug = crate::models_v2::policy_catalog_slug(upstream_model.as_str());
    let model = existing
        .iter()
        .find(|model| model.slug.eq_ignore_ascii_case(catalog_slug));
    let already_linked = model.is_some_and(|model| {
        model.routes.iter().any(|route| {
            route.source_kind == ACCOUNT_POOL_SOURCE_KIND
                && route.source_id == ACCOUNT_POOL_SOURCE_ID
                && (route
                    .upstream_model
                    .eq_ignore_ascii_case(upstream_model.as_str())
                    || crate::models_v2::should_preserve_luna_reserve_alias(
                        Some(upstream_model.as_str()),
                        Some(route.upstream_model.as_str()),
                    ))
        })
    });
    AccountFetchedModel {
        upstream_model,
        display_name,
        existing_model_slug: model.map(|model| model.slug.clone()),
        already_linked,
    }
}

async fn read_models_response(mut response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_MODELS_RESPONSE_BYTES)
    {
        return Err("account models response is too large".to_string());
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "account models response could not be read".to_string())?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_MODELS_RESPONSE_BYTES {
            return Err("account models response is too large".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() > MAX_MODELS_RESPONSE_BYTES {
        return Err("account models response is too large".to_string());
    }
    if !status.is_success() {
        return Err(format!("account models http_status={}", status.as_u16()));
    }
    serde_json::from_slice(bytes.as_slice())
        .map_err(|_| "account models response is not valid JSON".to_string())
}

fn validate_official_codex_backend(raw: &str) -> Result<reqwest::Url, String> {
    let raw = raw.trim();
    let raw_authority = raw
        .split_once("://")
        .map(|(_, remainder)| remainder.split(['/', '?', '#']).next().unwrap_or(remainder))
        .unwrap_or_default();
    let url = reqwest::Url::parse(raw).map_err(|_| OFFICIAL_CODEX_BACKEND_REQUIRED.to_string())?;
    let host_is_official = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("chatgpt.com") || host.eq_ignore_ascii_case("chat.openai.com")
    });
    let path_is_codex_backend = url.path().trim_end_matches('/') == OFFICIAL_CODEX_BACKEND_PATH;
    let has_userinfo =
        !url.username().is_empty() || url.password().is_some() || raw_authority.contains('@');

    if url.scheme() != "https"
        || !host_is_official
        || url.port_or_known_default() != Some(443)
        || has_userinfo
        || !path_is_codex_backend
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OFFICIAL_CODEX_BACKEND_REQUIRED.to_string());
    }
    Ok(url)
}

fn with_account_models_timeout(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.timeout(ACCOUNT_MODELS_REQUEST_TIMEOUT)
}

pub(crate) fn fetch_account_models(account_id: &str) -> Result<AccountFetchModelsResult, String> {
    crate::gateway::run_upstream_io(fetch_account_models_async(account_id))?
}

pub(crate) async fn fetch_account_models_async(
    account_id: &str,
) -> Result<AccountFetchModelsResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("account id required".to_string());
    }

    // Validate the exact destination before loading or resolving any selected-account credential.
    // Reuse this parsed URL below so the checked authority is also the requested authority.
    let upstream_base = crate::gateway::gateway_resolve_default_upstream_base_url();
    let upstream_base = validate_official_codex_backend(&upstream_base)?;

    let storage = open_storage()
        .map(|pooled| pooled.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let Some((account, mut token)) = storage
        .find_account_with_token_by_id(account_id)
        .map_err(|err| format!("read account failed: {err}"))?
    else {
        return if storage
            .find_account_by_id(account_id)
            .map_err(|err| format!("read account failed: {err}"))?
            .is_some()
        {
            Err("account token not found".to_string())
        } else {
            Err("account not found".to_string())
        };
    };

    let (models_url, _) =
        crate::gateway::gateway_compute_upstream_url(upstream_base.as_str(), "/v1/models");
    let mut models_url = reqwest::Url::parse(models_url.as_str())
        .map_err(|_| "build account models endpoint failed".to_string())?;
    models_url.query_pairs_mut().append_pair(
        "client_version",
        crate::gateway::current_codex_user_agent_version().as_str(),
    );

    let bearer =
        crate::gateway::gateway_resolve_openai_bearer_token_async(&storage, &account, &mut token)
            .await
            .map_err(|_| "resolve account authorization failed".to_string())?;
    let client = crate::gateway::async_upstream_client_for_account(account.id.as_str())
        .map_err(|_| "build account models request client failed".to_string())?;
    let mut request = with_account_models_timeout(client.get(models_url))
        .header(
            AUTHORIZATION,
            crate::agent_identity::format_upstream_authorization(&bearer),
        )
        .header(ACCEPT, "application/json")
        .header(ACCEPT_ENCODING, "identity")
        .header(USER_AGENT, crate::gateway::current_gateway_user_agent())
        .header("originator", crate::gateway::current_wire_originator());
    let chatgpt_account_id = account
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
        });
    if let Some(chatgpt_account_id) = chatgpt_account_id {
        request = request.header("ChatGPT-Account-ID", chatgpt_account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|_| "account models request failed".to_string())?;
    let body = read_models_response(response).await?;
    let parsed = parse_account_models(&body);
    let existing = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("read model catalog V2 failed: {err}"))?;
    let items = parsed
        .into_iter()
        .map(|(upstream_model, display_name)| {
            account_fetched_model(existing.as_slice(), upstream_model, display_name)
        })
        .collect();

    Ok(AccountFetchModelsResult {
        account_id: account.id,
        fetched_at: now_ts(),
        items,
    })
}

pub(crate) fn associate_account_models(
    account_id: &str,
    upstream_models: Vec<String>,
    display_names: BTreeMap<String, String>,
) -> Result<AggregateApiAssociateModelsResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("account id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("read account failed: {err}"))?
        .is_none()
    {
        return Err("account not found".to_string());
    }
    associate_account_models_with_storage(&storage, upstream_models, display_names)
}

fn associate_account_models_with_storage(
    storage: &Storage,
    upstream_models: Vec<String>,
    display_names: BTreeMap<String, String>,
) -> Result<AggregateApiAssociateModelsResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let mut requested = Vec::new();
    let mut seen = HashSet::new();
    for raw in upstream_models {
        let Some(model) = normalize_model_id(raw.as_str()) else {
            return Err("invalid upstream model id".to_string());
        };
        let catalog_model = crate::models_v2::policy_catalog_slug(model.as_str()).to_string();
        if seen.insert(catalog_model.to_ascii_lowercase()) {
            requested.push((model, catalog_model));
        }
    }
    if requested.is_empty() {
        return Ok(AggregateApiAssociateModelsResult::default());
    }

    let models = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("read model catalog V2 failed: {err}"))?;
    let next_sort = models
        .iter()
        .map(|model| model.sort_order)
        .max()
        .unwrap_or(0)
        + 1;
    let mut create_candidates = Vec::new();
    let mut route_inputs = Vec::new();

    for (upstream_model, catalog_model) in &requested {
        if let Some(model) = models
            .iter()
            .find(|model| model.slug.eq_ignore_ascii_case(catalog_model.as_str()))
        {
            let inherited = model
                .routes
                .iter()
                .find(|route| {
                    route.source_kind == ACCOUNT_POOL_SOURCE_KIND
                        && route.source_id == ACCOUNT_POOL_SOURCE_ID
                })
                .map(|route| (route.priority, route.weight))
                .unwrap_or((0, 1));
            route_inputs.push(ManagedModelRouteEnsureV2 {
                model_slug: model.slug.clone(),
                route: ModelRouteV2 {
                    source_kind: ACCOUNT_POOL_SOURCE_KIND.to_string(),
                    source_id: ACCOUNT_POOL_SOURCE_ID.to_string(),
                    upstream_model: if codexmanager_core::usage::is_luna_reserve_model(Some(
                        upstream_model.as_str(),
                    )) {
                        model.slug.clone()
                    } else {
                        upstream_model.clone()
                    },
                    enabled: true,
                    priority: inherited.0,
                    weight: inherited.1.max(1),
                    ..Default::default()
                },
            });
            continue;
        }

        let display_name = display_names
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(upstream_model.as_str()))
            .or_else(|| {
                display_names
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(catalog_model.as_str()))
            })
            .and_then(|(_, value)| model_display_name_from_value(&json!({ "name": value })))
            .unwrap_or_else(|| catalog_model.clone());
        let model = ManagedModelV2 {
            slug: catalog_model.clone(),
            display_name,
            provider: Some("OpenAI".to_string()),
            origin: "custom".to_string(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".to_string(),
            sort_order: next_sort + create_candidates.len() as i64,
            capabilities: json!({
                "supports_text_generation": true,
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            }),
            instructions_mode: "passthrough".to_string(),
            fast_policy: ModelFastPolicyV2::Passthrough,
            price: ModelPriceV2 {
                price_status: "missing".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        create_candidates.push(ManagedModelV2Upsert {
            model,
            ..Default::default()
        });
        route_inputs.push(ManagedModelRouteEnsureV2 {
            model_slug: catalog_model.clone(),
            route: ModelRouteV2 {
                source_kind: ACCOUNT_POOL_SOURCE_KIND.to_string(),
                source_id: ACCOUNT_POOL_SOURCE_ID.to_string(),
                upstream_model: catalog_model.clone(),
                enabled: true,
                priority: 0,
                weight: 1,
                ..Default::default()
            },
        });
    }

    let result = storage
        .upsert_missing_managed_models_and_ensure_routes_v2(&create_candidates, &route_inputs)
        .map_err(|err| format!("associate account models transaction failed: {err}"))?;
    Ok(AggregateApiAssociateModelsResult {
        created_models: result.created_models,
        added_routes: result.added_routes,
        unchanged_routes: result.unchanged_routes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_model(slug: &str) -> ManagedModelV2 {
        ManagedModelV2 {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            origin: "custom".to_string(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".to_string(),
            instructions_mode: "passthrough".to_string(),
            fast_policy: ModelFastPolicyV2::Passthrough,
            price: ModelPriceV2 {
                price_status: "missing".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn parses_official_models_with_safe_deduplication() {
        let value = json!({
            "models": [
                { "slug": "gpt-next", "display_name": "GPT Next" },
                { "id": "GPT-NEXT", "name": "duplicate" },
                { "model": "gpt-small", "displayName": "GPT Small" },
                { "slug": "bad model" },
                "gpt-string"
            ]
        });
        let parsed = parse_account_models(&value);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0],
            ("gpt-next".to_string(), Some("GPT Next".to_string()))
        );
        assert_eq!(
            parsed[1],
            ("gpt-small".to_string(), Some("GPT Small".to_string()))
        );
        assert_eq!(parsed[2], ("gpt-string".to_string(), None));
    }

    #[test]
    fn fetched_reserve_alias_uses_the_existing_luna_catalog_route() {
        let mut luna = custom_model(codexmanager_core::usage::LUNA_MODEL_SLUG);
        luna.routes.push(ModelRouteV2 {
            source_kind: ACCOUNT_POOL_SOURCE_KIND.to_string(),
            source_id: ACCOUNT_POOL_SOURCE_ID.to_string(),
            upstream_model: codexmanager_core::usage::LUNA_MODEL_SLUG.to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            ..Default::default()
        });

        let fetched = account_fetched_model(
            &[luna],
            codexmanager_core::usage::LUNA_RESERVE_MODEL_SLUG.to_string(),
            Some("Luna Reserve".to_string()),
        );
        assert_eq!(
            fetched.existing_model_slug.as_deref(),
            Some(codexmanager_core::usage::LUNA_MODEL_SLUG)
        );
        assert!(fetched.already_linked);
    }

    #[test]
    fn validates_only_official_https_codex_backend_bases() {
        for value in [
            "https://chatgpt.com/backend-api/codex",
            "https://chatgpt.com/backend-api/codex/",
            "HTTPS://CHAT.OPENAI.COM/backend-api/codex",
            "https://chatgpt.com:443/backend-api/codex",
        ] {
            let url = validate_official_codex_backend(value)
                .unwrap_or_else(|err| panic!("official URL {value} was rejected: {err}"));
            assert_eq!(url.scheme(), "https");
        }
    }

    #[test]
    fn rejects_non_official_or_disguised_codex_backend_bases() {
        for value in [
            "http://chatgpt.com/backend-api/codex",
            "https://evil.example/backend-api/codex",
            "https://chatgpt.com.evil.example/backend-api/codex",
            "https://chatgpt.com@evil.example/backend-api/codex",
            "https://evil.example@chatgpt.com/backend-api/codex",
            "https://@chatgpt.com/backend-api/codex",
            "https://chatgpt.com:8443/backend-api/codex",
            "https://chatgpt.com/not-backend-api/codex?next=/backend-api/codex",
            "https://chatgpt.com/backend-api/codex.evil",
            "https://chatgpt.com/backend-api/codex?next=https://evil.example",
            "https://chatgpt.com/backend-api/codex#@evil.example",
            "https://chatgpt.com/backend-api/%63odex",
        ] {
            assert!(
                validate_official_codex_backend(value).is_err(),
                "disguised URL {value} must be rejected"
            );
        }
    }

    #[test]
    fn builds_the_models_endpoint_and_applies_a_bounded_request_timeout() {
        let base = validate_official_codex_backend("https://chatgpt.com/backend-api/codex")
            .expect("official base");
        let (models_url, _) =
            crate::gateway::gateway_compute_upstream_url(base.as_str(), "/v1/models");
        assert_eq!(models_url, "https://chatgpt.com/backend-api/codex/models");

        let request = with_account_models_timeout(reqwest::Client::new().get(models_url))
            .build()
            .expect("build request");
        assert_eq!(request.timeout(), Some(&ACCOUNT_MODELS_REQUEST_TIMEOUT));
    }

    #[test]
    fn associates_account_pool_routes_transactionally_and_idempotently() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_managed_models_v2(&[ManagedModelV2Upsert {
                model: custom_model("account-model-existing-test"),
                ..Default::default()
            }])
            .expect("seed existing model");

        let result = associate_account_models_with_storage(
            &storage,
            vec![
                "account-model-existing-test".to_string(),
                "account-model-new-test".to_string(),
                "ACCOUNT-MODEL-NEW-TEST".to_string(),
            ],
            BTreeMap::from([(
                "account-model-new-test".to_string(),
                "Account Model New".to_string(),
            )]),
        )
        .expect("associate models");
        assert_eq!(result.created_models, ["account-model-new-test"]);
        assert_eq!(
            result.added_routes,
            ["account-model-existing-test", "account-model-new-test"]
        );

        for slug in ["account-model-existing-test", "account-model-new-test"] {
            let model = storage
                .get_managed_model_v2(slug)
                .expect("read model")
                .expect("model exists");
            assert!(model.routes.iter().any(|route| {
                route.source_kind == ACCOUNT_POOL_SOURCE_KIND
                    && route.source_id == ACCOUNT_POOL_SOURCE_ID
                    && route.upstream_model.eq_ignore_ascii_case(slug)
            }));
        }
        assert_eq!(
            storage
                .get_managed_model_v2("account-model-new-test")
                .expect("read new model")
                .expect("new model exists")
                .display_name,
            "Account Model New"
        );

        let repeated = associate_account_models_with_storage(
            &storage,
            vec![
                "account-model-existing-test".to_string(),
                "account-model-new-test".to_string(),
            ],
            BTreeMap::new(),
        )
        .expect("repeat association");
        assert!(repeated.created_models.is_empty());
        assert!(repeated.added_routes.is_empty());
        assert_eq!(
            repeated.unchanged_routes,
            ["account-model-existing-test", "account-model-new-test"]
        );
    }

    #[test]
    fn associates_reserve_alias_with_luna_without_creating_an_unreachable_model() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let result = associate_account_models_with_storage(
            &storage,
            vec![
                codexmanager_core::usage::LUNA_RESERVE_MODEL_SLUG.to_string(),
                codexmanager_core::usage::LUNA_MODEL_SLUG.to_string(),
            ],
            BTreeMap::new(),
        )
        .expect("associate reserve alias");

        assert!(result.created_models.is_empty());
        assert!(result.added_routes.is_empty());
        assert_eq!(
            result.unchanged_routes,
            [codexmanager_core::usage::LUNA_MODEL_SLUG]
        );
        assert!(storage
            .get_managed_model_v2(codexmanager_core::usage::LUNA_RESERVE_MODEL_SLUG)
            .expect("read reserve alias")
            .is_none());
        let luna = storage
            .get_managed_model_v2(codexmanager_core::usage::LUNA_MODEL_SLUG)
            .expect("read Luna")
            .expect("Luna exists");
        assert!(luna.routes.iter().any(|route| {
            route.source_kind == ACCOUNT_POOL_SOURCE_KIND
                && route.source_id == ACCOUNT_POOL_SOURCE_ID
                && route
                    .upstream_model
                    .eq_ignore_ascii_case(codexmanager_core::usage::LUNA_MODEL_SLUG)
        }));
    }
}

#[cfg(test)]
#[path = "account_models_async_tests.rs"]
mod async_network_tests;
