use bytes::Bytes;
use codexmanager_core::storage::Storage;
use codexmanager_kiro_provider::{
    anthropic::{
        converter::convert_request,
        stream::StreamContext,
        types::MessagesRequest,
        websearch::{
            create_mcp_request, create_websearch_sse_stream, extract_search_query,
            has_web_search_tool, parse_search_results, McpResponse,
        },
    },
    kiro::{
        endpoint::{IdeEndpoint, KiroEndpoint},
        model::{
            credentials::KiroCredentials, events::Event, requests::kiro::KiroRequest,
            usage_limits::UsageLimitsResponse,
        },
        parser::decoder::EventStreamDecoder,
        provider::{KiroProvider, KiroRequestOutcomeKind},
        token_manager::MultiTokenManager,
    },
    model::config::Config,
};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::gateway::{
    canonical::{CanonicalRequest, ProviderAdapter},
    protocol_adapter::adapt_openai_responses_to_anthropic_messages,
    upstream::{
        GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse, GatewayUpstreamResponse,
    },
};

const KIRO_STREAM_CHANNEL_CAPACITY: usize = 128;
const KIRO_CONNECT_TIMEOUT: Duration = Duration::from_secs(120);
const KIRO_MODEL_PROBE_TIMEOUT: Duration = Duration::from_secs(12);
const KIRO_MODEL_PROBE_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KiroQuotaSummary {
    pub credential_id: String,
    pub subscription: Option<String>,
    pub credit_limit: f64,
    pub credit_used: f64,
    pub remaining: f64,
    pub next_reset_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KiroModelProbeSummary {
    pub credential_id: String,
    pub available_models: Vec<String>,
    pub checked: usize,
    pub unknown: usize,
    pub checked_at: i64,
}

pub(crate) fn is_kiro_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("kiro/"))
}

pub(crate) fn is_supported_kiro_model(model: &str) -> bool {
    let upstream = model.trim().strip_prefix("kiro/").unwrap_or(model.trim());
    codexmanager_kiro_provider::anthropic::converter::map_model(upstream).is_some()
}

struct KiroCanonicalAdapter<'a> {
    upstream_model: &'a str,
}

impl ProviderAdapter for KiroCanonicalAdapter<'_> {
    type ProviderRequest = MessagesRequest;

    fn adapt_request(&self, request: &CanonicalRequest) -> Result<Self::ProviderRequest, String> {
        let responses_body = request.to_responses_bytes()?;
        let anthropic_body = adapt_openai_responses_to_anthropic_messages(
            responses_body.as_slice(),
            Some(self.upstream_model),
        )?;
        serde_json::from_slice(&anthropic_body)
            .map_err(|error| format!("invalid converted Kiro request: {error}"))
    }
}

