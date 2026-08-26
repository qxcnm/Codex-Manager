use super::{
    get_persisted_app_setting, save_persisted_app_setting,
    APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_KEY,
    APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_MODE_KEY,
};

pub(crate) const AGGREGATE_API_PROBE_USER_AGENT_MODE_CODEX: &str = "codex";
pub(crate) const AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM: &str = "custom";
const MAX_AGGREGATE_API_PROBE_USER_AGENT_BYTES: usize = 512;

fn normalize_probe_user_agent_mode(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        AGGREGATE_API_PROBE_USER_AGENT_MODE_CODEX | AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM => {
            Ok(normalized)
        }
        _ => Err("aggregate api probe user agent mode must be codex or custom".to_string()),
    }
}

fn normalize_probe_user_agent(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.len() > MAX_AGGREGATE_API_PROBE_USER_AGENT_BYTES {
        return Err(format!(
            "aggregate api probe user agent must not exceed {MAX_AGGREGATE_API_PROBE_USER_AGENT_BYTES} bytes"
        ));
    }
    if normalized.chars().any(|ch| ch.is_ascii_control()) {
        return Err("aggregate api probe user agent contains control characters".to_string());
    }
    Ok(normalized.to_string())
}

pub(crate) fn current_aggregate_api_probe_user_agent_mode() -> String {
    get_persisted_app_setting(APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_MODE_KEY)
        .as_deref()
        .and_then(|value| normalize_probe_user_agent_mode(value).ok())
        .unwrap_or_else(|| AGGREGATE_API_PROBE_USER_AGENT_MODE_CODEX.to_string())
}

pub(crate) fn current_aggregate_api_probe_user_agent() -> String {
    get_persisted_app_setting(APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_KEY)
        .and_then(|value| normalize_probe_user_agent(value.as_str()).ok())
        .unwrap_or_default()
}

pub(crate) fn set_aggregate_api_probe_user_agent_settings(
    mode: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(String, String), String> {
    let next_mode = match mode {
        Some(value) => normalize_probe_user_agent_mode(value)?,
        None => current_aggregate_api_probe_user_agent_mode(),
    };
    let next_user_agent = match user_agent {
        Some(value) => normalize_probe_user_agent(value)?,
        None => current_aggregate_api_probe_user_agent(),
    };
    if next_mode == AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM && next_user_agent.is_empty() {
        return Err("aggregate api probe custom user agent is required".to_string());
    }

    save_persisted_app_setting(
        APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_MODE_KEY,
        Some(next_mode.as_str()),
    )?;
    save_persisted_app_setting(
        APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_KEY,
        if next_user_agent.is_empty() {
            None
        } else {
            Some(next_user_agent.as_str())
        },
    )?;
    Ok((next_mode, next_user_agent))
}

#[cfg(test)]
#[path = "aggregate_api_settings_tests.rs"]
mod tests;
