use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{error::Error, fmt};

use super::{now_ts, Storage};

const MASTER_KEY_ID: &str = "credential-vault-v1";
#[cfg(windows)]
const PROTECTOR: &str = "windows-dpapi-current-user-v1";
const AAD_PREFIX: &[u8] = b"codexmanager:kiro-credential:v1:";
const VAULT_TEXT_PREFIX: &str = "vault:v1:";
const VAULT_TEXT_AAD_PREFIX: &[u8] = b"codexmanager:vault-text:v1:";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroCredentialSecret {
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroCredentialRecord {
    pub id: String,
    pub auth_method: String,
    pub email: Option<String>,
    pub auth_region: Option<String>,
    pub api_region: Option<String>,
    pub subscription: Option<String>,
    pub status: String,
    pub priority: i64,
    pub weight: f64,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub metadata_json: String,
    pub credit_limit: Option<f64>,
    pub credit_used: Option<f64>,
    pub expires_at: Option<i64>,
    pub cooldown_until: Option<i64>,
    pub failure_count: i64,
    pub request_count: i64,
    pub success_count: i64,
    pub last_latency_ms: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroCredentialModelAvailability {
    pub credential_id: String,
    pub model_slug: String,
    pub status: String,
    pub error_code: Option<String>,
    pub latency_ms: Option<i64>,
    pub checked_at: i64,
}

#[derive(Debug, Clone)]
pub struct KiroCredentialUpsert {
    pub id: String,
    pub auth_method: String,
    pub identity_hint: String,
    pub email: Option<String>,
    pub auth_region: Option<String>,
    pub api_region: Option<String>,
    pub subscription: Option<String>,
    pub status: String,
    pub priority: i64,
    pub weight: f64,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub metadata_json: String,
    pub credit_limit: Option<f64>,
    pub credit_used: Option<f64>,
    pub expires_at: Option<i64>,
    pub secret: KiroCredentialSecret,
}

#[derive(Debug)]
pub enum KiroVaultError {
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
    Cryptography(&'static str),
    Platform(String),
}

impl fmt::Display for KiroVaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "credential database error: {error}"),
            Self::Serialization(error) => write!(f, "credential serialization error: {error}"),
            Self::Cryptography(message) => write!(f, "credential cryptography error: {message}"),
            Self::Platform(message) => write!(f, "credential protector error: {message}"),
        }
    }
}

impl Error for KiroVaultError {}
impl From<rusqlite::Error> for KiroVaultError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

fn stable_fingerprint_metadata(input: &str, existing: Option<&str>) -> VaultResult<String> {
    let mut value =
        serde_json::from_str::<serde_json::Value>(input).unwrap_or_else(|_| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({});
    }
    let old = existing
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let object = value.as_object_mut().expect("fingerprint metadata object");

    let imported_or_old = |camel: &str, snake: &str| -> Option<String> {
        object
            .get(camel)
            .or_else(|| object.get(snake))
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| {
                old.get(camel)
                    .or_else(|| old.get(snake))
                    .and_then(serde_json::Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
            })
    };

    let machine_id = imported_or_old("machineId", "machine_id").unwrap_or_else(|| {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    });
    let system_version = imported_or_old("systemVersion", "system_version")
        .unwrap_or_else(|| "win32#10.0.22631".to_string());
    let node_version =
        imported_or_old("nodeVersion", "node_version").unwrap_or_else(|| "22.22.0".to_string());
    let kiro_version =
        imported_or_old("kiroVersion", "kiro_version").unwrap_or_else(|| "0.11.107".to_string());

    object.remove("machine_id");
    object.remove("system_version");
    object.remove("node_version");
    object.remove("kiro_version");
    object.insert("machineId".into(), machine_id.into());
    object.insert("systemVersion".into(), system_version.into());
    object.insert("nodeVersion".into(), node_version.into());
    object.insert("kiroVersion".into(), kiro_version.into());
    serde_json::to_string(&value).map_err(Into::into)
}
impl From<serde_json::Error> for KiroVaultError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

type VaultResult<T> = Result<T, KiroVaultError>;

impl Storage {
    pub(super) fn vault_identity_hash(
        &self,
        namespace: &str,
        identity_hint: &str,
    ) -> VaultResult<String> {
        let master_key = self.load_or_create_vault_master_key()?;
        identity_hash(&master_key, namespace, identity_hint)
    }

