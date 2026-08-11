use super::{now_ts, KiroVaultError, Storage};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

type VaultResult<T> = Result<T, KiroVaultError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProfileRecord {
    pub id: String,
    pub name: String,
    pub proxy_url: String,
    pub proxy_username: Option<String>,
    pub has_password: bool,
    pub status: String,
    pub fallback_mode: String,
    pub backup_proxy_id: Option<String>,
    pub exit_ip: Option<String>,
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub latency_ms: Option<i64>,
    pub last_probe_status: String,
    pub last_probe_error: Option<String>,
    pub last_probe_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ProxyProfileUpsert {
    pub id: String,
    pub name: String,
    pub proxy_url: String,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub keep_existing_password: bool,
    pub status: String,
    pub fallback_mode: String,
    pub backup_proxy_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountProxyBindingRecord {
    pub account_id: String,
    pub mode: String,
    pub proxy_profile_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ProxyProbeUpdate<'a> {
    pub status: &'a str,
    pub exit_ip: Option<&'a str>,
    pub country_code: Option<&'a str>,
    pub region: Option<&'a str>,
    pub latency_ms: Option<i64>,
    pub error: Option<&'a str>,
}

impl Storage {
    pub fn list_proxy_profiles(&self) -> VaultResult<Vec<ProxyProfileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, proxy_url, proxy_username,
                    CASE WHEN encrypted_password IS NULL OR encrypted_password = '' THEN 0 ELSE 1 END,
                    status, fallback_mode, backup_proxy_id, exit_ip, country_code, region,
                    latency_ms, last_probe_status, last_probe_error, last_probe_at, created_at, updated_at
             FROM proxy_profiles ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProxyProfileRecord {
                id: row.get(0)?, name: row.get(1)?, proxy_url: row.get(2)?,
                proxy_username: row.get(3)?, has_password: row.get::<_, i64>(4)? != 0,
                status: row.get(5)?, fallback_mode: row.get(6)?, backup_proxy_id: row.get(7)?,
                exit_ip: row.get(8)?, country_code: row.get(9)?, region: row.get(10)?,
                latency_ms: row.get(11)?, last_probe_status: row.get(12)?,
                last_probe_error: row.get(13)?, last_probe_at: row.get(14)?,
                created_at: row.get(15)?, updated_at: row.get(16)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_proxy_profile(&self, input: &ProxyProfileUpsert) -> VaultResult<()> {
        let existing_password = if input.keep_existing_password && input.proxy_password.is_none() {
            self.conn.query_row(
                "SELECT encrypted_password FROM proxy_profiles WHERE id = ?1",
                [&input.id], |row| row.get::<_, Option<String>>(0),
            ).optional()?.flatten()
        } else { None };
        let encrypted_password = match input.proxy_password.as_deref() {
            Some(value) if !value.is_empty() => Some(self.encrypt_vault_text(
                &format!("proxy-profile:{}", input.id), value,
            )?),
            _ => existing_password,
        };
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO proxy_profiles (
                id, name, proxy_url, proxy_username, encrypted_password, status,
                fallback_mode, backup_proxy_id, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, proxy_url = excluded.proxy_url,
                proxy_username = excluded.proxy_username,
                encrypted_password = excluded.encrypted_password,
                status = excluded.status, fallback_mode = excluded.fallback_mode,
                backup_proxy_id = excluded.backup_proxy_id, updated_at = excluded.updated_at",
            params![input.id, input.name, input.proxy_url, input.proxy_username,
                encrypted_password, input.status, input.fallback_mode, input.backup_proxy_id, now],
        )?;
        Ok(())
    }

    pub fn proxy_profile_effective_url(&self, id: &str) -> VaultResult<Option<String>> {
        let row = self.conn.query_row(
            "SELECT proxy_url, proxy_username, encrypted_password FROM proxy_profiles
             WHERE id = ?1",
            [id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)),
        ).optional()?;
        let Some((url, username, encrypted_password)) = row else { return Ok(None); };
        let password = encrypted_password.map(|value| {
            self.decrypt_vault_text(&format!("proxy-profile:{id}"), &value)
        }).transpose()?;
        Ok(Some(inject_proxy_credentials(&url, username.as_deref(), password.as_deref())))
    }

    pub fn delete_proxy_profile(&self, id: &str) -> VaultResult<bool> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("UPDATE account_proxy_bindings SET mode = 'inherit', proxy_profile_id = NULL, updated_at = ?2 WHERE proxy_profile_id = ?1", params![id, now_ts()])?;
        let changed = tx.execute("DELETE FROM proxy_profiles WHERE id = ?1", [id])? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn set_account_proxy_bindings(&self, account_ids: &[String], mode: &str, profile_id: Option<&str>) -> VaultResult<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let now = now_ts();
        let mut changed = 0;
        for account_id in account_ids {
            changed += tx.execute(
                "INSERT INTO account_proxy_bindings(account_id, mode, proxy_profile_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(account_id) DO UPDATE SET mode = excluded.mode,
                    proxy_profile_id = excluded.proxy_profile_id, updated_at = excluded.updated_at",
                params![account_id, mode, profile_id, now],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn list_account_proxy_bindings(&self) -> VaultResult<Vec<AccountProxyBindingRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, mode, proxy_profile_id, updated_at FROM account_proxy_bindings",
        )?;
        let rows = stmt.query_map([], |row| Ok(AccountProxyBindingRecord {
            account_id: row.get(0)?, mode: row.get(1)?, proxy_profile_id: row.get(2)?, updated_at: row.get(3)?,
        }))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_proxy_probe(&self, id: &str, update: ProxyProbeUpdate<'_>) -> VaultResult<()> {
        self.conn.execute(
            "UPDATE proxy_profiles SET exit_ip = ?2, country_code = ?3, region = ?4,
                latency_ms = ?5, last_probe_status = ?6, last_probe_error = ?7,
                last_probe_at = ?8, updated_at = ?8 WHERE id = ?1",
            params![id, update.exit_ip, update.country_code, update.region, update.latency_ms,
                update.status, update.error, now_ts()],
        )?;
        Ok(())
    }
}

fn inject_proxy_credentials(url: &str, username: Option<&str>, password: Option<&str>) -> String {
    let Some(username) = username.filter(|value| !value.is_empty()) else { return url.to_string(); };
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_string(); };
    let encoded_user = percent_encode_userinfo(username);
    let encoded_password = password.map(percent_encode_userinfo).unwrap_or_default();
    let auth = if password.is_some() { format!("{encoded_user}:{encoded_password}@") } else { format!("{encoded_user}@") };
    format!("{scheme}://{auth}{rest}")
}

fn percent_encode_userinfo(value: &str) -> String {
    value.bytes().map(|byte| match byte {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
        _ => format!("%{byte:02X}"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
    }

    #[test]
    fn proxy_password_is_encrypted_and_restored_only_for_runtime() {
        let storage = storage();
        storage.upsert_proxy_profile(&ProxyProfileUpsert {
            id: "proxy-a".into(), name: "Tokyo".into(),
            proxy_url: "http://127.0.0.1:7890/".into(),
            proxy_username: Some("user name".into()), proxy_password: Some("secret:value".into()),
            keep_existing_password: false, status: "active".into(), fallback_mode: "none".into(),
            backup_proxy_id: None,
        }).expect("save proxy");
        let stored: String = storage.conn.query_row(
            "SELECT encrypted_password FROM proxy_profiles WHERE id = 'proxy-a'", [], |row| row.get(0),
        ).expect("encrypted password");
        assert!(!stored.contains("secret:value"));
        assert_eq!(storage.proxy_profile_effective_url("proxy-a").expect("url").as_deref(),
            Some("http://user%20name:secret%3Avalue@127.0.0.1:7890/"));
        let listed = storage.list_proxy_profiles().expect("list");
        assert!(listed[0].has_password);
        assert_eq!(listed[0].proxy_url, "http://127.0.0.1:7890/");
    }

    #[test]
    fn account_binding_modes_are_persisted_without_credentials() {
        let storage = storage();
        storage.conn.execute(
            "INSERT INTO accounts(id,label,issuer,status,created_at,updated_at) VALUES('acc','A','openai','active',1,1)", [],
        ).expect("account");
        storage.set_account_proxy_bindings(&["acc".into()], "direct", None).expect("bind direct");
        let bindings = storage.list_account_proxy_bindings().expect("bindings");
        assert_eq!(bindings[0].mode, "direct");
        assert_eq!(bindings[0].proxy_profile_id, None);
    }
}
