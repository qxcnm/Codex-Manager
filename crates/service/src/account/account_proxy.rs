use codexmanager_core::storage::{
    derive_proxy_profile_url_metadata, now_ts, AccountProxySettings, ProxyProfile, Storage,
};
use codexmanager_core::rpc::types::{
    ProxySpeedTestListResult, ProxySpeedTestEntry,
    AccountProxyUrlTestListResult, AccountProxyUrlTestEntry,
    ProxyDiagnosticTestListResult, ProxyDiagnosticTestEntry,
};
use serde::Serialize;

use crate::account::proxy_health::{ProxyGeoInfo, ProxyHealthCheckResult};
use crate::account::proxy_testing::jobs::JobState;
use crate::account::proxy_testing::cloudflare_style::config::CfStyleConfig;
use crate::storage_helpers::{open_storage, StorageHandle};


pub(crate) const STATUS_NOT_CONFIGURED: &str = "not_configured";
pub(crate) const STATUS_UNCHECKED: &str = "unchecked";
pub(crate) const STATUS_CHECKING: &str = "checking";
pub(crate) const STATUS_INVALID_URL: &str = "invalid_url";
pub(crate) const ENV_ACCOUNT_PROXY_DEBUG: &str = "CODEXMANAGER_ACCOUNT_PROXY_DEBUG";
pub(crate) const SOURCE_CUSTOM: &str = "custom";
pub(crate) const SOURCE_PROFILE: &str = "profile";
const LOCAL_PROXY_EXPECTED_MESSAGE: &str = "Codex-Manager supports HTTP, HTTPS, SOCKS4, and SOCKS5 proxy URLs, for example http://host:port or socks5://host:port. For sing-box, paste the local mixed inbound address, e.g. http://127.0.0.1:7891.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountProxyMode {
    Disabled,
    Explicit {
        proxy_url: String,
        source: AccountProxySource,
    },
    Invalid {
        proxy_url: Option<String>,
        error: String,
        source: AccountProxySource,
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
    pub source: String,
    pub proxy_profile_id: Option<String>,
    pub proxy_profile_name: Option<String>,
    pub proxy_profile_enabled: Option<bool>,
    pub proxy_url: String,
    pub proxy_url_redacted: String,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub last_download_mbps: Option<f64>,
    pub last_upload_mbps: Option<f64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountProxySource {
    Custom,
    Profile,
}

#[derive(Debug, Clone)]
pub(crate) struct AccountProxyTestTarget {
    pub proxy_profile_id: Option<String>,
    pub proxy_url: String,
}

impl AccountProxySource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Custom => SOURCE_CUSTOM,
            Self::Profile => SOURCE_PROFILE,
        }
    }
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
    source: Option<&str>,
    proxy_profile_id: Option<&str>,
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
    let requested_source =
        resolve_requested_source(source, proxy_profile_id, proxy_url, previous.as_ref());
    let normalized_profile_id =
        normalize_profile_id_for_source(requested_source, proxy_profile_id, previous.as_ref())?;
    let normalized_proxy_url = match requested_source {
        AccountProxySource::Custom => normalize_proxy_url_for_setting(enabled, proxy_url)?,
        AccountProxySource::Profile => {
            normalize_proxy_url_for_storage_only(proxy_url).or_else(|| {
                previous
                    .as_ref()
                    .and_then(|settings| settings.proxy_url.clone())
            })
        }
    };
    let normalized_proxy_url_ref = normalized_proxy_url.as_deref();
    let previous_source = previous
        .as_ref()
        .map(account_proxy_source_from_settings)
        .unwrap_or(AccountProxySource::Custom);
    let previous_profile_id = previous
        .as_ref()
        .and_then(|settings| normalize_optional_str(settings.proxy_profile_id.as_deref()));
    let previous_proxy_url = previous
        .as_ref()
        .and_then(|settings| normalize_optional_str(settings.proxy_url.as_deref()));
    let binding_changed = requested_source != previous_source
        || normalized_profile_id.as_deref() != previous_profile_id.as_deref()
        || (requested_source == AccountProxySource::Custom
            && normalized_proxy_url_ref != previous_proxy_url.as_deref());
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
    ) = if binding_changed {
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
            Some(requested_source.as_str()),
            normalized_profile_id.as_deref(),
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

    if enabled && status.is_none() {
        let account_id_clone = account_id.to_string();
        std::thread::spawn(move || {
            if let Err(err) = test_account_proxy_settings(&account_id_clone, None, None, None, None)
            {
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
    source: Option<&str>,
    proxy_profile_id: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<AccountProxySettingsResponse, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;
    if enabled.is_some() || source.is_some() || proxy_profile_id.is_some() || proxy_url.is_some() {
        return test_account_proxy_draft_with_checker(
            &storage,
            account_id,
            enabled.unwrap_or(true),
            source,
            proxy_profile_id,
            proxy_url,
            |proxy_url| {
                crate::account::proxy_health::check_account_proxy(proxy_url, |country_code| {
                    storage
                        .find_cached_proxy_flag_by_country(country_code)
                        .unwrap_or(None)
                })
            },
        );
    }

    test_account_proxy_settings_with_checker(&storage, account_id, |proxy_url| {
        crate::account::proxy_health::check_account_proxy(proxy_url, |country_code| {
            storage
                .find_cached_proxy_flag_by_country(country_code)
                .unwrap_or(None)
        })
    })
}

pub(crate) fn test_account_proxy_latency(
    account_id: &str,
) -> Result<JobState, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let resolved = resolve_stored_account_proxy_test_target(&storage, account_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    Ok(registry.create_account_latency_job(
        account_id,
        resolved.proxy_profile_id.as_deref(),
        resolved.proxy_url.as_str(),
    ))
}

pub(crate) fn test_account_proxy_speed(
    account_id: &str,
    provider_id: Option<&str>,
    file_size_id: Option<&str>,
    diagnostic_provider_id: Option<&str>,
    diagnostic_file_size_id: Option<&str>,
) -> Result<JobState, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let resolved = resolve_stored_account_proxy_test_target(&storage, account_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    Ok(registry.create_account_speed_job(
        account_id,
        resolved.proxy_profile_id.as_deref(),
        resolved.proxy_url.as_str(),
        provider_id,
        file_size_id,
        diagnostic_provider_id,
        diagnostic_file_size_id,
    ))
}

pub(crate) fn test_account_proxy_cloudflare_style_speed(
    account_id: &str,
    config: CfStyleConfig,
) -> Result<JobState, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let resolved = resolve_stored_account_proxy_test_target(&storage, account_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    Ok(registry.create_account_cloudflare_style_speed_job(
        account_id,
        resolved.proxy_profile_id.as_deref(),
        resolved.proxy_url.as_str(),
        config,
    ))
}


pub(crate) fn get_account_proxy_test_job(
    account_id: &str,
    job_id: &str,
) -> Result<JobState, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    let job = registry
        .get_job(job_id.trim())
        .ok_or_else(|| "job not found".to_string())?;
    if job.scope != crate::account::proxy_testing::jobs::JobScope::AccountProxy {
        return Err("job not found".to_string());
    }
    if job.account_id.as_deref() != Some(account_id) {
        return Err("job not found".to_string());
    }
    Ok(job)
}

pub(crate) fn cancel_account_proxy_test_job(
    account_id: &str,
    job_id: &str,
) -> Result<(), String> {
    let _ = get_account_proxy_test_job(account_id, job_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    if registry.cancel_job(job_id.trim()) {
        Ok(())
    } else {
        Err("job not found".to_string())
    }
}

fn test_account_proxy_draft_with_checker<F>(
    storage: &Storage,
    account_id: &str,
    enabled: bool,
    source: Option<&str>,
    proxy_profile_id: Option<&str>,
    proxy_url: Option<&str>,
    checker: F,
) -> Result<AccountProxySettingsResponse, String>
where
    F: FnOnce(&str) -> ProxyHealthCheckResult,
{
    let draft_source = resolve_requested_source(source, proxy_profile_id, proxy_url, None);
    let normalized_profile_id =
        match normalize_profile_id_for_source(draft_source, proxy_profile_id, None) {
            Ok(value) => value,
            Err(err) => {
                return Ok(response_from_parts(
                    account_id,
                    enabled,
                    draft_source,
                    None,
                    None,
                    normalize_proxy_url_for_storage_only(proxy_url).unwrap_or_default(),
                    STATUS_INVALID_URL,
                    None,
                    None,
                    None,
                    Some(now_ts()),
                    Some(err),
                    None,
                ));
            }
        };
    let custom_proxy_url = normalize_proxy_url_for_storage_only(proxy_url).unwrap_or_default();
    let bound_profile = load_proxy_profile(storage, normalized_profile_id.as_deref())?;

    if !enabled {
        return Ok(response_from_parts(
            account_id,
            enabled,
            draft_source,
            normalized_profile_id,
            bound_profile.as_ref(),
            custom_proxy_url,
            STATUS_NOT_CONFIGURED,
            None,
            None,
            None,
            Some(now_ts()),
            None,
            None,
        ));
    }

    let normalized_proxy_url = match draft_source {
        AccountProxySource::Custom => {
            if custom_proxy_url.is_empty() {
                return Ok(response_from_parts(
                    account_id,
                    enabled,
                    draft_source,
                    normalized_profile_id,
                    bound_profile.as_ref(),
                    custom_proxy_url,
                    STATUS_NOT_CONFIGURED,
                    None,
                    None,
                    None,
                    Some(now_ts()),
                    None,
                    None,
                ));
            }
            match normalize_supported_proxy_url(custom_proxy_url.as_str()) {
                Ok(proxy_url) => proxy_url,
                Err(err) => {
                    return Ok(response_from_parts(
                        account_id,
                        enabled,
                        draft_source,
                        normalized_profile_id,
                        bound_profile.as_ref(),
                        custom_proxy_url,
                        STATUS_INVALID_URL,
                        None,
                        None,
                        None,
                        Some(now_ts()),
                        Some(err),
                        None,
                    ));
                }
            }
        }
        AccountProxySource::Profile => match resolve_profile_proxy_url(
            storage,
            account_id,
            normalized_profile_id.as_deref().unwrap_or_default(),
        ) {
            Ok((_, proxy_url)) => proxy_url,
            Err(err) => {
                return Ok(response_from_parts(
                    account_id,
                    enabled,
                    draft_source,
                    normalized_profile_id,
                    bound_profile.as_ref(),
                    custom_proxy_url,
                    STATUS_INVALID_URL,
                    None,
                    None,
                    None,
                    Some(now_ts()),
                    Some(err),
                    None,
                ));
            }
        },
    };

    let outcome = checker(normalized_proxy_url.as_str());
    Ok(response_from_parts(
        account_id,
        enabled,
        draft_source,
        normalized_profile_id,
        bound_profile.as_ref(),
        custom_proxy_url,
        outcome.status,
        outcome.latency_ms,
        None,
        None,
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

pub(crate) fn resolve_account_proxy_mode_from_storage(
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

    let source = account_proxy_source_from_settings(&settings);
    match source {
        AccountProxySource::Custom => {
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
                    source,
                };
            };

            match normalize_supported_proxy_url(proxy_url.as_str()) {
                Ok(proxy_url) => AccountProxyMode::Explicit { proxy_url, source },
                Err(err) => AccountProxyMode::Invalid {
                    proxy_url: Some(proxy_url.clone()),
                    error: format!(
                        "account explicit proxy for {} is invalid and fail-closed: {}. {}",
                        account_id, proxy_url, err
                    ),
                    source,
                },
            }
        }
        AccountProxySource::Profile => {
            let Some(proxy_profile_id) =
                normalize_optional_str(settings.proxy_profile_id.as_deref())
            else {
                return AccountProxyMode::Invalid {
                    proxy_url: None,
                    error: format!(
                        "account proxy profile for {} is invalid and fail-closed: missing proxyProfileId",
                        account_id
                    ),
                    source,
                };
            };
            match resolve_profile_proxy_url(storage, account_id, proxy_profile_id.as_str()) {
                Ok((profile, proxy_url)) => {
                    let source = if profile.enabled {
                        AccountProxySource::Profile
                    } else {
                        source
                    };
                    AccountProxyMode::Explicit { proxy_url, source }
                }
                Err(err) => AccountProxyMode::Invalid {
                    proxy_url: load_proxy_profile(storage, Some(proxy_profile_id.as_str()))
                        .ok()
                        .flatten()
                        .map(|profile| profile.proxy_url_redacted),
                    error: err,
                    source,
                },
            }
        }
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

fn resolve_requested_source(
    source: Option<&str>,
    proxy_profile_id: Option<&str>,
    proxy_url: Option<&str>,
    previous: Option<&AccountProxySettings>,
) -> AccountProxySource {
    match source.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case(SOURCE_PROFILE) => AccountProxySource::Profile,
        Some(value) if value.eq_ignore_ascii_case(SOURCE_CUSTOM) => AccountProxySource::Custom,
        Some(_) => AccountProxySource::Custom,
        None if proxy_profile_id.is_some_and(|value| !value.trim().is_empty()) => {
            AccountProxySource::Profile
        }
        None if proxy_url.is_some_and(|value| !value.trim().is_empty()) => {
            AccountProxySource::Custom
        }
        None => previous
            .map(account_proxy_source_from_settings)
            .unwrap_or(AccountProxySource::Custom),
    }
}

fn account_proxy_source_from_settings(settings: &AccountProxySettings) -> AccountProxySource {
    match settings
        .proxy_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) if value.eq_ignore_ascii_case(SOURCE_PROFILE) => AccountProxySource::Profile,
        Some(value) if value.eq_ignore_ascii_case(SOURCE_CUSTOM) => AccountProxySource::Custom,
        _ if settings
            .proxy_profile_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()) =>
        {
            AccountProxySource::Profile
        }
        _ => AccountProxySource::Custom,
    }
}

fn normalize_profile_id_for_source(
    source: AccountProxySource,
    proxy_profile_id: Option<&str>,
    previous: Option<&AccountProxySettings>,
) -> Result<Option<String>, String> {
    match source {
        AccountProxySource::Custom => Ok(None),
        AccountProxySource::Profile => {
            let current = normalize_optional_str(proxy_profile_id).or_else(|| {
                previous.and_then(|settings| {
                    normalize_optional_str(settings.proxy_profile_id.as_deref())
                })
            });
            current.map(Some).ok_or_else(|| {
                "proxyProfileId is required when account proxy source is profile".to_string()
            })
        }
    }
}

fn normalize_optional_str(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_proxy_url_for_storage_only(proxy_url: Option<&str>) -> Option<String> {
    normalize_optional_str(proxy_url)
}

fn load_proxy_profile(
    storage: &Storage,
    proxy_profile_id: Option<&str>,
) -> Result<Option<ProxyProfile>, String> {
    let Some(proxy_profile_id) = proxy_profile_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    storage
        .find_proxy_profile(proxy_profile_id)
        .map_err(|err| format!("read proxy profile failed: {err}"))
}

fn resolve_profile_proxy_url(
    storage: &Storage,
    account_id: &str,
    proxy_profile_id: &str,
) -> Result<(ProxyProfile, String), String> {
    let profile = load_proxy_profile(storage, Some(proxy_profile_id))?.ok_or_else(|| {
        format!(
            "account proxy profile for {} is missing and fail-closed: {}",
            account_id, proxy_profile_id
        )
    })?;
    if !profile.enabled {
        return Err(format!(
            "account proxy profile for {} is disabled and fail-closed: {}",
            account_id, proxy_profile_id
        ));
    }
    let normalized_proxy_url =
        normalize_supported_proxy_url(profile.proxy_url.as_str()).map_err(|err| {
            format!(
                "account proxy profile for {} is invalid and fail-closed: {} ({})",
                account_id, proxy_profile_id, err
            )
        })?;
    Ok((profile, normalized_proxy_url))
}

fn resolve_stored_account_proxy_test_target(
    storage: &Storage,
    account_id: &str,
) -> Result<AccountProxyTestTarget, String> {
    let settings = storage
        .find_account_proxy_settings(account_id)
        .map_err(|err| format!("read account proxy settings failed: {err}"))?
        .ok_or_else(|| format!("account proxy for {account_id} is not configured"))?;
    if !settings.enabled {
        return Err(format!("account proxy for {account_id} is not enabled"));
    }

    match account_proxy_source_from_settings(&settings) {
        AccountProxySource::Custom => {
            let proxy_url = settings
                .proxy_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("account proxy for {account_id} is not configured"))?;
            Ok(AccountProxyTestTarget {
                proxy_profile_id: None,
                proxy_url: normalize_supported_proxy_url(proxy_url)?,
            })
        }
        AccountProxySource::Profile => {
            let proxy_profile_id = normalize_optional_str(settings.proxy_profile_id.as_deref())
                .ok_or_else(|| {
                    format!(
                        "account proxy profile for {} is invalid and fail-closed: missing proxyProfileId",
                        account_id
                    )
                })?;
            let (_, proxy_url) =
                resolve_profile_proxy_url(storage, account_id, proxy_profile_id.as_str())?;
            Ok(AccountProxyTestTarget {
                proxy_profile_id: Some(proxy_profile_id),
                proxy_url,
            })
        }
    }
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
    let source = account_proxy_source_from_settings(&settings);

    let normalized_proxy_url = match resolve_account_proxy_mode_from_storage(storage, account_id) {
        AccountProxyMode::Disabled => {
            persist_check_status(storage, account_id, STATUS_NOT_CONFIGURED, None, None, None)?;
            crate::gateway::invalidate_account_proxy_cache(account_id);
            return read_or_default_response(storage, account_id);
        }
        AccountProxyMode::Invalid { error, .. } => {
            persist_check_status(
                storage,
                account_id,
                STATUS_INVALID_URL,
                None,
                Some(error.as_str()),
                None,
            )?;
            crate::gateway::invalidate_account_proxy_cache(account_id);
            return read_or_default_response(storage, account_id);
        }
        AccountProxyMode::Explicit {
            proxy_url,
            source: mode_source,
        } => {
            if mode_source == AccountProxySource::Custom {
                let stored_proxy_url = settings
                    .proxy_url
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default();
                if proxy_url != stored_proxy_url {
                    storage
                        .upsert_account_proxy_settings(
                            account_id,
                            settings.enabled,
                            Some(source.as_str()),
                            settings.proxy_profile_id.as_deref(),
                            Some(proxy_url.as_str()),
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
            }
            proxy_url
        }
    };

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
    match settings {
        Some(settings) => account_proxy_settings_response(storage, settings),
        None => Ok(default_response(account_id)),
    }
}

fn default_response(account_id: &str) -> AccountProxySettingsResponse {
    response_from_parts(
        account_id,
        false,
        AccountProxySource::Custom,
        None,
        None,
        String::new(),
        STATUS_NOT_CONFIGURED,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn account_proxy_settings_response(
    storage: &Storage,
    settings: AccountProxySettings,
) -> Result<AccountProxySettingsResponse, String> {
    let source = account_proxy_source_from_settings(&settings);
    let proxy_profile = load_proxy_profile(storage, settings.proxy_profile_id.as_deref())?;
    if source == AccountProxySource::Profile && proxy_profile.is_some() {
        let p = proxy_profile.as_ref().unwrap();
        Ok(response_from_parts(
            settings.account_id.as_str(),
            settings.enabled,
            source,
            settings.proxy_profile_id.clone(),
            proxy_profile.as_ref(),
            settings.proxy_url.unwrap_or_default(),
            p.status.as_str(),
            p.last_url_latency_ms,
            p.last_download_mbps,
            p.last_upload_mbps,
            p.last_tested_at,
            p.last_error.clone(),
            Some(&ProxyGeoInfo {
                ip: p.ip.clone(),
                country_code: p.country_code.clone(),
                country_name: p.country_name.clone(),
                region_name: p.region_name.clone(),
                city_name: p.city_name.clone(),
                geo_checked_at: p.last_tested_at,
                geo_error: p.last_error.clone(),
                asn: p.asn,
                as_org: p.as_org.clone(),
                isp: None,
                as_domain: None,
                timezone_id: p.timezone_id.clone(),
                timezone_offset: p.timezone_offset,
                timezone_utc: p.timezone_utc.clone(),
                flag_img_url: p.flag_img_url.clone(),
                flag_emoji: p.flag_emoji.clone(),
            }),
        ))
    } else {
        Ok(response_from_parts(
            settings.account_id.as_str(),
            settings.enabled,
            source,
            settings.proxy_profile_id.clone(),
            proxy_profile.as_ref(),
            settings.proxy_url.unwrap_or_default(),
            settings.status.as_str(),
            settings.latency_ms,
            settings.last_download_mbps,
            settings.last_upload_mbps,
            settings.last_check_at,
            settings.last_error,
            Some(&ProxyGeoInfo {
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
            }),
        ))
    }
}

fn response_from_parts(
    account_id: &str,
    enabled: bool,
    source: AccountProxySource,
    proxy_profile_id: Option<String>,
    proxy_profile: Option<&ProxyProfile>,
    proxy_url: impl Into<String>,
    status: &str,
    latency_ms: Option<i64>,
    last_download_mbps: Option<f64>,
    last_upload_mbps: Option<f64>,
    last_check_at: Option<i64>,
    last_error: Option<String>,
    geo: Option<&ProxyGeoInfo>,
) -> AccountProxySettingsResponse {
    let proxy_url = proxy_url.into();
    let proxy_url_redacted = match source {
        AccountProxySource::Custom => {
            if proxy_url.trim().is_empty() {
                String::new()
            } else {
                derive_proxy_profile_url_metadata(proxy_url.as_str()).proxy_url_redacted
            }
        }
        AccountProxySource::Profile => proxy_profile
            .map(|profile| profile.proxy_url_redacted.clone())
            .unwrap_or_else(|| {
                if proxy_profile_id.is_some() {
                    "<missing>".to_string()
                } else {
                    String::new()
                }
            }),
    };
    AccountProxySettingsResponse {
        account_id: account_id.to_string(),
        enabled,
        source: source.as_str().to_string(),
        proxy_profile_id,
        proxy_profile_name: proxy_profile.map(|profile| profile.name.clone()),
        proxy_profile_enabled: proxy_profile.map(|profile| profile.enabled),
        proxy_url,
        proxy_url_redacted,
        status: status.to_string(),
        latency_ms,
        last_download_mbps,
        last_upload_mbps,
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

pub(crate) fn get_account_proxy_speed_test_history(
    account_id: &str,
    limit: Option<usize>,
) -> Result<ProxySpeedTestListResult, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_proxy_speed_tests_by_account(account_id, limit)
        .map_err(|err| format!("list account speed tests failed: {err}"))?;

    let entries = items
        .into_iter()
        .map(|t| ProxySpeedTestEntry {
            id: t.id,
            scope: t.scope,
            proxy_profile_id: t.proxy_profile_id,
            account_id: t.account_id,
            status: t.status,
            provider: t.provider,
            observed_ip: t.observed_ip,
            observed_country: t.observed_country,
            observed_colo: t.observed_colo,
            max_payload_bytes: t.max_payload_bytes,
            samples_json: t.samples_json,
            download_summary_json: t.download_summary_json,
            upload_summary_json: t.upload_summary_json,
            started_at: t.started_at,
            finished_at: t.finished_at,
            error_code: t.error_code,
            error: t.error,
        })
        .collect();
    Ok(ProxySpeedTestListResult { items: entries })
}

pub(crate) fn get_account_proxy_latency_test_history(
    account_id: &str,
    limit: Option<usize>,
) -> Result<AccountProxyUrlTestListResult, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_account_proxy_url_tests(account_id, limit)
        .map_err(|err| format!("list account latency tests failed: {err}"))?;

    let entries = items
        .into_iter()
        .map(|t| AccountProxyUrlTestEntry {
            id: t.id,
            account_id: t.account_id,
            status: t.status,
            url_latency_ms: t.url_latency_ms,
            status_code: t.status_code,
            test_url: t.test_url,
            final_url: t.final_url,
            redirected: t.redirected,
            tested_at: t.tested_at,
            error_code: t.error_code,
            error: t.error,
        })
        .collect();
    Ok(AccountProxyUrlTestListResult { items: entries })
}

pub(crate) fn get_account_proxy_diagnostics_history(
    account_id: &str,
    limit: Option<usize>,
) -> Result<ProxyDiagnosticTestListResult, String> {
    let storage = open_storage_for_account(account_id)?;
    let account_id = normalize_account_id(account_id)?;
    ensure_account_exists(&storage, account_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_proxy_diagnostic_tests_by_account(account_id, limit)
        .map_err(|err| format!("list account diagnostic tests failed: {err}"))?;

    let entries = items
        .into_iter()
        .map(|t| ProxyDiagnosticTestEntry {
            id: t.id,
            scope: t.scope,
            proxy_profile_id: t.proxy_profile_id,
            account_id: t.account_id,
            status: t.status,
            provider: t.provider,
            file_size_id: t.file_size_id,
            downloaded_bytes: t.downloaded_bytes,
            duration_ms: t.duration_ms,
            mbps: t.mbps,
            tested_at: t.tested_at,
            error: t.error,
        })
        .collect();
    Ok(ProxyDiagnosticTestListResult { items: entries })
}
