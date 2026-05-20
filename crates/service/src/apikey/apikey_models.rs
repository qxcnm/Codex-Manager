use std::collections::{BTreeMap, HashSet};

use codexmanager_core::rpc::types::{
    ManagedModelCatalogEntry, ManagedModelCatalogResult, ManagedModelCatalogUpsertParams,
    ManagedModelRoutingResult, ManagedModelSourceMappingEntry,
    ManagedModelSourceMappingUpsertParams, ManagedModelSourceModelEntry,
    ManagedModelSourceModelUpsertParams, ManagedModelSourceSyncParams, ModelInfo,
    ModelReasoningLevel, ModelTruncationPolicy, ModelsResponse,
};
use codexmanager_core::storage::{
    now_ts, ModelCatalogModelRecord, ModelCatalogReasoningLevelRecord, ModelCatalogScopeRecord,
    ModelCatalogStringItemRecord, ModelSourceMapping, ModelSourceModel, Storage,
};
use rand::RngCore;
use serde_json::Value;

use crate::gateway;
use crate::storage_helpers;

const CODEX_IMAGE_TOOL_MODEL: &str = "gpt-image-2";
const MODEL_CACHE_SCOPE_DEFAULT: &str = "default";
const MODEL_SOURCE_KIND_REMOTE: &str = "remote";
const MODEL_SOURCE_KIND_CUSTOM: &str = "custom";
const ROUTING_SOURCE_KIND_OPENAI_ACCOUNT: &str = "openai_account";
const ROUTING_SOURCE_KIND_AGGREGATE_API: &str = "aggregate_api";

/// 函数 `read_model_options`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - refresh_remote: 参数 refresh_remote
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_model_options(refresh_remote: bool) -> Result<ModelsResponse, String> {
    read_managed_model_catalog(refresh_remote)
        .map(|catalog| managed_catalog_to_models_response(&catalog))
}

pub(crate) fn save_model_options_with_storage(
    storage: &Storage,
    models: &ModelsResponse,
) -> Result<(), String> {
    let normalized = normalize_models_response(models.clone());
    let catalog = ManagedModelCatalogResult {
        items: normalized
            .models
            .into_iter()
            .enumerate()
            .map(|(index, model)| ManagedModelCatalogEntry {
                model,
                source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                user_edited: false,
                sort_index: index as i64,
                updated_at: 0,
            })
            .collect(),
        extra: normalized.extra,
    };
    save_managed_model_catalog_with_storage(storage, &catalog)
}

pub(crate) fn read_model_options_from_storage(storage: &Storage) -> Result<ModelsResponse, String> {
    read_managed_model_catalog_from_storage(storage)
        .map(|catalog| managed_catalog_to_models_response(&catalog))
}

pub(crate) fn read_managed_model_catalog(
    refresh_remote: bool,
) -> Result<ManagedModelCatalogResult, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let cached_catalog = read_managed_model_catalog_from_storage(&storage)?;
    let cached = managed_catalog_to_models_response(&cached_catalog);
    if !refresh_remote && !cached.is_empty() {
        return Ok(ensure_codex_image_tool_model_in_catalog(&cached_catalog));
    }

    match gateway::fetch_models_for_picker() {
        Ok(models) => {
            let models = normalize_models_response(models);
            if !models_response_has_catalog_text_model(&models) {
                if managed_catalog_has_catalog_text_model(&cached_catalog) {
                    log::warn!(
                        "event=model_catalog_refresh_ignored_empty_remote cached_models={}",
                        cached_catalog.items.len()
                    );
                    return Ok(ensure_codex_image_tool_model_in_catalog(&cached_catalog));
                }
                if refresh_remote {
                    return Err(
                        "远端模型目录没有返回可用 Codex 文本模型，已拒绝覆盖本地目录".to_string(),
                    );
                }
            }
            let merged_catalog = ensure_codex_image_tool_model_in_catalog(
                &merge_managed_model_catalog(cached_catalog.clone(), models),
            );
            if !merged_catalog.items.is_empty() {
                let _ = save_managed_model_catalog_with_storage(&storage, &merged_catalog);
            }
            Ok(merged_catalog)
        }
        Err(err) => {
            if managed_catalog_has_catalog_text_model(&cached_catalog) {
                return Ok(ensure_codex_image_tool_model_in_catalog(&cached_catalog));
            }
            if refresh_remote {
                Err(err)
            } else {
                Ok(ensure_codex_image_tool_model_in_catalog(
                    &ManagedModelCatalogResult::default(),
                ))
            }
        }
    }
}

fn is_codex_image_tool_slug(slug: &str) -> bool {
    slug.trim().eq_ignore_ascii_case(CODEX_IMAGE_TOOL_MODEL)
}

fn model_is_catalog_text_model(model: &ModelInfo) -> bool {
    !is_codex_image_tool_slug(model.slug.as_str())
}

fn models_response_has_catalog_text_model(models: &ModelsResponse) -> bool {
    models.models.iter().any(model_is_catalog_text_model)
}

fn managed_catalog_has_catalog_text_model(catalog: &ManagedModelCatalogResult) -> bool {
    catalog
        .items
        .iter()
        .any(|item| model_is_catalog_text_model(&item.model))
}

pub(crate) fn read_managed_model_catalog_from_storage(
    storage: &Storage,
) -> Result<ManagedModelCatalogResult, String> {
    let rows = storage
        .list_model_catalog_models(MODEL_CACHE_SCOPE_DEFAULT)
        .map_err(|e| e.to_string())?;
    let scope_record = storage
        .get_model_catalog_scope(MODEL_CACHE_SCOPE_DEFAULT)
        .map_err(|e| e.to_string())?;

    if !rows.is_empty() {
        let reasoning_levels = storage
            .list_model_catalog_reasoning_levels(MODEL_CACHE_SCOPE_DEFAULT)
            .map_err(|e| e.to_string())?;
        let additional_speed_tiers = storage
            .list_model_catalog_additional_speed_tiers(MODEL_CACHE_SCOPE_DEFAULT)
            .map_err(|e| e.to_string())?;
        let experimental_supported_tools = storage
            .list_model_catalog_experimental_supported_tools(MODEL_CACHE_SCOPE_DEFAULT)
            .map_err(|e| e.to_string())?;
        let input_modalities = storage
            .list_model_catalog_input_modalities(MODEL_CACHE_SCOPE_DEFAULT)
            .map_err(|e| e.to_string())?;
        let available_in_plans = storage
            .list_model_catalog_available_in_plans(MODEL_CACHE_SCOPE_DEFAULT)
            .map_err(|e| e.to_string())?;

        let mut reasoning_by_slug = group_reasoning_levels_by_slug(reasoning_levels);
        let mut speed_tiers_by_slug = group_string_items_by_slug(additional_speed_tiers);
        let mut tools_by_slug = group_string_items_by_slug(experimental_supported_tools);
        let mut modalities_by_slug = group_string_items_by_slug(input_modalities);
        let mut plans_by_slug = group_string_items_by_slug(available_in_plans);

        let response_extra = scope_record
            .as_ref()
            .and_then(|record| parse_extra_json_map(Some(record.extra_json.as_str())))
            .unwrap_or_default();

        let mut rebuilt_items = Vec::new();
        for row in rows.iter().cloned() {
            let slug = row.slug.clone();
            if let Some(item) = managed_catalog_entry_from_row(
                row,
                reasoning_by_slug.remove(&slug),
                speed_tiers_by_slug.remove(&slug),
                tools_by_slug.remove(&slug),
                modalities_by_slug.remove(&slug),
                plans_by_slug.remove(&slug),
            ) {
                rebuilt_items.push(item);
            }
        }

        if !rebuilt_items.is_empty() {
            let updated_at = rows
                .iter()
                .map(|row| row.updated_at)
                .max()
                .unwrap_or_else(now_ts);
            let response = normalize_managed_model_catalog(ManagedModelCatalogResult {
                items: rebuilt_items,
                extra: response_extra,
            });
            if needs_structured_backfill(&rows, scope_record.is_none()) {
                let _ = save_managed_model_catalog_rows(storage, &response, updated_at);
            }
            return Ok(response);
        }
    }
    Ok(ManagedModelCatalogResult::default())
}

pub(crate) fn save_managed_model_catalog_with_storage(
    storage: &Storage,
    catalog: &ManagedModelCatalogResult,
) -> Result<(), String> {
    let normalized = normalize_managed_model_catalog(catalog.clone());
    let updated_at = now_ts();
    save_managed_model_catalog_rows(storage, &normalized, updated_at)
}

pub(crate) fn save_managed_model_catalog_model(
    params: ManagedModelCatalogUpsertParams,
) -> Result<ManagedModelCatalogEntry, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let current_catalog = read_managed_model_catalog_from_storage(&storage)?;
    let normalized_model =
        normalize_model_info(params.model).ok_or_else(|| "模型 slug 不能为空".to_string())?;
    let previous_slug = params
        .previous_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let existing_entry = previous_slug
        .as_ref()
        .and_then(|slug| {
            current_catalog
                .items
                .iter()
                .find(|item| item.model.slug == *slug)
        })
        .cloned()
        .or_else(|| {
            current_catalog
                .items
                .iter()
                .find(|item| item.model.slug == normalized_model.slug)
                .cloned()
        });

    if previous_slug
        .as_ref()
        .is_some_and(|slug| slug != &normalized_model.slug)
        && current_catalog
            .items
            .iter()
            .any(|item| item.model.slug == normalized_model.slug)
    {
        return Err(format!("模型 `{}` 已存在", normalized_model.slug));
    }

    let next_sort_index = params.sort_index.unwrap_or_else(|| {
        existing_entry
            .as_ref()
            .map(|item| item.sort_index)
            .unwrap_or_else(|| {
                current_catalog
                    .items
                    .iter()
                    .map(|item| item.sort_index)
                    .max()
                    .unwrap_or(-1)
                    + 1
            })
    });
    let next_source_kind = params
        .source_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| existing_entry.as_ref().map(|item| item.source_kind.clone()))
        .unwrap_or_else(|| MODEL_SOURCE_KIND_CUSTOM.to_string());
    let next_entry = ManagedModelCatalogEntry {
        model: normalized_model,
        source_kind: next_source_kind,
        user_edited: params.user_edited.unwrap_or(true),
        sort_index: next_sort_index,
        updated_at: now_ts(),
    };
    ensure_platform_model_enableable(&storage, &next_entry.model)?;

    replace_model_catalog_entry(&storage, previous_slug.as_deref(), &next_entry)?;
    Ok(next_entry)
}

pub(crate) fn delete_managed_model_catalog_model(slug: &str) -> Result<(), String> {
    let normalized_slug = slug.trim();
    if normalized_slug.is_empty() {
        return Err("模型 slug 不能为空".to_string());
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    delete_model_catalog_entry(&storage, normalized_slug)
}

pub(crate) fn read_managed_model_routing() -> Result<ManagedModelRoutingResult, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    read_managed_model_routing_from_storage(&storage, true)
}