    /// Ensures the client identity used by Kiro is created once per credential and
    /// then kept in public metadata. This deliberately does not derive machineId
    /// from refreshToken, because refresh-token rotation must not change the device.
    pub fn ensure_kiro_credential_fingerprint(&self, id: &str) -> VaultResult<String> {
        let metadata: String = self.conn.query_row(
            "SELECT metadata_json FROM kiro_credentials WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        let stable = stable_fingerprint_metadata(&metadata, None)?;
        if stable != metadata {
            self.conn.execute(
                "UPDATE kiro_credentials SET metadata_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, &stable, now_ts()],
            )?;
        }
        Ok(stable)
    }

    pub(super) fn encrypt_vault_text(&self, context: &str, plaintext: &str) -> VaultResult<String> {
        let master_key = self.load_or_create_vault_master_key()?;
        let nonce = random_bytes::<12>();
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .map_err(|_| KiroVaultError::Cryptography("invalid master key"))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &vault_text_aad(context),
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("encryption failed"))?;
        Ok(format!(
            "{VAULT_TEXT_PREFIX}{}:{}",
            STANDARD_NO_PAD.encode(nonce),
            STANDARD_NO_PAD.encode(ciphertext)
        ))
    }

    pub(super) fn decrypt_vault_text(&self, context: &str, stored: &str) -> VaultResult<String> {
        let Some(encoded) = stored.strip_prefix(VAULT_TEXT_PREFIX) else {
            // Backward compatibility for databases created before the vault
            // format was introduced. The next write upgrades the value.
            return Ok(stored.to_string());
        };
        let (nonce, ciphertext) = encoded
            .split_once(':')
            .ok_or(KiroVaultError::Cryptography("invalid encrypted text"))?;
        let nonce = STANDARD_NO_PAD
            .decode(nonce)
            .map_err(|_| KiroVaultError::Cryptography("invalid encrypted text nonce"))?;
        if nonce.len() != 12 {
            return Err(KiroVaultError::Cryptography("invalid encrypted text nonce"));
        }
        let ciphertext = STANDARD_NO_PAD
            .decode(ciphertext)
            .map_err(|_| KiroVaultError::Cryptography("invalid encrypted text"))?;
        let master_key = self.load_or_create_vault_master_key()?;
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .map_err(|_| KiroVaultError::Cryptography("invalid master key"))?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &vault_text_aad(context),
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("decryption failed"))?;
        String::from_utf8(plaintext)
            .map_err(|_| KiroVaultError::Cryptography("invalid encrypted text encoding"))
    }

    pub fn upsert_kiro_credential(&self, input: &KiroCredentialUpsert) -> VaultResult<()> {
        let master_key = self.load_or_create_vault_master_key()?;
        let identity_hash = identity_hash(&master_key, &input.auth_method, &input.identity_hint)?;
        let effective_id = self
            .conn
            .query_row(
                "SELECT id FROM kiro_credentials WHERE auth_method = ?1 AND identity_hash = ?2",
                params![&input.auth_method, &identity_hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| input.id.clone());
        let existing_metadata = self
            .conn
            .query_row(
                "SELECT metadata_json FROM kiro_credentials WHERE id = ?1",
                params![&effective_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let effective_metadata =
            stable_fingerprint_metadata(&input.metadata_json, existing_metadata.as_deref())?;
        let mut effective_secret = input.secret.clone();
        if let Some(existing) = self.read_kiro_credential_secret(&effective_id)? {
            if effective_secret.access_token.is_none() {
                effective_secret.access_token = existing.access_token;
            }
            if effective_secret.client_id.is_none() {
                effective_secret.client_id = existing.client_id;
            }
            if effective_secret.client_secret.is_none() {
                effective_secret.client_secret = existing.client_secret;
            }
            if effective_secret.proxy_password.is_none() {
                effective_secret.proxy_password = existing.proxy_password;
            }
        }
        let nonce = random_bytes::<12>();
        let aad = credential_aad(&effective_id);
        let plaintext = serde_json::to_vec(&effective_secret)?;
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .map_err(|_| KiroVaultError::Cryptography("invalid master key"))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("encryption failed"))?;
        let now = now_ts();

        self.conn.execute(
            "INSERT INTO kiro_credentials (
                id, auth_method, identity_hash, email, auth_region, api_region, subscription,
                status, priority, weight, proxy_url, proxy_username, encrypted_secret, secret_nonce,
                metadata_json, credit_limit, credit_used, expires_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?19)
             ON CONFLICT(auth_method, identity_hash) DO UPDATE SET
                email=excluded.email, auth_region=excluded.auth_region, api_region=excluded.api_region,
                subscription=excluded.subscription, status=kiro_credentials.status,
                priority=kiro_credentials.priority, weight=kiro_credentials.weight,
                proxy_url=COALESCE(excluded.proxy_url, kiro_credentials.proxy_url),
                proxy_username=COALESCE(excluded.proxy_username, kiro_credentials.proxy_username),
                encrypted_secret=excluded.encrypted_secret, secret_nonce=excluded.secret_nonce,
                metadata_json=excluded.metadata_json, credit_limit=excluded.credit_limit,
                credit_used=excluded.credit_used, expires_at=excluded.expires_at, updated_at=excluded.updated_at",
            params![
                &effective_id, &input.auth_method, identity_hash, &input.email, &input.auth_region,
                &input.api_region, &input.subscription, &input.status, input.priority, input.weight,
                &input.proxy_url, &input.proxy_username, ciphertext, nonce.as_slice(),
                &effective_metadata, input.credit_limit, input.credit_used, input.expires_at, now,
            ],
        )?;
        Ok(())
    }

    pub fn kiro_credential_exists(
        &self,
        auth_method: &str,
        identity_hint: &str,
    ) -> VaultResult<bool> {
        let master_key = self.load_or_create_vault_master_key()?;
        let identity_hash = identity_hash(&master_key, auth_method, identity_hint)?;
        self.conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM kiro_credentials
                    WHERE auth_method = ?1 AND identity_hash = ?2
                )",
                params![auth_method, identity_hash],
                |row| row.get::<_, i64>(0),
            )
            .map(|exists| exists != 0)
            .map_err(Into::into)
    }

    pub fn list_kiro_credentials(&self) -> VaultResult<Vec<KiroCredentialRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, auth_method, email, auth_region, api_region, subscription, status,
                    priority, weight, proxy_url, proxy_username, metadata_json, credit_limit, credit_used,
                    expires_at, cooldown_until, failure_count, request_count, success_count,
                    last_latency_ms, created_at, updated_at
             FROM kiro_credentials ORDER BY priority DESC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(KiroCredentialRecord {
                id: row.get(0)?,
                auth_method: row.get(1)?,
                email: row.get(2)?,
                auth_region: row.get(3)?,
                api_region: row.get(4)?,
                subscription: row.get(5)?,
                status: row.get(6)?,
                priority: row.get(7)?,
                weight: row.get(8)?,
                proxy_url: row.get(9)?,
                proxy_username: row.get(10)?,
                metadata_json: row.get(11)?,
                credit_limit: row.get(12)?,
                credit_used: row.get(13)?,
                expires_at: row.get(14)?,
                cooldown_until: row.get(15)?,
                failure_count: row.get(16)?,
                request_count: row.get(17)?,
                success_count: row.get(18)?,
                last_latency_ms: row.get(19)?,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn read_kiro_credential_secret(
        &self,
        id: &str,
    ) -> VaultResult<Option<KiroCredentialSecret>> {
        let encrypted = self
            .conn
            .query_row(
                "SELECT encrypted_secret, secret_nonce FROM kiro_credentials WHERE id = ?1",
                [id],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        let Some((ciphertext, nonce)) = encrypted else {
            return Ok(None);
        };
        if nonce.len() != 12 {
            return Err(KiroVaultError::Cryptography("invalid nonce"));
        }
        let master_key = self.load_or_create_vault_master_key()?;
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .map_err(|_| KiroVaultError::Cryptography("invalid master key"))?;
        let aad = credential_aad(id);
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("decryption failed"))?;
        Ok(Some(serde_json::from_slice(&plaintext)?))
    }

    pub fn update_kiro_credential_tokens(
        &self,
        id: &str,
        mut secret: KiroCredentialSecret,
        expires_at: Option<i64>,
    ) -> VaultResult<bool> {
        let Some(existing) = self.read_kiro_credential_secret(id)? else {
            return Ok(false);
        };
        if secret.proxy_password.is_none() {
            secret.proxy_password = existing.proxy_password;
        }
        if secret.client_id.is_none() {
            secret.client_id = existing.client_id;
        }
        if secret.client_secret.is_none() {
            secret.client_secret = existing.client_secret;
        }
        let master_key = self.load_or_create_vault_master_key()?;
        let nonce = random_bytes::<12>();
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .map_err(|_| KiroVaultError::Cryptography("invalid master key"))?;
        let plaintext = serde_json::to_vec(&secret)?;
        let aad = credential_aad(id);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("encryption failed"))?;
        let changed = self.conn.execute(
            "UPDATE kiro_credentials SET encrypted_secret = ?2, secret_nonce = ?3, expires_at = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, ciphertext, nonce.as_slice(), expires_at, now_ts()],
        )?;
        Ok(changed > 0)
    }

    pub fn set_kiro_credential_enabled(&self, id: &str, enabled: bool) -> VaultResult<bool> {
        let changed = self.conn.execute(
            "UPDATE kiro_credentials
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

    pub fn delete_kiro_credential(&self, id: &str) -> VaultResult<bool> {
        self.conn.execute(
            "DELETE FROM kiro_credential_models WHERE credential_id = ?1",
            [id],
        )?;
        Ok(self
            .conn
            .execute("DELETE FROM kiro_credentials WHERE id = ?1", [id])?
            > 0)
    }

    pub fn upsert_kiro_credential_model_availability(
        &self,
        credential_id: &str,
        model_slug: &str,
        status: &str,
        error_code: Option<&str>,
        latency_ms: Option<u64>,
    ) -> VaultResult<()> {
        self.conn.execute(
            "INSERT INTO kiro_credential_models (
                credential_id, model_slug, status, error_code, latency_ms, checked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(credential_id, model_slug) DO UPDATE SET
                status=excluded.status, error_code=excluded.error_code,
                latency_ms=excluded.latency_ms, checked_at=excluded.checked_at",
            params![
                credential_id,
                model_slug,
                status,
                error_code,
                latency_ms.map(|value| value.min(i64::MAX as u64) as i64),
                now_ts(),
            ],
        )?;
        Ok(())
    }

    pub fn list_kiro_credential_model_availability(
        &self,
        credential_id: Option<&str>,
    ) -> VaultResult<Vec<KiroCredentialModelAvailability>> {
        let mut stmt = self.conn.prepare(
            "SELECT credential_id, model_slug, status, error_code, latency_ms, checked_at
             FROM kiro_credential_models
             WHERE (?1 IS NULL OR credential_id = ?1)
             ORDER BY credential_id, model_slug",
        )?;
        let rows = stmt.query_map([credential_id], |row| {
            Ok(KiroCredentialModelAvailability {
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

    /// Returns only models proven usable by at least one currently active credential.
    pub fn list_available_kiro_models(&self) -> VaultResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT m.model_slug
             FROM kiro_credential_models m
             JOIN kiro_credentials c ON c.id = m.credential_id
             WHERE m.status = 'available' AND c.status = 'active'
             ORDER BY m.model_slug",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_kiro_credential_routing(
        &self,
        id: &str,
        priority: i64,
        weight: f64,
        auth_region: Option<String>,
        api_region: Option<String>,
        proxy_url: Option<String>,
        proxy_username: Option<String>,
        proxy_password: Option<String>,
    ) -> VaultResult<bool> {
        let exists = self
            .conn
            .query_row(
                "SELECT expires_at FROM kiro_credentials WHERE id = ?1",
                [id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?;
        let Some(expires_at) = exists else {
            return Ok(false);
        };
        if let Some(proxy_password) = proxy_password {
            let Some(mut secret) = self.read_kiro_credential_secret(id)? else {
                return Ok(false);
            };
            secret.proxy_password = Some(proxy_password);
            self.update_kiro_credential_tokens(id, secret, expires_at)?;
        }
        let changed = self.conn.execute(
            "UPDATE kiro_credentials SET priority = ?2, weight = ?3, auth_region = ?4,
             api_region = ?5, proxy_url = ?6, proxy_username = ?7, updated_at = ?8
             WHERE id = ?1",
            params![
                id,
                priority,
                weight,
                auth_region,
                api_region,
                proxy_url,
                proxy_username,
                now_ts()
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn record_kiro_credential_success(&self, id: &str, latency_ms: u64) -> VaultResult<bool> {
        let changed = self.conn.execute(
            "UPDATE kiro_credentials SET failure_count = 0, request_count = request_count + 1,
             success_count = success_count + 1, cooldown_until = NULL, last_success_at = ?2,
             last_latency_ms = ?3, last_failure_code = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now_ts(), latency_ms.min(i64::MAX as u64) as i64],
        )?;
        Ok(changed > 0)
    }

    pub fn record_kiro_credential_failure(
        &self,
        id: &str,
        code: &str,
        cooldown_seconds: Option<i64>,
        new_status: Option<&str>,
        latency_ms: u64,
    ) -> VaultResult<bool> {
        let now = now_ts();
        let cooldown_until = cooldown_seconds.map(|seconds| now.saturating_add(seconds.max(0)));
        let changed = self.conn.execute(
            "UPDATE kiro_credentials SET failure_count = failure_count + 1,
             request_count = request_count + 1,
             status = COALESCE(?3, status),
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
        )?;
        Ok(changed > 0)
    }

    pub fn update_kiro_credential_quota(
        &self,
        id: &str,
        subscription: Option<String>,
        credit_limit: f64,
        credit_used: f64,
    ) -> VaultResult<bool> {
        let now = now_ts();
        let has_remaining = credit_limit <= 0.0 || credit_used < credit_limit;
        let changed = self.conn.execute(
            "UPDATE kiro_credentials SET subscription = COALESCE(?2, subscription),
             credit_limit = ?3, credit_used = ?4,
             status = CASE WHEN status = 'quota_exhausted' AND ?5 THEN 'active' ELSE status END,
             cooldown_until = CASE WHEN ?5 THEN NULL ELSE cooldown_until END,
             updated_at = ?6 WHERE id = ?1",
            params![
                id,
                subscription,
                credit_limit,
                credit_used,
                has_remaining,
                now
            ],
        )?;
        Ok(changed > 0)
    }

    fn load_or_create_vault_master_key(&self) -> VaultResult<[u8; 32]> {
        if let Some(key) = self.vault_master_key.get() {
            return Ok(*key);
        }

        if let Some((protector, wrapped)) = self
            .conn
            .query_row(
                "SELECT protector, wrapped_key FROM credential_vault_keys WHERE id = ?1",
                [MASTER_KEY_ID],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?
        {
            if protector != platform::protector_name() {
                return Err(KiroVaultError::Platform(format!(
                    "unsupported protector: {protector}"
                )));
            }
            let key = to_master_key(platform::unprotect(&wrapped)?)?;
            let _ = self.vault_master_key.set(key);
            return Ok(key);
        }

        let key = random_bytes::<32>();
        let wrapped = platform::protect(&key)?;
        let now = now_ts();
        self.conn.execute(
            "INSERT OR IGNORE INTO credential_vault_keys (id, protector, wrapped_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![MASTER_KEY_ID, platform::protector_name(), wrapped, now],
        )?;
        // Another process may have won INSERT OR IGNORE; always reload the persisted value.
        let (protector, wrapped) = self.conn.query_row(
            "SELECT protector, wrapped_key FROM credential_vault_keys WHERE id = ?1",
            [MASTER_KEY_ID],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        if protector != platform::protector_name() {
            return Err(KiroVaultError::Platform(
                "protector changed concurrently".into(),
            ));
        }
        let key = to_master_key(platform::unprotect(&wrapped)?)?;
        let _ = self.vault_master_key.set(key);
        Ok(key)
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0_u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn to_master_key(bytes: Vec<u8>) -> VaultResult<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| KiroVaultError::Cryptography("invalid wrapped master key"))
}

fn credential_aad(id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_PREFIX.len() + id.len());
    aad.extend_from_slice(AAD_PREFIX);
    aad.extend_from_slice(id.as_bytes());
    aad
}

fn vault_text_aad(context: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(VAULT_TEXT_AAD_PREFIX.len() + context.len());
    aad.extend_from_slice(VAULT_TEXT_AAD_PREFIX);
    aad.extend_from_slice(context.as_bytes());
    aad
}

fn identity_hash(master_key: &[u8; 32], auth_method: &str, hint: &str) -> VaultResult<String> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(master_key)
        .map_err(|_| KiroVaultError::Cryptography("identity key failed"))?;
    mac.update(auth_method.trim().to_ascii_lowercase().as_bytes());
    mac.update(b"\0");
    mac.update(hint.trim().to_ascii_lowercase().as_bytes());
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        mac.finalize().into_bytes(),
    ))
}

#[cfg(windows)]
mod platform {
    use super::{KiroVaultError, VaultResult, PROTECTOR};
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    pub fn protector_name() -> &'static str {
        PROTECTOR
    }

    pub fn protect(input: &[u8]) -> VaultResult<Vec<u8>> {
        crypt(input, true)
    }
    pub fn unprotect(input: &[u8]) -> VaultResult<Vec<u8>> {
        crypt(input, false)
    }

    fn crypt(input: &[u8], protect: bool) -> VaultResult<Vec<u8>> {
        let mut input_blob = CRYPT_INTEGER_BLOB {
            cbData: input.len() as u32,
            pbData: input.as_ptr() as *mut u8,
        };
        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &mut input_blob,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output_blob,
                )
            } else {
                CryptUnprotectData(
                    &mut input_blob,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output_blob,
                )
            }
        };
        if ok == 0 {
            return Err(KiroVaultError::Platform(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let output = unsafe {
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
        };
        unsafe {
            LocalFree(output_blob.pbData.cast());
        }
        Ok(output)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{KiroVaultError, VaultResult};
    use aes_gcm::{
        aead::{Aead, KeyInit, Payload},
        Aes256Gcm, Nonce,
    };
    use base64::Engine;
    use rand::{rngs::OsRng, RngCore};
    use std::{fs, path::PathBuf};

    const PROTECTOR: &str = "env-aes256gcm-v1";
    const ENV_KEY: &str = "CODEXMANAGER_VAULT_MASTER_KEY";
    const ENV_KEY_FILE: &str = "CODEXMANAGER_VAULT_MASTER_KEY_FILE";
    const NONCE_LEN: usize = 12;
    const AAD: &[u8] = b"codexmanager:vault-master-key:v1";

    pub fn protector_name() -> &'static str {
        PROTECTOR
    }

    pub fn protect(input: &[u8]) -> VaultResult<Vec<u8>> {
        let key = load_key_encryption_key()?;
        protect_with_key(input, &key)
    }

    fn protect_with_key(input: &[u8], key: &[u8; 32]) -> VaultResult<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|_| KiroVaultError::Cryptography("invalid vault master key"))?;
        let mut nonce = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: input,
                    aad: AAD,
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("master key wrapping failed"))?;
        let mut wrapped = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        wrapped.extend_from_slice(&nonce);
        wrapped.extend_from_slice(&ciphertext);
        Ok(wrapped)
    }

    pub fn unprotect(input: &[u8]) -> VaultResult<Vec<u8>> {
        let key = load_key_encryption_key()?;
        unprotect_with_key(input, &key)
    }

    fn unprotect_with_key(input: &[u8], key: &[u8; 32]) -> VaultResult<Vec<u8>> {
        if input.len() <= NONCE_LEN {
            return Err(KiroVaultError::Cryptography("invalid wrapped master key"));
        }
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|_| KiroVaultError::Cryptography("invalid vault master key"))?;
        cipher
            .decrypt(
                Nonce::from_slice(&input[..NONCE_LEN]),
                Payload {
                    msg: &input[NONCE_LEN..],
                    aad: AAD,
                },
            )
            .map_err(|_| KiroVaultError::Cryptography("master key unwrapping failed"))
    }

    fn load_key_encryption_key() -> VaultResult<[u8; 32]> {
        let raw = std::env::var(ENV_KEY)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Ok)
            .or_else(|| {
                std::env::var(ENV_KEY_FILE)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map(|path| read_secret_file(PathBuf::from(path)))
            })
            .ok_or_else(|| {
                KiroVaultError::Platform(format!(
                    "set {ENV_KEY} or {ENV_KEY_FILE} for the Linux credential vault"
                ))
            })??;
        decode_key(raw.trim())
    }

    fn read_secret_file(path: PathBuf) -> VaultResult<String> {
        fs::read_to_string(path).map_err(|error| {
            KiroVaultError::Platform(format!("read vault key file failed: {error}"))
        })
    }

    fn decode_key(raw: &str) -> VaultResult<[u8; 32]> {
        let raw = raw.strip_prefix("base64:").unwrap_or(raw).trim();
        let decoded = if raw.len() == 64 && raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            decode_hex(raw)?
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(raw)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw))
                .map_err(|_| {
                    KiroVaultError::Platform(
                        "vault master key must be 32-byte base64 or 64-character hex".into(),
                    )
                })?
        };
        decoded.try_into().map_err(|_| {
            KiroVaultError::Platform("vault master key must decode to exactly 32 bytes".into())
        })
    }

    fn decode_hex(raw: &str) -> VaultResult<Vec<u8>> {
        raw.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair)
                    .map_err(|_| KiroVaultError::Platform("invalid hex vault master key".into()))?;
                u8::from_str_radix(pair, 16)
                    .map_err(|_| KiroVaultError::Platform("invalid hex vault master key".into()))
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn key_decoder_accepts_base64_and_hex_without_echoing_material() {
            let bytes = [7_u8; 32];
            let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            assert_eq!(decode_key(&base64).unwrap(), bytes);
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(decode_key(&hex).unwrap(), bytes);
            assert!(decode_key("too-short").is_err());
        }

        #[test]
        fn wrapping_round_trips_and_rejects_a_different_key() {
            let key = [7_u8; 32];
            let wrong_key = [8_u8; 32];
            let plaintext = b"database-random-data-key";
            let wrapped = protect_with_key(plaintext, &key).unwrap();

            assert_ne!(wrapped.as_slice(), plaintext);
            assert_eq!(unprotect_with_key(&wrapped, &key).unwrap(), plaintext);
            assert!(unprotect_with_key(&wrapped, &wrong_key).is_err());
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn vault_master_key_is_cached_per_storage_connection() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();

        let first = storage.load_or_create_vault_master_key().unwrap();
        assert_eq!(storage.vault_master_key.get(), Some(&first));
        assert_eq!(storage.load_or_create_vault_master_key().unwrap(), first);
    }

    #[test]
    fn secrets_round_trip_without_plaintext_storage() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let input = KiroCredentialUpsert {
            id: "kiro-test".into(),
            auth_method: "idc".into(),
            identity_hint: "user@example.test".into(),
            email: Some("user@example.test".into()),
            auth_region: Some("us-east-1".into()),
            api_region: Some("us-east-1".into()),
            subscription: None,
            status: "active".into(),
            priority: 0,
            weight: 1.0,
            proxy_url: None,
            proxy_username: Some("proxy-user".into()),
            metadata_json: "{}".into(),
            credit_limit: None,
            credit_used: None,
            expires_at: None,
            secret: KiroCredentialSecret {
                refresh_token: "refresh-secret-sentinel".into(),
                access_token: None,
                client_id: Some("client".into()),
                client_secret: Some("client-secret-sentinel".into()),
                proxy_password: Some("proxy-secret-sentinel".into()),
            },
        };
        assert!(!storage
            .kiro_credential_exists("idc", "user@example.test")
            .unwrap());
        storage.upsert_kiro_credential(&input).unwrap();
        let first_metadata = storage.list_kiro_credentials().unwrap()[0]
            .metadata_json
            .clone();
        let first_fingerprint: serde_json::Value = serde_json::from_str(&first_metadata).unwrap();
        assert_eq!(first_fingerprint["machineId"].as_str().unwrap().len(), 64);
        assert_eq!(first_fingerprint["systemVersion"], "win32#10.0.22631");
        assert_eq!(first_fingerprint["nodeVersion"], "22.22.0");
        assert_eq!(first_fingerprint["kiroVersion"], "0.11.107");
        assert!(storage
            .kiro_credential_exists("idc", "user@example.test")
            .unwrap());
        assert!(!storage
            .kiro_credential_exists("social", "user@example.test")
            .unwrap());
        assert_eq!(
            storage.read_kiro_credential_secret("kiro-test").unwrap(),
            Some(input.secret.clone())
        );
        let raw: Vec<u8> = storage
            .conn
            .query_row("SELECT encrypted_secret FROM kiro_credentials", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret-sentinel"));
        assert_eq!(
            storage.list_kiro_credentials().unwrap()[0]
                .proxy_username
                .as_deref(),
            Some("proxy-user")
        );

        let mut update = input.clone();
        update.id = "ignored-new-id".into();
        update.secret.refresh_token = "rotated-secret".into();
        update.secret.client_id = None;
        update.secret.client_secret = None;
        update.secret.proxy_password = None;
        storage.upsert_kiro_credential(&update).unwrap();
        assert_eq!(
            storage.list_kiro_credentials().unwrap()[0].metadata_json,
            first_metadata
        );
        assert_eq!(storage.list_kiro_credentials().unwrap().len(), 1);
        let reimported = storage
            .read_kiro_credential_secret("kiro-test")
            .unwrap()
            .unwrap();
        assert_eq!(reimported.refresh_token, "rotated-secret");
        assert_eq!(reimported.client_id.as_deref(), Some("client"));
        assert_eq!(
            reimported.client_secret.as_deref(),
            Some("client-secret-sentinel")
        );
        assert_eq!(
            reimported.proxy_password.as_deref(),
            Some("proxy-secret-sentinel")
        );

        storage
            .update_kiro_credential_tokens(
                "kiro-test",
                KiroCredentialSecret {
                    refresh_token: "callback-rotated-secret".into(),
                    access_token: Some("fresh-access".into()),
                    client_id: None,
                    client_secret: None,
                    proxy_password: None,
                },
                Some(1_900_000_000),
            )
            .unwrap();
        let callback_secret = storage
            .read_kiro_credential_secret("kiro-test")
            .unwrap()
            .unwrap();
        assert_eq!(
            storage.list_kiro_credentials().unwrap()[0].metadata_json,
            first_metadata
        );
        assert_eq!(callback_secret.refresh_token, "callback-rotated-secret");
        assert_eq!(callback_secret.client_id.as_deref(), Some("client"));
        assert_eq!(
            callback_secret.client_secret.as_deref(),
            Some("client-secret-sentinel")
        );

        assert!(storage
            .record_kiro_credential_failure("kiro-test", "429", Some(60), None, 125,)
            .unwrap());
        let rate_limited = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(rate_limited.failure_count, 1);
        assert_eq!(rate_limited.request_count, 1);
        assert_eq!(rate_limited.last_latency_ms, Some(125));
        assert!(rate_limited.cooldown_until.is_some());
        assert!(storage
            .record_kiro_credential_success("kiro-test", 80)
            .unwrap());
        let recovered = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(recovered.failure_count, 0);
        assert_eq!(recovered.request_count, 2);
        assert_eq!(recovered.success_count, 1);
        assert_eq!(recovered.last_latency_ms, Some(80));
        assert!(recovered.cooldown_until.is_none());

        assert!(storage
            .update_kiro_credential_routing(
                "kiro-test",
                10,
                2.5,
                Some("us-west-2".into()),
                Some("eu-west-1".into()),
                Some("http://127.0.0.1:7897/".into()),
                Some("next-proxy-user".into()),
                Some("next-proxy-secret".into()),
            )
            .unwrap());
        let routed = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(routed.priority, 10);
        assert_eq!(routed.weight, 2.5);
        assert_eq!(routed.auth_region.as_deref(), Some("us-west-2"));
        assert_eq!(routed.api_region.as_deref(), Some("eu-west-1"));
        assert_eq!(routed.proxy_username.as_deref(), Some("next-proxy-user"));
        assert_eq!(
            storage
                .read_kiro_credential_secret("kiro-test")
                .unwrap()
                .unwrap()
                .proxy_password
                .as_deref(),
            Some("next-proxy-secret")
        );
        assert!(storage
            .update_kiro_credential_quota("kiro-test", Some("KIRO PRO".into()), 100.0, 25.5)
            .unwrap());
        let quota = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(quota.subscription.as_deref(), Some("KIRO PRO"));
        assert_eq!(quota.credit_limit, Some(100.0));
        assert_eq!(quota.credit_used, Some(25.5));

        storage
            .upsert_kiro_credential_model_availability(
                "kiro-test",
                "kiro/claude-sonnet-4.5",
                "available",
                None,
                Some(42),
            )
            .unwrap();
        storage
            .upsert_kiro_credential_model_availability(
                "kiro-test",
                "kiro/claude-opus-4.8",
                "unavailable",
                Some("unsupported_model"),
                Some(24),
            )
            .unwrap();
        assert_eq!(
            storage.list_available_kiro_models().unwrap(),
            vec!["kiro/claude-sonnet-4.5"]
        );
        assert_eq!(
            storage
                .list_kiro_credential_model_availability(Some("kiro-test"))
                .unwrap()
                .len(),
            2
        );

        assert!(storage
            .set_kiro_credential_enabled("kiro-test", false)
            .unwrap());
        assert_eq!(
            storage.list_kiro_credentials().unwrap()[0].status,
            "disabled"
        );
        assert!(storage.list_available_kiro_models().unwrap().is_empty());
        assert!(storage.delete_kiro_credential("kiro-test").unwrap());
        assert!(storage.list_kiro_credentials().unwrap().is_empty());
        assert!(storage
            .list_kiro_credential_model_availability(None)
            .unwrap()
            .is_empty());
    }
}
