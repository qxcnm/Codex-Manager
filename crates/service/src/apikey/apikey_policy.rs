use codexmanager_core::storage::{ApiKeyPolicy, Storage};

const ALLOWED_PLATFORMS: &[&str] = &["codex", "kiro", "grok"];

pub(crate) fn normalize_policy(
    key_id: &str,
    allowed_models: Vec<String>,
    allowed_platforms: Vec<String>,
    model_visibility: Option<String>,
    expires_at: Option<i64>,
    concurrency_limit: Option<i64>,
) -> Result<ApiKeyPolicy, String> {
    let allowed_models = normalize_list(allowed_models);
    let allowed_platforms = normalize_list(allowed_platforms)
        .into_iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if let Some(platform) = allowed_platforms
        .iter()
        .find(|value| !ALLOWED_PLATFORMS.contains(&value.as_str()))
    {
        return Err(format!("unsupported platform restriction: {platform}"));
    }
    if concurrency_limit.is_some_and(|value| value <= 0) {
        return Err("concurrency limit must be greater than zero".to_string());
    }
    let model_visibility = normalize_model_visibility(model_visibility)?;
    Ok(ApiKeyPolicy {
        key_id: key_id.to_string(),
        allowed_models,
        allowed_platforms,
        model_visibility,
        expires_at,
        concurrency_limit,
    })
}

pub(crate) fn save_api_key_policy_with_storage(
    storage: &Storage,
    policy: &ApiKeyPolicy,
) -> Result<(), String> {
    storage
        .upsert_api_key_policy(policy)
        .map_err(|err| format!("persist api key policy failed: {err}"))
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort_by_key(|value| value.to_ascii_lowercase());
    values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    values
}

fn normalize_model_visibility(value: Option<String>) -> Result<String, String> {
    match value
        .as_deref()
        .unwrap_or("selectable")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "selectable" => Ok("selectable".to_string()),
        "managed" => Ok("managed".to_string()),
        other => Err(format!("unsupported model visibility: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_policy;

    #[test]
    fn policy_normalization_deduplicates_and_validates_platforms() {
        let policy = normalize_policy(
            "key",
            vec![" smart ".into(), "SMART".into()],
            vec!["KIRO".into()],
            Some("managed".into()),
            None,
            Some(2),
        )
        .expect("policy");
        assert_eq!(policy.allowed_models, vec!["smart"]);
        assert_eq!(policy.allowed_platforms, vec!["kiro"]);
        assert_eq!(policy.model_visibility, "managed");
        assert!(normalize_policy("key", vec![], vec!["grok".into()], None, None, None).is_ok());
        assert!(normalize_policy("key", vec![], vec!["gemini".into()], None, None, None).is_err());
        assert!(
            normalize_policy("key", vec![], vec![], Some("hidden".into()), None, None).is_err()
        );
        assert!(normalize_policy("key", vec![], vec![], None, None, Some(0)).is_err());
    }
}
