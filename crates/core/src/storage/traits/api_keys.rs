use super::super::{ApiKey, AppUser};

pub const DUPLICATE_API_KEY: &str = "自定义 API Key 已存在(custom api key already exists)";

// Deliberately no Debug: this value carries a plaintext secret.
pub struct ApiKeyCreate {
    pub key: ApiKey,
    pub secret: String,
    pub account_group_filter: Option<String>,
    pub quota_limit_tokens: Option<i64>,
    pub owner_user_id: Option<String>,
}

#[derive(Default)]
pub struct ApiKeyConfigPatch {
    pub owner_user_id: Option<String>,
    pub name: Option<Option<String>>,
    pub model: Option<ApiKeyModelConfig>,
    pub routing: Option<ApiKeyRoutingConfig>,
    pub protocol: Option<ApiKeyProtocolConfig>,
    pub upstream_base_url: Option<Option<String>>,
    pub static_headers_json: Option<Option<String>>,
    pub account_group_filter: Option<Option<String>>,
    pub quota_limit_tokens: Option<Option<i64>>,
}

pub struct ApiKeyModelConfig {
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
}

pub struct ApiKeyRoutingConfig {
    pub rotation_strategy: String,
    pub aggregate_api_id: Option<String>,
    pub account_plan_filter: Option<String>,
}

pub struct ApiKeyProtocolConfig {
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
}

impl ApiKeyConfigPatch {
    /// Merge into the locked current record, preserving unrelated concurrent writes.
    pub fn apply(&self, key: &mut ApiKey, group: &mut Option<String>) {
        if let Some(name) = &self.name {
            key.name = name.clone();
        }
        if let Some(model) = &self.model {
            key.model_slug = model.model_slug.clone();
            key.reasoning_effort = model.reasoning_effort.clone();
            key.service_tier = model.service_tier.clone();
        }
        if let Some(routing) = &self.routing {
            key.rotation_strategy = routing.rotation_strategy.clone();
            key.aggregate_api_id = routing.aggregate_api_id.clone();
            key.account_plan_filter = routing.account_plan_filter.clone();
        }
        if self.routing.is_some() || self.account_group_filter.is_some() {
            if key.rotation_strategy == "aggregate_api_rotation" {
                *group = None;
            } else if let Some(value) = &self.account_group_filter {
                *group = value.clone();
            }
        }
        if let Some(protocol) = &self.protocol {
            key.client_type = protocol.client_type.clone();
            key.protocol_type = protocol.protocol_type.clone();
            key.auth_scheme = protocol.auth_scheme.clone();
        }
        if let Some(value) = &self.upstream_base_url {
            key.upstream_base_url = value.clone();
        }
        if let Some(value) = &self.static_headers_json {
            key.static_headers_json = value.clone();
        }
    }
}

pub fn validate_api_key_owner_user(user: Option<&AppUser>) -> Result<(), String> {
    let user = user.ok_or("用户不存在")?;
    if user.role == "admin" {
        return Err("管理员账号不参与额度分发".into());
    }
    if user.status != "active" {
        return Err("用户已禁用".into());
    }
    Ok(())
}
