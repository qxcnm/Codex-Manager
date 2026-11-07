//! Per-login proxy selection. Only registry IDs are persisted in login sessions.
use codexmanager_core::storage::Storage;

pub(crate) fn resolve_login_proxy_url(
    storage: &Storage,
    proxy_profile_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(profile_id) = proxy_profile_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let storage = crate::account::remote_storage::AccountStorage::new(storage);
    let profile = storage
        .find_proxy_profile(profile_id)
        .map_err(|err| format!("read login proxy profile failed: {err}"))?
        .ok_or_else(|| "selected login proxy profile no longer exists".to_owned())?;
    if !profile.enabled {
        return Err("selected login proxy profile is disabled".to_owned());
    }
    crate::account_proxy::normalize_supported_proxy_url(&profile.proxy_url).map(Some)
}

pub(crate) fn persist_login_proxy(
    storage: &Storage,
    account_id: &str,
    proxy_profile_id: Option<&str>,
) -> Result<(), String> {
    let Some(profile_id) = proxy_profile_id else {
        // Reauthorizing without a selection must preserve an existing account binding.
        return Ok(());
    };
    resolve_login_proxy_url(storage, Some(profile_id))?;
    let storage = crate::account::remote_storage::AccountStorage::new(storage);
    storage
        .upsert_account_proxy_settings(
            account_id,
            true,
            Some(crate::account_proxy::SOURCE_PROFILE),
            Some(profile_id),
            None,
            crate::account_proxy::STATUS_UNCHECKED,
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
            None,
        )
        .map_err(|err| format!("store login account proxy failed: {err}"))?;
    crate::gateway::invalidate_account_proxy_cache(account_id);
    Ok(())
}
