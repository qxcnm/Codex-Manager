use codexmanager_core::storage::{now_ts, AccountProxySettings, Storage};
use serde::Serialize;

use crate::account::proxy_health::{ProxyGeoInfo, ProxyHealthCheckResult};
use crate::storage_helpers::{open_storage, StorageHandle};

pub(crate) const STATUS_NOT_CONFIGURED: &str = "not_configured";
pub(crate) const STATUS_UNCHECKED: &str = "unchecked";
pub(crate) const STATUS_CHECKING: &str = "checking";
pub(crate) const STATUS_INVALID_URL: &str = "invalid_url";
pub(crate) const ENV_ACCOUNT_PROXY_DEBUG: &str = "CODEXMANAGER_ACCOUNT_PROXY_DEBUG";
const LOCAL_PROXY_EXPECTED_MESSAGE: &str = "Codex-Manager supports HTTP, HTTPS, SOCKS4, and SOCKS5 proxy URLs, for example http://host:port or socks5://host:port. For sing-box, paste the local mixed inbound address, e.g. http://127.0.0.1:7891.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountProxyMode {
    Disabled,
    Explicit {
        proxy_url: String,
    },
    Invalid {
        proxy_url: Option<String>,
        error: String,
    },
}

impl AccountProxyMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Explicit { .. } => "explicit",
            Self::Invalid { .. } => "invalid",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountProxySettingsResponse {
    pub account_id: String,
    pub enabled: bool,
    pub proxy_url: String,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub last_check_at: Option<i64>,
    pub last_error: Option<String>,
    pub ip: Option<String>,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub region_name: Option<String>,
    pub city_name: Option<String>,
    pub geo_checked_at: Option<i64>,
    pub geo_error: Option<String>,
    pub asn: Option<i64>,
    pub as_org: Option<String>,
    pub isp: Option<String>,
    pub as_domain: Option<String>,
    pub timezone_id: Option<String>,
    pub timezone_offset: Option<i64>,
    pub timezone_utc: Option<String>,
    pub flag_img_url: Option<String>,
    pub flag_emoji: Option<String>,
}

pub(crate) fn get_account_proxy_settings(
    account_id: &str,
) -> Result<AccountProxySettingsResponse, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;
    read_or_default_response(&storage, account_id)
}

