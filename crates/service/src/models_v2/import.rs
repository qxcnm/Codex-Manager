use std::collections::{BTreeSet, HashMap};

use codexmanager_core::storage::{
    ManagedModelV2, ManagedModelV2Upsert, ModelPriceV2, ModelRouteV2,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelImportPreviewV2Params {
    pub json_content: String,
    #[serde(default = "default_conflict_strategy")]
    pub conflict_strategy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelImportCommitV2Params {
    pub json_content: String,
    #[serde(default = "default_conflict_strategy")]
    pub conflict_strategy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedModelImportPreviewV2Result {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub conflicts: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub ignored_fields: Vec<String>,
    #[serde(default)]
    pub committed: usize,
}

fn default_conflict_strategy() -> String {
    "keep_existing".to_string()
}

fn validate_conflict_strategy(strategy: &str) -> Result<(), String> {
    if matches!(strategy, "keep_existing" | "replace_custom") {
        Ok(())
    } else {
        Err("conflictStrategy must be keep_existing or replace_custom".to_string())
    }
}

fn text_field(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn i64_field(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_i64))
}

fn bool_field(object: &Map<String, Value>, keys: &[&str], default: bool) -> bool {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_bool))
        .unwrap_or(default)
}

fn sanitize_v2_model(mut model: ManagedModelV2) -> ManagedModelV2 {
    model.id.clear();
    model.origin = "custom".to_string();
    model.builtin_revision = None;
    model.user_edited = true;
    model.created_at = 0;
    model.updated_at = 0;
    for route in &mut model.routes {
        route.id.clear();
    }
    model.permission_group_ids.retain(|id| id != "mg_default");
    model
}

