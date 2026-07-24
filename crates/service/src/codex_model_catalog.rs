use codexmanager_core::rpc::types::ModelsResponse;
use codexmanager_core::storage::Storage;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn write_gateway_model_catalog(
    storage: &Storage,
    catalog_path: &Path,
    use_official_account_catalog: bool,
) -> Result<usize, String> {
    let catalog = crate::models_v2::text_generation_models_response_with_storage(storage)?;
    let official_metadata = load_official_model_metadata()?;
    let content = if use_official_account_catalog {
        let custom_model_slugs = storage
            .list_api_models_v2()
            .map_err(|err| format!("list custom account-pool models failed: {err}"))?
            .into_iter()
            .filter(|model| model.origin == "custom")
            .map(|model| model.slug)
            .collect();
        serialize_account_pool_model_catalog(&catalog, &official_metadata, &custom_model_slugs)?
    } else {
        serialize_gateway_model_catalog_with_official(&catalog, &official_metadata)?
    };
    write_atomic(catalog_path, &content)?;
    Ok(catalog.models.len())
}

#[derive(Debug, Default)]
struct OfficialModelMetadata {
    by_slug: HashMap<String, Value>,
}

fn load_official_model_metadata() -> Result<OfficialModelMetadata, String> {
    let codex_home = crate::codex_profile::resolve_profile_dir(None)?;
    let cache_path = codex_home.join("models_cache.json");
    if !cache_path.is_file() {
        return Ok(OfficialModelMetadata::default());
    }

    let content = fs::read_to_string(&cache_path).map_err(|err| {
        format!(
            "read official Codex model cache failed ({}): {err}",
            cache_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&content).map_err(|err| {
        format!(
            "parse official Codex model cache failed ({}): {err}",
            cache_path.display()
        )
    })?;
    official_model_metadata_from_value(&value)
}

fn official_model_metadata_from_value(value: &Value) -> Result<OfficialModelMetadata, String> {
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "official Codex model cache is missing models array".to_string())?;
    let mut metadata = OfficialModelMetadata::default();
    for model in models {
        let Some(slug) = model.get("slug").and_then(Value::as_str) else {
            continue;
        };
        metadata.by_slug.insert(slug.to_string(), model.clone());
    }
    Ok(metadata)
}

fn model_has_instruction_template(model_messages: Option<&Value>) -> bool {
    model_messages
        .and_then(Value::as_object)
        .and_then(|messages| messages.get("instructions_template"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn merge_model_messages(target: &mut Option<Value>, source: Option<&Value>) {
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };
    if target.is_none() {
        *target = Some(Value::Object(source.clone()));
        return;
    }
    let Some(target) = target.as_mut().and_then(Value::as_object_mut) else {
        return;
    };
    for (key, value) in source {
        let should_fill = match target.get(key) {
            None | Some(Value::Null) => true,
            Some(Value::String(value)) => value.trim().is_empty(),
            Some(_) => false,
        };
        if should_fill {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn fill_missing_official_metadata(
    model: &mut codexmanager_core::rpc::types::ModelInfo,
    official_metadata: &OfficialModelMetadata,
) {
    let exact = official_metadata.by_slug.get(&model.slug);

    if model
        .base_instructions
        .as_deref()
        .map(str::trim)
        .is_none_or(|value| value.is_empty())
    {
        model.base_instructions = exact
            .and_then(|source| source.get("base_instructions"))
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    if !model_has_instruction_template(model.model_messages.as_ref()) {
        merge_model_messages(
            &mut model.model_messages,
            exact.and_then(|source| source.get("model_messages")),
        );
    }

    if let Some(exact) = exact {
        for (target, key) in [
            (&mut model.availability_nux, "availability_nux"),
            (&mut model.upgrade, "upgrade"),
        ] {
            if target.as_ref().is_none_or(Value::is_null) {
                *target = exact.get(key).cloned();
            }
        }
    }
}

#[cfg(test)]
fn serialize_gateway_model_catalog(catalog: &ModelsResponse) -> Result<String, String> {
    serialize_gateway_model_catalog_with_official(catalog, &OfficialModelMetadata::default())
}

fn serialize_gateway_model_catalog_with_official(
    catalog: &ModelsResponse,
    official_metadata: &OfficialModelMetadata,
) -> Result<String, String> {
    if catalog.models.is_empty() {
        return Err(
            "managed model catalog is empty; refusing to replace the Codex catalog".to_string(),
        );
    }
    let mut catalog = catalog.clone();
    for model in &mut catalog.models {
        prepare_managed_model(model, official_metadata)?;
    }
    let mut content = serde_json::to_string_pretty(&catalog)
        .map_err(|err| format!("serialize managed model catalog failed: {err}"))?;
    content.push('\n');
    Ok(content)
}

fn serialize_account_pool_model_catalog(
    catalog: &ModelsResponse,
    official_metadata: &OfficialModelMetadata,
    custom_model_slugs: &HashSet<String>,
) -> Result<String, String> {
    if catalog.models.is_empty() {
        return Err(
            "managed model catalog is empty; refusing to replace the Codex catalog".to_string(),
        );
    }

    let mut models = Vec::with_capacity(catalog.models.len());
    for model in &catalog.models {
        if let Some(official) = official_metadata.by_slug.get(&model.slug) {
            // Account-pool mode intentionally preserves the complete official object. New Codex
            // fields therefore flow through without requiring a matching Manager release.
            models.push(official.clone());
            continue;
        }

        // Official entries missing from the current Codex catalog are intentionally omitted so
        // account-pool mode follows removals as well as additions. Only explicit Manager custom
        // aliases retain the compatibility fallback.
        if !custom_model_slugs.contains(&model.slug) {
            continue;
        }
        let mut fallback = model.clone();
        prepare_managed_model(&mut fallback, official_metadata)?;
        models.push(
            serde_json::to_value(fallback)
                .map_err(|err| format!("serialize account-pool fallback model failed: {err}"))?,
        );
    }

    if models.is_empty() {
        return Err(
            "official Codex model cache has no enabled account-pool models; refusing to replace the Codex catalog"
                .to_string(),
        );
    }

    let mut content = serde_json::to_string_pretty(&serde_json::json!({ "models": models }))
        .map_err(|err| format!("serialize account-pool model catalog failed: {err}"))?;
    content.push('\n');
    Ok(content)
}

fn prepare_managed_model(
    model: &mut codexmanager_core::rpc::types::ModelInfo,
    official_metadata: &OfficialModelMetadata,
) -> Result<(), String> {
    fill_missing_official_metadata(model, official_metadata);
    if model
        .shell_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        model.shell_type = Some("shell_command".to_string());
    }
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
    model
        .extra
        .entry("use_responses_lite".to_string())
        .or_insert(serde_json::Value::Bool(false));
    model
        .extra
        .entry("include_skills_usage_instructions".to_string())
        .or_insert(serde_json::Value::Bool(false));

    let uses_responses_lite = model
        .extra
        .get("use_responses_lite")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_base_instructions = model
        .base_instructions
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if uses_responses_lite && !has_base_instructions {
        return Err(format!(
            "model {} enables Responses Lite but official base instructions are unavailable; refusing to replace the Codex catalog",
            model.slug
        ));
    }
    Ok(())
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
    fn gateway_catalog_fills_official_instruction_metadata() {
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
        let official = official_model_metadata_from_value(&serde_json::json!({
            "models": [{
                "slug": "gpt-test",
                "base_instructions": "official instructions",
                "model_messages": {
                    "instructions_template": "official template",
                    "instructions_variables": {"personality_default": ""}
                },
                "availability_nux": {"message": "new"},
                "upgrade": null
            }]
        }))
        .expect("parse official metadata");

        let content = serialize_gateway_model_catalog_with_official(&catalog, &official)
            .expect("serialize catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");
        let model = &value["models"][0];

        assert_eq!(
            model["base_instructions"].as_str(),
            Some("official instructions")
        );
        assert_eq!(
            model["model_messages"]["instructions_template"].as_str(),
            Some("official template")
        );
        assert_eq!(model["availability_nux"]["message"].as_str(), Some("new"));
        assert!(model["upgrade"].is_null());
        assert_eq!(model["use_responses_lite"].as_bool(), Some(true));
    }

    #[test]
    fn account_pool_catalog_preserves_complete_official_model_object() {
        let catalog = ModelsResponse {
            models: vec![ModelInfo {
                slug: "gpt-test".to_string(),
                display_name: "Manager Name".to_string(),
                shell_type: Some("manager_shell".to_string()),
                ..ModelInfo::default()
            }],
            ..ModelsResponse::default()
        };
        let official = official_model_metadata_from_value(&serde_json::json!({
            "models": [{
                "slug": "gpt-test",
                "display_name": "Official Name",
                "shell_type": "official_shell",
                "base_instructions": "official instructions",
                "future_codex_field": {"revision": 7}
            }]
        }))
        .expect("parse official metadata");

        let content = serialize_account_pool_model_catalog(&catalog, &official, &HashSet::new())
            .expect("serialize account-pool catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");
        let model = &value["models"][0];

        assert_eq!(model["display_name"].as_str(), Some("Official Name"));
        assert_eq!(model["shell_type"].as_str(), Some("official_shell"));
        assert_eq!(model["future_codex_field"]["revision"].as_i64(), Some(7));
        assert!(model.get("max_context_window").is_none());
    }

    #[test]
    fn account_pool_catalog_keeps_custom_model_fallback() {
        let catalog = ModelsResponse {
            models: vec![ModelInfo {
                slug: "custom-alias".to_string(),
                display_name: "Custom Alias".to_string(),
                ..ModelInfo::default()
            }],
            ..ModelsResponse::default()
        };

        let content = serialize_account_pool_model_catalog(
            &catalog,
            &OfficialModelMetadata::default(),
            &HashSet::from(["custom-alias".to_string()]),
        )
        .expect("serialize custom fallback");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");

        assert_eq!(value["models"][0]["slug"].as_str(), Some("custom-alias"));
        assert_eq!(
            value["models"][0]["shell_type"].as_str(),
            Some("shell_command")
        );
    }

    #[test]
    fn account_pool_catalog_omits_stale_builtin_model() {
        let catalog = ModelsResponse {
            models: vec![
                ModelInfo {
                    slug: "retired-official-model".to_string(),
                    display_name: "Retired".to_string(),
                    ..ModelInfo::default()
                },
                ModelInfo {
                    slug: "current-official-model".to_string(),
                    display_name: "Generated Current".to_string(),
                    ..ModelInfo::default()
                },
            ],
            ..ModelsResponse::default()
        };
        let official = official_model_metadata_from_value(&serde_json::json!({
            "models": [{
                "slug": "current-official-model",
                "display_name": "Official Current"
            }]
        }))
        .expect("parse official metadata");

        let content = serialize_account_pool_model_catalog(&catalog, &official, &HashSet::new())
            .expect("serialize account-pool catalog");
        let value: Value = serde_json::from_str(&content).expect("parse catalog");

        assert_eq!(value["models"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            value["models"][0]["slug"].as_str(),
            Some("current-official-model")
        );
    }

    #[test]
    fn gateway_catalog_rejects_responses_lite_without_instructions() {
        let mut model = ModelInfo {
            slug: "gpt-lite".to_string(),
            display_name: "GPT Lite".to_string(),
            ..ModelInfo::default()
        };
        model
            .extra
            .insert("use_responses_lite".to_string(), Value::Bool(true));
        let catalog = ModelsResponse {
            models: vec![model],
            ..ModelsResponse::default()
        };

        let err = serialize_gateway_model_catalog(&catalog)
            .expect_err("Responses Lite without instructions must fail");
        assert!(err.contains("gpt-lite"));
        assert!(err.contains("Responses Lite"));
    }

    #[test]
    fn gateway_catalog_rejects_empty_models() {
        let err = serialize_gateway_model_catalog(&ModelsResponse::default())
            .expect_err("empty catalog must fail");
        assert!(err.contains("empty"));
    }
}