pub(crate) fn sync_managed_model_source_models(
    params: ManagedModelSourceSyncParams,
) -> Result<ManagedModelRoutingResult, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let source_kind = normalize_routing_source_kind(params.source_kind.as_str())?;
    match source_kind.as_str() {
        ROUTING_SOURCE_KIND_OPENAI_ACCOUNT => {
            sync_openai_account_source_models(&storage, params.source_id.as_deref())?
        }
        ROUTING_SOURCE_KIND_AGGREGATE_API => {
            sync_aggregate_api_source_models(&storage, params.source_id.as_deref())?
        }
        _ => return Err("unsupported model source kind".to_string()),
    }
    routing_result_from_storage(&storage)
}

pub(crate) fn upsert_managed_model_source_model(
    params: ManagedModelSourceModelUpsertParams,
) -> Result<ManagedModelSourceModelEntry, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let source_kind = normalize_routing_source_kind(params.source_kind.as_str())?;
    let source_id = normalize_required("sourceId", params.source_id.as_str())?;
    let upstream_model = normalize_required("upstreamModel", params.upstream_model.as_str())?;
    ensure_source_exists(&storage, source_kind.as_str(), source_id.as_str())?;
    let now = now_ts();
    let record = ModelSourceModel {
        source_kind,
        source_id,
        upstream_model,
        display_name: params.display_name.and_then(normalize_optional),
        status: "available".to_string(),
        discovery_kind: "manual".to_string(),
        last_synced_at: None,
        extra_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    storage
        .upsert_model_source_model(&record)
        .map_err(|err| format!("save source model failed: {err}"))?;
    if record.source_kind == ROUTING_SOURCE_KIND_OPENAI_ACCOUNT
        || record.source_kind == ROUTING_SOURCE_KIND_AGGREGATE_API
    {
        auto_associate_source_models(
            &storage,
            record.source_kind.as_str(),
            record.source_id.as_str(),
            true,
        )?;
    }
    Ok(source_model_entry(record))
}

pub(crate) fn save_managed_model_source_mapping(
    params: ManagedModelSourceMappingUpsertParams,
) -> Result<ManagedModelSourceMappingEntry, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let platform_model_slug = normalize_required("platformModelSlug", &params.platform_model_slug)?;
    ensure_platform_model_exists(&storage, platform_model_slug.as_str())?;
    let source_kind = normalize_routing_source_kind(params.source_kind.as_str())?;
    let source_id = normalize_required("sourceId", &params.source_id)?;
    let upstream_model = normalize_required("upstreamModel", &params.upstream_model)?;
    ensure_source_exists(&storage, source_kind.as_str(), source_id.as_str())?;
    ensure_source_model_exists(
        &storage,
        source_kind.as_str(),
        source_id.as_str(),
        upstream_model.as_str(),
    )?;
    let now = now_ts();
    let mapping = ModelSourceMapping {
        id: params
            .id
            .and_then(normalize_optional)
            .unwrap_or_else(generate_mapping_id),
        platform_model_slug,
        source_kind,
        source_id,
        upstream_model,
        enabled: params.enabled.unwrap_or(true),
        priority: params.priority.unwrap_or(0),
        weight: params.weight.unwrap_or(1).max(1),
        billing_model_slug: params.billing_model_slug.and_then(normalize_optional),
        created_at: now,
        updated_at: now,
    };
    storage
        .upsert_model_source_mapping(&mapping)
        .map_err(|err| format!("save model mapping failed: {err}"))?;
    Ok(source_mapping_entry(mapping))
}

pub(crate) fn delete_managed_model_source_mapping(id: &str) -> Result<(), String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let id = normalize_required("id", id)?;
    storage
        .delete_model_source_mapping(id.as_str())
        .map_err(|err| format!("delete model mapping failed: {err}"))
}

fn routing_result_from_storage(storage: &Storage) -> Result<ManagedModelRoutingResult, String> {
    let source_models = storage
        .list_model_source_models(None, None)
        .map_err(|err| format!("list source models failed: {err}"))?
        .into_iter()
        .map(source_model_entry)
        .collect();
    let mappings = storage
        .list_model_source_mappings(None)
        .map_err(|err| format!("list model mappings failed: {err}"))?
        .into_iter()
        .map(source_mapping_entry)
        .collect();
    Ok(ManagedModelRoutingResult {
        source_models,
        mappings,
    })
}

fn read_managed_model_routing_from_storage(
    storage: &Storage,
    allow_remote_account_catalog_fetch: bool,
) -> Result<ManagedModelRoutingResult, String> {
    bootstrap_account_pool_model_routes(storage, allow_remote_account_catalog_fetch)?;
    bootstrap_aggregate_api_model_routes(storage)?;
    routing_result_from_storage(storage)
}

fn sync_openai_account_source_models(
    storage: &Storage,
    source_id: Option<&str>,
) -> Result<(), String> {
    sync_openai_account_source_models_with_options(storage, source_id, true)
}

pub(crate) fn bootstrap_account_pool_model_routes(
    storage: &Storage,
    allow_remote_catalog_fetch: bool,
) -> Result<(), String> {
    sync_openai_account_source_models_with_options(storage, None, allow_remote_catalog_fetch)
}

pub(crate) fn bootstrap_aggregate_api_model_routes(storage: &Storage) -> Result<(), String> {
    let active_source_ids = active_aggregate_api_source_ids(storage)?;
    prune_stale_aggregate_api_source_routes(storage, &active_source_ids)?;
    for source_id in active_source_ids {
        auto_associate_source_models(
            storage,
            ROUTING_SOURCE_KIND_AGGREGATE_API,
            source_id.as_str(),
            true,
        )?;
    }
    Ok(())
}

pub(crate) fn auto_associate_aggregate_api_source_models(
    storage: &Storage,
    source_id: &str,
) -> Result<(), String> {
    auto_associate_source_models(storage, ROUTING_SOURCE_KIND_AGGREGATE_API, source_id, true)
}

fn sync_openai_account_source_models_with_options(
    storage: &Storage,
    source_id: Option<&str>,
    allow_remote_catalog_fetch: bool,
) -> Result<(), String> {
    let requested_source_id = source_id.and_then(normalize_optional);
    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?
        .into_iter()
        .filter(|account| account.status == "active")
        .filter(|account| match requested_source_id.as_deref() {
            Some(source_id) => account.id == source_id,
            None => true,
        })
        .collect::<Vec<_>>();
    let active_source_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<HashSet<_>>();
    if let Some(source_id) = requested_source_id.as_deref() {
        if !active_source_ids.contains(source_id) {
            storage
                .delete_model_source_routes_for_source(
                    ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                    source_id,
                )
                .map_err(|err| format!("delete stale account source routes failed: {err}"))?;
        }
    } else {
        prune_stale_openai_account_source_routes(storage, &active_source_ids)?;
    }
    if accounts.is_empty() {
        return Ok(());
    }

    let platform_models = read_account_pool_platform_catalog(storage, allow_remote_catalog_fetch)?
        .items
        .into_iter()
        .filter(|item| item.model.supported_in_api)
        .map(|item| item.model.slug)
        .collect::<Vec<_>>();
    for account in accounts {
        if !platform_models.is_empty() {
            storage
                .upsert_discovered_model_source_models(
                    ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                    account.id.as_str(),
                    platform_models.as_slice(),
                    "synced",
                )
                .map_err(|err| format!("sync account source models failed: {err}"))?;
        }
        auto_associate_source_models(
            storage,
            ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
            account.id.as_str(),
            true,
        )?;
    }
    Ok(())
}

fn prune_stale_openai_account_source_routes(
    storage: &Storage,
    active_source_ids: &HashSet<String>,
) -> Result<(), String> {
    let mut known_source_ids = storage
        .list_model_source_models(Some(ROUTING_SOURCE_KIND_OPENAI_ACCOUNT), None)
        .map_err(|err| format!("list account source models failed: {err}"))?
        .into_iter()
        .map(|model| model.source_id)
        .collect::<HashSet<_>>();
    for mapping in storage
        .list_model_source_mappings(None)
        .map_err(|err| format!("list model mappings failed: {err}"))?
        .into_iter()
        .filter(|mapping| mapping.source_kind == ROUTING_SOURCE_KIND_OPENAI_ACCOUNT)
    {
        known_source_ids.insert(mapping.source_id);
    }
    for source_id in known_source_ids {
        if active_source_ids.contains(source_id.as_str()) {
            continue;
        }
        storage
            .delete_model_source_routes_for_source(
                ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                source_id.as_str(),
            )
            .map_err(|err| format!("delete stale account source routes failed: {err}"))?;
    }
    Ok(())
}

fn sync_aggregate_api_source_models(
    storage: &Storage,
    source_id: Option<&str>,
) -> Result<(), String> {
    sync_aggregate_api_source_models_with_discovery(storage, source_id, |api_id| {
        crate::discover_aggregate_api_models(api_id)
    })
}