pub(crate) fn set_account_proxy_settings(
    account_id: &str,
    enabled: bool,
    proxy_url: Option<&str>,
    status: Option<&str>,
    latency_ms: Option<i64>,
    last_error: Option<&str>,
    ip: Option<&str>,
    country_code: Option<&str>,
    country_name: Option<&str>,
    region_name: Option<&str>,
    city_name: Option<&str>,
    geo_checked_at: Option<i64>,
    geo_error: Option<&str>,
) -> Result<AccountProxySettingsResponse, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let previous = storage
        .find_account_proxy_settings(account_id)
        .map_err(|err| format!("read account proxy settings failed: {err}"))?;
    let normalized_proxy_url = normalize_proxy_url_for_setting(enabled, proxy_url)?;
    let normalized_proxy_url_ref = normalized_proxy_url.as_deref();
    let previous_proxy_url = previous
        .as_ref()
        .and_then(|settings| settings.proxy_url.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let proxy_url_changed = normalized_proxy_url_ref != previous_proxy_url;
    let default_status = if enabled {
        STATUS_UNCHECKED
    } else {
        STATUS_NOT_CONFIGURED
    };

    let (
        final_status,
        final_latency,
        final_last_error,
        final_ip,
        final_country_code,
        final_country_name,
        final_region_name,
        final_city_name,
        final_geo_checked_at,
        final_geo_error,
        final_asn,
        final_as_org,
        final_isp,
        final_as_domain,
        final_timezone_id,
        final_timezone_offset,
        final_timezone_utc,
        final_flag_img_url,
        final_flag_emoji,
    ) = if proxy_url_changed {
        if status.is_some() {
            (
                status.unwrap_or(default_status),
                latency_ms,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        } else {
            (
                default_status,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        }
    } else {
        let prev = previous.as_ref();
        (
            status.unwrap_or_else(|| prev.map(|s| s.status.as_str()).unwrap_or(default_status)),
            latency_ms.or_else(|| prev.and_then(|s| s.latency_ms)),
            last_error.or_else(|| prev.and_then(|s| s.last_error.as_deref())),
            ip.or_else(|| prev.and_then(|s| s.ip.as_deref())),
            country_code.or_else(|| prev.and_then(|s| s.country_code.as_deref())),
            country_name.or_else(|| prev.and_then(|s| s.country_name.as_deref())),
            region_name.or_else(|| prev.and_then(|s| s.region_name.as_deref())),
            city_name.or_else(|| prev.and_then(|s| s.city_name.as_deref())),
            geo_checked_at.or_else(|| prev.and_then(|s| s.geo_checked_at)),
            geo_error.or_else(|| prev.and_then(|s| s.geo_error.as_deref())),
            prev.and_then(|s| s.asn),
            prev.and_then(|s| s.as_org.as_deref()),
            prev.and_then(|s| s.isp.as_deref()),
            prev.and_then(|s| s.as_domain.as_deref()),
            prev.and_then(|s| s.timezone_id.as_deref()),
            prev.and_then(|s| s.timezone_offset),
            prev.and_then(|s| s.timezone_utc.as_deref()),
            prev.and_then(|s| s.flag_img_url.as_deref()),
            prev.and_then(|s| s.flag_emoji.as_deref()),
        )
    };

    let final_last_check_at =
        if final_status == STATUS_UNCHECKED || final_status == STATUS_NOT_CONFIGURED {
            None
        } else {
            Some(now_ts())
        };

    storage
        .upsert_account_proxy_settings(
            account_id,
            enabled,
            normalized_proxy_url_ref,
            final_status,
            final_latency,
            final_last_check_at,
            final_last_error,
            final_ip,
            final_country_code,
            final_country_name,
            final_region_name,
            final_city_name,
            final_geo_checked_at,
            final_geo_error,
            final_asn,
            final_as_org,
            final_isp,
            final_as_domain,
            final_timezone_id,
            final_timezone_offset,
            final_timezone_utc,
            final_flag_img_url,
            final_flag_emoji,
        )
        .map_err(|err| format!("store account proxy settings failed: {err}"))?;
    crate::gateway::invalidate_account_proxy_cache(account_id);

    // If enabled, proxy_url is present, and status was NOT passed, trigger an asynchronous background check
    if enabled && normalized_proxy_url_ref.is_some() && status.is_none() {
        let account_id_clone = account_id.to_string();
        std::thread::spawn(move || {
            if let Err(err) = test_account_proxy_settings(&account_id_clone, None, None) {
                log::error!(
                    "background proxy check failed for account {}: {}",
                    account_id_clone,
                    err
                );
            }
        });
    }

    read_or_default_response(&storage, account_id)
}

pub(crate) fn clear_account_proxy_settings(
    account_id: &str,
) -> Result<AccountProxySettingsResponse, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;
    storage
        .clear_account_proxy_settings(account_id)
        .map_err(|err| format!("clear account proxy settings failed: {err}"))?;
    crate::gateway::invalidate_account_proxy_cache(account_id);
    Ok(default_response(account_id))
}

pub(crate) fn test_account_proxy_settings(
    account_id: &str,
    enabled: Option<bool>,
    proxy_url: Option<&str>,
) -> Result<AccountProxySettingsResponse, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;
    match (enabled, proxy_url) {
        (Some(enabled), proxy_url) => {
            test_account_proxy_draft_with_checker(account_id, enabled, proxy_url, |proxy_url| {
                crate::account::proxy_health::check_account_proxy(proxy_url, |country_code| {
                    storage
                        .find_cached_proxy_flag_by_country(country_code)
                        .unwrap_or(None)
                })
            })
        }
        (None, None) => {
            test_account_proxy_settings_with_checker(&storage, account_id, |proxy_url| {
                crate::account::proxy_health::check_account_proxy(proxy_url, |country_code| {
                    storage
                        .find_cached_proxy_flag_by_country(country_code)
                        .unwrap_or(None)
                })
            })
        }
        (None, Some(proxy_url)) => {
            test_account_proxy_draft_with_checker(account_id, true, Some(proxy_url), |proxy_url| {
                crate::account::proxy_health::check_account_proxy(proxy_url, |country_code| {
                    storage
                        .find_cached_proxy_flag_by_country(country_code)
                        .unwrap_or(None)
                })
            })
        }
    }
}

