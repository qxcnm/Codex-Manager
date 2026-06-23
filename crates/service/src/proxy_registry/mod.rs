use codexmanager_core::rpc::types::{
    ProxyProfileEntry, ProxySpeedTestListResult, ProxySpeedTestEntry,
    ProxyProfileUrlTestListResult, ProxyProfileUrlTestEntry,
    ProxyDiagnosticTestListResult, ProxyDiagnosticTestEntry,
};
use codexmanager_core::storage::{
    ProxyProfile, ProxyProfileCreateInput, ProxyProfileUpdateInput,
};
use crate::account::proxy_testing::jobs::JobState;
use crate::account::proxy_testing::cloudflare_style::config::CfStyleConfig;
use serde_json::Value;


use crate::storage_helpers::{generate_proxy_profile_id, open_storage};

pub(crate) mod validation;

pub(crate) fn list_proxy_profiles() -> Result<Vec<ProxyProfileEntry>, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let items = storage
        .list_proxy_profiles()
        .map_err(|err| format!("list proxy profiles failed: {err}"))?;
    
    let mut entries = Vec::new();
    for item in items {
        let accounts_count = storage
            .list_account_ids_bound_to_proxy_profile(&item.id)
            .map(|l| l.len() as i64)
            .unwrap_or(0);
        let mut entry = proxy_profile_entry(item);
        entry.accounts_count = Some(accounts_count);
        entries.push(entry);
    }
    Ok(entries)
}

pub(crate) fn create_proxy_profile(
    name: Option<String>,
    proxy_url: Option<String>,
    enabled: Option<bool>,
    tags_json: Option<String>,
    notes: Option<String>,
) -> Result<ProxyProfileEntry, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let normalized_name = normalize_required_text("name", name)?;
    let normalized_proxy_url = normalize_proxy_url(proxy_url)?;
    let created = storage
        .create_proxy_profile(&ProxyProfileCreateInput {
            id: generate_proxy_profile_id(),
            name: normalized_name,
            proxy_url: normalized_proxy_url,
            enabled: enabled.unwrap_or(true),
            tags_json: normalize_tags_json(tags_json)?,
            notes: normalize_optional_text(notes),
        })
        .map_err(|err| format!("create proxy profile failed: {err}"))?;
    let mut entry = proxy_profile_entry(created);
    entry.accounts_count = Some(0);
    Ok(entry)
}

pub(crate) fn update_proxy_profile(
    id: &str,
    name: Option<String>,
    proxy_url: Option<String>,
    enabled: Option<bool>,
    tags_json: Option<String>,
    notes: Option<String>,
) -> Result<ProxyProfileEntry, String> {
    let normalized_id = normalize_required_str("id", id)?;
    let normalized_name = normalize_optional_required_text("name", name)?;
    let normalized_proxy_url = match proxy_url {
        Some(raw) => Some(validation::normalize_proxy_profile_url(raw.trim())?),
        None => None,
    };
    let url_changed = normalized_proxy_url.is_some();
    let updated = storage_update_proxy_profile(
        normalized_id,
        normalized_name,
        normalized_proxy_url,
        enabled,
        normalize_tags_json(tags_json)?,
        normalize_optional_text(notes),
        url_changed,
    )?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let accounts_count = storage
        .list_account_ids_bound_to_proxy_profile(&updated.id)
        .map(|l| l.len() as i64)
        .unwrap_or(0);
    let mut entry = proxy_profile_entry(updated);
    entry.accounts_count = Some(accounts_count);
    Ok(entry)
}

pub(crate) fn delete_proxy_profile(id: &str) -> Result<(), String> {
    let normalized_id = normalize_required_str("id", id)?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let bound_accounts = storage
        .list_account_ids_bound_to_proxy_profile(normalized_id)
        .map_err(|err| format!("read proxy profile bindings failed: {err}"))?;
    if !bound_accounts.is_empty() {
        return Err(format!(
            "proxy profile is still bound to accounts: {}. Update or clear those account proxy bindings first.",
            bound_accounts.join(", ")
        ));
    }
    let deleted = storage
        .delete_proxy_profile(normalized_id)
        .map_err(|err| format!("delete proxy profile failed: {err}"))?;
    if deleted {
        Ok(())
    } else {
        Err("proxy profile not found".to_string())
    }
}

