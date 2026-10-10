//! Service-mode API keys use one authoritative SeaORM store. These helpers
//! never mirror a mutation into the desktop SQLite database.
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{ApiKey, Storage};
use codexmanager_storage_seaorm::{
    ApiKeyDetailsRepository, ApiKeyRecord, ApiKeysRepository, UsersRepository,
};

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvRestore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn incomplete_remote_configuration_never_authenticates_or_reads_local_keys() {
        let _guard = crate::test_env_guard();
        let _restore = EnvRestore(
            ["CODEXMANAGER_STORAGE_BACKEND", "CODEXMANAGER_DATABASE_URL"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let key = ApiKey {
            id: "local-only-key".into(),
            name: None,
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".into(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".into(),
            protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "local-only-hash".into(),
            status: "active".into(),
            created_at: 1,
            last_used_at: None,
        };
        storage.insert_api_key(&key).unwrap();
        storage
            .upsert_api_key_secret(&key.id, "local-secret")
            .unwrap();
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "mysql");
        std::env::set_var("CODEXMANAGER_DATABASE_URL", "");
        assert!(find_by_id(&storage, &key.id).is_err());
        assert!(find_by_hash(&storage, &key.key_hash).is_err());
        assert!(gateway_auth(&storage, &key.id).is_err());
        assert!(quota(&storage, &key.id).is_err());
        assert!(token_usage(&storage, &key.id).is_err());
        assert!(list(&crate::RpcActor::system_admin()).is_err());
        assert!(set_status(&key.id, "disabled").is_err());
        assert!(delete(&key.id).is_err());
        assert_eq!(
            storage.find_api_key_by_id(&key.id).unwrap().unwrap().status,
            "active"
        );
        assert_eq!(
            storage
                .find_api_key_secret_by_id(&key.id)
                .unwrap()
                .as_deref(),
            Some("local-secret")
        );
    }
}