fn test_account_proxy_draft_with_checker<F>(
    account_id: &str,
    enabled: bool,
    proxy_url: Option<&str>,
    checker: F,
) -> Result<AccountProxySettingsResponse, String>
where
    F: FnOnce(&str) -> ProxyHealthCheckResult,
{
    let proxy_url = proxy_url.map(str::trim).unwrap_or_default();
    if proxy_url.is_empty() {
        return Ok(response_from_parts(
            account_id,
            enabled,
            proxy_url,
            STATUS_NOT_CONFIGURED,
            None,
            Some(now_ts()),
            None,
            None,
        ));
    }

    let normalized_proxy_url = match normalize_supported_proxy_url(proxy_url) {
        Ok(normalized_proxy_url) => normalized_proxy_url,
        Err(err) => {
            return Ok(response_from_parts(
                account_id,
                enabled,
                proxy_url,
                STATUS_INVALID_URL,
                None,
                Some(now_ts()),
                Some(err),
                None,
            ));
        }
    };

    let outcome = checker(normalized_proxy_url.as_str());
    Ok(response_from_parts(
        account_id,
        enabled,
        normalized_proxy_url,
        outcome.status,
        outcome.latency_ms,
        Some(now_ts()),
        outcome.last_error,
        outcome.geo.as_ref(),
    ))
}

pub(crate) fn resolve_account_proxy_mode(account_id: &str) -> AccountProxyMode {
    let normalized_account_id = account_id.trim();
    if normalized_account_id.is_empty() {
        return AccountProxyMode::Disabled;
    }

    let Some(storage) = open_storage() else {
        return AccountProxyMode::Disabled;
    };
    resolve_account_proxy_mode_from_storage(&storage, normalized_account_id)
}

pub(crate) fn account_proxy_debug_enabled() -> bool {
    std::env::var(ENV_ACCOUNT_PROXY_DEBUG)
        .ok()
        .map(|value| {
            let normalized = value.trim();
            normalized == "1"
                || normalized.eq_ignore_ascii_case("true")
                || normalized.eq_ignore_ascii_case("yes")
                || normalized.eq_ignore_ascii_case("on")
        })
        .unwrap_or(false)
}

pub(crate) fn redact_proxy_url_for_log(proxy_url: &str) -> String {
    let trimmed = proxy_url.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return "<invalid>".to_string();
    };
    let scheme = parsed.scheme();
    let host = parsed.host_str().unwrap_or("-");
    match parsed.port_or_known_default() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    }
}

fn open_storage_for_account(account_id: &str) -> Result<StorageHandle, String> {
    normalize_account_id(account_id)?;
    open_storage().ok_or_else(|| "storage unavailable".to_string())
}

fn normalize_account_id(account_id: &str) -> Result<&str, String> {
    let trimmed = account_id.trim();
    if trimmed.is_empty() {
        Err("missing accountId".to_string())
    } else {
        Ok(trimmed)
    }
}

fn resolve_account_proxy_mode_from_storage(
    storage: &Storage,
    account_id: &str,
) -> AccountProxyMode {
    let settings = match storage.find_account_proxy_settings(account_id) {
        Ok(settings) => settings,
        Err(err) => {
            log::warn!(
                "event=account_proxy_mode_read_failed account_id={} err={}",
                account_id,
                err
            );
            return AccountProxyMode::Disabled;
        }
    };
    let Some(settings) = settings else {
        return AccountProxyMode::Disabled;
    };
    if !settings.enabled {
        return AccountProxyMode::Disabled;
    }

    let trimmed_proxy_url = settings
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(proxy_url) = trimmed_proxy_url else {
        return AccountProxyMode::Invalid {
            proxy_url: None,
            error: format!(
                "account explicit proxy for {} is invalid and fail-closed: missing proxy URL",
                account_id
            ),
        };
    };

    match normalize_supported_proxy_url(proxy_url.as_str()) {
        Ok(proxy_url) => AccountProxyMode::Explicit { proxy_url },
        Err(err) => AccountProxyMode::Invalid {
            proxy_url: Some(proxy_url.clone()),
            error: format!(
                "account explicit proxy for {} is invalid and fail-closed: {}. {}",
                account_id, proxy_url, err
            ),
        },
    }
}

fn ensure_account_exists(storage: &Storage, account_id: &str) -> Result<(), String> {
    let found = storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("read account failed: {err}"))?
        .is_some();
    if found {
        Ok(())
    } else {
        Err("account not found".to_string())
    }
}

fn normalize_proxy_url_for_setting(
    enabled: bool,
    proxy_url: Option<&str>,
) -> Result<Option<String>, String> {
    let trimmed = proxy_url.map(str::trim).filter(|value| !value.is_empty());
    if !enabled {
        return Ok(trimmed.map(ToString::to_string));
    }
    let Some(proxy_url) = trimmed else {
        return Err("proxyUrl is required when account proxy is enabled".to_string());
    };
    normalize_supported_proxy_url(proxy_url).map(Some)
}

