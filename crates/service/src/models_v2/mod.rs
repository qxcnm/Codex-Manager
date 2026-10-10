pub(crate) mod fast_policy;
mod import;
pub(crate) mod instructions;
mod pricing_sync;
mod seaorm;

use codexmanager_core::rpc::types::{
    ModelInfo, ModelReasoningLevel, ModelServiceTier, ModelTruncationPolicy, ModelsResponse,
};
use codexmanager_core::storage::{
    ManagedModelBatchStateV2Update, ManagedModelStateV2Update, ManagedModelV2,
    ManagedModelV2Upsert, ModelCatalogV2Stats,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) use import::{
    commit_import, preview_import, ManagedModelImportCommitV2Params,
    ManagedModelImportPreviewV2Params, ManagedModelImportPreviewV2Result,
};
pub(crate) use pricing_sync::{sync_prices, ManagedModelPriceSyncV2Params};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelListV2Result {
    pub items: Vec<ManagedModelV2>,
    pub stats: ModelCatalogV2Stats,
}

pub(crate) fn list(include_hidden: bool) -> Result<ManagedModelListV2Result, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return seaorm::list(include_hidden);
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    list_with_storage(&storage, include_hidden)
}

pub(crate) fn list_with_storage(
    storage: &codexmanager_core::storage::Storage,
    include_hidden: bool,
) -> Result<ManagedModelListV2Result, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return seaorm::list(include_hidden);
    }
    Ok(ManagedModelListV2Result {
        items: storage
            .list_managed_models_v2(include_hidden)
            .map_err(|err| format!("list managed models V2 failed: {err}"))?,
        stats: storage
            .model_catalog_v2_stats()
            .map_err(|err| format!("read model catalog V2 stats failed: {err}"))?,
    })
}

pub(crate) fn get(slug: &str) -> Result<ManagedModelV2, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return seaorm::get(slug)?.ok_or_else(|| "model_not_found".into());
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .get_managed_model_v2(slug)
        .map_err(|err| format!("read managed model V2 failed: {err}"))?
        .ok_or_else(|| "model_not_found".to_string())
}

