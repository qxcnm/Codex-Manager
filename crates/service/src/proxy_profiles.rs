use codexmanager_core::storage::{
    AccountProxyBindingRecord, ProxyProbeUpdate, ProxyProfileRecord, ProxyProfileUpsert,
};
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};
use url::Url;

use crate::storage_helpers::open_storage;

#[derive(Debug, Clone)]
pub(crate) enum AccountProxyResolution {
    Inherit,
    Direct,
    Proxy(String),
    Blocked,
}

#[derive(Debug, Clone)]
struct RuntimeProfile {
    active: bool,
    effective_url: Option<String>,
    fallback_mode: String,
    backup_proxy_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RuntimeProxyState {
    profiles: HashMap<String, RuntimeProfile>,
    bindings: HashMap<String, AccountProxyBindingRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyProfilesSnapshot {
    items: Vec<ProxyProfileRecord>,
    bindings: Vec<AccountProxyBindingRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyBindingResult {
    updated: usize,
}

static RUNTIME_STATE: OnceLock<RwLock<Option<RuntimeProxyState>>> = OnceLock::new();

fn runtime_state_cell() -> &'static RwLock<Option<RuntimeProxyState>> {
    RUNTIME_STATE.get_or_init(|| RwLock::new(None))
}

pub(crate) fn list() -> Result<ProxyProfilesSnapshot, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    Ok(ProxyProfilesSnapshot {
        items: storage.list_proxy_profiles().map_err(|err| err.to_string())?,
        bindings: storage
            .list_account_proxy_bindings()
            .map_err(|err| err.to_string())?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upsert(
    id: Option<&str>,
    name: &str,
    raw_proxy_url: &str,
    username: Option<&str>,
    password: Option<&str>,
    keep_existing_password: bool,
    status: Option<&str>,
    fallback_mode: Option<&str>,
    backup_proxy_id: Option<&str>,
) -> Result<ProxyProfileRecord, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("代理名称不能为空".to_string());
    }
    let id = id.map(str::trim).filter(|value| !value.is_empty())
        .map(str::to_string).unwrap_or_else(new_id);
    let parsed = sanitize_proxy_url(raw_proxy_url, username, password)?;
    let status = normalize_status(status)?;
    let fallback_mode = normalize_fallback_mode(fallback_mode)?;
    let backup_proxy_id = backup_proxy_id.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    if backup_proxy_id.as_deref() == Some(id.as_str()) {
        return Err("备用代理不能选择自身".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage.upsert_proxy_profile(&ProxyProfileUpsert {
        id: id.clone(),
        name: name.to_string(),
        proxy_url: parsed.url,
        proxy_username: parsed.username,
        proxy_password: parsed.password,
        keep_existing_password,
        status,
        fallback_mode,
        backup_proxy_id,
    }).map_err(|err| err.to_string())?;
    reload_runtime_state()?;
    storage.list_proxy_profiles().map_err(|err| err.to_string())?
        .into_iter().find(|item| item.id == id).ok_or_else(|| "代理保存后未找到".to_string())
}

pub(crate) fn delete(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() { return Err("missing proxy id".to_string()); }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if !storage.delete_proxy_profile(id).map_err(|err| err.to_string())? {
        return Err("代理不存在".to_string());
    }
    reload_runtime_state()
}

pub(crate) fn bind_accounts(account_ids: Vec<String>, mode: &str, profile_id: Option<&str>) -> Result<ProxyBindingResult, String> {
    let mut account_ids = account_ids.into_iter().map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()).collect::<Vec<_>>();
    account_ids.sort(); account_ids.dedup();
    if account_ids.is_empty() { return Err("请选择账号".to_string()); }
    let mode = mode.trim().to_ascii_lowercase();
    if !matches!(mode.as_str(), "inherit" | "direct" | "profile") {
        return Err("不支持的代理绑定模式".to_string());
    }
    let profile_id = profile_id.map(str::trim).filter(|value| !value.is_empty());
    if mode == "profile" && profile_id.is_none() { return Err("请选择代理出口".to_string()); }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if let Some(profile_id) = profile_id {
        if !storage.list_proxy_profiles().map_err(|err| err.to_string())?.iter().any(|item| item.id == profile_id) {
            return Err("代理出口不存在".to_string());
        }
    }
    let updated = storage.set_account_proxy_bindings(&account_ids, &mode, if mode == "profile" { profile_id } else { None })
        .map_err(|err| err.to_string())?;
    reload_runtime_state()?;
    Ok(ProxyBindingResult { updated })
}

pub(crate) fn probe(id: &str) -> Result<ProxyProfileRecord, String> {
    let id = id.trim();
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let proxy_url = storage.proxy_profile_effective_url(id).map_err(|err| err.to_string())?
        .ok_or_else(|| "代理不存在".to_string())?;
    let started = Instant::now();
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(9))
        .proxy(reqwest::Proxy::all(&proxy_url).map_err(|_| "代理地址无效".to_string())?)
        .build().map_err(|err| format!("代理客户端创建失败: {err}"))?;
    let result = client.get("https://ipinfo.io/json").send().and_then(|response| response.error_for_status())
        .and_then(|response| response.json::<serde_json::Value>());
    let latency_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    match result {
        Ok(value) => {
            let ip = value.get("ip").and_then(|v| v.as_str());
            let country = value.get("country").and_then(|v| v.as_str());
            let region = value.get("region").and_then(|v| v.as_str());
            storage.update_proxy_probe(id, ProxyProbeUpdate { status: "available", exit_ip: ip,
                country_code: country, region, latency_ms: Some(latency_ms), error: None })
                .map_err(|err| err.to_string())?;
        }
        Err(err) => {
            let safe_error = safe_probe_error(&err.to_string());
            storage.update_proxy_probe(id, ProxyProbeUpdate { status: "failed", latency_ms: Some(latency_ms),
                error: Some(&safe_error), ..Default::default() }).map_err(|db_err| db_err.to_string())?;
        }
    }
    reload_runtime_state()?;
    storage.list_proxy_profiles().map_err(|err| err.to_string())?.into_iter()
        .find(|item| item.id == id).ok_or_else(|| "代理不存在".to_string())
}

pub(crate) fn resolve_for_account(account_id: &str) -> AccountProxyResolution {
    ensure_runtime_state();
    let guard = crate::lock_utils::read_recover(runtime_state_cell(), "proxy_profile_runtime_state");
    let Some(state) = guard.as_ref() else { return AccountProxyResolution::Inherit; };
    let Some(binding) = state.bindings.get(account_id) else { return AccountProxyResolution::Inherit; };
    match binding.mode.as_str() {
        "direct" => AccountProxyResolution::Direct,
        "profile" => binding.proxy_profile_id.as_ref()
            .map(|id| resolve_profile(state, id, 0))
            .unwrap_or(AccountProxyResolution::Blocked),
        _ => AccountProxyResolution::Inherit,
    }
}

pub(crate) fn account_is_proxy_blocked(account_id: &str) -> bool {
    matches!(resolve_for_account(account_id), AccountProxyResolution::Blocked)
}

pub(crate) fn reload_runtime_state() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let records = storage.list_proxy_profiles().map_err(|err| err.to_string())?;
    let bindings = storage.list_account_proxy_bindings().map_err(|err| err.to_string())?;
    let mut state = RuntimeProxyState::default();
    for record in records {
        let effective_url = storage.proxy_profile_effective_url(&record.id).map_err(|err| err.to_string())?;
        state.profiles.insert(record.id.clone(), RuntimeProfile {
            // A single probe failure is diagnostic only. Network jitter must not
            // instantly eject every account bound to this exit.
            active: record.status == "active",
            effective_url,
            fallback_mode: record.fallback_mode,
            backup_proxy_id: record.backup_proxy_id,
        });
    }
    state.bindings = bindings.into_iter().map(|item| (item.account_id.clone(), item)).collect();
    *crate::lock_utils::write_recover(runtime_state_cell(), "proxy_profile_runtime_state") = Some(state);
    crate::gateway::invalidate_account_proxy_clients();
    Ok(())
}

fn resolve_profile(state: &RuntimeProxyState, id: &str, depth: usize) -> AccountProxyResolution {
    if depth > 2 { return AccountProxyResolution::Blocked; }
    let Some(profile) = state.profiles.get(id) else { return AccountProxyResolution::Blocked; };
    if profile.active {
        return profile.effective_url.clone().map(AccountProxyResolution::Proxy)
            .unwrap_or(AccountProxyResolution::Blocked);
    }
    match profile.fallback_mode.as_str() {
        "direct" => AccountProxyResolution::Direct,
        "proxy" => profile.backup_proxy_id.as_deref()
            .map(|backup| resolve_profile(state, backup, depth + 1))
            .unwrap_or(AccountProxyResolution::Blocked),
        _ => AccountProxyResolution::Blocked,
    }
}

fn ensure_runtime_state() {
    if crate::lock_utils::read_recover(runtime_state_cell(), "proxy_profile_runtime_state").is_none() {
        let _ = reload_runtime_state();
    }
}

struct SanitizedProxy { url: String, username: Option<String>, password: Option<String> }

fn sanitize_proxy_url(raw: &str, username: Option<&str>, password: Option<&str>) -> Result<SanitizedProxy, String> {
    let mut url = Url::parse(raw.trim()).map_err(|_| "代理地址格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err("仅支持 HTTP、HTTPS、SOCKS5 代理".to_string());
    }
    if url.host_str().is_none() || url.port_or_known_default().is_none() {
        return Err("代理地址缺少主机或端口".to_string());
    }
    let embedded_username = (!url.username().is_empty()).then(|| url.username().to_string());
    let embedded_password = url.password().map(str::to_string);
    url.set_username("").map_err(|_| "代理用户名无效".to_string())?;
    url.set_password(None).map_err(|_| "代理密码无效".to_string())?;
    Ok(SanitizedProxy {
        url: url.to_string(),
        username: username.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).or(embedded_username),
        password: password.filter(|value| !value.is_empty()).map(str::to_string).or(embedded_password),
    })
}

