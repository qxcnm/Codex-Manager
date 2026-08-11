use super::{now_ts, KiroVaultError, Storage};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

type VaultResult<T> = Result<T, KiroVaultError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokCredentialSecret {
    pub account: String,
    pub password: String,
    pub sso_token: String,
}

#[derive(Debug, Clone)]
pub struct GrokCredentialUpsert {
    pub id: String,
    pub account: String,
    pub status: String,
    pub priority: i64,
    pub weight: f64,
    pub proxy_url: Option<String>,
    pub metadata_json: String,
    pub expires_at: Option<i64>,
    pub secret: GrokCredentialSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokCredentialRecord {
    pub id: String,
    pub account_masked: String,
    pub status: String,
    pub priority: i64,
    pub weight: f64,
    pub proxy_url: Option<String>,
    pub web_tier: String,
    pub metadata_json: String,
    pub expires_at: Option<i64>,
    pub cooldown_until: Option<i64>,
    pub failure_count: i64,
    pub request_count: i64,
    pub success_count: i64,
    pub last_latency_ms: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokQuotaWindowRecord {
    pub credential_id: String,
    pub mode: String,
    pub remaining_queries: i64,
    pub total_queries: i64,
    pub window_size_seconds: i64,
    pub reset_at: i64,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokCredentialModelAvailability {
    pub credential_id: String,
    pub model_slug: String,
    pub status: String,
    pub error_code: Option<String>,
    pub latency_ms: Option<i64>,
    pub checked_at: i64,
}

fn normalize_account(account: &str) -> String {
    account.trim().to_ascii_lowercase()
}

fn mask_account(account: &str) -> String {
    let normalized = normalize_account(account);
    let Some((local, domain)) = normalized.split_once('@') else {
        return "***".to_string();
    };
    let prefix = local.chars().take(2).collect::<String>();
    format!("{prefix}***@{domain}")
}

impl Storage {
    pub fn grok_credential_exists(&self, account: &str) -> VaultResult<bool> {
        let hash = self.vault_identity_hash("grok-web", &normalize_account(account))?;
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM grok_credentials WHERE identity_hash = ?1 LIMIT 1",
                [hash],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn upsert_grok_credential(&self, input: &GrokCredentialUpsert) -> VaultResult<()> {
        let normalized_account = normalize_account(&input.account);
        let hash = self.vault_identity_hash("grok-web", &normalized_account)?;
        let existing_id = self
            .conn
            .query_row(
                "SELECT id FROM grok_credentials WHERE identity_hash = ?1",
                [&hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let id = existing_id.as_deref().unwrap_or(input.id.as_str());
        let secret_json = serde_json::to_string(&input.secret)?;
        let encrypted_secret = self.encrypt_vault_text(&format!("grok:{id}"), &secret_json)?;
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO grok_credentials (
                id, identity_hash, account_masked, status, priority, weight, proxy_url,
                encrypted_secret, metadata_json, expires_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
             ON CONFLICT(identity_hash) DO UPDATE SET
                account_masked = excluded.account_masked,
                status = excluded.status,
                priority = excluded.priority,
                weight = excluded.weight,
                proxy_url = excluded.proxy_url,
                encrypted_secret = excluded.encrypted_secret,
                metadata_json = excluded.metadata_json,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at",
            params![
                id,
                hash,
                mask_account(&input.account),
                input.status,
                input.priority,
                input.weight,
                input.proxy_url,
                encrypted_secret,
                input.metadata_json,
                input.expires_at,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn read_grok_credential_secret(
        &self,
        id: &str,
    ) -> VaultResult<Option<GrokCredentialSecret>> {
        let encrypted = self
            .conn
            .query_row(
                "SELECT encrypted_secret FROM grok_credentials WHERE id = ?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        encrypted
            .map(|value| {
                let plain = self.decrypt_vault_text(&format!("grok:{id}"), &value)?;
                serde_json::from_str(&plain).map_err(Into::into)
            })
            .transpose()
    }

    pub fn list_grok_credentials(&self) -> VaultResult<Vec<GrokCredentialRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_masked, status, priority, weight, proxy_url, web_tier, metadata_json,
                    expires_at, cooldown_until, failure_count, request_count, success_count,
                    last_latency_ms, created_at, updated_at
             FROM grok_credentials ORDER BY priority DESC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GrokCredentialRecord {
                id: row.get(0)?,
                account_masked: row.get(1)?,
                status: row.get(2)?,
                priority: row.get(3)?,
                weight: row.get(4)?,
                proxy_url: row.get(5)?,
                web_tier: row.get(6)?,
                metadata_json: row.get(7)?,
                expires_at: row.get(8)?,
                cooldown_until: row.get(9)?,
                failure_count: row.get(10)?,
                request_count: row.get(11)?,
                success_count: row.get(12)?,
                last_latency_ms: row.get(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_grok_credential(&self, id: &str) -> VaultResult<bool> {
        self.conn.execute(
            "DELETE FROM grok_credential_models WHERE credential_id = ?1",
            [id],
        )?;
        self.conn.execute(
            "DELETE FROM grok_credential_quota_windows WHERE credential_id = ?1",
            [id],
        )?;
        Ok(self
            .conn
            .execute("DELETE FROM grok_credentials WHERE id = ?1", [id])?
            > 0)
    }

    pub fn set_grok_credential_enabled(&self, id: &str, enabled: bool) -> VaultResult<bool> {
        let changed = self.conn.execute(
            "UPDATE grok_credentials
             SET status = ?2, cooldown_until = CASE WHEN ?3 THEN NULL ELSE cooldown_until END,
                 failure_count = CASE WHEN ?3 THEN 0 ELSE failure_count END, updated_at = ?4
             WHERE id = ?1",
            params![
                id,
                if enabled { "active" } else { "disabled" },
                enabled,
                now_ts()
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn upsert_grok_quota_window(
        &self,
        credential_id: &str,
        mode: &str,
        remaining_queries: i64,
        total_queries: i64,
        window_size_seconds: i64,
        reset_at: i64,
    ) -> VaultResult<()> {
        self.conn.execute(
            "INSERT INTO grok_credential_quota_windows (
                credential_id, mode, remaining_queries, total_queries,
                window_size_seconds, reset_at, checked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(credential_id, mode) DO UPDATE SET
                remaining_queries=excluded.remaining_queries,
                total_queries=excluded.total_queries,
                window_size_seconds=excluded.window_size_seconds,
                reset_at=excluded.reset_at,
                checked_at=excluded.checked_at",
            params![
                credential_id,
                mode,
                remaining_queries.max(0),
                total_queries.max(0),
                window_size_seconds.max(1),
                reset_at,
                now_ts(),
            ],
        )?;
        Ok(())
    }

    pub fn list_grok_quota_windows(
        &self,
        credential_id: Option<&str>,
    ) -> VaultResult<Vec<GrokQuotaWindowRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT credential_id, mode, remaining_queries, total_queries,
                    window_size_seconds, reset_at, checked_at
             FROM grok_credential_quota_windows
             WHERE (?1 IS NULL OR credential_id = ?1)
             ORDER BY credential_id, mode",
        )?;
        let rows = stmt.query_map([credential_id], |row| {
            Ok(GrokQuotaWindowRecord {
                credential_id: row.get(0)?,
                mode: row.get(1)?,
                remaining_queries: row.get(2)?,
                total_queries: row.get(3)?,
                window_size_seconds: row.get(4)?,
                reset_at: row.get(5)?,
                checked_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_grok_credential_tier(&self, id: &str, tier: &str) -> VaultResult<bool> {
        Ok(self.conn.execute(
            "UPDATE grok_credentials SET web_tier = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, tier, now_ts()],
        )? > 0)
    }

    pub fn replace_grok_credential_models(
        &self,
        credential_id: &str,
        available_models: &[String],
        latency_ms: Option<u64>,
    ) -> VaultResult<()> {
        let checked_at = now_ts();
        self.conn.execute(
            "DELETE FROM grok_credential_models WHERE credential_id = ?1",
            [credential_id],
        )?;
        for model_slug in available_models {
            self.conn.execute(
                "INSERT INTO grok_credential_models (
                    credential_id, model_slug, status, error_code, latency_ms, checked_at
                 ) VALUES (?1, ?2, 'available', NULL, ?3, ?4)",
                params![
                    credential_id,
                    model_slug,
                    latency_ms.map(|value| value.min(i64::MAX as u64) as i64),
                    checked_at,
                ],
            )?;
        }
        Ok(())
    }

    pub fn list_grok_credential_model_availability(
        &self,
        credential_id: Option<&str>,
    ) -> VaultResult<Vec<GrokCredentialModelAvailability>> {
        let mut stmt = self.conn.prepare(
            "SELECT credential_id, model_slug, status, error_code, latency_ms, checked_at
             FROM grok_credential_models
             WHERE (?1 IS NULL OR credential_id = ?1)
             ORDER BY credential_id, model_slug",
        )?;
        let rows = stmt.query_map([credential_id], |row| {
            Ok(GrokCredentialModelAvailability {
                credential_id: row.get(0)?,
                model_slug: row.get(1)?,
                status: row.get(2)?,
                error_code: row.get(3)?,
                latency_ms: row.get(4)?,
                checked_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns only Grok models proven usable by at least one active credential.
    pub fn list_available_grok_models(&self) -> VaultResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT m.model_slug
             FROM grok_credential_models m
             JOIN grok_credentials c ON c.id = m.credential_id
             WHERE m.status = 'available' AND c.status = 'active'
               AND (c.cooldown_until IS NULL OR c.cooldown_until <= ?1)
               AND m.checked_at >= ?2
             ORDER BY m.model_slug",
        )?;
        let now = now_ts();
        let rows = stmt.query_map([now, now.saturating_sub(86_400)], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_grok_credential_success(&self, id: &str, latency_ms: u64) -> VaultResult<bool> {
        Ok(self.conn.execute(
            "UPDATE grok_credentials SET failure_count = 0, request_count = request_count + 1,
             success_count = success_count + 1, cooldown_until = NULL, last_success_at = ?2,
             last_latency_ms = ?3, last_failure_code = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now_ts(), latency_ms.min(i64::MAX as u64) as i64],
        )? > 0)
    }

    pub fn record_grok_credential_failure(
        &self,
        id: &str,
        code: &str,
        cooldown_seconds: Option<i64>,
        new_status: Option<&str>,
        latency_ms: u64,
    ) -> VaultResult<bool> {
        let now = now_ts();
        let cooldown_until = cooldown_seconds.map(|seconds| now.saturating_add(seconds.max(0)));
        Ok(self.conn.execute(
            "UPDATE grok_credentials SET failure_count = failure_count + 1,
             request_count = request_count + 1, status = COALESCE(?3, status),
             cooldown_until = ?4, last_failure_at = ?5, last_failure_code = ?2,
             last_latency_ms = ?6, updated_at = ?5 WHERE id = ?1",
            params![
                id,
                code,
                new_status,
                cooldown_until,
                now,
                latency_ms.min(i64::MAX as u64) as i64
            ],
        )? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_secret_is_encrypted_and_duplicate_email_updates() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let input = GrokCredentialUpsert {
            id: "grok-one".into(),
            account: "User@Example.test".into(),
            status: "active".into(),
            priority: 0,
            weight: 1.0,
            proxy_url: None,
            metadata_json: "{}".into(),
            expires_at: None,
            secret: GrokCredentialSecret {
                account: "User@Example.test".into(),
                password: "password-one".into(),
                sso_token: "header.payload.signature".into(),
            },
        };
        storage.upsert_grok_credential(&input).unwrap();
        assert!(storage.grok_credential_exists("user@example.test").unwrap());
        assert_eq!(storage.list_grok_credentials().unwrap().len(), 1);
        assert_eq!(
            storage.read_grok_credential_secret("grok-one").unwrap(),
            Some(input.secret.clone())
        );
        let raw: String = storage
            .conn
            .query_row("SELECT encrypted_secret FROM grok_credentials", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!raw.contains("password-one"));

        storage
            .upsert_grok_quota_window("grok-one", "fast", 12, 30, 18_000, 20_000)
            .unwrap();
        storage
            .set_grok_credential_tier("grok-one", "basic")
            .unwrap();
        storage
            .replace_grok_credential_models("grok-one", &["grok/grok-chat-fast".into()], Some(125))
            .unwrap();
        assert_eq!(storage.list_grok_quota_windows(None).unwrap().len(), 1);
        assert_eq!(
            storage.list_available_grok_models().unwrap(),
            ["grok/grok-chat-fast"]
        );
        assert_eq!(
            storage.list_grok_credentials().unwrap()[0].web_tier,
            "basic"
        );

        assert!(storage
            .set_grok_credential_enabled("grok-one", false)
            .unwrap());
        assert_eq!(
            storage.list_grok_credentials().unwrap()[0].status,
            "disabled"
        );
        assert!(storage
            .set_grok_credential_enabled("grok-one", true)
            .unwrap());
        assert_eq!(storage.list_grok_credentials().unwrap()[0].status, "active");
        assert!(!storage
            .set_grok_credential_enabled("missing", false)
            .unwrap());
    }
}