fn sync_aggregate_api_source_models_with_discovery<F>(
    storage: &Storage,
    source_id: Option<&str>,
    mut discover_models: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<Vec<String>, String>,
{
    let requested_source_id = source_id.and_then(normalize_optional);
    let apis = storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate apis failed: {err}"))?;
    let active_source_ids = apis
        .iter()
        .filter(|api| api.status == "active")
        .map(|api| api.id.clone())
        .collect::<HashSet<_>>();
    match requested_source_id.as_deref() {
        Some(source_id) if !active_source_ids.contains(source_id) => {
            storage
                .delete_model_source_routes_for_source(ROUTING_SOURCE_KIND_AGGREGATE_API, source_id)
                .map_err(|err| format!("delete stale aggregate api source routes failed: {err}"))?;
            if apis.iter().any(|api| api.id == source_id) {
                return Err(format!("aggregate api `{source_id}` is disabled"));
            }
            return Err(format!("aggregate api `{source_id}` not found"));
        }
        Some(_) => {}
        None => prune_stale_aggregate_api_source_routes(storage, &active_source_ids)?,
    }
    let mut synced_any = false;
    let mut last_error: Option<String> = None;
    for api in apis
        .into_iter()
        .filter(|api| api.status == "active")
        .filter(|api| match requested_source_id.as_deref() {
            Some(source_id) => api.id == source_id,
            None => true,
        })
    {
        match discover_models(api.id.as_str()) {
            Ok(models) => {
                storage
                    .upsert_discovered_model_source_models(
                        ROUTING_SOURCE_KIND_AGGREGATE_API,
                        api.id.as_str(),
                        models.as_slice(),
                        "synced",
                    )
                    .map_err(|err| format!("sync aggregate api source models failed: {err}"))?;
                auto_associate_source_models(
                    storage,
                    ROUTING_SOURCE_KIND_AGGREGATE_API,
                    api.id.as_str(),
                    true,
                )?;
                synced_any = true;
            }
            Err(err) => {
                last_error = Some(format!("{}: {err}", api.id));
            }
        }
    }
    if !synced_any {
        if let Some(err) = last_error {
            return Err(err);
        }
    }
    Ok(())
}

fn active_aggregate_api_source_ids(storage: &Storage) -> Result<HashSet<String>, String> {
    storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate apis failed: {err}"))
        .map(|apis| {
            apis.into_iter()
                .filter(|api| api.status == "active")
                .map(|api| api.id)
                .collect::<HashSet<_>>()
        })
}

fn prune_stale_aggregate_api_source_routes(
    storage: &Storage,
    active_source_ids: &HashSet<String>,
) -> Result<(), String> {
    let mut known_source_ids = storage
        .list_model_source_models(Some(ROUTING_SOURCE_KIND_AGGREGATE_API), None)
        .map_err(|err| format!("list aggregate api source models failed: {err}"))?
        .into_iter()
        .map(|model| model.source_id)
        .collect::<HashSet<_>>();
    for mapping in storage
        .list_model_source_mappings(None)
        .map_err(|err| format!("list model mappings failed: {err}"))?
        .into_iter()
        .filter(|mapping| mapping.source_kind == ROUTING_SOURCE_KIND_AGGREGATE_API)
    {
        known_source_ids.insert(mapping.source_id);
    }
    for source_id in known_source_ids {
        if active_source_ids.contains(source_id.as_str()) {
            continue;
        }
        storage
            .delete_model_source_routes_for_source(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                source_id.as_str(),
            )
            .map_err(|err| format!("delete stale aggregate api source routes failed: {err}"))?;
    }
    Ok(())
}

fn read_account_pool_platform_catalog(
    storage: &Storage,
    allow_remote_catalog_fetch: bool,
) -> Result<ManagedModelCatalogResult, String> {
    let cached_catalog = read_managed_model_catalog_from_storage(storage)?;
    if !cached_catalog.items.is_empty() {
        let catalog = ensure_codex_image_tool_model_in_catalog(&cached_catalog);
        if catalog.items.len() != cached_catalog.items.len() {
            let _ = save_managed_model_catalog_with_storage(storage, &catalog);
        }
        return Ok(catalog);
    }

    if allow_remote_catalog_fetch {
        if let Ok(models) = gateway::fetch_models_for_picker() {
            let catalog = ensure_codex_image_tool_model_in_catalog(&merge_managed_model_catalog(
                cached_catalog.clone(),
                models,
            ));
            if !catalog.items.is_empty() {
                save_managed_model_catalog_with_storage(storage, &catalog)?;
                return Ok(catalog);
            }
        }
    }

    Ok(cached_catalog)
}

fn auto_associate_source_models(
    storage: &Storage,
    source_kind: &str,
    source_id: &str,
    auto_create_platform_models: bool,
) -> Result<(), String> {
    let existing_source_platform_mappings = storage
        .list_model_source_mappings(None)
        .map_err(|err| format!("list model mappings failed: {err}"))?
        .into_iter()
        .filter(|mapping| mapping.source_kind == source_kind && mapping.source_id == source_id)
        .map(|mapping| mapping.platform_model_slug)
        .collect::<HashSet<_>>();

    let source_models = storage
        .list_model_source_models(Some(source_kind), Some(source_id))
        .map_err(|err| format!("list source models failed: {err}"))?
        .into_iter()
        .filter(|model| model.status == "available")
        .filter(|model| !model.upstream_model.trim().is_empty())
        .collect::<Vec<_>>();
    if source_models.is_empty() {
        return Ok(());
    }

    let mut catalog = read_managed_model_catalog_from_storage(storage)?;
    if auto_create_platform_models {
        let mut known_slugs = catalog
            .items
            .iter()
            .map(|item| item.model.slug.clone())
            .collect::<HashSet<_>>();
        let mut next_sort_index = catalog
            .items
            .iter()
            .map(|item| item.sort_index)
            .max()
            .unwrap_or(-1)
            + 1;
        let now = now_ts();
        let mut changed = false;
        for source_model in &source_models {
            let upstream_model = source_model.upstream_model.trim();
            if upstream_model.is_empty() || known_slugs.contains(upstream_model) {
                continue;
            }
            let Some(model) = auto_platform_model_from_source_model(source_model) else {
                continue;
            };
            known_slugs.insert(model.slug.clone());
            catalog.items.push(ManagedModelCatalogEntry {
                model,
                source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                user_edited: false,
                sort_index: next_sort_index,
                updated_at: now,
            });
            next_sort_index += 1;
            changed = true;
        }
        if changed {
            save_managed_model_catalog_with_storage(storage, &catalog)?;
            catalog = read_managed_model_catalog_from_storage(storage)?;
        }
    }

    let platform_slugs = catalog
        .items
        .iter()
        .map(|item| item.model.slug.clone())
        .collect::<HashSet<_>>();
    if platform_slugs.is_empty() {
        return Ok(());
    }

    let now = now_ts();
    for source_model in source_models {
        if !platform_slugs.contains(source_model.upstream_model.as_str()) {
            continue;
        }
        if existing_source_platform_mappings.contains(source_model.upstream_model.as_str()) {
            continue;
        }
        let mapping = ModelSourceMapping {
            id: generate_mapping_id(),
            platform_model_slug: source_model.upstream_model.clone(),
            source_kind: source_kind.to_string(),
            source_id: source_id.to_string(),
            upstream_model: source_model.upstream_model,
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        };
        storage
            .upsert_model_source_mapping(&mapping)
            .map_err(|err| format!("save model mapping failed: {err}"))?;
    }
    Ok(())
}

fn auto_platform_model_from_source_model(source_model: &ModelSourceModel) -> Option<ModelInfo> {
    normalize_model_info(ModelInfo {
        slug: source_model.upstream_model.trim().to_string(),
        display_name: source_model
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| source_model.upstream_model.trim())
            .to_string(),
        supported_in_api: true,
        visibility: Some("list".to_string()),
        input_modalities: default_input_modalities(),
        ..Default::default()
    })
}

fn ensure_platform_model_enableable(storage: &Storage, model: &ModelInfo) -> Result<(), String> {
    if !model.supported_in_api {
        return Ok(());
    }
    let mappings = storage
        .list_enabled_model_source_mappings_for_platform(model.slug.as_str())
        .map_err(|err| format!("list model mappings failed: {err}"))?;
    if mappings.is_empty() {
        return Err(format!(
            "模型 `{}` 启用 API 前至少需要一个启用的来源映射",
            model.slug
        ));
    }
    Ok(())
}

fn ensure_platform_model_exists(storage: &Storage, slug: &str) -> Result<(), String> {
    let exists = storage
        .list_model_catalog_models(MODEL_CACHE_SCOPE_DEFAULT)
        .map_err(|err| format!("list model catalog failed: {err}"))?
        .into_iter()
        .any(|model| model.slug == slug);
    if exists {
        Ok(())
    } else {
        Err(format!("平台模型 `{slug}` 不存在"))
    }
}

fn ensure_source_exists(
    storage: &Storage,
    source_kind: &str,
    source_id: &str,
) -> Result<(), String> {
    match source_kind {
        ROUTING_SOURCE_KIND_OPENAI_ACCOUNT => storage
            .find_account_by_id(source_id)
            .map_err(|err| format!("read account failed: {err}"))?
            .map(|_| ())
            .ok_or_else(|| "账号来源不存在".to_string()),
        ROUTING_SOURCE_KIND_AGGREGATE_API => storage
            .find_aggregate_api_by_id(source_id)
            .map_err(|err| format!("read aggregate api failed: {err}"))?
            .map(|_| ())
            .ok_or_else(|| "上游 API 来源不存在".to_string()),
        _ => Err("unsupported model source kind".to_string()),
    }
}

fn ensure_source_model_exists(
    storage: &Storage,
    source_kind: &str,
    source_id: &str,
    upstream_model: &str,
) -> Result<(), String> {
    let exists = storage
        .list_model_source_models(Some(source_kind), Some(source_id))
        .map_err(|err| format!("list source models failed: {err}"))?
        .into_iter()
        .any(|model| model.upstream_model == upstream_model && model.status == "available");
    if exists {
        Ok(())
    } else {
        Err("来源模型不存在或不可用".to_string())
    }
}

