use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn env_override_default_value(key: &str) -> String {
    super::process::env_override_original_process_value(key).unwrap_or_else(|| {
        super::catalog::env_override_catalog_item(key)
            .map(|item| item.default_value.to_string())
            .unwrap_or_default()
    })
}

pub(super) fn env_override_default_snapshot() -> BTreeMap<String, String> {
    let mut snapshot = BTreeMap::new();
    for item in super::catalog::ENV_OVERRIDE_CATALOG.iter() {
        snapshot.insert(item.key.to_string(), env_override_default_value(item.key));
    }
    snapshot
}

fn persisted_env_overrides(mut normalized: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let Some(raw) = super::get_persisted_app_setting(super::APP_SETTING_ENV_OVERRIDES_KEY) else {
        return normalized;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Map<String, Value>>(&raw) else {
        log::warn!("parse persisted env overrides failed: invalid json");
        return normalized;
    };

    for (raw_key, raw_value) in parsed {
        let Ok(key) = super::normalize::normalize_env_override_key(&raw_key) else {
            log::warn!(
                "skip persisted env override: key={} invalid",
                raw_key.trim()
            );
            continue;
        };
        if let Some(value) = super::normalize::parse_saved_env_override_value(&raw_value) {
            if super::catalog::is_env_override_catalog_key(&key) || !value.is_empty() {
                normalized.insert(key, value);
            } else {
                normalized.remove(&key);
            }
        }
    }
    normalized
}

pub(crate) fn persisted_env_overrides_only() -> BTreeMap<String, String> {
    persisted_env_overrides(BTreeMap::new())
}

pub(crate) fn persisted_env_overrides_missing_process_env() -> BTreeMap<String, String> {
    persisted_env_overrides_only()
        .into_iter()
        .filter(|(key, _)| std::env::var_os(key).is_none())
        .collect()
}

pub(crate) fn current_env_overrides() -> BTreeMap<String, String> {
    let mut current = env_override_default_snapshot();
    for (key, value) in persisted_env_overrides_only() {
        if std::env::var_os(&key).is_none() {
            current.insert(key, value);
        }
    }
    current
}

pub(crate) fn save_env_overrides_value(overrides: &BTreeMap<String, String>) -> Result<(), String> {
    let raw = serde_json::to_string(overrides)
        .map_err(|err| format!("serialize env overrides failed: {err}"))?;
    super::save_persisted_app_setting(super::APP_SETTING_ENV_OVERRIDES_KEY, Some(&raw))
}
