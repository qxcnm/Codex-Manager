//! Async aggregate provider operations with bounded storage-only phases.
use super::network::*;
use super::*;
use std::future::Future;
use std::time::Instant;

static AGGREGATE_STORAGE_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

// Compatibility for synchronous desktop/domain and background scheduler callers.
// Native RPC handlers call the async operations directly.
pub(crate) fn run_aggregate_future<F, T>(future: F) -> Result<T, String>
where
    F: Future<Output = Result<T, String>> + Send,
    T: Send,
{
    crate::runtime::service_runtime::run_sync(future)?
}

pub(crate) async fn run_aggregate_storage<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let permit = AGGREGATE_STORAGE_WORKERS
        .acquire()
        .await
        .map_err(|_| "aggregate storage unavailable".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    })
    .await
    .map_err(|_| "aggregate storage operation interrupted".to_owned())?
}

async fn load_provider(
    api_id: &str,
) -> Result<codexmanager_core::storage::AggregateApiWithSecrets, String> {
    let api_id = api_id.to_owned();
    run_aggregate_storage(move || {
        let handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&handle);
        storage
            .find_aggregate_api_with_secrets_by_id(&api_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "aggregate api not found".to_owned())
    })
    .await
}

pub(crate) async fn fetch_aggregate_api_models_async(
    api_id: &str,
) -> Result<AggregateApiFetchModelsResult, String> {
    if api_id.trim().is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let api_with_secrets = load_provider(api_id).await?;
    let api = api_with_secrets.api;
    let secret = api_with_secrets
        .secret_value
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let provider_type = normalize_provider_type_value(api.provider_type.as_str());
    let client = gateway::async_upstream_client_for_aggregate_url(api.url.as_str());
    let url = models_endpoint(&api, provider_type.as_str());
    let builder = client.get(url.as_str());
    let (builder, updated_url) = apply_probe_auth(builder, url.clone(), &api, secret.as_str())?;
    let response = if updated_url == url {
        send_aggregate_api_request(&client, builder.header("accept", "application/json"), &api)
            .await
    } else {
        let rebuilt = client.get(updated_url.as_str());
        let (rebuilt, _) = apply_probe_auth(rebuilt, updated_url, &api, secret.as_str())?;
        send_aggregate_api_request(&client, rebuilt.header("accept", "application/json"), &api)
            .await
    }
    .map_err(|err| format!("models request failed: {err}"))?;
    let body = read_models_response(response).await?;
    let parsed = parse_fetched_models(&body, provider_type == AGGREGATE_API_PROVIDER_GEMINI);
    let existing = run_aggregate_storage(|| {
        let handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&handle);
        storage
            .list_managed_models_v2(true)
            .map_err(|err| format!("read model catalog V2 failed: {err}"))
    })
    .await?;
    let mut items = Vec::with_capacity(parsed.len());
    for (upstream_model, display_name) in parsed {
        let model = existing
            .iter()
            .find(|model| model.slug.eq_ignore_ascii_case(upstream_model.as_str()));
        let already_linked = model.is_some_and(|model| {
            model.routes.iter().any(|route| {
                route.source_kind == "aggregate_api"
                    && route.source_id == api_id
                    && route
                        .upstream_model
                        .eq_ignore_ascii_case(upstream_model.as_str())
            })
        });
        items.push(AggregateApiFetchedModel {
            upstream_model,
            display_name,
            existing_model_slug: model.map(|model| model.slug.clone()),
            already_linked,
        });
    }
    Ok(AggregateApiFetchModelsResult {
        api_id: api_id.to_string(),
        provider_type,
        fetched_at: now_ts(),
        items,
    })
}