fn source_model_entry(model: ModelSourceModel) -> ManagedModelSourceModelEntry {
    ManagedModelSourceModelEntry {
        source_kind: model.source_kind,
        source_id: model.source_id,
        upstream_model: model.upstream_model,
        display_name: model.display_name,
        status: model.status,
        discovery_kind: model.discovery_kind,
        last_synced_at: model.last_synced_at,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn source_mapping_entry(mapping: ModelSourceMapping) -> ManagedModelSourceMappingEntry {
    ManagedModelSourceMappingEntry {
        id: mapping.id,
        platform_model_slug: mapping.platform_model_slug,
        source_kind: mapping.source_kind,
        source_id: mapping.source_id,
        upstream_model: mapping.upstream_model,
        enabled: mapping.enabled,
        priority: mapping.priority,
        weight: mapping.weight,
        billing_model_slug: mapping.billing_model_slug,
        created_at: mapping.created_at,
        updated_at: mapping.updated_at,
    }
}

fn normalize_routing_source_kind(value: &str) -> Result<String, String> {
    match value.trim() {
        ROUTING_SOURCE_KIND_OPENAI_ACCOUNT => Ok(ROUTING_SOURCE_KIND_OPENAI_ACCOUNT.to_string()),
        ROUTING_SOURCE_KIND_AGGREGATE_API => Ok(ROUTING_SOURCE_KIND_AGGREGATE_API.to_string()),
        _ => Err("unsupported model source kind".to_string()),
    }
}

fn normalize_required(label: &str, value: &str) -> Result<String, String> {
    value
        .trim()
        .is_empty()
        .then(|| format!("{label} required"))
        .map_or_else(|| Ok(value.trim().to_string()), Err)
}

fn normalize_optional(value: impl AsRef<str>) -> Option<String> {
    let trimmed = value.as_ref().trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn generate_mapping_id() -> String {
    let mut bytes = [0_u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut suffix = String::with_capacity(16);
    for byte in bytes {
        suffix.push_str(&format!("{byte:02x}"));
    }
    format!("msm_{suffix}")
}

fn managed_catalog_to_models_response(catalog: &ManagedModelCatalogResult) -> ModelsResponse {
    ensure_codex_image_tool_model_listed(&ModelsResponse {
        models: catalog
            .items
            .iter()
            .map(|item| item.model.clone())
            .collect::<Vec<_>>(),
        extra: catalog.extra.clone(),
    })
}

fn codex_image_tool_model_info() -> ModelInfo {
    let mut model = ModelInfo {
        slug: CODEX_IMAGE_TOOL_MODEL.to_string(),
        display_name: "GPT Image 2".to_string(),
        description: Some("Image generation tool model for Codex image workflows.".to_string()),
        supported_in_api: true,
        visibility: Some("list".to_string()),
        input_modalities: vec!["text".to_string(), "image".to_string()],
        ..Default::default()
    };
    model.extra.insert(
        "output_modalities".to_string(),
        serde_json::json!(["image"]),
    );
    model
}

pub(crate) fn ensure_codex_image_tool_model_listed(models: &ModelsResponse) -> ModelsResponse {
    if models.models.iter().any(|item| {
        item.slug
            .trim()
            .eq_ignore_ascii_case(CODEX_IMAGE_TOOL_MODEL)
    }) {
        return models.clone();
    }
    let mut augmented = models.clone();
    augmented.models.push(codex_image_tool_model_info());
    augmented.extra.remove("etag");
    augmented
}

fn ensure_codex_image_tool_model_in_catalog(
    catalog: &ManagedModelCatalogResult,
) -> ManagedModelCatalogResult {
    if catalog.items.iter().any(|item| {
        item.model
            .slug
            .trim()
            .eq_ignore_ascii_case(CODEX_IMAGE_TOOL_MODEL)
    }) {
        return catalog.clone();
    }

    let mut augmented = catalog.clone();
    let sort_index = augmented
        .items
        .iter()
        .map(|item| item.sort_index)
        .max()
        .unwrap_or(-1)
        + 1;
    augmented.items.push(ManagedModelCatalogEntry {
        model: codex_image_tool_model_info(),
        source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
        user_edited: false,
        sort_index,
        updated_at: 0,
    });
    augmented.extra.remove("etag");
    augmented
}

fn normalize_managed_model_catalog(
    catalog: ManagedModelCatalogResult,
) -> ManagedModelCatalogResult {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for item in catalog.items {
        let Some(model) = normalize_model_info(item.model) else {
            continue;
        };
        if !seen.insert(model.slug.clone()) {
            continue;
        }
        items.push(ManagedModelCatalogEntry {
            model,
            source_kind: normalize_source_kind(Some(item.source_kind.as_str())),
            user_edited: item.user_edited,
            sort_index: item.sort_index,
            updated_at: item.updated_at,
        });
    }
    ManagedModelCatalogResult {
        items,
        extra: catalog.extra,
    }
}

fn normalize_source_kind(source_kind: Option<&str>) -> String {
    match source_kind.unwrap_or("").trim() {
        MODEL_SOURCE_KIND_CUSTOM => MODEL_SOURCE_KIND_CUSTOM.to_string(),
        _ => MODEL_SOURCE_KIND_REMOTE.to_string(),
    }
}

fn merge_managed_model_catalog(
    cached: ManagedModelCatalogResult,
    incoming: ModelsResponse,
) -> ManagedModelCatalogResult {
    let cached = normalize_managed_model_catalog(cached);
    let incoming = normalize_models_response(incoming);
    if cached.items.is_empty() {
        let ModelsResponse {
            models: incoming_models,
            extra: incoming_extra,
        } = incoming;
        return ManagedModelCatalogResult {
            items: incoming_models
                .into_iter()
                .enumerate()
                .map(|(index, model)| ManagedModelCatalogEntry {
                    model,
                    source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                    user_edited: false,
                    sort_index: index as i64,
                    updated_at: 0,
                })
                .collect(),
            extra: incoming_extra,
        };
    }
    if incoming.is_empty() {
        return cached;
    }

    let ModelsResponse {
        models: incoming_models,
        extra: incoming_extra,
    } = incoming;

    let mut cached_by_slug = BTreeMap::new();
    for item in &cached.items {
        cached_by_slug.insert(item.model.slug.clone(), item.clone());
    }

    let mut merged_items = Vec::new();
    let mut seen = HashSet::new();
    for (index, incoming_model) in incoming_models.into_iter().enumerate() {
        let slug = incoming_model.slug.clone();
        let merged_item = match cached_by_slug.get(&slug) {
            Some(cached_item) if cached_item.user_edited => {
                let mut preserved = cached_item.clone();
                if preserved.sort_index < 0 {
                    preserved.sort_index = index as i64;
                }
                preserved
            }
            Some(cached_item) => ManagedModelCatalogEntry {
                model: merge_model_info(cached_item.model.clone(), incoming_model),
                source_kind: normalize_source_kind(Some(cached_item.source_kind.as_str())),
                user_edited: false,
                sort_index: cached_item.sort_index,
                updated_at: cached_item.updated_at,
            },
            None => ManagedModelCatalogEntry {
                model: incoming_model,
                source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                user_edited: false,
                sort_index: index as i64,
                updated_at: 0,
            },
        };
        seen.insert(slug);
        merged_items.push(merged_item);
    }

    for cached_item in cached.items {
        if seen.insert(cached_item.model.slug.clone()) {
            merged_items.push(cached_item);
        }
    }

    normalize_managed_model_catalog(ManagedModelCatalogResult {
        items: merged_items,
        extra: merge_extra_maps(cached.extra, incoming_extra),
    })
}

pub(crate) fn normalize_models_response(response: ModelsResponse) -> ModelsResponse {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for model in response.models {
        if let Some(normalized) = normalize_model_info(model) {
            if seen.insert(normalized.slug.clone()) {
                models.push(normalized);
            }
        }
    }

    ModelsResponse {
        models,
        extra: response.extra,
    }
}

pub(crate) fn merge_models_response(
    cached: ModelsResponse,
    incoming: ModelsResponse,
) -> ModelsResponse {
    let cached = normalize_models_response(cached);
    let incoming = normalize_models_response(incoming);
    if cached.is_empty() {
        return incoming;
    }
    if incoming.is_empty() {
        return cached;
    }

    let cached_models = cached.models;
    let incoming_models = incoming.models;
    let mut cached_by_slug = BTreeMap::new();
    for model in &cached_models {
        cached_by_slug.insert(model.slug.clone(), model.clone());
    }

    let mut merged_models = Vec::new();
    let mut seen = HashSet::new();
    for incoming_model in incoming_models {
        let slug = incoming_model.slug.clone();
        let merged_model = match cached_by_slug.get(&slug) {
            Some(cached_model) => merge_model_info(cached_model.clone(), incoming_model),
            None => incoming_model,
        };
        seen.insert(slug);
        merged_models.push(merged_model);
    }

    for cached_model in cached_models {
        if seen.insert(cached_model.slug.clone()) {
            merged_models.push(cached_model);
        }
    }

    ModelsResponse {
        models: merged_models,
        extra: merge_extra_maps(cached.extra, incoming.extra),
    }
}

fn normalize_model_info(mut model: ModelInfo) -> Option<ModelInfo> {
    let slug = model.slug.trim().to_string();
    if slug.is_empty() {
        return None;
    }

    model.slug = slug;
    if model.display_name.trim().is_empty() {
        model.display_name = model.slug.clone();
    }
    model.visibility = normalize_visibility(model.visibility);
    if model.input_modalities.is_empty() {
        model.input_modalities = default_input_modalities();
    }
    Some(model)
}

fn model_info_from_row(
    row: ModelCatalogModelRecord,
    reasoning_levels: Option<Vec<ModelReasoningLevel>>,
    additional_speed_tiers: Option<Vec<String>>,
    experimental_supported_tools: Option<Vec<String>>,
    input_modalities: Option<Vec<String>>,
    available_in_plans: Option<Vec<String>>,
) -> Option<ModelInfo> {
    let mut model = ModelInfo {
        slug: row.slug.clone(),
        display_name: row.display_name.clone(),
        extra: parse_extra_json_map(Some(row.extra_json.as_str())).unwrap_or_default(),
        ..Default::default()
    };

    model.slug = row.slug.clone();
    if !row.display_name.trim().is_empty() {
        model.display_name = row.display_name.clone();
    }
    if let Some(description) = row.description {
        model.description = Some(description);
    }
    if let Some(default_reasoning_level) = row.default_reasoning_level {
        model.default_reasoning_level = Some(default_reasoning_level);
    }
    if let Some(shell_type) = row.shell_type {
        model.shell_type = Some(shell_type);
    }
    if let Some(visibility) = row.visibility {
        model.visibility = Some(visibility);
    }
    if let Some(supported_in_api) = row.supported_in_api {
        model.supported_in_api = supported_in_api;
    }
    if let Some(priority) = row.priority {
        model.priority = priority;
    }
    if let Some(availability_nux) = parse_json_value(row.availability_nux_json.as_deref()) {
        model.availability_nux = Some(availability_nux);
    }
    if let Some(upgrade) = parse_json_value(row.upgrade_json.as_deref()) {
        model.upgrade = Some(upgrade);
    }
    if let Some(base_instructions) = row.base_instructions {
        model.base_instructions = Some(base_instructions);
    }
    if let Some(model_messages) = parse_json_value(row.model_messages_json.as_deref()) {
        model.model_messages = Some(model_messages);
    }
    if let Some(supports_reasoning_summaries) = row.supports_reasoning_summaries {
        model.supports_reasoning_summaries = Some(supports_reasoning_summaries);
    }
    if let Some(default_reasoning_summary) = row.default_reasoning_summary {
        model.default_reasoning_summary = Some(default_reasoning_summary);
    }
    if let Some(support_verbosity) = row.support_verbosity {
        model.support_verbosity = Some(support_verbosity);
    }
    if let Some(default_verbosity) = parse_json_value(row.default_verbosity_json.as_deref()) {
        model.default_verbosity = Some(default_verbosity);
    }
    if let Some(apply_patch_tool_type) = row.apply_patch_tool_type {
        model.apply_patch_tool_type = Some(apply_patch_tool_type);
    }
    if let Some(web_search_tool_type) = row.web_search_tool_type {
        model.web_search_tool_type = Some(web_search_tool_type);
    }
    if let Some(truncation_policy) = build_truncation_policy(
        row.truncation_mode.as_deref(),
        row.truncation_limit,
        row.truncation_extra_json.as_deref(),
        model.truncation_policy.take(),
    ) {
        model.truncation_policy = Some(truncation_policy);
    }
    if let Some(supports_parallel_tool_calls) = row.supports_parallel_tool_calls {
        model.supports_parallel_tool_calls = Some(supports_parallel_tool_calls);
    }
    if let Some(supports_image_detail_original) = row.supports_image_detail_original {
        model.supports_image_detail_original = Some(supports_image_detail_original);
    }
    if let Some(context_window) = row.context_window {
        model.context_window = Some(context_window);
    }
    if let Some(auto_compact_token_limit) = row.auto_compact_token_limit {
        model.auto_compact_token_limit = Some(auto_compact_token_limit);
    }
    if let Some(effective_context_window_percent) = row.effective_context_window_percent {
        model.effective_context_window_percent = Some(effective_context_window_percent);
    }
    if let Some(minimal_client_version) =
        parse_json_value(row.minimal_client_version_json.as_deref())
    {
        model.minimal_client_version = Some(minimal_client_version);
    }
    if let Some(supports_search_tool) = row.supports_search_tool {
        model.supports_search_tool = Some(supports_search_tool);
    }
    if let Some(levels) = reasoning_levels {
        model.supported_reasoning_levels = levels;
    }
    if let Some(speed_tiers) = additional_speed_tiers {
        model.additional_speed_tiers = speed_tiers;
    }
    if let Some(tools) = experimental_supported_tools {
        model.experimental_supported_tools = tools;
    }
    if let Some(modalities) = input_modalities {
        model.input_modalities = modalities;
    }
    if let Some(plans) = available_in_plans {
        model.available_in_plans = plans;
    }

    normalize_model_info(model)
}

fn managed_catalog_entry_from_row(
    row: ModelCatalogModelRecord,
    reasoning_levels: Option<Vec<ModelReasoningLevel>>,
    additional_speed_tiers: Option<Vec<String>>,
    experimental_supported_tools: Option<Vec<String>>,
    input_modalities: Option<Vec<String>>,
    available_in_plans: Option<Vec<String>>,
) -> Option<ManagedModelCatalogEntry> {
    let source_kind = normalize_source_kind(Some(row.source_kind.as_str()));
    let user_edited = row.user_edited;
    let sort_index = row.sort_index;
    let updated_at = row.updated_at;
    model_info_from_row(
        row,
        reasoning_levels,
        additional_speed_tiers,
        experimental_supported_tools,
        input_modalities,
        available_in_plans,
    )
    .map(|model| ManagedModelCatalogEntry {
        model,
        source_kind,
        user_edited,
        sort_index,
        updated_at,
    })
}

fn save_managed_model_catalog_rows(
    storage: &Storage,
    catalog: &ManagedModelCatalogResult,
    updated_at: i64,
) -> Result<(), String> {
    let scope_record = ModelCatalogScopeRecord {
        scope: MODEL_CACHE_SCOPE_DEFAULT.to_string(),
        extra_json: serialize_extra_map(&catalog.extra)?,
        updated_at,
    };
    storage
        .upsert_model_catalog_scope(&scope_record)
        .map_err(|e| e.to_string())?;

    let mut model_rows = Vec::new();
    let mut reasoning_rows = Vec::new();
    let mut additional_speed_tiers = Vec::new();
    let mut experimental_supported_tools = Vec::new();
    let mut input_modalities = Vec::new();
    let mut available_in_plans = Vec::new();

    for (index, item) in catalog.items.iter().enumerate() {
        let item_updated_at = if item.updated_at > 0 {
            item.updated_at
        } else {
            updated_at
        };
        let sort_index = if item.sort_index >= 0 {
            item.sort_index
        } else {
            index as i64
        };
        model_rows.push(model_record_from_model(item, sort_index, item_updated_at)?);
        reasoning_rows.extend(reasoning_records_from_model(&item.model, item_updated_at)?);
        additional_speed_tiers.extend(string_records_from_model(
            &item.model.slug,
            &item.model.additional_speed_tiers,
            item_updated_at,
        ));
        experimental_supported_tools.extend(string_records_from_model(
            &item.model.slug,
            &item.model.experimental_supported_tools,
            item_updated_at,
        ));
        input_modalities.extend(string_records_from_model(
            &item.model.slug,
            &item.model.input_modalities,
            item_updated_at,
        ));
        available_in_plans.extend(string_records_from_model(
            &item.model.slug,
            &item.model.available_in_plans,
            item_updated_at,
        ));
    }

    storage
        .upsert_model_catalog_models(&model_rows)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_model_catalog_reasoning_levels(&reasoning_rows)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_model_catalog_additional_speed_tiers(&additional_speed_tiers)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_model_catalog_experimental_supported_tools(&experimental_supported_tools)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_model_catalog_input_modalities(&input_modalities)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_model_catalog_available_in_plans(&available_in_plans)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn model_record_from_model(
    item: &ManagedModelCatalogEntry,
    sort_index: i64,
    updated_at: i64,
) -> Result<ModelCatalogModelRecord, String> {
    let model = &item.model;
    let truncation_extra_json = model
        .truncation_policy
        .as_ref()
        .map(|policy| serialize_extra_map(&policy.extra))
        .transpose()?;
    Ok(ModelCatalogModelRecord {
        scope: MODEL_CACHE_SCOPE_DEFAULT.to_string(),
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        source_kind: normalize_source_kind(Some(item.source_kind.as_str())),
        user_edited: item.user_edited,
        description: model.description.clone(),
        default_reasoning_level: model.default_reasoning_level.clone(),
        shell_type: model.shell_type.clone(),
        visibility: model.visibility.clone(),
        supported_in_api: Some(model.supported_in_api),
        priority: Some(model.priority),
        availability_nux_json: serialize_json_option(&model.availability_nux)?,
        upgrade_json: serialize_json_option(&model.upgrade)?,
        base_instructions: model.base_instructions.clone(),
        model_messages_json: serialize_json_option(&model.model_messages)?,
        supports_reasoning_summaries: model.supports_reasoning_summaries,
        default_reasoning_summary: model.default_reasoning_summary.clone(),
        support_verbosity: model.support_verbosity,
        default_verbosity_json: serialize_json_option(&model.default_verbosity)?,
        apply_patch_tool_type: model.apply_patch_tool_type.clone(),
        web_search_tool_type: model.web_search_tool_type.clone(),
        truncation_mode: model
            .truncation_policy
            .as_ref()
            .map(|policy| policy.mode.clone()),
        truncation_limit: model.truncation_policy.as_ref().map(|policy| policy.limit),
        truncation_extra_json,
        supports_parallel_tool_calls: model.supports_parallel_tool_calls,
        supports_image_detail_original: model.supports_image_detail_original,
        context_window: model.context_window,
        auto_compact_token_limit: model.auto_compact_token_limit,
        effective_context_window_percent: model.effective_context_window_percent,
        minimal_client_version_json: serialize_json_option(&model.minimal_client_version)?,
        supports_search_tool: model.supports_search_tool,
        extra_json: serialize_extra_map(&model.extra)?,
        sort_index,
        updated_at,
    })
}

fn replace_model_catalog_entry(
    storage: &Storage,
    previous_slug: Option<&str>,
    entry: &ManagedModelCatalogEntry,
) -> Result<(), String> {
    let target_slug = entry.model.slug.as_str();
    if let Some(previous_slug) = previous_slug {
        let normalized_previous = previous_slug.trim();
        if !normalized_previous.is_empty() && normalized_previous != target_slug {
            delete_model_catalog_entry(storage, normalized_previous)?;
        }
    }

    storage
        .delete_model_catalog_reasoning_levels(MODEL_CACHE_SCOPE_DEFAULT, target_slug)
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            target_slug,
            "additional_speed_tiers",
        )
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            target_slug,
            "experimental_supported_tools",
        )
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            target_slug,
            "input_modalities",
        )
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            target_slug,
            "available_in_plans",
        )
        .map_err(|e| e.to_string())?;

    let updated_at = if entry.updated_at > 0 {
        entry.updated_at
    } else {
        now_ts()
    };
    let model_row = model_record_from_model(entry, entry.sort_index, updated_at)?;
    storage
        .upsert_model_catalog_models(&[model_row])
        .map_err(|e| e.to_string())?;
    let reasoning_rows = reasoning_records_from_model(&entry.model, updated_at)?;
    storage
        .upsert_model_catalog_reasoning_levels(&reasoning_rows)
        .map_err(|e| e.to_string())?;
    let additional_speed_tiers = string_records_from_model(
        &entry.model.slug,
        &entry.model.additional_speed_tiers,
        updated_at,
    );
    storage
        .upsert_model_catalog_additional_speed_tiers(&additional_speed_tiers)
        .map_err(|e| e.to_string())?;
    let experimental_supported_tools = string_records_from_model(
        &entry.model.slug,
        &entry.model.experimental_supported_tools,
        updated_at,
    );
    storage
        .upsert_model_catalog_experimental_supported_tools(&experimental_supported_tools)
        .map_err(|e| e.to_string())?;
    let input_modalities =
        string_records_from_model(&entry.model.slug, &entry.model.input_modalities, updated_at);
    storage
        .upsert_model_catalog_input_modalities(&input_modalities)
        .map_err(|e| e.to_string())?;
    let available_in_plans = string_records_from_model(
        &entry.model.slug,
        &entry.model.available_in_plans,
        updated_at,
    );
    storage
        .upsert_model_catalog_available_in_plans(&available_in_plans)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn delete_model_catalog_entry(storage: &Storage, slug: &str) -> Result<(), String> {
    storage
        .delete_model_catalog_reasoning_levels(MODEL_CACHE_SCOPE_DEFAULT, slug)
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            slug,
            "additional_speed_tiers",
        )
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(
            MODEL_CACHE_SCOPE_DEFAULT,
            slug,
            "experimental_supported_tools",
        )
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(MODEL_CACHE_SCOPE_DEFAULT, slug, "input_modalities")
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_string_items(MODEL_CACHE_SCOPE_DEFAULT, slug, "available_in_plans")
        .map_err(|e| e.to_string())?;
    storage
        .delete_model_catalog_model(MODEL_CACHE_SCOPE_DEFAULT, slug)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn reasoning_records_from_model(
    model: &ModelInfo,
    updated_at: i64,
) -> Result<Vec<ModelCatalogReasoningLevelRecord>, String> {
    let mut records = Vec::new();
    for (index, level) in model.supported_reasoning_levels.iter().enumerate() {
        records.push(ModelCatalogReasoningLevelRecord {
            scope: MODEL_CACHE_SCOPE_DEFAULT.to_string(),
            slug: model.slug.clone(),
            effort: level.effort.clone(),
            description: level.description.clone(),
            extra_json: serialize_extra_map(&level.extra)?,
            sort_index: index as i64,
            updated_at,
        });
    }
    Ok(records)
}

fn string_records_from_model(
    slug: &str,
    values: &[String],
    updated_at: i64,
) -> Vec<ModelCatalogStringItemRecord> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| ModelCatalogStringItemRecord {
            scope: MODEL_CACHE_SCOPE_DEFAULT.to_string(),
            slug: slug.to_string(),
            value: value.clone(),
            sort_index: index as i64,
            updated_at,
        })
        .collect()
}