pub(crate) fn test_proxy_profile(id: &str) -> Result<ProxyProfileEntry, String> {
    let normalized_id = normalize_required_str("id", id)?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let profile = storage
        .find_proxy_profile(normalized_id)
        .map_err(|err| format!("read proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())?;

    let proxy_url = validation::normalize_proxy_profile_url(&profile.proxy_url)?;
    let outcome = crate::account::proxy_health::check_account_proxy(&proxy_url, |country_code| {
        storage
            .find_cached_proxy_flag_by_country(country_code)
            .unwrap_or(None)
    });

    let updated = storage
        .update_proxy_profile(&ProxyProfileUpdateInput {
            id: profile.id,
            name: None,
            proxy_url: None,
            enabled: None,
            status: Some(outcome.status.to_string()),
            last_error: Some(outcome.last_error.clone().unwrap_or_default()),
            last_url_latency_ms: outcome.latency_ms,
            last_download_mbps: None,
            last_upload_mbps: None,
            last_tested_at: Some(codexmanager_core::storage::now_ts()),
            ip: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.ip.clone())
                    .unwrap_or_default(),
            ),
            country_code: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.country_code.clone())
                    .unwrap_or_default(),
            ),
            country_name: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.country_name.clone())
                    .unwrap_or_default(),
            ),
            region_name: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.region_name.clone())
                    .unwrap_or_default(),
            ),
            city_name: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.city_name.clone())
                    .unwrap_or_default(),
            ),
            asn: outcome.geo.as_ref().and_then(|g| g.asn),
            as_org: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.as_org.clone())
                    .unwrap_or_default(),
            ),
            isp: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.isp.clone())
                    .unwrap_or_default(),
            ),
            as_domain: Some(
                outcome
                    .geo
                    .as_ref()
                    .and_then(|g| g.as_domain.clone())
                    .unwrap_or_default(),
            ),
            flag_img_url: outcome.geo.as_ref().and_then(|g| g.flag_img_url.clone()),
            flag_emoji: outcome.geo.as_ref().and_then(|g| g.flag_emoji.clone()),
            timezone_id: outcome.geo.as_ref().and_then(|g| g.timezone_id.clone()),
            timezone_offset: outcome.geo.as_ref().and_then(|g| g.timezone_offset),
            timezone_utc: outcome.geo.as_ref().and_then(|g| g.timezone_utc.clone()),
            tags_json: None,
            notes: None,
        })
        .map_err(|err| format!("update proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())?;

    let accounts_count = storage
        .list_account_ids_bound_to_proxy_profile(&updated.id)
        .map(|l| l.len() as i64)
        .unwrap_or(0);
    let mut entry = proxy_profile_entry(updated);
    entry.accounts_count = Some(accounts_count);
    Ok(entry)
}

pub(crate) fn test_proxy_profile_latency(
    id: &str,
) -> Result<JobState, String> {
    let normalized_id = normalize_required_str("id", id)?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    
    let _ = storage
        .find_proxy_profile(normalized_id)
        .map_err(|err| format!("read proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())?;

    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    let state = registry.create_latency_job(normalized_id);
    Ok(state)
}

pub(crate) fn test_proxy_profile_speed(
    id: &str,
    provider_id: Option<&str>,
    file_size_id: Option<&str>,
    diagnostic_provider_id: Option<&str>,
    diagnostic_file_size_id: Option<&str>,
) -> Result<JobState, String> {
    let normalized_id = normalize_required_str("id", id)?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    
    let _ = storage
        .find_proxy_profile(normalized_id)
        .map_err(|err| format!("read proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())?;

    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    let state = registry.create_speed_job(
        normalized_id,
        provider_id,
        file_size_id,
        diagnostic_provider_id,
        diagnostic_file_size_id,
    );
    Ok(state)
}

pub(crate) fn test_proxy_profile_cloudflare_style_speed(
    id: &str,
    config: CfStyleConfig,
) -> Result<JobState, String> {
    let normalized_id = normalize_required_str("id", id)?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    
    let _ = storage
        .find_proxy_profile(normalized_id)
        .map_err(|err| format!("read proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())?;

    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    let state = registry.create_cloudflare_style_speed_job(normalized_id, config);
    Ok(state)
}


pub(crate) fn get_proxy_test_job(job_id: &str) -> Result<JobState, String> {
    let normalized_job_id = normalize_required_str("jobId", job_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    registry
        .get_job(normalized_job_id)
        .ok_or_else(|| "job not found".to_string())
}

pub(crate) fn cancel_proxy_test_job(job_id: &str) -> Result<(), String> {
    let normalized_job_id = normalize_required_str("jobId", job_id)?;
    let registry = crate::account::proxy_testing::jobs::JobRegistry::global();
    if registry.cancel_job(normalized_job_id) {
        Ok(())
    } else {
        Err("job not found".to_string())
    }
}

fn storage_update_proxy_profile(
    id: &str,
    name: Option<String>,
    proxy_url: Option<String>,
    enabled: Option<bool>,
    tags_json: Option<String>,
    notes: Option<String>,
    url_changed: bool,
) -> Result<ProxyProfile, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_proxy_profile(&ProxyProfileUpdateInput {
            id: id.to_string(),
            name,
            proxy_url,
            enabled,
            status: url_changed.then(|| "unchecked".to_string()),
            last_error: None,
            last_url_latency_ms: None,
            last_download_mbps: None,
            last_upload_mbps: None,
            last_tested_at: None,
            ip: None,
            country_code: None,
            country_name: None,
            region_name: None,
            city_name: None,
            asn: None,
            as_org: None,
            isp: None,
            as_domain: None,
            flag_img_url: None,
            flag_emoji: None,
            timezone_id: None,
            timezone_offset: None,
            timezone_utc: None,
            tags_json,
            notes,
        })
        .map_err(|err| format!("update proxy profile failed: {err}"))?
        .ok_or_else(|| "proxy profile not found".to_string())
}

fn proxy_profile_entry(profile: ProxyProfile) -> ProxyProfileEntry {
    ProxyProfileEntry {
        id: profile.id,
        name: profile.name,
        proxy_url_redacted: profile.proxy_url_redacted,
        scheme: profile.scheme,
        host: profile.host,
        port: profile.port,
        enabled: profile.enabled,
        status: profile.status,
        last_error: profile.last_error,
        last_url_latency_ms: profile.last_url_latency_ms,
        last_download_mbps: profile.last_download_mbps,
        last_upload_mbps: profile.last_upload_mbps,
        last_tested_at: profile.last_tested_at,
        ip: profile.ip,
        country_code: profile.country_code,
        country_name: profile.country_name,
        region_name: profile.region_name,
        city_name: profile.city_name,
        asn: profile.asn,
        as_org: profile.as_org,
        isp: profile.isp,
        as_domain: profile.as_domain,
        flag_img_url: profile.flag_img_url,
        flag_emoji: profile.flag_emoji,
        timezone_id: profile.timezone_id,
        timezone_offset: profile.timezone_offset,
        timezone_utc: profile.timezone_utc,
        tags_json: profile.tags_json,
        notes: profile.notes,
        accounts_count: None,
        created_at: profile.created_at,
        updated_at: profile.updated_at,
    }
}

fn normalize_proxy_url(proxy_url: Option<String>) -> Result<String, String> {
    let raw = proxy_url.ok_or_else(|| "proxyUrl is required".to_string())?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("proxyUrl is required".to_string());
    }
    validation::normalize_proxy_profile_url(trimmed)
}

fn normalize_required_text(field_name: &str, value: Option<String>) -> Result<String, String> {
    let value = value.ok_or_else(|| format!("{field_name} is required"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    Ok(trimmed.to_string())
}

fn normalize_required_str<'a>(field_name: &str, value: &'a str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field_name} is required"))
    } else {
        Ok(trimmed)
    }
}

fn normalize_optional_required_text(
    field_name: &str,
    value: Option<String>,
) -> Result<Option<String>, String> {
    match value {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(format!("{field_name} is required"))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        None => Ok(None),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_tags_json(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: Value =
        serde_json::from_str(trimmed).map_err(|_| "tagsJson must be valid JSON".to_string())?;
    let items = parsed
        .as_array()
        .ok_or_else(|| "tagsJson must be a JSON array".to_string())?;
    let normalized_items = items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| "tagsJson must contain only non-empty strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&normalized_items)
        .map(Some)
        .map_err(|_| "serialize tagsJson failed".to_string())
}

pub(crate) fn get_proxy_profile_speed_test_history(
    profile_id: &str,
    limit: Option<usize>,
) -> Result<ProxySpeedTestListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let normalized_id = normalize_required_str("id", profile_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_proxy_speed_tests_by_profile(normalized_id, limit)
        .map_err(|err| format!("list profile speed tests failed: {err}"))?;

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

pub(crate) fn get_proxy_profile_latency_test_history(
    profile_id: &str,
    limit: Option<usize>,
) -> Result<ProxyProfileUrlTestListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let normalized_id = normalize_required_str("id", profile_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_proxy_profile_url_tests(normalized_id, limit)
        .map_err(|err| format!("list profile latency tests failed: {err}"))?;

    let entries = items
        .into_iter()
        .map(|t| ProxyProfileUrlTestEntry {
            id: t.id,
            proxy_profile_id: t.proxy_profile_id,
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
    Ok(ProxyProfileUrlTestListResult { items: entries })
}

pub(crate) fn get_proxy_profile_diagnostics_history(
    profile_id: &str,
    limit: Option<usize>,
) -> Result<ProxyDiagnosticTestListResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let normalized_id = normalize_required_str("id", profile_id)?;

    let limit = limit.unwrap_or(20);
    let items = storage
        .list_proxy_diagnostic_tests_by_profile(normalized_id, limit)
        .map_err(|err| format!("list profile diagnostic tests failed: {err}"))?;

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

