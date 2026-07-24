mod import;
pub(crate) mod instructions;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelListV2Result {
    pub items: Vec<ManagedModelV2>,
    pub stats: ModelCatalogV2Stats,
}

pub(crate) fn list(include_hidden: bool) -> Result<ManagedModelListV2Result, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    list_with_storage(&storage, include_hidden)
}

pub(crate) fn list_with_storage(
    storage: &codexmanager_core::storage::Storage,
    include_hidden: bool,
) -> Result<ManagedModelListV2Result, String> {
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
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .get_managed_model_v2(slug)
        .map_err(|err| format!("read managed model V2 failed: {err}"))?
        .ok_or_else(|| "model_not_found".to_string())
}

pub(crate) fn upsert(input: ManagedModelV2Upsert) -> Result<ManagedModelV2, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let model = storage
        .upsert_managed_model_v2(&input)
        .map_err(|err| format!("save managed model V2 failed: {err}"))?;
    sync_active_gateway_catalog_best_effort(&storage);
    Ok(model)
}

pub(crate) fn update_state(input: ManagedModelStateV2Update) -> Result<ManagedModelV2, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let model = storage
        .update_managed_model_state_v2(&input)
        .map_err(|err| format!("update managed model V2 state failed: {err}"))?;
    sync_active_gateway_catalog_best_effort(&storage);
    Ok(model)
}

pub(crate) fn batch_update_state(
    input: ManagedModelBatchStateV2Update,
) -> Result<Vec<ManagedModelV2>, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let models = storage
        .update_managed_models_state_v2(&input)
        .map_err(|err| format!("batch update managed model V2 state failed: {err}"))?;
    sync_active_gateway_catalog_best_effort(&storage);
    Ok(models)
}

pub(crate) fn delete(slug: &str) -> Result<(), String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_managed_model_v2(slug)
        .map_err(|err| format!("delete managed model V2 failed: {err}"))?;
    sync_active_gateway_catalog_best_effort(&storage);
    Ok(())
}

pub(super) fn sync_active_gateway_catalog_best_effort(
    storage: &codexmanager_core::storage::Storage,
) {
    if let Err(err) = crate::codex_profile::sync_active_gateway_model_catalog_from_storage(storage)
    {
        log::warn!("event=sync_active_gateway_model_catalog_failed error={err}");
    }
}

fn capability<'a>(model: &'a ManagedModelV2, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| model.capabilities.get(*key))
}

pub(crate) fn supports_text_generation(model: &ManagedModelV2) -> bool {
    capability(
        model,
        &["supports_text_generation", "supportsTextGeneration"],
    )
    .and_then(Value::as_bool)
    .unwrap_or(true)
}

pub(crate) fn ensure_text_generation_model(
    storage: &codexmanager_core::storage::Storage,
    slug: Option<&str>,
) -> Result<(), String> {
    let Some(slug) = slug.map(str::trim).filter(|slug| !slug.is_empty()) else {
        return Ok(());
    };
    let Some(model) = storage
        .get_managed_model_v2(slug)
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
    let service_tiers = string_list(&["service_tiers", "serviceTiers"])
        .into_iter()
        .map(|id| ModelServiceTier {
            name: id.clone(),
            id,
            ..Default::default()
        })
        .collect();
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
        service_tiers,
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
            &["supports_reasoning_summaries", "supportsReasoningSummaries"],
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
        effective_context_window_percent: capability(
            model,
            &[
                "effective_context_window_percent",
                "effectiveContextWindowPercent",
            ],
        )
        .and_then(Value::as_i64)
        .or(Some(95)),
        input_modalities: string_list(&["input_modalities", "inputModalities"]),
        supports_search_tool: capability(model, &["supports_search_tool", "supportsSearchTool"])
            .and_then(Value::as_bool),
        extra,
        ..Default::default()
    }
}

pub(crate) fn models_response_with_storage(
    storage: &codexmanager_core::storage::Storage,
) -> Result<ModelsResponse, String> {
    Ok(ModelsResponse {
        models: storage
            .list_api_models_v2()
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
        models: storage
            .list_api_models_v2()
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
    fn image_model_is_exposed_with_capabilities_but_excluded_from_text_catalog() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let all = models_response_with_storage(&storage).expect("full models response");
        let text_model = all
            .models
            .iter()
            .find(|model| model.slug == "gpt-5.6-sol")
            .expect("text model");
        assert_eq!(text_model.shell_type.as_deref(), Some("shell_command"));
        assert_eq!(text_model.base_instructions.as_deref(), Some(""));
        assert_eq!(text_model.effective_context_window_percent, Some(95));
        assert_eq!(text_model.extra["max_context_window"], 372_000);
        assert_eq!(text_model.extra["comp_hash"], "3000");
        assert_eq!(text_model.extra["tool_mode"], "code_mode_only");
        assert_eq!(text_model.extra["multi_agent_version"], "v2");
        assert_eq!(text_model.extra["use_responses_lite"], true);
        assert_eq!(text_model.extra["include_skills_usage_instructions"], false);
        let image = all
            .models
            .iter()
            .find(|model| model.slug == "gpt-image-2")
            .expect("image model");
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

        let text = text_generation_models_response_with_storage(&storage)
            .expect("text generation models response");
        assert!(!text.models.iter().any(|model| model.slug == "gpt-image-2"));
        assert_eq!(text.models.len() + 1, all.models.len());
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