fn merge_model_info(mut cached: ModelInfo, incoming: ModelInfo) -> ModelInfo {
    cached.slug = incoming.slug;
    cached.display_name = merge_string(cached.display_name, incoming.display_name);
    cached.description = merge_option_string(cached.description, incoming.description);
    cached.default_reasoning_level = merge_option_string(
        cached.default_reasoning_level,
        incoming.default_reasoning_level,
    );
    cached.supported_reasoning_levels = merge_reasoning_levels(
        cached.supported_reasoning_levels,
        incoming.supported_reasoning_levels,
    );
    cached.shell_type = merge_option_string(cached.shell_type, incoming.shell_type);
    cached.visibility = merge_option_string(cached.visibility, incoming.visibility);
    cached.supported_in_api = cached.supported_in_api || incoming.supported_in_api;
    cached.priority = merge_number(cached.priority, incoming.priority);
    cached.additional_speed_tiers = merge_string_vec(
        cached.additional_speed_tiers,
        incoming.additional_speed_tiers,
    );
    cached.availability_nux = incoming.availability_nux.or(cached.availability_nux);
    cached.upgrade = incoming.upgrade.or(cached.upgrade);
    cached.base_instructions =
        merge_option_string(cached.base_instructions, incoming.base_instructions);
    cached.model_messages = incoming.model_messages.or(cached.model_messages);
    cached.supports_reasoning_summaries = incoming
        .supports_reasoning_summaries
        .or(cached.supports_reasoning_summaries);
    cached.default_reasoning_summary = merge_option_string(
        cached.default_reasoning_summary,
        incoming.default_reasoning_summary,
    );
    cached.support_verbosity = incoming.support_verbosity.or(cached.support_verbosity);
    cached.default_verbosity = incoming.default_verbosity.or(cached.default_verbosity);
    cached.apply_patch_tool_type =
        merge_option_string(cached.apply_patch_tool_type, incoming.apply_patch_tool_type);
    cached.web_search_tool_type =
        merge_option_string(cached.web_search_tool_type, incoming.web_search_tool_type);
    cached.truncation_policy = incoming.truncation_policy.or(cached.truncation_policy);
    cached.supports_parallel_tool_calls = incoming
        .supports_parallel_tool_calls
        .or(cached.supports_parallel_tool_calls);
    cached.supports_image_detail_original = incoming
        .supports_image_detail_original
        .or(cached.supports_image_detail_original);
    cached.context_window = incoming.context_window.or(cached.context_window);
    cached.auto_compact_token_limit = incoming
        .auto_compact_token_limit
        .or(cached.auto_compact_token_limit);
    cached.effective_context_window_percent = incoming
        .effective_context_window_percent
        .or(cached.effective_context_window_percent);
    cached.experimental_supported_tools = merge_string_vec(
        cached.experimental_supported_tools,
        incoming.experimental_supported_tools,
    );
    cached.input_modalities = merge_string_vec(cached.input_modalities, incoming.input_modalities);
    cached.minimal_client_version = incoming
        .minimal_client_version
        .or(cached.minimal_client_version);
    cached.supports_search_tool = incoming
        .supports_search_tool
        .or(cached.supports_search_tool);
    cached.available_in_plans =
        merge_string_vec(cached.available_in_plans, incoming.available_in_plans);
    cached.extra = merge_extra_maps(cached.extra, incoming.extra);
    normalize_model_info(cached).unwrap_or_default()
}