pub(crate) fn normalize_supported_proxy_url(proxy_url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(proxy_url)
        .map_err(|err| format!("invalid proxyUrl: {err}. {LOCAL_PROXY_EXPECTED_MESSAGE}"))?;
    match parsed.scheme() {
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h" => {
            Ok(proxy_url.trim().to_string())
        }
        "vless" | "trojan" | "ss" | "hysteria2" => Err(LOCAL_PROXY_EXPECTED_MESSAGE.to_string()),
        other => Err(format!(
            "unsupported proxy URL scheme: {other}. {LOCAL_PROXY_EXPECTED_MESSAGE}"
        )),
    }
}

fn test_account_proxy_settings_with_checker<F>(
    storage: &Storage,
    account_id: &str,
    checker: F,
) -> Result<AccountProxySettingsResponse, String>
where
    F: FnOnce(&str) -> ProxyHealthCheckResult,
{
    let settings = storage
        .find_account_proxy_settings(account_id)
        .map_err(|err| format!("read account proxy settings failed: {err}"))?;
    let Some(settings) = settings else {
        return Ok(default_response(account_id));
    };

    let proxy_url = settings
        .proxy_url
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if proxy_url.is_empty() {
        persist_check_status(storage, account_id, STATUS_NOT_CONFIGURED, None, None, None)?;
        crate::gateway::invalidate_account_proxy_cache(account_id);
        return read_or_default_response(storage, account_id);
    }

    let normalized_proxy_url = match normalize_supported_proxy_url(proxy_url) {
        Ok(normalized_proxy_url) => normalized_proxy_url,
        Err(err) => {
            persist_check_status(
                storage,
                account_id,
                STATUS_INVALID_URL,
                None,
                Some(err.as_str()),
                None,
            )?;
            crate::gateway::invalidate_account_proxy_cache(account_id);
            return read_or_default_response(storage, account_id);
        }
    };

    if normalized_proxy_url != proxy_url {
        storage
            .upsert_account_proxy_settings(
                account_id,
                settings.enabled,
                Some(normalized_proxy_url.as_str()),
                settings.status.as_str(),
                settings.latency_ms,
                settings.last_check_at,
                settings.last_error.as_deref(),
                settings.ip.as_deref(),
                settings.country_code.as_deref(),
                settings.country_name.as_deref(),
                settings.region_name.as_deref(),
                settings.city_name.as_deref(),
                settings.geo_checked_at,
                settings.geo_error.as_deref(),
                settings.asn,
                settings.as_org.as_deref(),
                settings.isp.as_deref(),
                settings.as_domain.as_deref(),
                settings.timezone_id.as_deref(),
                settings.timezone_offset,
                settings.timezone_utc.as_deref(),
                settings.flag_img_url.as_deref(),
                settings.flag_emoji.as_deref(),
            )
            .map_err(|err| format!("store account proxy settings failed: {err}"))?;
    }

    let current_geo = ProxyGeoInfo {
        ip: settings.ip.clone(),
        country_code: settings.country_code.clone(),
        country_name: settings.country_name.clone(),
        region_name: settings.region_name.clone(),
        city_name: settings.city_name.clone(),
        geo_checked_at: settings.geo_checked_at,
        geo_error: settings.geo_error.clone(),
        asn: settings.asn,
        as_org: settings.as_org.clone(),
        isp: settings.isp.clone(),
        as_domain: settings.as_domain.clone(),
        timezone_id: settings.timezone_id.clone(),
        timezone_offset: settings.timezone_offset,
        timezone_utc: settings.timezone_utc.clone(),
        flag_img_url: settings.flag_img_url.clone(),
        flag_emoji: settings.flag_emoji.clone(),
    };
    persist_check_status(
        storage,
        account_id,
        STATUS_CHECKING,
        None,
        None,
        Some(&current_geo),
    )?;
    let outcome = checker(normalized_proxy_url.as_str());
    persist_check_status(
        storage,
        account_id,
        outcome.status,
        outcome.latency_ms,
        outcome.last_error.as_deref(),
        outcome.geo.as_ref(),
    )?;
    crate::gateway::invalidate_account_proxy_cache(account_id);
    read_or_default_response(storage, account_id)
}