pub(crate) fn find_by_id(storage: &Storage, id: &str) -> Result<Option<ApiKey>, String> {
    if !seaorm_enabled() {
        return storage
            .find_api_key_by_id(id)
            .map_err(|error| error.to_string());
    }
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeysRepository::get(storage.connection(), &id)
            .await
            .map(|row| row.map(Into::into))
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn find_by_hash(storage: &Storage, hash: &str) -> Result<Option<ApiKey>, String> {
    if !seaorm_enabled() {
        return storage
            .find_api_key_by_hash(hash)
            .map_err(|error| error.to_string());
    }
    let hash = hash.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeysRepository::find_by_hash(storage.connection(), &hash)
            .await
            .map(|row| row.map(Into::into))
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn group_filter(storage: &Storage, id: &str) -> Result<Option<String>, String> {
    if !seaorm_enabled() {
        return storage
            .find_api_key_account_group_filter(id)
            .map_err(|error| error.to_string());
    }
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeysRepository::get(storage.connection(), &id)
            .await
            .map(|row| row.and_then(|key| key.account_group_filter))
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn quota(storage: &Storage, id: &str) -> Result<Option<i64>, String> {
    if !seaorm_enabled() {
        return storage
            .find_api_key_quota_limit(id)
            .map_err(|error| error.to_string());
    }
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::quota(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn token_usage(storage: &Storage, id: &str) -> Result<i64, String> {
    if !seaorm_enabled() {
        return storage
            .api_key_total_token_usage(id)
            .map_err(|error| error.to_string());
    }
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::token_usage(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn create(
    key: ApiKey,
    group: Option<String>,
    quota: Option<i64>,
    secret: String,
) -> Result<(), String> {
    let mut key = ApiKeyRecord::from(key);
    key.account_group_filter = group;
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::create(storage.connection(), key, secret, quota)
            .await
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn set_status(id: &str, status: &str) -> Result<(), String> {
    let id = id.to_owned();
    let status = status.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::update(storage.connection(), &id, None, |key| {
            key.status = status;
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
    })
}

pub(crate) fn delete(id: &str) -> Result<(), String> {
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::delete(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn secret(id: &str) -> Result<Option<String>, String> {
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::secret(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn gateway_auth(
    storage: &Storage,
    id: &str,
) -> Result<Option<codexmanager_core::storage::ApiKeyGatewayAuth>, String> {
    if !seaorm_enabled() {
        return storage
            .find_api_key_gateway_auth_by_id(id)
            .map_err(|error| error.to_string());
    }
    let id = id.to_owned();
    seaorm_block_on(move |storage| async move {
        let Some(key) = ApiKeysRepository::get(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let secret = ApiKeyDetailsRepository::secret(storage.connection(), &id)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Some(codexmanager_core::storage::ApiKeyGatewayAuth {
            id: key.id,
            status: key.status,
            secret,
        }))
    })
}

pub(crate) fn profile_candidates(
    storage: &Storage,
) -> Result<Vec<codexmanager_core::storage::ApiKeyCodexProfileCandidate>, String> {
    if !seaorm_enabled() {
        return storage
            .list_api_key_codex_profile_candidates()
            .map_err(|error| error.to_string());
    }
    seaorm_block_on(move |storage| async move {
        ApiKeysRepository::list(storage.connection())
            .await
            .map(|keys| {
                keys.into_iter()
                    .map(
                        |key| codexmanager_core::storage::ApiKeyCodexProfileCandidate {
                            id: key.id,
                            name: key.name,
                            model_slug: key.model_slug,
                            reasoning_effort: key.reasoning_effort,
                            rotation_strategy: key.rotation_strategy,
                            status: key.status,
                        },
                    )
                    .collect()
            })
            .map_err(|error| error.to_string())
    })
}

pub(crate) fn list(
    actor: &crate::RpcActor,
) -> Result<Vec<codexmanager_core::rpc::types::ApiKeySummary>, String> {
    let user_id = if actor.is_admin() {
        None
    } else {
        Some(
            actor
                .user_id
                .clone()
                .ok_or_else(|| "permission_denied: apikey requires user session".to_owned())?,
        )
    };
    seaorm_block_on(move |storage| async move {
        let allowed = match user_id {
            Some(user_id) => Some(
                UsersRepository::list_owners(storage.connection())
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .filter(|owner| {
                        owner.owner_kind == "user"
                            && owner.owner_user_id.as_deref() == Some(user_id.as_str())
                    })
                    .map(|owner| owner.key_id)
                    .collect::<std::collections::HashSet<_>>(),
            ),
            None => None,
        };
        let mut keys = Vec::new();
        for key in ApiKeysRepository::list(storage.connection())
            .await
            .map_err(|error| error.to_string())?
        {
            if allowed
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(&key.id))
            {
                continue;
            }
            let quota_limit_tokens = ApiKeyDetailsRepository::quota(storage.connection(), &key.id)
                .await
                .map_err(|error| error.to_string())?;
            let mut summary = super::list::map_seaorm_api_key(key);
            summary.quota_limit_tokens = quota_limit_tokens;
            keys.push(summary);
        }
        Ok(keys)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    id: &str,
    name: Option<String>,
    has_name: bool,
    model: Option<String>,
    reasoning: Option<String>,
    tier: Option<String>,
    protocol: Option<String>,
    base_url: Option<String>,
    headers: Option<String>,
    rotation: Option<String>,
    aggregate: Option<String>,
    plan: Option<String>,
    group: Option<String>,
    update_model: bool,
    update_routing: bool,
    update_group: bool,
    update_quota: bool,
    quota: Option<i64>,
) -> Result<(), String> {
    use crate::apikey_profile::*;
    let id = id.to_owned();
    let sync_profile_id = id.clone();
    let sync_profile = update_routing || protocol.is_some() || base_url.is_some();
    if update_model {
        let storage = crate::storage_helpers::open_storage()
            .ok_or_else(|| "storage unavailable".to_string())?;
        crate::models_v2::ensure_text_generation_model(&storage, model.as_deref())?;
    }
    let tier = super::service_tier::normalize_service_tier_owned(tier)?;
    let base_changed = base_url.is_some();
    let headers_changed = headers.is_some();
    let profile_changed = protocol.is_some() || base_changed || headers_changed;
    let base_url = normalize_upstream_base_url(base_url)?;
    let headers = normalize_static_headers_json(headers)?;
    let rotation = if update_routing {
        Some(normalize_rotation_strategy(rotation)?)
    } else {
        None
    };
    let plan = if update_routing && rotation.as_deref() != Some(ROTATION_AGGREGATE_API) {
        crate::account_plan::normalize_account_plan_filter(plan)?
    } else {
        None
    };
    seaorm_block_on(move |storage| async move {
        ApiKeyDetailsRepository::update(
            storage.connection(),
            &id,
            update_quota.then_some(quota),
            move |key| {
                if has_name {
                    key.name = name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned);
                }
                if update_model {
                    key.model_slug = model
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned);
                    key.reasoning_effort = reasoning
                        .as_deref()
                        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
                        .map(str::to_owned);
                    key.service_tier = tier.clone();
                }
                if let Some(rotation) = rotation {
                    key.rotation_strategy = rotation;
                    key.aggregate_api_id = if key.rotation_strategy == ROTATION_AGGREGATE_API {
                        aggregate
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    } else {
                        None
                    };
                    key.account_plan_filter = if key.rotation_strategy == ROTATION_AGGREGATE_API {
                        None
                    } else {
                        plan
                    };
                }
                if key.rotation_strategy == ROTATION_AGGREGATE_API {
                    key.account_group_filter = None;
                } else if update_group {
                    key.account_group_filter =
                        crate::account_group::normalize_account_group_filter(group);
                }
                if profile_changed {
                    let protocol = normalize_protocol_type(Some(
                        protocol.unwrap_or_else(|| key.protocol_type.clone()),
                    ))?;
                    let (client, protocol, auth) = profile_from_protocol(&protocol)?;
                    key.client_type = client;
                    key.protocol_type = protocol;
                    key.auth_scheme = auth;
                    if base_changed {
                        key.upstream_base_url = base_url;
                    }
                    if headers_changed {
                        key.static_headers_json = headers;
                    }
                    if tier.is_some() {
                        key.service_tier = tier;
                    }
                }
                Ok(())
            },
        )
        .await
        .map_err(|error| error.to_string())
    })?;
    if sync_profile {
        let storage = crate::storage_helpers::open_storage()
            .ok_or_else(|| "storage unavailable".to_string())?;
        crate::codex_profile::sync_active_gateway_profile_for_api_key(&storage, &sync_profile_id)?;
    }
    Ok(())
}
