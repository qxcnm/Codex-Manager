use rusqlite::{params, OptionalExtension, Result};

use super::{now_ts, ApiKeyPolicy, Storage};

impl Storage {
    pub fn upsert_api_key_policy(&self, policy: &ApiKeyPolicy) -> Result<()> {
        let allowed_models_json = encode_list(&policy.allowed_models)?;
        let allowed_platforms_json = encode_list(&policy.allowed_platforms)?;
        let concurrency_limit = policy.concurrency_limit.filter(|value| *value > 0);
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO api_key_policies (
                key_id, allowed_models_json, allowed_platforms_json, model_visibility, expires_at,
                concurrency_limit, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(key_id) DO UPDATE SET
                allowed_models_json = excluded.allowed_models_json,
                allowed_platforms_json = excluded.allowed_platforms_json,
                model_visibility = excluded.model_visibility,
                expires_at = excluded.expires_at,
                concurrency_limit = excluded.concurrency_limit,
                updated_at = excluded.updated_at",
            params![
                policy.key_id,
                allowed_models_json,
                allowed_platforms_json,
                normalize_model_visibility(&policy.model_visibility),
                policy.expires_at,
                concurrency_limit,
                now
            ],
        )?;
        Ok(())
    }

    pub fn find_api_key_policy(&self, key_id: &str) -> Result<Option<ApiKeyPolicy>> {
        self.conn
            .query_row(
                "SELECT key_id, allowed_models_json, allowed_platforms_json,
                        COALESCE(model_visibility, 'selectable'), expires_at, concurrency_limit
                   FROM api_key_policies
                  WHERE key_id = ?1
                  LIMIT 1",
                [key_id],
                |row| {
                    let models: Option<String> = row.get(1)?;
                    let platforms: Option<String> = row.get(2)?;
                    Ok(ApiKeyPolicy {
                        key_id: row.get(0)?,
                        allowed_models: decode_list(models.as_deref()),
                        allowed_platforms: decode_list(platforms.as_deref()),
                        model_visibility: normalize_model_visibility(&row.get::<_, String>(3)?),
                        expires_at: row.get(4)?,
                        concurrency_limit: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    pub(super) fn ensure_api_key_policies_table(&self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("../../migrations/115_api_key_policies.sql"))
    }
}

fn normalize_model_visibility(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "managed" => "managed".to_string(),
        _ => "selectable".to_string(),
    }
}

fn encode_list(values: &[String]) -> Result<Option<String>> {
    if values.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(values)
        .map(Some)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}

fn decode_list(value: Option<&str>) -> Vec<String> {
    value
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_policy_round_trips_and_normalizes_zero_concurrency() {
        let storage = Storage::open_in_memory().expect("storage");
        storage.init().expect("init storage");
        let key = crate::storage::ApiKey {
            id: "policy-key".into(),
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
            key_hash: "policy-hash".into(),
            status: "active".into(),
            created_at: now_ts(),
            last_used_at: None,
        };
        storage.insert_api_key(&key).expect("insert key");
        storage
            .upsert_api_key_policy(&ApiKeyPolicy {
                key_id: key.id.clone(),
                allowed_models: vec!["smart".into(), "kiro/claude-sonnet-4.5".into()],
                allowed_platforms: vec!["kiro".into()],
                model_visibility: "managed".into(),
                expires_at: Some(2_000_000_000),
                concurrency_limit: Some(0),
            })
            .expect("upsert policy");
        let stored = storage
            .find_api_key_policy(&key.id)
            .expect("read policy")
            .expect("policy exists");
        assert_eq!(stored.allowed_platforms, vec!["kiro"]);
        assert_eq!(stored.model_visibility, "managed");
        assert_eq!(stored.allowed_models.len(), 2);
        assert_eq!(stored.expires_at, Some(2_000_000_000));
        assert_eq!(stored.concurrency_limit, None);
    }
}
