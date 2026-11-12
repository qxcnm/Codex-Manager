use codexmanager_core::storage::Storage;
use serde_json::Value;

use super::LocalValidationError;
use crate::gateway::IncomingHeaderSnapshot;

/// The Codex image extension hardcodes its model independently of the conversation model.
/// Extension requests use the gateway's configured image model instead, including custom providers.
/// The image-turn header selects behavior; the final model still needs normal catalog/access checks.
pub(super) fn resolve_model_override(
    storage: &Storage,
    protocol_type: &str,
    path: &str,
    headers: &IncomingHeaderSnapshot,
    body: Option<&Value>,
) -> Result<Option<String>, LocalValidationError> {
    if protocol_type != crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        || !matches!(
            path.split('?').next(),
            Some("/v1/images/generations" | "/v1/images/edits")
        )
        || !headers.has_codex_image_turn_id()
        || !body.is_some_and(Value::is_object)
    {
        return Ok(None);
    }

    let model_slug = crate::gateway::runtime_config::current_codex_image_tool_model();
    if let Some(model) =
        crate::models_v2::managed_model(storage, crate::models_v2::policy_catalog_slug(&model_slug))
            .map_err(|err| {
                LocalValidationError::new(500, format!("model_catalog_v2_read_failed: {err}"))
            })?
    {
        let editing = path.split('?').next() == Some("/v1/images/edits");
        let supported = if editing {
            crate::models_v2::supports_image_editing(&model)
        } else {
            crate::models_v2::supports_image_generation(&model)
        };
        if !supported {
            let (action_zh, action_en) = if editing {
                ("图片编辑", "image editing")
            } else {
                ("图片生成", "image generation")
            };
            return Err(LocalValidationError::new(
                400,
                crate::gateway::bilingual_error(
                    format!("配置的图片工具模型 {model_slug} 不支持{action_zh}，请选择支持该操作的图片模型"),
                    format!("configured image tool model {model_slug} does not support {action_en}; select an image model supporting this operation"),
                ),
            ));
        }
    }
    Ok(Some(model_slug))
}