fn merge_string(cached: String, incoming: String) -> String {
    if incoming.trim().is_empty() {
        cached
    } else {
        incoming
    }
}

fn merge_option_string(cached: Option<String>, incoming: Option<String>) -> Option<String> {
    match incoming {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => cached,
    }
}

fn merge_number(cached: i64, incoming: i64) -> i64 {
    if incoming == 0 {
        cached
    } else {
        incoming
    }
}

fn merge_reasoning_levels(
    cached: Vec<ModelReasoningLevel>,
    incoming: Vec<ModelReasoningLevel>,
) -> Vec<ModelReasoningLevel> {
    if incoming.is_empty() {
        cached
    } else {
        let mut cached_by_effort = BTreeMap::new();
        for level in cached {
            cached_by_effort.insert(level.effort.clone(), level);
        }

        let mut merged = Vec::new();
        let mut seen = HashSet::new();
        for level in incoming {
            let effort = level.effort.clone();
            let merged_level = match cached_by_effort.get(&effort) {
                Some(cached_level) => ModelReasoningLevel {
                    effort: effort.clone(),
                    description: merge_string(cached_level.description.clone(), level.description),
                    extra: merge_extra_maps(cached_level.extra.clone(), level.extra),
                },
                None => level,
            };
            seen.insert(effort);
            merged.push(merged_level);
        }

        for (effort, level) in cached_by_effort {
            if seen.insert(effort) {
                merged.push(level);
            }
        }
        merged
    }
}

fn merge_string_vec(cached: Vec<String>, incoming: Vec<String>) -> Vec<String> {
    if incoming.is_empty() {
        return cached;
    }

    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for value in incoming.into_iter().chain(cached) {
        let normalized = value.trim().to_string();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        merged.push(normalized);
    }
    merged
}

fn merge_extra_maps(
    mut cached: BTreeMap<String, Value>,
    incoming: BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    cached.extend(incoming);
    cached
}

fn default_input_modalities() -> Vec<String> {
    vec!["text".to_string(), "image".to_string()]
}

fn normalize_visibility(value: Option<String>) -> Option<String> {
    let normalized = value
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.to_ascii_lowercase())?;
    match normalized.as_str() {
        "hidden" => Some("hide".to_string()),
        _ => Some(normalized),
    }
}

fn serialize_json_option(value: &Option<Value>) -> Result<Option<String>, String> {
    value
        .as_ref()
        .map(|item| serde_json::to_string(item).map_err(|e| e.to_string()))
        .transpose()
}

fn serialize_extra_map(extra: &BTreeMap<String, Value>) -> Result<String, String> {
    serde_json::to_string(extra).map_err(|e| e.to_string())
}

fn parse_json_value(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|item| serde_json::from_str::<Value>(item).ok())
}

fn parse_extra_json_map(raw: Option<&str>) -> Option<BTreeMap<String, Value>> {
    raw.and_then(|item| serde_json::from_str::<BTreeMap<String, Value>>(item).ok())
}

fn build_truncation_policy(
    mode: Option<&str>,
    limit: Option<i64>,
    extra_json: Option<&str>,
    existing: Option<ModelTruncationPolicy>,
) -> Option<ModelTruncationPolicy> {
    let has_row_value = mode.is_some() || limit.is_some() || extra_json.is_some();
    if !has_row_value {
        return existing;
    }

    let mut policy = existing.unwrap_or_default();
    if let Some(mode) = mode {
        policy.mode = mode.to_string();
    }
    if let Some(limit) = limit {
        policy.limit = limit;
    }
    if let Some(extra) = parse_extra_json_map(extra_json) {
        policy.extra = extra;
    }
    Some(policy)
}

fn group_reasoning_levels_by_slug(
    records: Vec<ModelCatalogReasoningLevelRecord>,
) -> BTreeMap<String, Vec<ModelReasoningLevel>> {
    let mut grouped = BTreeMap::new();
    for record in records {
        grouped
            .entry(record.slug)
            .or_insert_with(Vec::new)
            .push(ModelReasoningLevel {
                effort: record.effort,
                description: record.description,
                extra: parse_extra_json_map(Some(record.extra_json.as_str())).unwrap_or_default(),
            });
    }
    grouped
}

fn group_string_items_by_slug(
    records: Vec<ModelCatalogStringItemRecord>,
) -> BTreeMap<String, Vec<String>> {
    let mut grouped = BTreeMap::new();
    for record in records {
        grouped
            .entry(record.slug)
            .or_insert_with(Vec::new)
            .push(record.value);
    }
    grouped
}