fn parse_codex_model(
    object: &Map<String, Value>,
    ignored_fields: &mut BTreeSet<String>,
) -> Result<ManagedModelV2, String> {
    for field in [
        "base_instructions",
        "baseInstructions",
        "model_messages",
        "modelMessages",
        "instructions_template",
        "instructionsTemplate",
        "instructions_variables",
        "instructionsVariables",
        "include_skills_usage_instructions",
        "includeSkillsUsageInstructions",
    ] {
        if object.contains_key(field) {
            ignored_fields.insert(field.to_string());
        }
    }
    let slug = text_field(object, &["slug", "id"])
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "model slug is required".to_string())?;
    let display_name = text_field(object, &["displayName", "display_name", "name"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| slug.clone());
    let visibility = match text_field(object, &["visibility"]).as_deref() {
        Some("hide" | "hidden") => "hide",
        _ => "list",
    };
    let reasoning_efforts = object
        .get("supported_reasoning_levels")
        .or_else(|| object.get("supportedReasoningLevels"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str().map(str::to_string).or_else(|| {
                        item.as_object()
                            .and_then(|object| object.get("effort"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let input_modalities = object
        .get("input_modalities")
        .or_else(|| object.get("inputModalities"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let service_tiers = object
        .get("service_tiers")
        .or_else(|| object.get("serviceTiers"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str().map(Value::from).or_else(|| {
                        item.as_object()
                            .and_then(|object| object.get("id"))
                            .and_then(Value::as_str)
                            .map(Value::from)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let capabilities = serde_json::json!({
        "reasoningEfforts": reasoning_efforts,
        "serviceTiers": service_tiers,
        "inputModalities": input_modalities,
        "supportsParallelToolCalls": bool_field(object,&["supports_parallel_tool_calls","supportsParallelToolCalls"],false),
        "supportsReasoningSummaries": bool_field(object,&["supports_reasoning_summary_parameter","supports_reasoning_summaries","supportsReasoningSummaries"],false),
        "supportsVerbosity": bool_field(object,&["support_verbosity","supportsVerbosity"],false),
        "supportsImageDetailOriginal": bool_field(object,&["supports_image_detail_original","supportsImageDetailOriginal"],false),
        "supportsSearchTool": bool_field(object,&["supports_search_tool","supportsSearchTool"],false),
        "truncationMode": text_field(object,&["truncation_mode","truncationMode"]),
        "truncationLimit": i64_field(object,&["truncation_limit","truncationLimit"]),
        "applyPatchToolType": text_field(object,&["apply_patch_tool_type","applyPatchToolType"]),
        "webSearchToolType": text_field(object,&["web_search_tool_type","webSearchToolType"]),
    });
    Ok(ManagedModelV2 {
        slug,
        display_name,
        description: text_field(object, &["description"]),
        origin: "custom".to_string(),
        enabled: true,
        supported_in_api: bool_field(object, &["supported_in_api", "supportedInApi"], true),
        visibility: visibility.to_string(),
        sort_order: i64_field(object, &["sortOrder", "priority", "sort_index"]).unwrap_or(0),
        context_window: i64_field(object, &["contextWindow", "context_window"]),
        max_context_window: i64_field(object, &["maxContextWindow", "max_context_window"])
            .or_else(|| i64_field(object, &["contextWindow", "context_window"])),
        default_reasoning_effort: text_field(
            object,
            &["defaultReasoningEffort", "default_reasoning_level"],
        ),
        capabilities,
        instructions_mode: "passthrough".to_string(),
        price: ModelPriceV2 {
            price_status: "missing".to_string(),
            ..Default::default()
        },
        routes: Vec::<ModelRouteV2>::new(),
        ..Default::default()
    })
}

fn parse_models(
    json_content: &str,
) -> Result<(Vec<ManagedModelV2>, BTreeSet<String>, Vec<String>), String> {
    let root: Value = serde_json::from_str(json_content)
        .map_err(|err| format!("invalid model import JSON: {err}"))?;
    let items = root
        .as_object()
        .and_then(|object| object.get("models").or_else(|| object.get("items")))
        .or_else(|| root.as_array().map(|_| &root))
        .and_then(Value::as_array)
        .ok_or_else(|| "model import JSON must contain a models array".to_string())?;
    let mut models = Vec::new();
    let mut ignored_fields = BTreeSet::new();
    let mut errors = Vec::new();
    for (index, value) in items.iter().enumerate() {
        let Some(object) = value.as_object() else {
            errors.push(format!("models[{index}] must be an object"));
            continue;
        };
        let looks_like_v2 = object.contains_key("price")
            || object.contains_key("instructionsMode")
            || object.contains_key("priceTiers");
        let parsed = if looks_like_v2 {
            serde_json::from_value::<ManagedModelV2>(value.clone())
                .map(sanitize_v2_model)
                .map_err(|err| format!("models[{index}] invalid V2 model: {err}"))
        } else {
            parse_codex_model(object, &mut ignored_fields)
                .map_err(|err| format!("models[{index}] {err}"))
        };
        match parsed {
            Ok(model) => models.push(model),
            Err(err) => errors.push(err),
        }
    }
    Ok((models, ignored_fields, errors))
}

fn prepare_import(
    storage: &codexmanager_core::storage::Storage,
    json_content: &str,
    conflict_strategy: &str,
) -> Result<(ManagedModelImportPreviewV2Result, Vec<ManagedModelV2Upsert>), String> {
    validate_conflict_strategy(conflict_strategy)?;
    let (models, ignored_fields, errors) = parse_models(json_content)?;
    let existing = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("list models for import failed: {err}"))?
        .into_iter()
        .map(|model| (model.slug.to_ascii_lowercase(), model))
        .collect::<HashMap<_, _>>();
    let mut preview = ManagedModelImportPreviewV2Result {
        errors,
        ignored_fields: ignored_fields.into_iter().collect(),
        ..Default::default()
    };
    let mut writes = Vec::new();
    let mut seen = BTreeSet::new();
    for model in models {
        let slug_key = model.slug.trim().to_ascii_lowercase();
        if slug_key.is_empty() || !seen.insert(slug_key.clone()) {
            preview.skipped.push(model.slug);
            continue;
        }
        match existing.get(&slug_key) {
            None => {
                preview.added.push(model.slug.clone());
                writes.push(ManagedModelV2Upsert {
                    previous_slug: None,
                    model,
                });
            }
            Some(existing) if existing.origin == "builtin" => {
                preview.conflicts.push(model.slug);
            }
            Some(_) if conflict_strategy == "replace_custom" => {
                preview.updated.push(model.slug.clone());
                writes.push(ManagedModelV2Upsert {
                    previous_slug: Some(model.slug.clone()),
                    model,
                });
            }
            Some(_) => {
                preview.conflicts.push(model.slug.clone());
                preview.skipped.push(model.slug);
            }
        }
    }
    Ok((preview, writes))
}

pub(crate) fn preview_import(
    params: ManagedModelImportPreviewV2Params,
) -> Result<ManagedModelImportPreviewV2Result, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    prepare_import(&storage, &params.json_content, &params.conflict_strategy)
        .map(|(preview, _)| preview)
}

pub(crate) fn commit_import(
    params: ManagedModelImportCommitV2Params,
) -> Result<ManagedModelImportPreviewV2Result, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let (mut preview, writes) =
        prepare_import(&storage, &params.json_content, &params.conflict_strategy)?;
    if !preview.errors.is_empty() {
        return Err(format!(
            "model import contains {} validation errors",
            preview.errors.len()
        ));
    }
    storage
        .upsert_managed_models_v2(&writes)
        .map_err(|err| format!("commit model import failed: {err}"))?;
    super::sync_active_gateway_catalog_best_effort(&storage);
    preview.committed = writes.len();
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_import_ignores_prompt_fields() {
        let json = r#"{"models":[{"slug":"local-x","display_name":"Local X","base_instructions":"secret","model_messages":{"instructions_template":"secret"}}]}"#;
        let (models, ignored, errors) = parse_models(json).unwrap();
        assert!(errors.is_empty());
        assert_eq!(models[0].instructions_mode, "passthrough");
        assert!(models[0].instructions_text.is_none());
        assert!(ignored.contains("base_instructions"));
        assert!(ignored.contains("model_messages"));
    }
}
