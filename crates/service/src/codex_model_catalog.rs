use codexmanager_core::rpc::types::ModelsResponse;
use codexmanager_core::storage::Storage;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayCatalogPolicy {
    OfficialAccountPool,
    Managed,
}

pub(crate) fn write_gateway_model_catalog(
    storage: &Storage,
    catalog_path: &Path,
    policy: GatewayCatalogPolicy,
) -> Result<usize, String> {
    let (content, models_count) = match policy {
        GatewayCatalogPolicy::OfficialAccountPool => {
            let official_models = load_official_model_catalog()?;
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

fn load_official_model_catalog() -> Result<Vec<Value>, String> {
    let codex_home = crate::codex_profile::resolve_profile_dir(None)?;
    let cache_path = codex_home.join("models_cache.json");
    if !cache_path.is_file() {
        return Err(format!(
            "official Codex model cache not found: {}",
            cache_path.display()
        ));
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
    official_model_catalog_from_value(&value)
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
    // Managed aggregate and hybrid catalogs keep the established full Responses transport.
    // Responses Lite requires official prompt metadata and must remain exclusive to the raw
    // official account-pool catalog.
    model.extra.insert(
        "use_responses_lite".to_string(),
        serde_json::Value::Bool(false),
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
    fn gateway_catalog_rejects_empty_models() {
        let err = serialize_gateway_model_catalog(&ModelsResponse::default())
            .expect_err("empty catalog must fail");
        assert!(err.contains("empty"));
    }
}