fn needs_structured_backfill(rows: &[ModelCatalogModelRecord], missing_scope_row: bool) -> bool {
    missing_scope_row
        || rows.iter().any(|row| {
            row.supported_in_api.is_none()
                && row.priority.is_none()
                && row.visibility.is_none()
                && row.minimal_client_version_json.is_none()
                && row.context_window.is_none()
                && row.extra_json.trim().is_empty()
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use codexmanager_core::storage::{now_ts, Account, AggregateApi, ModelSourceMapping, Storage};
    use serde_json::{json, Value};

    use super::{
        auto_associate_aggregate_api_source_models, auto_associate_source_models,
        bootstrap_account_pool_model_routes, bootstrap_aggregate_api_model_routes,
        ensure_codex_image_tool_model_in_catalog, ensure_codex_image_tool_model_listed,
        managed_catalog_to_models_response, merge_managed_model_catalog, merge_models_response,
        normalize_models_response, read_managed_model_catalog_from_storage,
        read_managed_model_routing_from_storage, read_model_options_from_storage,
        save_managed_model_catalog_with_storage, save_model_options_with_storage,
        sync_aggregate_api_source_models, sync_aggregate_api_source_models_with_discovery,
        MODEL_SOURCE_KIND_CUSTOM, MODEL_SOURCE_KIND_REMOTE, ROUTING_SOURCE_KIND_AGGREGATE_API,
        ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
    };
    use codexmanager_core::rpc::types::{
        ManagedModelCatalogEntry, ManagedModelCatalogResult, ModelInfo, ModelsResponse,
    };

    fn insert_test_account(storage: &Storage, id: &str) {
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: Some(id.to_string()),
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }

    fn insert_test_aggregate_api(storage: &Storage, id: &str, status: &str) {
        let now = now_ts();
        storage
            .insert_aggregate_api(&AggregateApi {
                id: id.to_string(),
                provider_type: "codex".to_string(),
                supplier_name: Some(id.to_string()),
                sort: 0,
                url: format!("https://{id}.example/v1"),
                auth_type: "apikey".to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
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
            })
            .expect("insert aggregate api");
    }

    fn seed_platform_catalog(storage: &Storage, slugs: &[&str]) {
        let payload = ManagedModelCatalogResult {
            items: slugs
                .iter()
                .enumerate()
                .map(|(index, slug)| ManagedModelCatalogEntry {
                    model: ModelInfo {
                        slug: (*slug).to_string(),
                        display_name: (*slug).to_string(),
                        supported_in_api: true,
                        visibility: Some("list".to_string()),
                        ..Default::default()
                    },
                    source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                    user_edited: false,
                    sort_index: index as i64,
                    updated_at: 0,
                })
                .collect(),
            extra: BTreeMap::new(),
        };
        save_managed_model_catalog_with_storage(storage, &payload).expect("seed platform catalog");
    }

    #[test]
    fn normalize_models_response_keeps_full_model_metadata() {
        let response = ModelsResponse {
            models: vec![
                serde_json::from_value(json!({
                    "slug": "gpt-5",
                    "display_name": "GPT-5",
                    "supported_in_api": true,
                    "visibility": "list",
                    "supported_reasoning_levels": [
                        { "effort": "medium", "description": "balanced" }
                    ]
                }))
                .expect("parse model"),
                ModelInfo {
                    slug: " ".to_string(),
                    display_name: String::new(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let normalized = normalize_models_response(response);
        assert_eq!(normalized.models.len(), 1);
        assert_eq!(normalized.models[0].slug, "gpt-5");
        assert_eq!(normalized.models[0].display_name, "GPT-5");
        assert!(normalized.models[0].supported_in_api);
        assert_eq!(normalized.models[0].supported_reasoning_levels.len(), 1);
    }

    #[test]
    fn normalize_models_response_maps_hidden_visibility_to_hide() {
        let response = ModelsResponse {
            models: vec![serde_json::from_value(json!({
                "slug": "gpt-5.4-mini",
                "display_name": "GPT-5.4-Mini",
                "supported_in_api": true,
                "visibility": "hidden"
            }))
            .expect("parse model")],
            ..Default::default()
        };

        let normalized = normalize_models_response(response);
        assert_eq!(normalized.models.len(), 1);
        assert_eq!(normalized.models[0].visibility.as_deref(), Some("hide"));
    }

    #[test]
    fn merge_models_response_updates_existing_without_removing_cached_fields() {
        let cached = ModelsResponse {
            models: vec![
                ModelInfo {
                    slug: "gpt-5".to_string(),
                    display_name: "GPT-5".to_string(),
                    description: Some("cached description".to_string()),
                    supported_in_api: true,
                    priority: 200,
                    input_modalities: vec!["text".to_string(), "image".to_string()],
                    additional_speed_tiers: vec!["fast".to_string()],
                    supported_reasoning_levels: vec![serde_json::from_value(json!({
                        "effort": "medium",
                        "description": "balanced"
                    }))
                    .expect("reasoning preset")],
                    ..Default::default()
                },
                ModelInfo {
                    slug: "gpt-legacy".to_string(),
                    display_name: "GPT Legacy".to_string(),
                    supported_in_api: true,
                    ..Default::default()
                },
            ],
            extra: BTreeMap::from([("etag".to_string(), json!("cached"))]),
        };
        let incoming = ModelsResponse {
            models: vec![
                ModelInfo {
                    slug: "gpt-5".to_string(),
                    display_name: "GPT-5 New".to_string(),
                    supported_in_api: false,
                    supported_reasoning_levels: vec![serde_json::from_value(json!({
                        "effort": "high",
                        "description": "deeper"
                    }))
                    .expect("reasoning preset")],
                    visibility: Some("list".to_string()),
                    additional_speed_tiers: vec!["turbo".to_string()],
                    ..Default::default()
                },
                ModelInfo {
                    slug: "gpt-new".to_string(),
                    display_name: "GPT New".to_string(),
                    supported_in_api: true,
                    ..Default::default()
                },
            ],
            extra: BTreeMap::from([("etag".to_string(), json!("fresh"))]),
        };

        let merged = merge_models_response(cached, incoming);
        assert_eq!(
            merged
                .models
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5", "gpt-new", "gpt-legacy"]
        );
        assert_eq!(merged.models[0].display_name, "GPT-5 New");
        assert_eq!(
            merged.models[0].description.as_deref(),
            Some("cached description")
        );
        assert!(merged.models[0].supported_in_api);
        assert_eq!(merged.models[0].priority, 200);
        assert_eq!(
            merged.models[0].input_modalities,
            vec!["text".to_string(), "image".to_string()]
        );
        assert_eq!(
            merged.models[0].additional_speed_tiers,
            vec!["turbo".to_string(), "fast".to_string()]
        );
        assert_eq!(merged.models[0].supported_reasoning_levels.len(), 2);
        assert_eq!(
            merged.extra.get("etag").and_then(Value::as_str),
            Some("fresh")
        );
    }

    #[test]
    fn model_options_response_appends_codex_image_tool_model() {
        let response = ModelsResponse {
            models: vec![ModelInfo {
                slug: "gpt-5.4-mini".to_string(),
                display_name: "GPT-5.4 Mini".to_string(),
                supported_in_api: true,
                ..Default::default()
            }],
            extra: BTreeMap::from([("etag".to_string(), json!("cached"))]),
        };

        let augmented = ensure_codex_image_tool_model_listed(&response);

        assert_eq!(
            augmented
                .models
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.4-mini", "gpt-image-2"]
        );
        let image_model = augmented
            .models
            .iter()
            .find(|model| model.slug == "gpt-image-2")
            .expect("image tool model");
        assert_eq!(image_model.display_name, "GPT Image 2");
        assert_eq!(
            image_model.input_modalities,
            vec!["text".to_string(), "image".to_string()]
        );
        assert_eq!(
            image_model
                .extra
                .get("output_modalities")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(Value::as_str),
            Some("image")
        );
        assert!(!augmented.extra.contains_key("etag"));
    }

    #[test]
    fn managed_catalog_response_appends_codex_image_tool_model_once() {
        let catalog = ManagedModelCatalogResult {
            items: vec![ManagedModelCatalogEntry {
                model: ModelInfo {
                    slug: "gpt-image-2".to_string(),
                    display_name: "Existing Image Model".to_string(),
                    supported_in_api: true,
                    ..Default::default()
                },
                source_kind: MODEL_SOURCE_KIND_CUSTOM.to_string(),
                user_edited: true,
                sort_index: 7,
                updated_at: 123,
            }],
            extra: BTreeMap::new(),
        };

        let augmented = ensure_codex_image_tool_model_in_catalog(&catalog);
        let response = managed_catalog_to_models_response(&augmented);

        assert_eq!(augmented.items.len(), 1);
        assert_eq!(response.models.len(), 1);
        assert_eq!(response.models[0].display_name, "Existing Image Model");
    }

    #[test]
    fn read_model_options_from_storage_reads_structured_catalog() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let payload = ModelsResponse {
            models: vec![serde_json::from_value(json!({
                "slug": "gpt-5.4",
                "display_name": "GPT-5.4",
                "description": "Latest frontier model",
                "supported_in_api": true,
                "supported_reasoning_levels": [
                    { "effort": "medium", "description": "balanced" }
                ],
                "input_modalities": ["text", "image"],
                "available_in_plans": ["pro", "team"]
            }))
            .expect("parse model")],
            extra: BTreeMap::from([("etag".to_string(), json!("legacy"))]),
        };
        save_model_options_with_storage(&storage, &payload).expect("seed structured catalog");

        let response = read_model_options_from_storage(&storage).expect("read models");
        assert_eq!(response.models.len(), 2);
        assert_eq!(response.models[0].slug, "gpt-5.4");
        assert_eq!(response.models[1].slug, "gpt-image-2");
        assert_eq!(response.extra.get("etag").and_then(Value::as_str), None);

        let scope = storage
            .get_model_catalog_scope("default")
            .expect("read scope")
            .expect("scope exists");
        assert_eq!(
            serde_json::from_str::<BTreeMap<String, Value>>(&scope.extra_json)
                .expect("parse scope extra")
                .get("etag")
                .and_then(Value::as_str),
            Some("legacy")
        );
        let models = storage
            .list_model_catalog_models("default")
            .expect("list model rows");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name, "GPT-5.4");
        assert_eq!(
            models[0].description.as_deref(),
            Some("Latest frontier model")
        );
        let reasoning_levels = storage
            .list_model_catalog_reasoning_levels("default")
            .expect("list reasoning levels");
        assert_eq!(reasoning_levels.len(), 1);
        assert_eq!(reasoning_levels[0].effort, "medium");
        let plans = storage
            .list_model_catalog_available_in_plans("default")
            .expect("list plans");
        assert_eq!(
            plans
                .iter()
                .map(|item| item.value.as_str())
                .collect::<Vec<_>>(),
            vec!["pro", "team"]
        );
    }

    #[test]
    fn managed_catalog_round_trip_preserves_source_kind_and_user_overrides() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let payload = ManagedModelCatalogResult {
            items: vec![ManagedModelCatalogEntry {
                model: serde_json::from_value(json!({
                    "slug": "gpt-5.4",
                    "display_name": "GPT-5.4 Custom",
                    "description": "customized locally",
                    "supported_in_api": true,
                    "input_modalities": ["text", "image"]
                }))
                .expect("parse managed model"),
                source_kind: MODEL_SOURCE_KIND_CUSTOM.to_string(),
                user_edited: true,
                sort_index: 9,
                updated_at: 1_770_000_123,
            }],
            extra: BTreeMap::from([("etag".to_string(), json!("managed"))]),
        };

        save_managed_model_catalog_with_storage(&storage, &payload)
            .expect("save managed model catalog");

        let response =
            read_managed_model_catalog_from_storage(&storage).expect("read managed model catalog");
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].model.slug, "gpt-5.4");
        assert_eq!(response.items[0].source_kind, MODEL_SOURCE_KIND_CUSTOM);
        assert!(response.items[0].user_edited);
        assert_eq!(response.items[0].sort_index, 9);
        assert_eq!(
            response.extra.get("etag").and_then(Value::as_str),
            Some("managed")
        );
    }

    #[test]
    fn merge_managed_catalog_preserves_user_edited_entries_when_remote_refreshes() {
        let cached = ManagedModelCatalogResult {
            items: vec![ManagedModelCatalogEntry {
                model: serde_json::from_value(json!({
                    "slug": "gpt-5.4",
                    "display_name": "GPT-5.4 Local",
                    "description": "keep local override",
                    "supported_in_api": true
                }))
                .expect("parse cached model"),
                source_kind: MODEL_SOURCE_KIND_REMOTE.to_string(),
                user_edited: true,
                sort_index: 3,
                updated_at: 55,
            }],
            extra: BTreeMap::new(),
        };
        let incoming = ModelsResponse {
            models: vec![serde_json::from_value(json!({
                "slug": "gpt-5.4",
                "display_name": "GPT-5.4 Remote",
                "description": "remote version",
                "supported_in_api": true
            }))
            .expect("parse incoming model")],
            extra: BTreeMap::new(),
        };

        let merged = merge_managed_model_catalog(cached, incoming);
        assert_eq!(merged.items.len(), 1);
        assert_eq!(merged.items[0].model.display_name, "GPT-5.4 Local");
        assert_eq!(
            merged.items[0].model.description.as_deref(),
            Some("keep local override")
        );
        assert_eq!(merged.items[0].source_kind, MODEL_SOURCE_KIND_REMOTE);
        assert!(merged.items[0].user_edited);
    }

    #[test]
    fn account_pool_bootstrap_links_catalog_models_on_first_load() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_account(&storage, "acc-auto");
        seed_platform_catalog(&storage, &["gpt-auto"]);

        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap account routes");

        let source_models = storage
            .list_model_source_models(Some(ROUTING_SOURCE_KIND_OPENAI_ACCOUNT), Some("acc-auto"))
            .expect("list source models");
        assert!(source_models
            .iter()
            .any(|model| model.upstream_model == "gpt-auto"));

        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("gpt-auto")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_OPENAI_ACCOUNT);
        assert_eq!(mappings[0].source_id, "acc-auto");
        assert_eq!(mappings[0].upstream_model, "gpt-auto");
    }

    #[test]
    fn account_pool_bootstrap_links_new_account_after_initial_sync() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_account(&storage, "acc-first");
        seed_platform_catalog(&storage, &["gpt-auto"]);

        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap first account");
        insert_test_account(&storage, "acc-second");
        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap second account");

        let mut source_ids = storage
            .list_enabled_model_source_mappings_for_platform("gpt-auto")
            .expect("list mappings")
            .into_iter()
            .map(|mapping| mapping.source_id)
            .collect::<Vec<_>>();
        source_ids.sort();
        assert_eq!(
            source_ids,
            vec!["acc-first".to_string(), "acc-second".to_string()]
        );
    }

    #[test]
    fn account_pool_bootstrap_fills_missing_mappings_for_existing_source() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_account(&storage, "acc-expand");
        seed_platform_catalog(&storage, &["gpt-old"]);

        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap initial catalog");
        seed_platform_catalog(&storage, &["gpt-old", "gpt-new"]);
        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap expanded catalog");

        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("gpt-new")
            .expect("list new mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_OPENAI_ACCOUNT);
        assert_eq!(mappings[0].source_id, "acc-expand");
        assert_eq!(mappings[0].upstream_model, "gpt-new");
    }

    #[test]
    fn account_pool_bootstrap_prunes_stale_account_source_routes() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        seed_platform_catalog(&storage, &["gpt-stale"]);
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                "acc-stale",
                &["gpt-stale".to_string()],
                "synced",
            )
            .expect("seed stale source model");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-stale".to_string(),
                platform_model_slug: "gpt-stale".to_string(),
                source_kind: ROUTING_SOURCE_KIND_OPENAI_ACCOUNT.to_string(),
                source_id: "acc-stale".to_string(),
                upstream_model: "gpt-stale".to_string(),
                enabled: true,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed stale mapping");

        bootstrap_account_pool_model_routes(&storage, false).expect("bootstrap account routes");

        assert!(storage
            .list_model_source_models(Some(ROUTING_SOURCE_KIND_OPENAI_ACCOUNT), Some("acc-stale"))
            .expect("list source models")
            .is_empty());
        assert!(storage
            .list_model_source_mappings(Some("gpt-stale"))
            .expect("list mappings")
            .is_empty());
    }

    #[test]
    fn account_pool_auto_association_creates_missing_platform_model() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                "acc-source",
                &["vendor-new".to_string()],
                "synced",
            )
            .expect("seed source model");

        auto_associate_source_models(
            &storage,
            ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
            "acc-source",
            true,
        )
        .expect("auto associate");

        let catalog =
            read_managed_model_catalog_from_storage(&storage).expect("read platform catalog");
        assert!(catalog
            .items
            .iter()
            .any(|item| item.model.slug == "vendor-new" && item.model.supported_in_api));

        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-new")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_OPENAI_ACCOUNT);
        assert_eq!(mappings[0].source_id, "acc-source");
    }

    #[test]
    fn aggregate_auto_association_creates_missing_platform_models() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        seed_platform_catalog(&storage, &["gpt-known"]);
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-1",
                &["gpt-known".to_string(), "vendor-only".to_string()],
                "synced",
            )
            .expect("seed aggregate source models");

        auto_associate_source_models(&storage, ROUTING_SOURCE_KIND_AGGREGATE_API, "agg-1", true)
            .expect("auto associate");

        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("gpt-known")
            .expect("list known mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_AGGREGATE_API);
        assert_eq!(mappings[0].source_id, "agg-1");
        assert_eq!(mappings[0].upstream_model, "gpt-known");

        let catalog =
            read_managed_model_catalog_from_storage(&storage).expect("read platform catalog");
        assert!(catalog
            .items
            .iter()
            .any(|item| item.model.slug == "vendor-only" && item.model.supported_in_api));
        let vendor_mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-only")
            .expect("list vendor mappings");
        assert_eq!(vendor_mappings.len(), 1);
        assert_eq!(
            vendor_mappings[0].source_kind,
            ROUTING_SOURCE_KIND_AGGREGATE_API
        );
        assert_eq!(vendor_mappings[0].source_id, "agg-1");
        assert_eq!(vendor_mappings[0].upstream_model, "vendor-only");
    }

    #[test]
    fn aggregate_route_bootstrap_links_existing_source_models() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-bootstrap", "active");
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-bootstrap",
                &["vendor-bootstrap".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");

        bootstrap_aggregate_api_model_routes(&storage).expect("bootstrap aggregate routes");

        let catalog =
            read_managed_model_catalog_from_storage(&storage).expect("read platform catalog");
        assert!(catalog
            .items
            .iter()
            .any(|item| item.model.slug == "vendor-bootstrap"));
        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-bootstrap")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_AGGREGATE_API);
        assert_eq!(mappings[0].source_id, "agg-bootstrap");
    }

    #[test]
    fn managed_model_routing_read_bootstraps_aggregate_sources() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-routing", "active");
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-routing",
                &["vendor-routing".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");

        let result =
            read_managed_model_routing_from_storage(&storage, false).expect("read routing");

        assert!(result
            .source_models
            .iter()
            .any(|item| item.source_kind == ROUTING_SOURCE_KIND_AGGREGATE_API
                && item.source_id == "agg-routing"
                && item.upstream_model == "vendor-routing"));
        assert!(result.mappings.iter().any(|item| item.source_kind
            == ROUTING_SOURCE_KIND_AGGREGATE_API
            && item.source_id == "agg-routing"
            && item.platform_model_slug == "vendor-routing"
            && item.upstream_model == "vendor-routing"
            && item.enabled));
    }

    #[test]
    fn aggregate_source_sync_creates_platform_models_and_mappings() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-sync", "active");

        sync_aggregate_api_source_models_with_discovery(&storage, Some("agg-sync"), |source_id| {
            assert_eq!(source_id, "agg-sync");
            Ok(vec!["vendor-sync".to_string()])
        })
        .expect("sync aggregate models");

        let catalog =
            read_managed_model_catalog_from_storage(&storage).expect("read platform catalog");
        assert!(catalog
            .items
            .iter()
            .any(|item| item.model.slug == "vendor-sync" && item.model.supported_in_api));
        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-sync")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_AGGREGATE_API);
        assert_eq!(mappings[0].source_id, "agg-sync");
    }

    #[test]
    fn aggregate_supplier_import_association_creates_platform_route() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-template", "active");
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-template",
                &["vendor-template".to_string()],
                "template",
            )
            .expect("seed template source model");

        auto_associate_aggregate_api_source_models(&storage, "agg-template")
            .expect("associate template import");

        let catalog =
            read_managed_model_catalog_from_storage(&storage).expect("read platform catalog");
        assert!(catalog
            .items
            .iter()
            .any(|item| item.model.slug == "vendor-template"));
        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-template")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, ROUTING_SOURCE_KIND_AGGREGATE_API);
        assert_eq!(mappings[0].source_id, "agg-template");
    }

    #[test]
    fn aggregate_bootstrap_preserves_disabled_mapping_state() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-disabled-mapping", "active");
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-disabled-mapping",
                &["vendor-disabled".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-aggregate-disabled".to_string(),
                platform_model_slug: "vendor-disabled".to_string(),
                source_kind: ROUTING_SOURCE_KIND_AGGREGATE_API.to_string(),
                source_id: "agg-disabled-mapping".to_string(),
                upstream_model: "vendor-disabled".to_string(),
                enabled: false,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed disabled mapping");

        bootstrap_aggregate_api_model_routes(&storage).expect("bootstrap aggregate routes");

        let mappings = storage
            .list_model_source_mappings(Some("vendor-disabled"))
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].id, "mapping-aggregate-disabled");
        assert!(!mappings[0].enabled);
    }

    #[test]
    fn aggregate_bootstrap_prunes_stale_source_routes() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-stale", "disabled");
        seed_platform_catalog(&storage, &["vendor-stale"]);
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_AGGREGATE_API,
                "agg-stale",
                &["vendor-stale".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-aggregate-stale".to_string(),
                platform_model_slug: "vendor-stale".to_string(),
                source_kind: ROUTING_SOURCE_KIND_AGGREGATE_API.to_string(),
                source_id: "agg-stale".to_string(),
                upstream_model: "vendor-stale".to_string(),
                enabled: true,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed stale mapping");

        bootstrap_aggregate_api_model_routes(&storage).expect("bootstrap aggregate routes");

        assert!(storage
            .list_model_source_models(Some(ROUTING_SOURCE_KIND_AGGREGATE_API), Some("agg-stale"))
            .expect("list source models")
            .is_empty());
        assert!(storage
            .list_model_source_mappings(Some("vendor-stale"))
            .expect("list mappings")
            .is_empty());
    }

    #[test]
    fn aggregate_source_sync_rejects_disabled_api() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_aggregate_api(&codexmanager_core::storage::AggregateApi {
                id: "agg-disabled".to_string(),
                provider_type: "codex".to_string(),
                supplier_name: Some("disabled".to_string()),
                sort: 0,
                url: "https://disabled.example/v1".to_string(),
                auth_type: "apikey".to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                status: "disabled".to_string(),
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
            })
            .expect("insert disabled aggregate api");

        let err = sync_aggregate_api_source_models(&storage, Some("agg-disabled"))
            .expect_err("disabled aggregate api should not sync");
        assert!(err.contains("agg-disabled"));
        assert!(err.contains("disabled"));
    }

    #[test]
    fn auto_association_preserves_existing_source_mapping_state() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        seed_platform_catalog(&storage, &["gpt-disabled"]);
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                "acc-disabled",
                &["gpt-disabled".to_string()],
                "synced",
            )
            .expect("seed source model");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-disabled".to_string(),
                platform_model_slug: "gpt-disabled".to_string(),
                source_kind: ROUTING_SOURCE_KIND_OPENAI_ACCOUNT.to_string(),
                source_id: "acc-disabled".to_string(),
                upstream_model: "gpt-disabled".to_string(),
                enabled: false,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed disabled mapping");

        auto_associate_source_models(
            &storage,
            ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
            "acc-disabled",
            true,
        )
        .expect("auto associate");

        let mappings = storage
            .list_model_source_mappings(Some("gpt-disabled"))
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert!(!mappings[0].enabled);
        assert_eq!(mappings[0].id, "mapping-disabled");
    }

    #[test]
    fn auto_association_preserves_existing_platform_mapping_override() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        seed_platform_catalog(&storage, &["gpt-platform"]);
        storage
            .upsert_discovered_model_source_models(
                ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
                "acc-override",
                &["gpt-upstream".to_string(), "gpt-platform".to_string()],
                "synced",
            )
            .expect("seed source models");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-override".to_string(),
                platform_model_slug: "gpt-platform".to_string(),
                source_kind: ROUTING_SOURCE_KIND_OPENAI_ACCOUNT.to_string(),
                source_id: "acc-override".to_string(),
                upstream_model: "gpt-upstream".to_string(),
                enabled: true,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed mapping override");

        auto_associate_source_models(
            &storage,
            ROUTING_SOURCE_KIND_OPENAI_ACCOUNT,
            "acc-override",
            true,
        )
        .expect("auto associate");

        let mappings = storage
            .list_model_source_mappings(Some("gpt-platform"))
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].id, "mapping-override");
        assert_eq!(mappings[0].upstream_model, "gpt-upstream");
    }
}
