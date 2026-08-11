use codexmanager_core::{
    rpc::types::{ModelInfo, ModelsResponse},
    storage::Storage,
};
use codexmanager_grok_provider::{GrokModelCapability, GROK_WEB_MODELS};

pub(crate) fn append_models(storage: &Storage, models: &mut ModelsResponse) {
    let Ok(available) = storage.list_available_grok_models() else {
        return;
    };
    for spec in GROK_WEB_MODELS
        .iter()
        .filter(|spec| spec.capability == GrokModelCapability::Chat)
    {
        let slug = format!("grok/{}", spec.public_id);
        if !available.iter().any(|item| item == &slug)
            || models.models.iter().any(|item| item.slug == slug)
        {
            continue;
        }
        models.models.push(ModelInfo {
            slug,
            display_name: format!("Grok {}", spec.public_id.trim_start_matches("grok-chat-")),
            description: Some("Grok Web via the unified OpenAI gateway".into()),
            supported_in_api: true,
            visibility: Some("list".into()),
            priority: 90,
            supports_reasoning_summaries: Some(true),
            supports_parallel_tool_calls: Some(false),
            supports_image_detail_original: Some(false),
            input_modalities: vec!["text".into()],
            ..Default::default()
        });
    }
}