pub(crate) async fn test_aggregate_api_connection_async(
    api_id: &str,
) -> Result<AggregateApiTestResult, String> {
    if api_id.is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let api_with_secrets = load_provider(api_id).await?;
    let api = api_with_secrets.api;
    let secret = api_with_secrets
        .secret_value
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let probe_api_id = api_id.to_owned();
    let probe_model = run_aggregate_storage(move || {
        let handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&handle);
        configured_aggregate_probe_model(&storage, &probe_api_id)
    })
    .await?;
    let client = gateway::async_upstream_client_for_aggregate_url(api.url.as_str());
    let started_at = Instant::now();
    let provider_type = normalize_provider_type_value(api.provider_type.as_str());
    let result = match provider_type.as_str() {
        AGGREGATE_API_PROVIDER_CLAUDE => {
            probe_claude_endpoint(&client, &api, &secret, probe_model.as_str()).await
        }
        AGGREGATE_API_PROVIDER_GEMINI => {
            probe_gemini_endpoint(&client, &api, &secret, probe_model.as_str()).await
        }
        _ => probe_codex_endpoint(&client, &api, &secret, probe_model.as_str()).await,
    };
    let (ok, status_code, last_error) = match result {
        Ok(code) => (true, Some(code), None),
        Err(err) => (false, None, Some(err)),
    };
    let message =
        last_error.map(|err| format!("provider={provider_type}; model={probe_model}; {err}"));

    let saved_id = api_id.to_owned();
    let saved_message = message.clone();
    let _ = run_aggregate_storage(move || {
        let handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&handle);
        storage
            .update_aggregate_api_test_result(&saved_id, ok, status_code, saved_message.as_deref())
            .map_err(|err| err.to_string())
    })
    .await;
    Ok(AggregateApiTestResult {
        id: api_id.to_string(),
        ok,
        status_code,
        message,
        tested_at: now_ts(),
        latency_ms: started_at.elapsed().as_millis() as i64,
    })
}

pub(crate) async fn refresh_aggregate_api_balance_async(
    api_id: &str,
) -> Result<AggregateApiBalanceRefreshResult, String> {
    if api_id.is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let api_with_secrets = load_provider(api_id).await?;
    let api = api_with_secrets.api;
    if !api.balance_query_enabled {
        return Err("aggregate api balance query is disabled".to_string());
    }
    let provider_secret = api_with_secrets
        .secret_value
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let balance_secret = api_with_secrets.balance_access_token;
    let template = default_balance_query_template(normalize_balance_query_template(
        api.balance_query_template.clone(),
    )?);
    let client = gateway::async_upstream_client_for_aggregate_url(api.url.as_str());
    let started_at = Instant::now();
    let result = match template.as_str() {
        AGGREGATE_API_BALANCE_TEMPLATE_NEW_API => {
            query_new_api_balance(&client, &api, &provider_secret, balance_secret).await
        }
        AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM => {
            query_custom_balance(&client, &api, &provider_secret, balance_secret).await
        }
        _ => query_generic_balance(&client, &api, &provider_secret).await,
    };
    let queried_at = now_ts();
    let latency_ms = started_at.elapsed().as_millis() as i64;

    match result {
        Ok(snapshot) => {
            let ok = snapshot.is_valid;
            let message = if ok {
                None
            } else {
                snapshot
                    .invalid_message
                    .clone()
                    .or_else(|| Some("balance query returned invalid account".to_string()))
            };
            let balance_json = serde_json::to_string(&snapshot)
                .map_err(|_| "serialize balance result failed".to_string())?;
            persist_balance_result(api_id, ok, Some(balance_json), message.clone()).await;
            Ok(AggregateApiBalanceRefreshResult {
                id: api_id.to_string(),
                ok,
                balance: Some(snapshot),
                message,
                queried_at,
                latency_ms,
            })
        }
        Err(err) => {
            let message = format!("template={template}; {err}");
            persist_balance_result(api_id, false, None, Some(message.clone())).await;
            Ok(AggregateApiBalanceRefreshResult {
                id: api_id.to_string(),
                ok: false,
                balance: None,
                message: Some(message),
                queried_at,
                latency_ms,
            })
        }
    }
}

async fn persist_balance_result(
    api_id: &str,
    ok: bool,
    balance: Option<String>,
    message: Option<String>,
) {
    let api_id = api_id.to_owned();
    let _ = run_aggregate_storage(move || {
        let handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&handle);
        storage
            .update_aggregate_api_balance_result(
                &api_id,
                ok,
                balance.as_deref(),
                message.as_deref(),
            )
            .map_err(|err| err.to_string())
    })
    .await;
}