pub(crate) fn upsert(input: ManagedModelV2Upsert) -> Result<ManagedModelV2, String> {
    let changes = selection_changes_for_upserts(std::slice::from_ref(&input));
    if crate::storage_helpers::seaorm_enabled() {
        let model = seaorm::upsert_many(vec![input])?
            .pop()
            .ok_or_else(|| "model_not_found".to_string())?;
        sync_active_gateway_catalog_for_current_backend_best_effort(changes);
        return Ok(model);
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let model = storage
        .upsert_managed_model_v2(&input)
        .map_err(|err| format!("save managed model V2 failed: {err}"))?;
    sync_active_gateway_catalog_after_model_changes_best_effort(&storage, changes);
    Ok(model)
}

pub(crate) fn update_state(input: ManagedModelStateV2Update) -> Result<ManagedModelV2, String> {
    let changes = selection_changes_for_state(&input);
    if crate::storage_helpers::seaorm_enabled() {
        let model = seaorm::update_states(ManagedModelBatchStateV2Update {
            slugs: vec![input.slug],
            enabled: input.enabled,
            visibility: input.visibility,
        })?
        .pop()
        .ok_or_else(|| "model_not_found".to_string())?;
        sync_active_gateway_catalog_for_current_backend_best_effort(changes);
        return Ok(model);
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let model = storage
        .update_managed_model_state_v2(&input)
        .map_err(|err| format!("update managed model V2 state failed: {err}"))?;
    sync_active_gateway_catalog_after_model_changes_best_effort(&storage, changes);
    Ok(model)
}

pub(crate) fn batch_update_state(
    input: ManagedModelBatchStateV2Update,
) -> Result<Vec<ManagedModelV2>, String> {
    let changes = selection_changes_for_batch_state(&input);
    if crate::storage_helpers::seaorm_enabled() {
        let models = seaorm::update_states(input)?;
        sync_active_gateway_catalog_for_current_backend_best_effort(changes);
        return Ok(models);
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let models = storage
        .update_managed_models_state_v2(&input)
        .map_err(|err| format!("batch update managed model V2 state failed: {err}"))?;
    sync_active_gateway_catalog_after_model_changes_best_effort(&storage, changes);
    Ok(models)
}

pub(crate) fn delete(slug: &str) -> Result<(), String> {
    let changes = vec![crate::codex_profile::ManagedModelSelectionChange::remove(
        slug,
    )];
    if crate::storage_helpers::seaorm_enabled() {
        seaorm::delete(slug)?;
        sync_active_gateway_catalog_for_current_backend_best_effort(changes);
        return Ok(());
    }
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_managed_model_v2(slug)
        .map_err(|err| format!("delete managed model V2 failed: {err}"))?;
    sync_active_gateway_catalog_after_model_changes_best_effort(&storage, changes);
    Ok(())
}

// Kept as a compatibility shim for callers that do not have a change list yet.
#[allow(dead_code)]
pub(super) fn sync_active_gateway_catalog_best_effort(
    storage: &codexmanager_core::storage::Storage,
) {
    sync_active_gateway_catalog_after_model_changes_best_effort(storage, Vec::new());
}

pub(super) fn sync_active_gateway_catalog_after_model_changes_best_effort(
    storage: &codexmanager_core::storage::Storage,
    changes: Vec<crate::codex_profile::ManagedModelSelectionChange>,
) {
    if let Err(err) =
        crate::codex_profile::sync_active_gateway_profile_after_model_changes(storage, changes)
    {
        log::warn!("event=sync_active_gateway_profile_failed error={err}");
    }
}

pub(super) fn sync_active_gateway_catalog_for_current_backend_best_effort(
    changes: Vec<crate::codex_profile::ManagedModelSelectionChange>,
) {
    let Some(storage) = crate::storage_helpers::open_storage() else {
        log::warn!("event=sync_active_gateway_profile_failed error=storage unavailable");
        return;
    };
    sync_active_gateway_catalog_after_model_changes_best_effort(&storage, changes);
}

pub(super) fn selection_changes_for_upserts(
    inputs: &[ManagedModelV2Upsert],
) -> Vec<crate::codex_profile::ManagedModelSelectionChange> {
    inputs
        .iter()
        .filter_map(|input| {
            let previous_slug = input
                .previous_slug
                .as_deref()
                .unwrap_or(input.model.slug.as_str());
            if input.model.visibility.eq_ignore_ascii_case("hide") {
                return Some(crate::codex_profile::ManagedModelSelectionChange::remove(
                    previous_slug,
                ));
            }
            (!previous_slug.eq_ignore_ascii_case(&input.model.slug)).then(|| {
                crate::codex_profile::ManagedModelSelectionChange::rename(
                    previous_slug,
                    input.model.slug.clone(),
                )
            })
        })
        .collect()
}

fn selection_changes_for_state(
    input: &ManagedModelStateV2Update,
) -> Vec<crate::codex_profile::ManagedModelSelectionChange> {
    if input.visibility.eq_ignore_ascii_case("hide") {
        vec![crate::codex_profile::ManagedModelSelectionChange::remove(
            input.slug.clone(),
        )]
    } else {
        Vec::new()
    }
}

fn selection_changes_for_batch_state(
    input: &ManagedModelBatchStateV2Update,
) -> Vec<crate::codex_profile::ManagedModelSelectionChange> {
    if input.visibility.eq_ignore_ascii_case("hide") {
        input
            .slugs
            .iter()
            .cloned()
            .map(crate::codex_profile::ManagedModelSelectionChange::remove)
            .collect()
    } else {
        Vec::new()
    }
}

fn capability<'a>(model: &'a ManagedModelV2, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| model.capabilities.get(*key))
}

pub(crate) fn managed_model(
    storage: &codexmanager_core::storage::Storage,
    slug: &str,
) -> rusqlite::Result<Option<ManagedModelV2>> {
    if crate::storage_helpers::seaorm_enabled() {
        return seaorm::get(slug).map_err(|e| rusqlite::Error::SqliteFailure((), Some(e)));
    }
    storage.get_managed_model_v2(slug)
}

pub(crate) fn enabled_model(
    storage: &codexmanager_core::storage::Storage,
    slug: &str,
) -> rusqlite::Result<Option<ManagedModelV2>> {
    managed_model(storage, slug).map(|model| model.filter(|m| m.enabled && m.supported_in_api))
}

pub(crate) fn api_models(
    storage: &codexmanager_core::storage::Storage,
) -> rusqlite::Result<Vec<ManagedModelV2>> {
    if crate::storage_helpers::seaorm_enabled() {
        return seaorm::list(false)
            .map(|r| {
                r.items
                    .into_iter()
                    .filter(|m| m.enabled && m.supported_in_api)
                    .collect()
            })
            .map_err(|e| rusqlite::Error::SqliteFailure((), Some(e)));
    }
    storage.list_api_models_v2()
}

pub(crate) fn policy_catalog_slug(model_slug: &str) -> &str {
    let model_slug = model_slug.trim();
    if codexmanager_core::usage::is_luna_reserve_model(Some(model_slug)) {
        codexmanager_core::usage::LUNA_MODEL_SLUG
    } else {
        model_slug
    }
}

pub(crate) fn request_exceeds_model_ceiling(
    storage: &codexmanager_core::storage::Storage,
    request_model: Option<&str>,
    configured_ceiling: &str,
) -> rusqlite::Result<bool> {
    let ceiling = configured_ceiling.trim();
    let Some(request_model) = request_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    if ceiling.is_empty() || ceiling.eq_ignore_ascii_case("auto") {
        return Ok(false);
    }

    let ceiling_catalog_slug = policy_catalog_slug(ceiling);
    let request_catalog_slug = policy_catalog_slug(request_model);
    if ceiling_catalog_slug.eq_ignore_ascii_case(request_catalog_slug) {
        return Ok(false);
    }

    let ceiling_model = enabled_model(storage, ceiling_catalog_slug)?;
    let request_model = enabled_model(storage, request_catalog_slug)?;

    // The catalog is ordered from highest to lowest priority. Unknown models
    // cannot be proven to stay within a concrete ceiling, so fail closed.
    Ok(match (request_model, ceiling_model) {
        (Some(request_model), Some(ceiling_model)) => {
            request_model.sort_order < ceiling_model.sort_order
        }
        _ => true,
    })
}

pub(crate) fn should_preserve_luna_reserve_alias(
    request_model: Option<&str>,
    configured_model: Option<&str>,
) -> bool {
    if !codexmanager_core::usage::is_luna_reserve_model(request_model) {
        return false;
    }
    configured_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .is_none_or(|model| {
            codexmanager_core::usage::is_luna_catalog_model(Some(model))
                || codexmanager_core::usage::is_luna_reserve_model(Some(model))
        })
}

pub(crate) fn supports_text_generation(model: &ManagedModelV2) -> bool {
    capability(
        model,
        &["supports_text_generation", "supportsTextGeneration"],
    )
    .and_then(Value::as_bool)
    .unwrap_or(true)
}

pub(crate) fn supports_image_generation(model: &ManagedModelV2) -> bool {
    capability(
        model,
        &["supports_image_generation", "supportsImageGeneration"],
    )
    .and_then(Value::as_bool)
    .unwrap_or(false)
}

pub(crate) fn ensure_text_generation_model(
    storage: &codexmanager_core::storage::Storage,
    slug: Option<&str>,
) -> Result<(), String> {
    let Some(slug) = slug.map(str::trim).filter(|slug| !slug.is_empty()) else {
        return Ok(());
    };
    let Some(model) = managed_model(storage, policy_catalog_slug(slug))
        .map_err(|err| format!("read managed model V2 failed: {err}"))?
    else {
        // Preserve existing behavior for external or not-yet-cataloged model slugs.
        return Ok(());
    };
    if supports_text_generation(&model) {
        return Ok(());
    }
    Err(format!(
        "图片专用模型不能作为文本主模型(image-only model cannot be used as a text-generation primary model): {}",
        model.slug
    ))
}

fn service_tier_display_name(id: &str) -> &str {
    if id.eq_ignore_ascii_case("priority") {
        "Fast"
    } else if id.eq_ignore_ascii_case("ultrafast") {
        "Ultrafast"
    } else if id.eq_ignore_ascii_case("flex") {
        "Flex"
    } else {
        id
    }
}

fn service_tier_description(model_slug: &str, id: &str) -> &'static str {
    if id.eq_ignore_ascii_case("priority") {
        if model_slug.eq_ignore_ascii_case("gpt-6-astra") {
            "2x speed, increased usage"
        } else if [
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.4",
            "gpt-5.5",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
        ]
        .iter()
        .any(|known_slug| model_slug.eq_ignore_ascii_case(known_slug))
        {
            "1.5x speed, increased usage"
        } else {
            ""
        }
    } else if id.eq_ignore_ascii_case("ultrafast") {
        "The fastest available responses for latency-sensitive work."
    } else {
        ""
    }
}