fn persist_check_status(
    storage: &Storage,
    account_id: &str,
    status: &str,
    latency_ms: Option<i64>,
    last_error: Option<&str>,
    geo: Option<&ProxyGeoInfo>,
) -> Result<(), String> {
    storage
        .update_account_proxy_check_status(
            account_id,
            status,
            latency_ms,
            Some(now_ts()),
            last_error,
            geo.and_then(|value| value.ip.as_deref()),
            geo.and_then(|value| value.country_code.as_deref()),
            geo.and_then(|value| value.country_name.as_deref()),
            geo.and_then(|value| value.region_name.as_deref()),
            geo.and_then(|value| value.city_name.as_deref()),
            geo.and_then(|value| value.geo_checked_at),
            geo.and_then(|value| value.geo_error.as_deref()),
            geo.and_then(|value| value.asn),
            geo.and_then(|value| value.as_org.as_deref()),
            geo.and_then(|value| value.isp.as_deref()),
            geo.and_then(|value| value.as_domain.as_deref()),
            geo.and_then(|value| value.timezone_id.as_deref()),
            geo.and_then(|value| value.timezone_offset),
            geo.and_then(|value| value.timezone_utc.as_deref()),
            geo.and_then(|value| value.flag_img_url.as_deref()),
            geo.and_then(|value| value.flag_emoji.as_deref()),
        )
        .map_err(|err| format!("update account proxy status failed: {err}"))
}

fn read_or_default_response(
    storage: &Storage,
    account_id: &str,
) -> Result<AccountProxySettingsResponse, String> {
    let settings = storage
        .find_account_proxy_settings(account_id)
        .map_err(|err| format!("read account proxy settings failed: {err}"))?;
    Ok(settings
        .map(account_proxy_settings_response)
        .unwrap_or_else(|| default_response(account_id)))
}

fn default_response(account_id: &str) -> AccountProxySettingsResponse {
    response_from_parts(
        account_id,
        false,
        String::new(),
        STATUS_NOT_CONFIGURED,
        None,
        None,
        None,
        None,
    )
}

fn account_proxy_settings_response(settings: AccountProxySettings) -> AccountProxySettingsResponse {
    AccountProxySettingsResponse {
        account_id: settings.account_id,
        enabled: settings.enabled,
        proxy_url: settings.proxy_url.unwrap_or_default(),
        status: settings.status,
        latency_ms: settings.latency_ms,
        last_check_at: settings.last_check_at,
        last_error: settings.last_error,
        ip: settings.ip,
        country_code: settings.country_code,
        country_name: settings.country_name,
        region_name: settings.region_name,
        city_name: settings.city_name,
        geo_checked_at: settings.geo_checked_at,
        geo_error: settings.geo_error,
        asn: settings.asn,
        as_org: settings.as_org,
        isp: settings.isp,
        as_domain: settings.as_domain,
        timezone_id: settings.timezone_id,
        timezone_offset: settings.timezone_offset,
        timezone_utc: settings.timezone_utc,
        flag_img_url: settings.flag_img_url,
        flag_emoji: settings.flag_emoji,
    }
}

fn response_from_parts(
    account_id: &str,
    enabled: bool,
    proxy_url: impl Into<String>,
    status: &str,
    latency_ms: Option<i64>,
    last_check_at: Option<i64>,
    last_error: Option<String>,
    geo: Option<&ProxyGeoInfo>,
) -> AccountProxySettingsResponse {
    AccountProxySettingsResponse {
        account_id: account_id.to_string(),
        enabled,
        proxy_url: proxy_url.into(),
        status: status.to_string(),
        latency_ms,
        last_check_at,
        last_error,
        ip: geo.and_then(|value| value.ip.clone()),
        country_code: geo.and_then(|value| value.country_code.clone()),
        country_name: geo.and_then(|value| value.country_name.clone()),
        region_name: geo.and_then(|value| value.region_name.clone()),
        city_name: geo.and_then(|value| value.city_name.clone()),
        geo_checked_at: geo.and_then(|value| value.geo_checked_at),
        geo_error: geo.and_then(|value| value.geo_error.clone()),
        asn: geo.and_then(|value| value.asn),
        as_org: geo.and_then(|value| value.as_org.clone()),
        isp: geo.and_then(|value| value.isp.clone()),
        as_domain: geo.and_then(|value| value.as_domain.clone()),
        timezone_id: geo.and_then(|value| value.timezone_id.clone()),
        timezone_offset: geo.and_then(|value| value.timezone_offset),
        timezone_utc: geo.and_then(|value| value.timezone_utc.clone()),
        flag_img_url: geo.and_then(|value| value.flag_img_url.clone()),
        flag_emoji: geo.and_then(|value| value.flag_emoji.clone()),
    }
}