pub(crate) fn execute_responses_request(
    storage: &Storage,
    body: &Bytes,
    platform_model: &str,
) -> Result<GatewayUpstreamResponse, String> {
    let upstream_model = platform_model
        .trim()
        .strip_prefix("kiro/")
        .or_else(|| platform_model.trim().strip_prefix("KIRO/"))
        .unwrap_or(platform_model.trim());
    if upstream_model.is_empty() {
        return Err("missing Kiro upstream model".into());
    }

    let canonical = CanonicalRequest::from_responses_bytes(body.as_ref())?;
    let mut messages = KiroCanonicalAdapter { upstream_model }.adapt_request(&canonical)?;
    messages.model = upstream_model.to_string();
    messages.stream = true;
    let thinking_enabled = messages
        .thinking
        .as_ref()
        .is_some_and(|thinking| thinking.is_enabled());
    let web_search_request = if has_web_search_tool(&messages) {
        let query = extract_search_query(&messages)
            .ok_or_else(|| "Kiro web search requires a non-empty text query".to_string())?;
        let (tool_use_id, request) = create_mcp_request(&query);
        let request_body = serde_json::to_string(&request)
            .map_err(|error| format!("serialize Kiro web search request failed: {error}"))?;
        let input_tokens = ((canonical.to_responses_bytes()?.len() + 3) / 4) as i32;
        Some((query, tool_use_id, request_body, input_tokens))
    } else {
        None
    };
    let (request_body, tool_name_map) = if web_search_request.is_some() {
        (String::new(), HashMap::new())
    } else {
        let converted = convert_request(&messages).map_err(|error| error.to_string())?;
        let request_body = serde_json::to_string(&KiroRequest {
            conversation_state: converted.conversation_state,
            profile_arn: None,
        })
        .map_err(|error| format!("serialize Kiro request failed: {error}"))?;
        (request_body, converted.tool_name_map)
    };

    let (credentials, credential_ids) = load_runtime_credentials(storage, platform_model)?;
    if credentials.is_empty() {
        return Err("no active Kiro credentials".into());
    }

    let (body_tx, body_rx) =
        mpsc::sync_channel::<GatewayByteStreamItem>(KIRO_STREAM_CHANNEL_CAPACITY);
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(HeaderMap, Option<String>), String>>(1);
    let model = upstream_model.to_string();
    let storage_path = storage.path().map(std::path::Path::to_path_buf);

    thread::Builder::new()
        .name("codexmanager-kiro-stream".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("start Kiro runtime failed: {error}")));
                    return;
                }
            };
            runtime.block_on(async move {
                let config = Config::default();
                let mut manager =
                    match MultiTokenManager::new(config, credentials, None, None, false) {
                        Ok(manager) => manager,
                        Err(error) => {
                            let _ = ready_tx
                                .send(Err(format!("initialize Kiro credentials failed: {error}")));
                            return;
                        }
                    };
                if let Some(storage_path) = storage_path.as_ref() {
                    let token_storage_path = storage_path.clone();
                    let token_credential_ids = credential_ids.clone();
                    manager = manager.with_credential_update_handler(Arc::new(
                        move |runtime_id, refreshed| {
                            let Some(database_id) = token_credential_ids.get(&runtime_id) else {
                                return;
                            };
                            let Ok(storage) = Storage::open(&token_storage_path) else {
                                return;
                            };
                            let Some(refresh_token) = refreshed.refresh_token.clone() else {
                                return;
                            };
                            let expires_at = refreshed
                                .expires_at
                                .as_deref()
                                .and_then(parse_rfc3339_timestamp);
                            if let Err(error) = storage.update_kiro_credential_tokens(
                                database_id,
                                codexmanager_core::storage::KiroCredentialSecret {
                                    refresh_token,
                                    access_token: refreshed.access_token.clone(),
                                    client_id: refreshed.client_id.clone(),
                                    client_secret: refreshed.client_secret.clone(),
                                    proxy_password: refreshed.proxy_password.clone(),
                                },
                                expires_at,
                            ) {
                                log::warn!("persist refreshed Kiro token failed: {error}");
                            }
                        },
                    ));
                }
                let manager = Arc::new(manager);
                let selected_database_id = Arc::new(Mutex::new(None::<String>));
                let mut endpoints: HashMap<String, Arc<dyn KiroEndpoint>> = HashMap::new();
                endpoints.insert("ide".into(), Arc::new(IdeEndpoint::new()));
                let mut provider =
                    match KiroProvider::with_proxy(manager, None, endpoints, "ide".into()) {
                        Ok(provider) => provider,
                        Err(error) => {
                            let _ = ready_tx.send(Err(format!(
                                "initialize Kiro HTTP provider failed: {error}"
                            )));
                            return;
                        }
                    };
                if let Some(outcome_storage_path) = storage_path {
                    let selected_database_id_for_outcome = selected_database_id.clone();
                    provider = provider.with_outcome_handler(Arc::new(move |outcome| {
                        let Some(database_id) = credential_ids.get(&outcome.credential_id) else {
                            return;
                        };
                        let Ok(storage) = Storage::open(&outcome_storage_path) else {
                            return;
                        };
                        let result = match outcome.kind {
                            KiroRequestOutcomeKind::Success => {
                                if let Ok(mut selected) = selected_database_id_for_outcome.lock() {
                                    *selected = Some(database_id.clone());
                                }
                                storage
                                    .record_kiro_credential_success(database_id, outcome.latency_ms)
                            }
                            KiroRequestOutcomeKind::Unauthorized => storage
                                .record_kiro_credential_failure(
                                    database_id,
                                    &outcome.status_code.unwrap_or(401).to_string(),
                                    None,
                                    Some("isolated"),
                                    outcome.latency_ms,
                                ),
                            KiroRequestOutcomeKind::RateLimited => storage
                                .record_kiro_credential_failure(
                                    database_id,
                                    "429",
                                    Some(60),
                                    None,
                                    outcome.latency_ms,
                                ),
                            KiroRequestOutcomeKind::QuotaExhausted => storage
                                .record_kiro_credential_failure(
                                    database_id,
                                    "quota_exhausted",
                                    None,
                                    Some("quota_exhausted"),
                                    outcome.latency_ms,
                                ),
                            KiroRequestOutcomeKind::TransientFailure => storage
                                .record_kiro_credential_failure(
                                    database_id,
                                    &outcome.status_code.unwrap_or(503).to_string(),
                                    Some(15),
                                    None,
                                    outcome.latency_ms,
                                ),
                            KiroRequestOutcomeKind::NetworkFailure => storage
                                .record_kiro_credential_failure(
                                    database_id,
                                    "network_error",
                                    Some(15),
                                    None,
                                    outcome.latency_ms,
                                ),
                        };
                        if let Err(error) = result {
                            log::warn!("persist Kiro credential health failed: {error}");
                        }
                    }));
                }
                if let Some((query, tool_use_id, mcp_body, input_tokens)) = web_search_request {
                    let response = match tokio::time::timeout(
                        KIRO_CONNECT_TIMEOUT,
                        provider.call_mcp(&mcp_body),
                    )
                    .await
                    {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => {
                            let _ = ready_tx
                                .send(Err(format!("Kiro web search request failed: {error}")));
                            return;
                        }
                        Err(_) => {
                            let _ = ready_tx.send(Err("Kiro web search request timed out".into()));
                            return;
                        }
                    };
                    let status = response.status();
                    let response_body = response.text().await.unwrap_or_default();
                    if !status.is_success() {
                        let safe_body = response_body.chars().take(512).collect::<String>();
                        let _ = ready_tx.send(Err(format!(
                            "Kiro web search returned {status}: {safe_body}"
                        )));
                        return;
                    }
                    let mcp_response = match serde_json::from_str::<McpResponse>(&response_body) {
                        Ok(response) => response,
                        Err(error) => {
                            let _ = ready_tx
                                .send(Err(format!("invalid Kiro web search response: {error}")));
                            return;
                        }
                    };
                    let results = parse_search_results(&mcp_response);
                    let mut headers = HeaderMap::new();
                    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
                    let selected = selected_database_id
                        .lock()
                        .ok()
                        .and_then(|value| value.clone());
                    if ready_tx.send(Ok((headers, selected))).is_err() {
                        return;
                    }
                    let stream = create_websearch_sse_stream(
                        model,
                        query,
                        tool_use_id,
                        results,
                        input_tokens,
                    );
                    futures_util::pin_mut!(stream);
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(bytes) => {
                                if body_tx.send(GatewayByteStreamItem::Chunk(bytes)).is_err() {
                                    return;
                                }
                            }
                            Err(never) => match never {},
                        }
                    }
                    let _ = body_tx.send(GatewayByteStreamItem::Eof);
                    return;
                }
                let response = match tokio::time::timeout(
                    KIRO_CONNECT_TIMEOUT,
                    provider.call_api_stream(&request_body),
                )
                .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => {
                        let _ =
                            ready_tx.send(Err(format!("Kiro upstream request failed: {error}")));
                        return;
                    }
                    Err(_) => {
                        let _ = ready_tx.send(Err("Kiro upstream request timed out".into()));
                        return;
                    }
                };
                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    let safe_body = body.chars().take(512).collect::<String>();
                    let _ =
                        ready_tx.send(Err(format!("Kiro upstream returned {status}: {safe_body}")));
                    return;
                }

                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
                let selected = selected_database_id
                    .lock()
                    .ok()
                    .and_then(|value| value.clone());
                if ready_tx.send(Ok((headers, selected))).is_err() {
                    return;
                }

                let mut stream_context =
                    StreamContext::new_with_thinking(model, 0, thinking_enabled, tool_name_map);
                for event in stream_context.generate_initial_events() {
                    if !send_sse(&body_tx, event.to_sse_string()) {
                        return;
                    }
                }

                let mut decoder = EventStreamDecoder::new();
                let mut bytes_stream = response.bytes_stream();
                while let Some(chunk) = bytes_stream.next().await {
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            let _ = body_tx.send(GatewayByteStreamItem::Error(format!(
                                "Kiro stream read failed: {error}"
                            )));
                            return;
                        }
                    };
                    if let Err(error) = decoder.feed(&chunk) {
                        let _ = body_tx.send(GatewayByteStreamItem::Error(format!(
                            "Kiro EventStream buffer failed: {error}"
                        )));
                        return;
                    }
                    while let Some(frame) = match decoder.decode() {
                        Ok(frame) => frame,
                        Err(error) => {
                            let _ = body_tx.send(GatewayByteStreamItem::Error(format!(
                                "Kiro EventStream decode failed: {error}"
                            )));
                            return;
                        }
                    } {
                        let event = match Event::from_frame(frame) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = body_tx.send(GatewayByteStreamItem::Error(format!(
                                    "Kiro event parse failed: {error}"
                                )));
                                return;
                            }
                        };
                        if let Event::Error {
                            error_code,
                            error_message,
                        } = &event
                        {
                            let _ = body_tx.send(GatewayByteStreamItem::Error(format!(
                                "Kiro event error {error_code}: {error_message}"
                            )));
                            return;
                        }
                        for event in stream_context.process_kiro_event(&event) {
                            if !send_sse(&body_tx, event.to_sse_string()) {
                                return;
                            }
                        }
                    }
                }
                for event in stream_context.generate_final_events() {
                    if !send_sse(&body_tx, event.to_sse_string()) {
                        return;
                    }
                }
                let _ = body_tx.send(GatewayByteStreamItem::Eof);
            });
        })
        .map_err(|error| format!("spawn Kiro stream worker failed: {error}"))?;

    let (headers, actual_source_id) = ready_rx
        .recv_timeout(KIRO_CONNECT_TIMEOUT + Duration::from_secs(5))
        .map_err(|_| "Kiro stream worker did not become ready".to_string())??;
    let responses_stream = crate::gateway::adapt_anthropic_gateway_stream_to_responses(
        GatewayByteStream::from_receiver(body_rx),
        upstream_model,
        std::time::Instant::now(),
    );
    Ok(GatewayUpstreamResponse::Stream(
        GatewayStreamResponse::new(reqwest::StatusCode::OK, headers, responses_stream)
            .with_actual_source_id(actual_source_id),
    ))
}