pub(crate) fn model_info(model: &ManagedModelV2) -> ModelInfo {
    let string_list = |keys: &[&str]| {
        capability(model, keys)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let supported_reasoning_levels = string_list(&["reasoning_efforts", "reasoningEfforts"])
        .into_iter()
        .map(|effort| ModelReasoningLevel {
            effort,
            description: String::new(),
            ..Default::default()
        })
        .collect();
    let additional_speed_tiers = string_list(&["additional_speed_tiers", "additionalSpeedTiers"]);
    let service_tiers = string_list(&["service_tiers", "serviceTiers"])
        .into_iter()
        .map(|id| ModelServiceTier {
            name: service_tier_display_name(&id).to_string(),
            description: service_tier_description(model.slug.as_str(), &id).to_string(),
            id,
            ..Default::default()
        })
        .collect();
    let default_service_tier = capability(model, &["default_service_tier", "defaultServiceTier"])
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let truncation_policy = capability(model, &["truncation_mode", "truncationMode"])
        .and_then(Value::as_str)
        .zip(capability(model, &["truncation_limit", "truncationLimit"]).and_then(Value::as_i64))
        .map(|(mode, limit)| ModelTruncationPolicy {
            mode: mode.to_string(),
            limit,
            ..Default::default()
        });
    let output_modalities = string_list(&["output_modalities", "outputModalities"]);
    let supported_endpoints = string_list(&["supported_endpoints", "supportedEndpoints"]);
    let experimental_supported_tools =
        string_list(&["experimental_supported_tools", "experimentalSupportedTools"]);
    let available_in_plans = string_list(&["available_in_plans", "availableInPlans"]);
    let extra = std::collections::BTreeMap::from([
        (
            "output_modalities".to_string(),
            serde_json::json!(output_modalities),
        ),
        (
            "supported_endpoints".to_string(),
            serde_json::json!(supported_endpoints),
        ),
        (
            "supports_text_generation".to_string(),
            serde_json::json!(supports_text_generation(model)),
        ),
        (
            "supports_image_generation".to_string(),
            serde_json::json!(supports_image_generation(model)),
        ),
        (
            "supports_image_editing".to_string(),
            capability(model, &["supports_image_editing", "supportsImageEditing"])
                .and_then(Value::as_bool)
                .map(Value::Bool)
                .unwrap_or(Value::Bool(false)),
        ),
        (
            "supports_transparent_background".to_string(),
            capability(
                model,
                &[
                    "supports_transparent_background",
                    "supportsTransparentBackground",
                ],
            )
            .and_then(Value::as_bool)
            .map(Value::Bool)
            .unwrap_or(Value::Bool(false)),
        ),
        (
            "api_context_window".to_string(),
            capability(model, &["api_context_window", "apiContextWindow"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "max_output_tokens".to_string(),
            capability(model, &["max_output_tokens", "maxOutputTokens"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "prefer_websockets".to_string(),
            capability(model, &["prefer_websockets", "preferWebsockets"])
                .and_then(Value::as_bool)
                .map(Value::Bool)
                .unwrap_or(Value::Bool(false)),
        ),
        (
            "reasoning_summary_format".to_string(),
            capability(
                model,
                &["reasoning_summary_format", "reasoningSummaryFormat"],
            )
            .cloned()
            .unwrap_or(Value::Null),
        ),
        (
            "multi_agent_reasoning_effort".to_string(),
            capability(
                model,
                &["multi_agent_reasoning_effort", "multiAgentReasoningEffort"],
            )
            .cloned()
            .unwrap_or(Value::Null),
        ),
        (
            "quality_settings".to_string(),
            capability(model, &["quality_settings", "qualitySettings"])
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        ),
        (
            "snapshot".to_string(),
            capability(model, &["snapshot"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "auto_review_model_override".to_string(),
            capability(
                model,
                &["auto_review_model_override", "autoReviewModelOverride"],
            )
            .cloned()
            .unwrap_or(Value::Null),
        ),
        (
            "max_context_window".to_string(),
            serde_json::json!(model
                .max_context_window
                .or(model.context_window)
                .unwrap_or(200_000)),
        ),
        (
            "comp_hash".to_string(),
            capability(model, &["comp_hash", "compHash"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "tool_mode".to_string(),
            capability(model, &["tool_mode", "toolMode"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "multi_agent_version".to_string(),
            capability(model, &["multi_agent_version", "multiAgentVersion"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "use_responses_lite".to_string(),
            capability(model, &["use_responses_lite", "useResponsesLite"])
                .and_then(Value::as_bool)
                .map(Value::Bool)
                .unwrap_or(Value::Bool(false)),
        ),
        (
            "include_skills_usage_instructions".to_string(),
            capability(
                model,
                &[
                    "include_skills_usage_instructions",
                    "includeSkillsUsageInstructions",
                ],
            )
            .and_then(Value::as_bool)
            .map(Value::Bool)
            .unwrap_or(Value::Bool(false)),
        ),
    ]);
    ModelInfo {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        default_reasoning_level: model.default_reasoning_effort.clone(),
        supported_reasoning_levels,
        shell_type: capability(model, &["shell_type", "shellType"])
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| supports_text_generation(model).then(|| "shell_command".to_string())),
        visibility: Some(model.visibility.clone()),
        supported_in_api: model.supported_in_api,
        priority: model.sort_order,
        additional_speed_tiers,
        service_tiers,
        default_service_tier,
        availability_nux: Some(
            capability(model, &["availability_nux", "availabilityNux"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        upgrade: Some(
            capability(model, &["upgrade"])
                .cloned()
                .unwrap_or(Value::Null),
        ),
        base_instructions: Some(String::new()),
        model_messages: Some(serde_json::json!({
            "instructions_template": "",
            "instructions_variables": null,
            "approvals": null,
        })),
        supports_reasoning_summaries: capability(
            model,
            &[
                "supports_reasoning_summary_parameter",
                "supports_reasoning_summaries",
                "supportsReasoningSummaries",
            ],
        )
        .and_then(Value::as_bool),
        default_reasoning_summary: capability(
            model,
            &["default_reasoning_summary", "defaultReasoningSummary"],
        )
        .and_then(Value::as_str)
        .map(str::to_string),
        support_verbosity: capability(model, &["supports_verbosity", "supportsVerbosity"])
            .and_then(Value::as_bool),
        default_verbosity: capability(model, &["default_verbosity", "defaultVerbosity"]).cloned(),
        apply_patch_tool_type: capability(model, &["apply_patch_tool_type", "applyPatchToolType"])
            .and_then(Value::as_str)
            .map(str::to_string),
        web_search_tool_type: capability(model, &["web_search_tool_type", "webSearchToolType"])
            .and_then(Value::as_str)
            .map(str::to_string),
        truncation_policy,
        supports_parallel_tool_calls: capability(
            model,
            &["supports_parallel_tool_calls", "supportsParallelToolCalls"],
        )
        .and_then(Value::as_bool),
        supports_image_detail_original: capability(
            model,
            &[
                "supports_image_detail_original",
                "supportsImageDetailOriginal",
            ],
        )
        .and_then(Value::as_bool),
        context_window: model.context_window,
        auto_compact_token_limit: capability(
            model,
            &["auto_compact_token_limit", "autoCompactTokenLimit"],
        )
        .and_then(Value::as_i64),
        effective_context_window_percent: capability(
            model,
            &[
                "effective_context_window_percent",
                "effectiveContextWindowPercent",
            ],
        )
        .and_then(Value::as_i64)
        .or(Some(95)),
        experimental_supported_tools,
        input_modalities: string_list(&["input_modalities", "inputModalities"]),
        minimal_client_version: capability(
            model,
            &["minimal_client_version", "minimalClientVersion"],
        )
        .cloned(),
        supports_search_tool: capability(model, &["supports_search_tool", "supportsSearchTool"])
            .and_then(Value::as_bool),
        available_in_plans,
        extra,
        ..Default::default()
    }
}

pub(crate) fn models_response_with_storage(
    storage: &codexmanager_core::storage::Storage,
) -> Result<ModelsResponse, String> {
    Ok(ModelsResponse {
        models: api_models(storage)
            .map_err(|err| format!("list API models V2 failed: {err}"))?
            .iter()
            .map(model_info)
            .collect(),
        extra: Default::default(),
    })
}

pub(crate) fn text_generation_models_response_with_storage(
    storage: &codexmanager_core::storage::Storage,
) -> Result<ModelsResponse, String> {
    Ok(ModelsResponse {
        models: api_models(storage)
            .map_err(|err| format!("list API models V2 failed: {err}"))?
            .iter()
            .filter(|model| supports_text_generation(model))
            .map(model_info)
            .collect(),
        extra: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::Storage;

    #[test]
    fn sol61_is_available_in_managed_and_codex_text_catalogs() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        for catalog in [
            models_response_with_storage(&storage).unwrap(),
            text_generation_models_response_with_storage(&storage).unwrap(),
        ] {
            let sol = catalog
                .models
                .iter()
                .find(|m| m.slug == "gpt-6.1-sol")
                .unwrap();
            assert_eq!(sol.default_reasoning_level.as_deref(), Some("medium"));
            assert_eq!(
                sol.supported_reasoning_levels
                    .iter()
                    .map(|level| level.effort.as_str())
                    .collect::<Vec<_>>(),
                ["low", "medium", "high", "xhigh", "max"]
            );
            assert_eq!(sol.input_modalities, ["text", "image"]);
            assert_eq!(sol.extra["api_context_window"], 1_050_000);
            assert_eq!(sol.extra["max_output_tokens"], 128_000);
            assert_eq!(sol.service_tiers.len(), 1);
            assert_eq!(sol.service_tiers[0].id, "priority");
        }
    }

    #[test]
    fn policy_catalog_slug_normalizes_reserve_alias_and_whitespace() {
        assert_eq!(policy_catalog_slug(" GPT-RESERVE "), "gpt-6-luna");
        assert_eq!(policy_catalog_slug(" gpt-5.4 "), "gpt-5.4");
    }

    #[test]
    fn image_model_is_exposed_with_capabilities_but_excluded_from_text_catalog() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let all = models_response_with_storage(&storage).expect("full models response");
        let text_model = all
            .models
            .iter()
            .find(|model| model.slug == "gpt-6-sol")
            .expect("text model");
        assert_eq!(text_model.shell_type.as_deref(), Some("shell_command"));
        assert_eq!(text_model.base_instructions.as_deref(), Some(""));
        assert_eq!(text_model.effective_context_window_percent, Some(95));
        assert_eq!(text_model.extra["max_context_window"], 872_000);
        assert_eq!(text_model.extra["comp_hash"], "3000");
        assert_eq!(text_model.extra["tool_mode"], "code_mode_only");
        assert_eq!(text_model.extra["multi_agent_version"], "v2");
        assert_eq!(text_model.extra["api_context_window"], 1_050_000);
        assert_eq!(text_model.extra["max_output_tokens"], 128_000);
        assert_eq!(text_model.extra["prefer_websockets"], true);
        assert_eq!(text_model.extra["reasoning_summary_format"], "experimental");
        assert_eq!(
            text_model.minimal_client_version,
            Some(serde_json::json!("0.155.0"))
        );
        assert_eq!(text_model.extra["use_responses_lite"], true);
        assert_eq!(text_model.extra["include_skills_usage_instructions"], false);
        for slug in [
            "gpt-image-2",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
        ] {
            let image = all
                .models
                .iter()
                .find(|model| model.slug == slug)
                .unwrap_or_else(|| panic!("image model {slug}"));
            assert_eq!(image.input_modalities, ["text", "image"]);
            assert_eq!(
                image.extra["output_modalities"],
                serde_json::json!(["image"])
            );
            assert_eq!(
                image.extra["supported_endpoints"],
                serde_json::json!(["/v1/images/generations", "/v1/images/edits"])
            );
            assert_eq!(image.extra["supports_text_generation"], false);
            assert_eq!(image.extra["supports_image_generation"], true);
            assert_eq!(image.extra["supports_image_editing"], true);
            assert!(image.extra["snapshot"].as_str().is_some());
        }
        for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
            let image = all
                .models
                .iter()
                .find(|model| model.slug == slug)
                .unwrap_or_else(|| panic!("image model {slug}"));
            assert_eq!(
                image.extra["quality_settings"],
                serde_json::json!(["low", "medium", "high", "xhigh", "max", "auto"])
            );
            assert_eq!(image.extra["supports_transparent_background"], true);
        }

        let text = text_generation_models_response_with_storage(&storage)
            .expect("text generation models response");
        for slug in [
            "gpt-image-2",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
        ] {
            assert!(!text.models.iter().any(|model| model.slug == slug));
        }
        assert_eq!(text.models.len() + 3, all.models.len());
    }

    #[test]
    fn model_info_exposes_fast_service_tier_for_codex_clients() {
        let model = ManagedModelV2 {
            slug: "gpt-6-sol".to_string(),
            display_name: "Fast Model".to_string(),
            capabilities: serde_json::json!({
                "service_tiers": ["priority", "ultrafast", "flex"],
                "additional_speed_tiers": ["fast"],
                "default_service_tier": "priority"
            }),
            ..Default::default()
        };

        let info = model_info(&model);
        assert_eq!(info.additional_speed_tiers, ["fast"]);
        assert_eq!(info.default_service_tier.as_deref(), Some("priority"));
        assert_eq!(info.service_tiers.len(), 3);
        assert_eq!(info.service_tiers[0].id, "priority");
        assert_eq!(info.service_tiers[0].name, "Fast");
        assert_eq!(
            info.service_tiers[0].description,
            "1.5x speed, increased usage"
        );
        assert_eq!(info.service_tiers[1].id, "ultrafast");
        assert_eq!(info.service_tiers[1].name, "Ultrafast");
        assert_eq!(
            info.service_tiers[1].description,
            "The fastest available responses for latency-sensitive work."
        );
        assert_eq!(info.service_tiers[2].id, "flex");
        assert_eq!(info.service_tiers[2].name, "Flex");
    }

    #[test]
    fn astra_fast_service_tier_uses_catalog_usage_copy() {
        let model = ManagedModelV2 {
            slug: "gpt-6-astra".to_string(),
            capabilities: serde_json::json!({ "service_tiers": ["priority"] }),
            ..Default::default()
        };

        let info = model_info(&model);
        assert_eq!(
            info.service_tiers[0].description,
            "2x speed, increased usage"
        );
    }

    #[test]
    fn custom_model_fast_service_tier_does_not_invent_a_speed_claim() {
        let model = ManagedModelV2 {
            slug: "custom-fast-model".to_string(),
            capabilities: serde_json::json!({ "service_tiers": ["priority"] }),
            ..Default::default()
        };

        let info = model_info(&model);
        assert_eq!(info.service_tiers[0].description, "");
    }

    #[test]
    fn text_model_without_shell_capability_uses_codex_compatible_default() {
        let model = ManagedModelV2 {
            slug: "custom-text-model".to_string(),
            display_name: "Custom Text Model".to_string(),
            capabilities: serde_json::json!({}),
            ..Default::default()
        };

        assert_eq!(
            model_info(&model).shell_type.as_deref(),
            Some("shell_command")
        );
        let info = model_info(&model);
        assert_eq!(info.base_instructions.as_deref(), Some(""));
        assert_eq!(info.effective_context_window_percent, Some(95));
        assert_eq!(info.extra["max_context_window"], 200_000);
        assert_eq!(info.extra["comp_hash"], Value::Null);
        assert_eq!(info.extra["use_responses_lite"], false);
    }

    #[test]
    fn text_generation_validation_rejects_known_image_model_only() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        assert!(ensure_text_generation_model(&storage, Some("gpt-5.4")).is_ok());
        assert!(ensure_text_generation_model(&storage, Some("external-model")).is_ok());
        let error = ensure_text_generation_model(&storage, Some("gpt-image-2"))
            .expect_err("image model must be rejected");
        assert!(error.contains("image-only model"));
    }
}
