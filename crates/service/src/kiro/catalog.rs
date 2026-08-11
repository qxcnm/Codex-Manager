use codexmanager_core::{
    rpc::types::{ModelInfo, ModelsResponse},
    storage::Storage,
};

/// Kiro owns its public model catalog. Gateway code only asks providers to
/// contribute models and does not need to know individual Kiro model IDs.
pub(crate) const MODELS: &[(&str, &str, i64)] = &[
    (
        "kiro/claude-sonnet-4.6",
        "Kiro Claude Sonnet 4.6",
        1_000_000,
    ),
    ("kiro/claude-sonnet-4.5", "Kiro Claude Sonnet 4.5", 200_000),
    ("kiro/claude-opus-4.8", "Kiro Claude Opus 4.8", 1_000_000),
    ("kiro/claude-opus-4.7", "Kiro Claude Opus 4.7", 1_000_000),
    ("kiro/claude-opus-4.6", "Kiro Claude Opus 4.6", 1_000_000),
    ("kiro/claude-opus-4.5", "Kiro Claude Opus 4.5", 200_000),
    ("kiro/claude-haiku-4.5", "Kiro Claude Haiku 4.5", 200_000),
];

pub(crate) fn append_models(storage: &Storage, models: &mut ModelsResponse) {
    // A static candidate list is not a promise that a Kiro subscription or
    // region can use a model. Only publish models successfully probed with at
    // least one currently active credential.
    let Ok(available) = storage.list_available_kiro_models() else {
        return;
    };
    for (slug, display_name, context_window) in MODELS {
        if !available.iter().any(|available| available == slug) {
            continue;
        }
        if models.models.iter().any(|model| model.slug == *slug) {
            continue;
        }
        models.models.push(ModelInfo {
            slug: (*slug).into(),
            display_name: (*display_name).into(),
            description: Some("Kiro provider via the unified OpenAI gateway".into()),
            supported_in_api: true,
            visibility: Some("list".into()),
            priority: 100,
            supports_reasoning_summaries: Some(true),
            supports_parallel_tool_calls: Some(true),
            supports_image_detail_original: Some(true),
            context_window: Some(*context_window),
            experimental_supported_tools: vec!["tools".into(), "web_search".into()],
            input_modalities: vec!["text".into(), "image".into()],
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{KiroCredentialSecret, KiroCredentialUpsert};

    #[cfg(windows)]
    #[test]
    fn provider_catalog_contains_only_successfully_probed_models() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let mut models = ModelsResponse::default();
        append_models(&storage, &mut models);
        assert!(models.models.is_empty());

        storage
            .upsert_kiro_credential(&KiroCredentialUpsert {
                id: "catalog-runtime".into(),
                auth_method: "social".into(),
                identity_hint: "catalog@example.test".into(),
                email: Some("catalog@example.test".into()),
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
                    refresh_token: "secret".into(),
                    access_token: None,
                    client_id: None,
                    client_secret: None,
                    proxy_password: None,
                },
            })
            .unwrap();

        append_models(&storage, &mut models);
        assert!(models.models.is_empty(), "unprobed models must stay hidden");

        storage
            .upsert_kiro_credential_model_availability(
                "catalog-runtime",
                "kiro/claude-sonnet-4.5",
                "available",
                None,
                Some(12),
            )
            .unwrap();
        storage
            .upsert_kiro_credential_model_availability(
                "catalog-runtime",
                "kiro/claude-opus-4.8",
                "unavailable",
                Some("unsupported_model"),
                Some(9),
            )
            .unwrap();

        append_models(&storage, &mut models);
        assert!(models
            .models
            .iter()
            .any(|model| model.slug == "kiro/claude-sonnet-4.5"));
        assert!(!models
            .models
            .iter()
            .any(|model| model.slug == "kiro/claude-opus-4.8"));
        assert!(models.models.iter().all(|model| {
            model.slug.starts_with("kiro/") && model.visibility.as_deref() == Some("list")
        }));
    }
}
