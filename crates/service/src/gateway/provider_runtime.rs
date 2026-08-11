use bytes::Bytes;
use codexmanager_core::{rpc::types::ModelsResponse, storage::Storage};

use super::upstream::GatewayUpstreamResponse;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    /// `legacy_account_pool` or `runtime_adapter`.
    pub kind: &'static str,
    pub model_namespace: &'static str,
    pub capabilities: &'static [&'static str],
}

const CODEX_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "codex",
    display_name: "Codex / GPT",
    kind: "legacy_account_pool",
    model_namespace: "codex/",
    capabilities: &["responses", "streaming", "tools", "images", "reasoning"],
};

const KIRO_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "kiro",
    display_name: "Kiro",
    kind: "runtime_adapter",
    model_namespace: "kiro/",
    capabilities: &[
        "responses",
        "streaming",
        "tools",
        "images",
        "reasoning",
        "web_search",
    ],
};

const GROK_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "grok",
    display_name: "Grok",
    kind: "runtime_adapter",
    model_namespace: "grok/",
    capabilities: &["responses", "streaming"],
};

/// A provider-owned runtime adapter.
///
/// The gateway only knows this contract. Model discovery, availability checks,
/// request execution, and catalog contribution remain owned by the provider.
/// Adding another direct provider therefore requires an adapter implementation
/// plus one entry in `PROVIDER_RUNTIME_REGISTRY`; no gateway dispatch branch is
/// needed.
pub(crate) trait ProviderRuntimeAdapter: Sync {
    fn descriptor(&self) -> &'static AdapterDescriptor;

    fn id(&self) -> &'static str {
        self.descriptor().id
    }

    fn matches_model(&self, model: &str) -> bool;

    /// Legacy providers are present in the same registry, while their request
    /// execution can remain on the existing account-pool pipeline during a
    /// staged migration.
    fn supports_direct_execution(&self) -> bool {
        self.descriptor().kind == "runtime_adapter"
    }

    fn is_supported_model(&self, model: &str) -> bool;

    fn has_available_credentials(&self, storage: &Storage) -> Result<bool, String>;

    fn execute(
        &self,
        storage: &Storage,
        body: &Bytes,
        model: &str,
    ) -> Result<GatewayUpstreamResponse, String>;

    fn append_models(&self, storage: &Storage, models: &mut ModelsResponse);

    fn upstream_model<'a>(&self, model: &'a str) -> &'a str {
        model
            .strip_prefix(self.id())
            .and_then(|model| model.strip_prefix('/'))
            .unwrap_or(model)
    }
}

struct CodexRuntimeAdapter;

impl ProviderRuntimeAdapter for CodexRuntimeAdapter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        &CODEX_DESCRIPTOR
    }

    fn matches_model(&self, model: &str) -> bool {
        let model = model.trim().to_ascii_lowercase();
        model.starts_with("codex/") || model.starts_with("gpt-")
    }

    fn is_supported_model(&self, model: &str) -> bool {
        self.matches_model(model)
    }

    fn has_available_credentials(&self, storage: &Storage) -> Result<bool, String> {
        storage
            .list_accounts()
            .map(|accounts| {
                accounts
                    .iter()
                    .any(|account| matches!(account.status.trim(), "active" | "available"))
            })
            .map_err(|error| format!("codex_accounts_read_failed: {error}"))
    }

    fn execute(
        &self,
        _storage: &Storage,
        _body: &Bytes,
        _model: &str,
    ) -> Result<GatewayUpstreamResponse, String> {
        Err("codex_uses_legacy_account_pool_execution".to_string())
    }

    fn append_models(&self, _storage: &Storage, _models: &mut ModelsResponse) {
        // Codex models are refreshed by the existing account-pool catalog.
    }
}

struct KiroRuntimeAdapter;

impl ProviderRuntimeAdapter for KiroRuntimeAdapter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        &KIRO_DESCRIPTOR
    }

    fn matches_model(&self, model: &str) -> bool {
        crate::kiro::runtime::is_kiro_model(Some(model))
    }

    fn is_supported_model(&self, model: &str) -> bool {
        crate::kiro::runtime::is_supported_kiro_model(model)
    }

    fn has_available_credentials(&self, storage: &Storage) -> Result<bool, String> {
        storage
            .list_available_kiro_models()
            .map(|items| !items.is_empty())
            .map_err(|error| format!("kiro_credentials_read_failed: {error}"))
    }

    fn execute(
        &self,
        storage: &Storage,
        body: &Bytes,
        model: &str,
    ) -> Result<GatewayUpstreamResponse, String> {
        crate::kiro::runtime::execute_responses_request(storage, body, model)
    }

    fn append_models(&self, storage: &Storage, models: &mut ModelsResponse) {
        crate::kiro::catalog::append_models(storage, models);
    }
}