fn send_sse(tx: &mpsc::SyncSender<GatewayByteStreamItem>, value: String) -> bool {
    tx.send(GatewayByteStreamItem::Chunk(Bytes::from(value)))
        .is_ok()
}

fn load_runtime_credentials(
    storage: &Storage,
    platform_model: &str,
) -> Result<(Vec<KiroCredentials>, HashMap<u64, String>), String> {
    let now = codexmanager_core::storage::now_ts();
    let records = storage
        .list_kiro_credentials()
        .map_err(|error| error.to_string())?;
    let mut credentials = Vec::new();
    let mut credential_ids = HashMap::new();
    let proven_ids = storage
        .list_kiro_credential_model_availability(None)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|item| {
            item.model_slug.eq_ignore_ascii_case(platform_model) && item.status == "available"
        })
        .map(|item| item.credential_id)
        .collect::<std::collections::HashSet<_>>();
    for (index, mut record) in records.into_iter().enumerate() {
        if record.status != "active" || record.cooldown_until.is_some_and(|until| until > now) {
            continue;
        }
        if !proven_ids.contains(&record.id) {
            continue;
        }
        let Some(secret) = storage
            .read_kiro_credential_secret(record.id.as_str())
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        record.metadata_json = storage
            .ensure_kiro_credential_fingerprint(&record.id)
            .map_err(|error| error.to_string())?;
        let runtime_id = (index + 1) as u64;
        credential_ids.insert(runtime_id, record.id.clone());
        credentials.push(runtime_credential(record, secret, runtime_id));
    }
    Ok((credentials, credential_ids))
}