fn normalize_status(value: Option<&str>) -> Result<String, String> {
    match value.unwrap_or("active").trim().to_ascii_lowercase().as_str() {
        "active" => Ok("active".into()), "disabled" => Ok("disabled".into()),
        _ => Err("不支持的代理状态".into()),
    }
}

fn normalize_fallback_mode(value: Option<&str>) -> Result<String, String> {
    match value.unwrap_or("none").trim().to_ascii_lowercase().as_str() {
        "none" => Ok("none".into()), "direct" => Ok("direct".into()), "proxy" => Ok("proxy".into()),
        _ => Err("不支持的故障切换方式".into()),
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 12]; OsRng.fill_bytes(&mut bytes);
    format!("proxy-{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

fn safe_probe_error(raw: &str) -> String {
    let mut text = raw.replace('\n', " ").replace('\r', " ");
    if text.len() > 240 { text.truncate(240); }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_proxy_credentials_are_removed_from_public_url() {
        let parsed = sanitize_proxy_url("http://user:secret@proxy.test:8080", None, None)
            .expect("parse proxy");
        assert_eq!(parsed.url, "http://proxy.test:8080/");
        assert_eq!(parsed.username.as_deref(), Some("user"));
        assert_eq!(parsed.password.as_deref(), Some("secret"));
    }

    #[test]
    fn failed_profile_is_fail_closed_unless_fallback_is_explicit() {
        let mut state = RuntimeProxyState::default();
        state.profiles.insert("blocked".into(), RuntimeProfile {
            active: false, effective_url: Some("http://proxy:8080".into()),
            fallback_mode: "none".into(), backup_proxy_id: None,
        });
        state.profiles.insert("direct".into(), RuntimeProfile {
            active: false, effective_url: Some("http://proxy:8080".into()),
            fallback_mode: "direct".into(), backup_proxy_id: None,
        });
        assert!(matches!(resolve_profile(&state, "blocked", 0), AccountProxyResolution::Blocked));
        assert!(matches!(resolve_profile(&state, "direct", 0), AccountProxyResolution::Direct));
    }
}