struct GrokRuntimeAdapter;

impl ProviderRuntimeAdapter for GrokRuntimeAdapter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        &GROK_DESCRIPTOR
    }

    fn matches_model(&self, model: &str) -> bool {
        crate::grok::runtime::is_grok_model(Some(model))
    }

    fn is_supported_model(&self, model: &str) -> bool {
        crate::grok::runtime::is_supported_grok_model(model)
    }

    fn has_available_credentials(&self, storage: &Storage) -> Result<bool, String> {
        storage
            .list_available_grok_models()
            .map(|items| !items.is_empty())
            .map_err(|_| "grok_credentials_read_failed".to_string())
    }

    fn execute(
        &self,
        storage: &Storage,
        body: &Bytes,
        model: &str,
    ) -> Result<GatewayUpstreamResponse, String> {
        crate::grok::runtime::execute_responses_request(storage, body, model)
    }

    fn append_models(&self, storage: &Storage, models: &mut ModelsResponse) {
        crate::grok::catalog::append_models(storage, models);
    }
}

static CODEX_RUNTIME_ADAPTER: CodexRuntimeAdapter = CodexRuntimeAdapter;
static KIRO_RUNTIME_ADAPTER: KiroRuntimeAdapter = KiroRuntimeAdapter;
static GROK_RUNTIME_ADAPTER: GrokRuntimeAdapter = GrokRuntimeAdapter;

/// The only place where built-in providers are registered. Codex is included
/// even while its execution remains on the legacy account-pool pipeline.
static PROVIDER_RUNTIME_REGISTRY: &[&dyn ProviderRuntimeAdapter] = &[
    &CODEX_RUNTIME_ADAPTER,
    &KIRO_RUNTIME_ADAPTER,
    &GROK_RUNTIME_ADAPTER,
];

pub(crate) fn append_provider_models(storage: &Storage, models: &mut ModelsResponse) {
    for provider in PROVIDER_RUNTIME_REGISTRY {
        provider.append_models(storage, models);
    }
}

/// Safe metadata for RPC/UI discovery. It intentionally contains no secrets,
/// callback names, credential state, or executable implementation details.
pub(crate) fn adapter_descriptors() -> Vec<AdapterDescriptor> {
    PROVIDER_RUNTIME_REGISTRY
        .iter()
        .map(|provider| provider.descriptor().clone())
        .collect()
}

pub(crate) fn provider_for_model(model: &str) -> Option<&'static dyn ProviderRuntimeAdapter> {
    PROVIDER_RUNTIME_REGISTRY
        .iter()
        .copied()
        .find(|provider| provider.matches_model(model))
}

pub(crate) fn direct_provider_for_model(
    model: &str,
) -> Option<&'static dyn ProviderRuntimeAdapter> {
    provider_for_model(model).filter(|provider| provider.supports_direct_execution())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_resolves_namespaces_without_model_id_branches() {
        let provider = direct_provider_for_model("kiro/claude-sonnet-4.5").unwrap();
        assert_eq!(provider.id(), "kiro");
        assert_eq!(
            provider.upstream_model("kiro/claude-sonnet-4.5"),
            "claude-sonnet-4.5"
        );
        assert!(direct_provider_for_model("gpt-5.5").is_none());
        assert_eq!(provider_for_model("gpt-5.5").unwrap().id(), "codex");
        let grok = direct_provider_for_model("grok/grok-chat-fast").unwrap();
        assert_eq!(grok.id(), "grok");
        assert_eq!(grok.upstream_model("grok/grok-chat-fast"), "grok-chat-fast");
    }

    #[test]
    fn registry_has_unique_provider_ids() {
        let ids = PROVIDER_RUNTIME_REGISTRY
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>();
        let unique = ids.iter().copied().collect::<HashSet<_>>();
        assert_eq!(ids.len(), unique.len());
        assert_eq!(ids, vec!["codex", "kiro", "grok"]);
        assert!(PROVIDER_RUNTIME_REGISTRY
            .iter()
            .any(|provider| provider.id() == "codex" && !provider.supports_direct_execution()));
    }

    #[test]
    fn descriptors_are_safe_serializable_metadata() {
        let descriptors = adapter_descriptors();
        let value = serde_json::to_value(&descriptors).unwrap();
        assert_eq!(value[0]["id"], "codex");
        assert_eq!(value[0]["kind"], "legacy_account_pool");
        assert_eq!(value[1]["modelNamespace"], "kiro/");
        let serialized = serde_json::to_string(&descriptors).unwrap();
        assert!(!serialized.contains("token"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("execute_responses_request"));
    }
}