fn runtime_credential(
    record: codexmanager_core::storage::KiroCredentialRecord,
    secret: codexmanager_core::storage::KiroCredentialSecret,
    runtime_id: u64,
) -> KiroCredentials {
    let metadata = serde_json::from_str::<serde_json::Value>(&record.metadata_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    let metadata_text = |camel: &str, snake: &str| {
        metadata
            .get(camel)
            .or_else(|| metadata.get(snake))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let machine_id = metadata_text("machineId", "machine_id");
    KiroCredentials {
        id: Some(runtime_id),
        access_token: secret.access_token,
        refresh_token: Some(secret.refresh_token),
        expires_at: record.expires_at.map(unix_timestamp_to_rfc3339),
        auth_method: Some(record.auth_method),
        client_id: secret.client_id,
        client_secret: secret.client_secret,
        priority: record.priority.max(0) as u32,
        region: record.auth_region.clone().or(record.api_region.clone()),
        auth_region: record.auth_region,
        api_region: record.api_region,
        machine_id,
        system_version: metadata_text("systemVersion", "system_version"),
        node_version: metadata_text("nodeVersion", "node_version"),
        kiro_version: metadata_text("kiroVersion", "kiro_version"),
        email: record.email,
        subscription_title: record.subscription,
        proxy_url: record.proxy_url,
        proxy_username: record.proxy_username,
        proxy_password: secret.proxy_password,
        disabled: false,
        endpoint: Some("ide".into()),
        ..Default::default()
    }
}

fn load_management_credential(storage: &Storage, id: &str) -> Result<KiroCredentials, String> {
    let mut record = storage
        .list_kiro_credentials()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| "kiro credential not found".to_string())?;
    record.metadata_json = storage
        .ensure_kiro_credential_fingerprint(id)
        .map_err(|error| error.to_string())?;
    let secret = storage
        .read_kiro_credential_secret(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "kiro credential secret not found".to_string())?;
    Ok(runtime_credential(record, secret, 1))
}

fn persist_management_refresh(
    storage: &Storage,
    id: &str,
    refreshed: KiroCredentials,
) -> Result<(), String> {
    let refresh_token = refreshed
        .refresh_token
        .ok_or_else(|| "refreshed Kiro credential has no refresh token".to_string())?;
    let expires_at = refreshed
        .expires_at
        .as_deref()
        .and_then(parse_rfc3339_timestamp);
    storage
        .update_kiro_credential_tokens(
            id,
            codexmanager_core::storage::KiroCredentialSecret {
                refresh_token,
                access_token: refreshed.access_token,
                client_id: refreshed.client_id,
                client_secret: refreshed.client_secret,
                proxy_password: refreshed.proxy_password,
            },
            expires_at,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn management_token_manager(
    credential: KiroCredentials,
) -> Result<(MultiTokenManager, mpsc::Receiver<KiroCredentials>), String> {
    let (tx, rx) = mpsc::sync_channel(2);
    let manager = MultiTokenManager::new(Config::default(), vec![credential], None, None, false)
        .map_err(|error| error.to_string())?
        .with_credential_update_handler(Arc::new(move |_id, refreshed| {
            let _ = tx.send(refreshed.clone());
        }));
    Ok((manager, rx))
}

pub(crate) fn refresh_credential(storage: &Storage, id: &str) -> Result<bool, String> {
    let credential = load_management_credential(storage, id)?;
    let (manager, refreshed_rx) = management_token_manager(credential)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("start Kiro refresh runtime failed: {error}"))?;
    runtime
        .block_on(manager.force_refresh_token_for(1))
        .map_err(|error| format!("Kiro credential refresh failed: {error}"))?;
    let refreshed = refreshed_rx
        .try_recv()
        .map_err(|_| "Kiro refresh completed without updated credentials".to_string())?;
    persist_management_refresh(storage, id, refreshed)?;
    storage
        .set_kiro_credential_enabled(id, true)
        .map_err(|error| error.to_string())
}

pub(crate) fn query_credential_quota(
    storage: &Storage,
    id: &str,
) -> Result<KiroQuotaSummary, String> {
    let credential = load_management_credential(storage, id)?;
    let (manager, refreshed_rx) = management_token_manager(credential)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("start Kiro quota runtime failed: {error}"))?;
    let usage = runtime
        .block_on(manager.get_usage_limits_for(1))
        .map_err(|error| format!("Kiro quota query failed: {error}"))?;
    if let Ok(refreshed) = refreshed_rx.try_recv() {
        persist_management_refresh(storage, id, refreshed)?;
    }
    let credit_limit = usage.usage_limit();
    let credit_used = usage.current_usage();
    let subscription = usage.subscription_title().map(str::to_string);
    storage
        .update_kiro_credential_quota(id, subscription.clone(), credit_limit, credit_used)
        .map_err(|error| error.to_string())?;
    Ok(summarize_quota(id, &usage))
}

/// Probe the candidate catalog with exactly one credential. A model is made
/// public only after this function receives a successful upstream response.
/// Transient failures never become false "available" entries.
pub(crate) fn probe_credential_models(
    storage: &Storage,
    id: &str,
) -> Result<KiroModelProbeSummary, String> {
    // Clear stale successes before touching the network. A failed probe must
    // never leave the dashboard claiming that an old model result is current.
    for (public_slug, _, _) in super::catalog::MODELS {
        storage
            .upsert_kiro_credential_model_availability(
                id,
                public_slug,
                "unknown",
                Some("probe_running"),
                None,
            )
            .map_err(|error| error.to_string())?;
    }

    let credential = load_management_credential(storage, id)?;
    let (manager, refreshed_rx) = management_token_manager(credential)?;
    let manager = Arc::new(manager);
    let mut endpoints: HashMap<String, Arc<dyn KiroEndpoint>> = HashMap::new();
    endpoints.insert("ide".into(), Arc::new(IdeEndpoint::new()));
    let provider = Arc::new(
        KiroProvider::with_proxy(manager, None, endpoints, "ide".into())
            .map_err(|error| format!("initialize Kiro model probe failed: {error}"))?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("start Kiro model probe runtime failed: {error}"))?;

    // Sonnet 4.5 is the broadest compatibility check in the current catalog.
    // If it cannot even reach Kiro because of auth/network failure, stop after
    // this one bounded request instead of waiting for every model to time out.
    let baseline = super::catalog::MODELS
        .iter()
        .find(|(slug, _, _)| *slug == "kiro/claude-sonnet-4.5")
        .copied()
        .or_else(|| super::catalog::MODELS.first().copied());
    let mut outcomes = Vec::with_capacity(super::catalog::MODELS.len());
    let mut skip_remaining = false;
    if let Some((public_slug, _, _)) = baseline {
        let started = Instant::now();
        let outcome = runtime.block_on(probe_one_model(
            provider.clone(),
            public_slug.trim_start_matches("kiro/"),
        ));
        let transient_failure = outcome
            .as_ref()
            .err()
            .is_some_and(|error| !is_definitive_model_error(error));
        outcomes.push((public_slug, started.elapsed(), outcome));
        skip_remaining = transient_failure;
    }

    if !skip_remaining {
        let remaining = super::catalog::MODELS
            .iter()
            .filter(|(slug, _, _)| Some(*slug) != baseline.map(|item| item.0));
        let mut parallel = runtime.block_on(async {
            futures_util::stream::iter(remaining.map(|(public_slug, _, _)| {
                let provider = provider.clone();
                async move {
                    let started = Instant::now();
                    let outcome =
                        probe_one_model(provider, public_slug.trim_start_matches("kiro/")).await;
                    (*public_slug, started.elapsed(), outcome)
                }
            }))
            .buffer_unordered(KIRO_MODEL_PROBE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
        });
        outcomes.append(&mut parallel);
    }

    let mut available_models = Vec::new();
    let mut unknown = 0usize;
    for (public_slug, elapsed, outcome) in outcomes {
        let latency = elapsed.as_millis().min(u64::MAX as u128) as u64;
        let (status, error_code) = match outcome {
            Ok(()) => {
                available_models.push(public_slug.to_string());
                ("available", None)
            }
            Err(error) if is_definitive_model_error(&error) => {
                ("unavailable", Some("unsupported_model"))
            }
            Err(_) => {
                unknown += 1;
                ("unknown", Some("probe_failed"))
            }
        };
        storage
            .upsert_kiro_credential_model_availability(
                id,
                public_slug,
                status,
                error_code,
                Some(latency),
            )
            .map_err(|error| error.to_string())?;
    }

    if skip_remaining {
        unknown = super::catalog::MODELS.len();
        for (public_slug, _, _) in super::catalog::MODELS {
            if Some(*public_slug) == baseline.map(|item| item.0) {
                continue;
            }
            storage
                .upsert_kiro_credential_model_availability(
                    id,
                    public_slug,
                    "unknown",
                    Some("probe_skipped"),
                    None,
                )
                .map_err(|error| error.to_string())?;
        }
    }

    // A shared token manager prevents parallel model checks from rotating the
    // same refresh token independently. Persist the newest rotated credential.
    let mut latest_refresh = None;
    while let Ok(refreshed) = refreshed_rx.try_recv() {
        latest_refresh = Some(refreshed);
    }
    if let Some(refreshed) = latest_refresh {
        persist_management_refresh(storage, id, refreshed)?;
    }

    Ok(KiroModelProbeSummary {
        credential_id: id.to_string(),
        available_models,
        checked: super::catalog::MODELS.len(),
        unknown,
        checked_at: codexmanager_core::storage::now_ts(),
    })
}

async fn probe_one_model(provider: Arc<KiroProvider>, upstream_model: &str) -> Result<(), String> {
    let messages: MessagesRequest = serde_json::from_value(serde_json::json!({
        "model": upstream_model,
        "max_tokens": 1,
        "stream": false,
        "messages": [{"role": "user", "content": "Reply OK"}]
    }))
    .map_err(|error| format!("build Kiro model probe failed: {error}"))?;
    let converted = convert_request(&messages).map_err(|error| error.to_string())?;
    let request_body = serde_json::to_string(&KiroRequest {
        conversation_state: converted.conversation_state,
        profile_arn: None,
    })
    .map_err(|error| format!("serialize Kiro model probe failed: {error}"))?;
    tokio::time::timeout(KIRO_MODEL_PROBE_TIMEOUT, provider.call_api(&request_body))
        .await
        .map_err(|_| "Kiro model probe timed out".to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn is_definitive_model_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("model")
        && [
            "unsupported",
            "not supported",
            "not available",
            "invalid model",
            "unknown model",
            "model access",
        ]
        .iter()
        .any(|marker| error.contains(marker))
}

fn summarize_quota(id: &str, usage: &UsageLimitsResponse) -> KiroQuotaSummary {
    let credit_limit = usage.usage_limit();
    let credit_used = usage.current_usage();
    KiroQuotaSummary {
        credential_id: id.to_string(),
        subscription: usage.subscription_title().map(str::to_string),
        credit_limit,
        credit_used,
        remaining: (credit_limit - credit_used).max(0.0),
        next_reset_at: usage.next_reset_at().map(|value| value as i64),
    }
}

fn unix_timestamp_to_rfc3339(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

fn parse_rfc3339_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{KiroCredentialSecret, KiroCredentialUpsert};
    use std::sync::mpsc;

    #[test]
    fn recognizes_only_explicit_kiro_models() {
        assert!(is_kiro_model(Some("kiro/claude-sonnet-4.6")));
        assert!(!is_kiro_model(Some("codex/gpt-5.4")));
        assert!(!is_kiro_model(Some("smart")));
        assert!(!is_kiro_model(None));
    }

    #[test]
    fn quota_summary_uses_precise_totals_and_breakdown_reset_fallback() {
        let usage: UsageLimitsResponse = serde_json::from_value(serde_json::json!({
            "subscriptionInfo": { "subscriptionTitle": "KIRO PRO" },
            "usageBreakdownList": [{
                "usageLimitWithPrecision": 100.5,
                "currentUsageWithPrecision": 20.25,
                "nextDateReset": 2_000_000_000.0,
                "bonuses": [{
                    "usageLimit": 10.0,
                    "currentUsage": 1.5,
                    "status": "ACTIVE"
                }]
            }]
        }))
        .unwrap();

        let summary = summarize_quota("credential-1", &usage);
        assert_eq!(summary.subscription.as_deref(), Some("KIRO PRO"));
        assert_eq!(summary.credit_limit, 110.5);
        assert_eq!(summary.credit_used, 21.75);
        assert_eq!(summary.remaining, 88.75);
        assert_eq!(summary.next_reset_at, Some(2_000_000_000));
    }

    #[test]
    fn runtime_credential_restores_machine_id_from_encrypted_record_metadata() {
        let record = codexmanager_core::storage::KiroCredentialRecord {
            id: "credential-machine".into(),
            auth_method: "social".into(),
            email: None,
            auth_region: Some("us-east-1".into()),
            api_region: Some("eu-central-1".into()),
            subscription: None,
            status: "active".into(),
            priority: 0,
            weight: 1.0,
            proxy_url: None,
            proxy_username: None,
            metadata_json: serde_json::json!({ "machineId": "machine-test-id" }).to_string(),
            credit_limit: None,
            credit_used: None,
            expires_at: None,
            cooldown_until: None,
            failure_count: 0,
            request_count: 0,
            success_count: 0,
            last_latency_ms: None,
            created_at: 0,
            updated_at: 0,
        };
        let secret = KiroCredentialSecret {
            refresh_token: "refresh".into(),
            access_token: None,
            client_id: None,
            client_secret: None,
            proxy_password: None,
        };

        let credential = runtime_credential(record, secret, 1);
        assert_eq!(credential.machine_id.as_deref(), Some("machine-test-id"));
        assert_eq!(credential.auth_region.as_deref(), Some("us-east-1"));
        assert_eq!(credential.api_region.as_deref(), Some("eu-central-1"));
    }

    #[cfg(windows)]
    #[test]
    fn refreshed_management_tokens_are_persisted_in_the_encrypted_vault() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .upsert_kiro_credential(&KiroCredentialUpsert {
                id: "credential-refresh".into(),
                auth_method: "idc".into(),
                identity_hint: "refresh@example.test".into(),
                email: Some("refresh@example.test".into()),
                auth_region: Some("us-east-1".into()),
                api_region: Some("us-east-1".into()),
                subscription: None,
                status: "active".into(),
                priority: 0,
                weight: 1.0,
                proxy_url: None,
                proxy_username: None,
                metadata_json: "{}".into(),
                credit_limit: None,
                credit_used: None,
                expires_at: None,
                secret: KiroCredentialSecret {
                    refresh_token: "old-refresh".into(),
                    access_token: Some("old-access".into()),
                    client_id: Some("client-id".into()),
                    client_secret: Some("client-secret".into()),
                    proxy_password: None,
                },
            })
            .unwrap();

        persist_management_refresh(
            &storage,
            "credential-refresh",
            KiroCredentials {
                refresh_token: Some("new-refresh".into()),
                access_token: Some("new-access".into()),
                client_id: Some("client-id".into()),
                client_secret: Some("client-secret".into()),
                expires_at: Some("2033-05-18T03:33:20Z".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let secret = storage
            .read_kiro_credential_secret("credential-refresh")
            .unwrap()
            .unwrap();
        assert_eq!(secret.refresh_token, "new-refresh");
        assert_eq!(secret.access_token.as_deref(), Some("new-access"));
        let record = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(record.expires_at, Some(2_000_000_000));
    }

    #[test]
    fn anthropic_sse_is_canonicalized_to_responses_sse() {
        let (tx, rx) = mpsc::sync_channel(8);
        for frame in [
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"model\":\"claude-sonnet-4.6\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ] {
            tx.send(GatewayByteStreamItem::Chunk(Bytes::from(frame)))
                .unwrap();
        }
        tx.send(GatewayByteStreamItem::Eof).unwrap();
        let responses = crate::gateway::adapt_anthropic_gateway_stream_to_responses(
            GatewayByteStream::from_receiver(rx),
            "claude-sonnet-4.6",
            std::time::Instant::now(),
        )
        .read_all_bytes()
        .unwrap();
        let text = String::from_utf8(responses.to_vec()).unwrap();
        assert!(text.contains("response.created"));
        assert!(text.contains("response.output_text.delta"));
        assert!(text.contains("hello"));
        assert!(text.contains("response.completed"));
    }

    #[test]
    fn responses_request_reaches_kiro_with_instructions_tools_image_and_thinking() {
        let body = serde_json::json!({
            "model": "kiro/claude-sonnet-4.6",
            "instructions": "be concise",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "inspect this" },
                    { "type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=" }
                ]
            }],
            "tools": [{
                "type": "function",
                "name": "read_file",
                "description": "Read a file",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            }],
            "reasoning": { "effort": "high" },
            "max_output_tokens": 4096,
            "stream": true
        });
        let canonical =
            CanonicalRequest::from_responses_bytes(serde_json::to_vec(&body).unwrap().as_slice())
                .unwrap();
        let messages = KiroCanonicalAdapter {
            upstream_model: "claude-sonnet-4.6",
        }
        .adapt_request(&canonical)
        .unwrap();
        assert!(messages
            .thinking
            .as_ref()
            .is_some_and(|value| value.is_enabled()));
        assert_eq!(messages.tools.as_ref().map(Vec::len), Some(1));
        assert_eq!(messages.system.as_ref().map(Vec::len), Some(1));

        let converted = convert_request(&messages).unwrap();
        let payload = serde_json::to_value(&converted.conversation_state).unwrap();
        let current = &payload["currentMessage"]["userInputMessage"];
        assert_eq!(current["modelId"], "claude-sonnet-4.6");
        assert_eq!(
            current["images"].as_array().map(Vec::len),
            Some(1),
            "{payload}"
        );
        assert_eq!(
            current["userInputMessageContext"]["tools"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert!(current["content"]
            .as_str()
            .is_some_and(|content| content.contains("inspect this")));
    }
}
