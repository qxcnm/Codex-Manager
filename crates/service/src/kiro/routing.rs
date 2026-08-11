use bytes::Bytes;
use codexmanager_core::storage::Storage;
use std::cmp::Ordering;

// Sonnet 4.5 is the broadly available Kiro quality model across Free and paid
// subscriptions. Newer IDs can be called explicitly, but using 4.6 as the
// automatic default makes `smart` fail with INVALID_MODEL_ID for valid Free
// credentials in regions where that rollout is not enabled.
const DEFAULT_KIRO_MODEL: &str = "kiro/claude-sonnet-4.5";
const FAST_KIRO_MODEL: &str = "kiro/claude-haiku-4.5";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SmartRouteDecision {
    pub alias: String,
    pub selected_model: String,
    pub route_source: &'static str,
    pub score: f64,
    pub explanation: String,
}

pub(crate) fn is_smart_alias(model: Option<&str>) -> bool {
    matches!(
        model.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("smart" | "coding" | "fast" | "cheap")
    )
}

pub(crate) fn is_public_model_visibility(visibility: Option<&str>) -> bool {
    !matches!(
        visibility
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("hide" | "hidden" | "disabled" | "unavailable")
    )
}

pub(crate) fn resolve_smart_route(
    storage: &Storage,
    key_id: &str,
    alias: &str,
    body: &Bytes,
    allowed_platforms: &[String],
) -> Result<SmartRouteDecision, String> {
    let alias = alias.trim().to_ascii_lowercase();
    if !is_smart_alias(Some(alias.as_str())) {
        return Err(format!("unknown smart route alias: {alias}"));
    }
    let now = codexmanager_core::storage::now_ts();
    let credentials = storage
        .list_kiro_credentials()
        .map_err(|error| error.to_string())?;
    let eligible = credentials
        .iter()
        .filter(|item| {
            item.status == "active" && item.cooldown_until.is_none_or(|until| until <= now)
        })
        .collect::<Vec<_>>();
    let payload: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let requires_reasoning = payload.get("reasoning").is_some();
    let requires_tools = payload
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let requires_image = body.windows(10).any(|window| window == b"input_image")
        || body.windows(9).any(|window| window == b"image_url");
    let available_kiro_models = storage.list_available_kiro_models().unwrap_or_default();
    let preferred_kiro_model =
        if matches!(alias.as_str(), "fast" | "cheap") && !requires_reasoning && !requires_image {
            FAST_KIRO_MODEL
        } else {
            DEFAULT_KIRO_MODEL
        };
    let kiro_model = available_kiro_models
        .iter()
        .find(|model| model.as_str() == preferred_kiro_model)
        .or_else(|| {
            available_kiro_models
                .iter()
                .find(|model| model.as_str() == DEFAULT_KIRO_MODEL)
        })
        .or_else(|| available_kiro_models.first())
        .map(String::as_str)
        .unwrap_or(preferred_kiro_model);
    let kiro_allowed =
        crate::resolve_api_key_model_group_access(storage, key_id, kiro_model).is_ok();
    let total_requests = eligible.iter().map(|item| item.request_count).sum::<i64>();
    let total_successes = eligible.iter().map(|item| item.success_count).sum::<i64>();
    let success_rate = if total_requests > 0 {
        total_successes as f64 / total_requests as f64
    } else {
        1.0
    };
    let average_weight =
        eligible.iter().map(|item| item.weight).sum::<f64>() / eligible.len().max(1) as f64;
    let average_latency = eligible
        .iter()
        .filter_map(|item| item.last_latency_ms)
        .sum::<i64>() as f64
        / eligible
            .iter()
            .filter(|item| item.last_latency_ms.is_some())
            .count()
            .max(1) as f64;
    let remaining_ratio = eligible
        .iter()
        .filter_map(|item| match (item.credit_limit, item.credit_used) {
            (Some(limit), Some(used)) if limit > 0.0 => {
                Some(((limit - used) / limit).clamp(0.0, 1.0))
            }
            _ => None,
        })
        .fold(None::<f64>, |best, value| {
            Some(best.map_or(value, |old| old.max(value)))
        })
        .unwrap_or(0.5);
    let kiro_platform_allowed = allowed_platforms.is_empty()
        || allowed_platforms
            .iter()
            .any(|value| value.eq_ignore_ascii_case("kiro"));
    let codex_platform_allowed = allowed_platforms.is_empty()
        || allowed_platforms
            .iter()
            .any(|value| value.eq_ignore_ascii_case("codex"));
    let kiro_score = (!eligible.is_empty()
        && !available_kiro_models.is_empty()
        && kiro_allowed
        && kiro_platform_allowed)
        .then(|| {
            let capability_score = if requires_tools || requires_image || requires_reasoning {
                35.0
            } else {
                25.0
            };
            let alias_bonus = match alias.as_str() {
                "coding" => 15.0,
                "fast" => 10.0,
                "smart" => 5.0,
                _ => 0.0,
            };
            capability_score
                + alias_bonus
                + success_rate * 25.0
                + remaining_ratio * 20.0
                + average_weight.clamp(0.0, 5.0) * 3.0
                + (15.0 - average_latency / 1_000.0).clamp(0.0, 15.0)
        });

    let active_codex_accounts = storage
        .list_accounts()
        .unwrap_or_default()
        .into_iter()
        .filter(|account| {
            matches!(account.status.trim(), "active" | "available")
                && !crate::gateway::is_account_in_cooldown(account.id.as_str())
        })
        .count();
    let codex_candidate = (active_codex_accounts > 0 && codex_platform_allowed)
        .then(|| crate::apikey_models::read_model_options_from_storage(storage).ok())
        .flatten()
        .and_then(|catalog| {
            catalog
                .models
                .into_iter()
                .filter(|model| {
                    model.supported_in_api
                        && is_public_model_visibility(model.visibility.as_deref())
                        && !model.slug.starts_with("kiro/")
                        && !is_smart_alias(Some(model.slug.as_str()))
                        && crate::resolve_api_key_model_group_access(
                            storage,
                            key_id,
                            model.slug.as_str(),
                        )
                        .is_ok()
                        && (!requires_image
                            || model.input_modalities.is_empty()
                            || model.input_modalities.iter().any(|item| item == "image"))
                })
                .map(|model| {
                    let slug = model.slug.to_ascii_lowercase();
                    let profile_score = match alias.as_str() {
                        "coding" => 90.0 + if slug.contains("codex") { 25.0 } else { 0.0 },
                        "fast" => {
                            80.0 + if ["mini", "nano", "spark"]
                                .iter()
                                .any(|needle| slug.contains(needle))
                            {
                                20.0
                            } else {
                                0.0
                            }
                        }
                        "cheap" => {
                            85.0 + if slug.contains("nano") {
                                30.0
                            } else if slug.contains("mini") {
                                20.0
                            } else {
                                0.0
                            }
                        }
                        _ => {
                            90.0 + if !slug.contains("mini") && !slug.contains("nano") {
                                10.0
                            } else {
                                0.0
                            }
                        }
                    };
                    let capability_bonus = if requires_reasoning
                        && model.supports_reasoning_summaries.unwrap_or(false)
                    {
                        5.0
                    } else {
                        0.0
                    } + if requires_tools
                        && model.supports_parallel_tool_calls.unwrap_or(false)
                    {
                        5.0
                    } else {
                        0.0
                    };
                    let score = profile_score
                        + capability_bonus
                        + active_codex_accounts.min(10) as f64
                        + (model.priority as f64 / 100.0).clamp(0.0, 5.0);
                    (model.slug, score)
                })
                .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal))
        });

    let kiro_score_trace = kiro_score
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "unavailable".into());
    let codex_score_trace = codex_candidate
        .as_ref()
        .map(|(model, value)| format!("{model}:{value:.2}"))
        .unwrap_or_else(|| "unavailable".into());
    let (selected_model, score, selected_provider) = match (kiro_score, codex_candidate) {
        (Some(kiro_score), Some((codex_model, codex_score))) if codex_score > kiro_score => {
            (codex_model, codex_score, "codex")
        }
        (Some(kiro_score), _) => (kiro_model.into(), kiro_score, "kiro"),
        (None, Some((codex_model, codex_score))) => (codex_model, codex_score, "codex"),
        (None, None) => {
            return Err("model_unavailable: no healthy Codex or Kiro route candidate".into())
        }
    };
    Ok(SmartRouteDecision {
        alias: alias.clone(),
        selected_model: selected_model.clone(),
        route_source: "smart_alias_score",
        score,
        explanation: format!(
            "alias={alias}; provider={selected_provider}; candidates=kiro:{kiro_model}:{kiro_score_trace},codex:{codex_score_trace}; healthy_kiro_credentials={}; active_codex_accounts={active_codex_accounts}; success_rate={success_rate:.3}; remaining_ratio={remaining_ratio:.3}; average_latency_ms={average_latency:.0}; tools={requires_tools}; image={requires_image}; reasoning={requires_reasoning}; selected={selected_model}; score={score:.2}", eligible.len()
        ),
    })
}

pub(crate) fn rewrite_request_model(body: &Bytes, model: &str) -> Result<Bytes, String> {
    let mut payload: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| format!("invalid canonical request JSON: {error}"))?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "canonical request body must be an object".to_string())?;
    object.insert("model".into(), serde_json::Value::String(model.into()));
    serde_json::to_vec(&payload)
        .map(Bytes::from)
        .map_err(|error| format!("serialize smart-routed request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{KiroCredentialSecret, KiroCredentialUpsert};

    #[test]
    fn hidden_models_are_not_public_smart_candidates() {
        for visibility in ["hide", "hidden", "disabled", "unavailable"] {
            assert!(!is_public_model_visibility(Some(visibility)));
        }
        assert!(is_public_model_visibility(None));
        assert!(is_public_model_visibility(Some("list")));
    }

    #[cfg(windows)]
    #[test]
    fn smart_alias_prefers_haiku_for_fast_text_and_sonnet_for_reasoning() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .upsert_kiro_credential(&KiroCredentialUpsert {
                id: "kiro-routing".into(),
                auth_method: "social".into(),
                identity_hint: "routing@example.test".into(),
                email: Some("routing@example.test".into()),
                auth_region: Some("us-east-1".into()),
                api_region: Some("us-east-1".into()),
                subscription: Some("pro".into()),
                status: "active".into(),
                priority: 0,
                weight: 1.0,
                proxy_url: None,
                proxy_username: None,
                metadata_json: "{}".into(),
                credit_limit: Some(100.0),
                credit_used: Some(20.0),
                expires_at: None,
                secret: KiroCredentialSecret {
                    refresh_token: "secret".into(),
                    access_token: None,
                    client_id: None,
                    client_secret: None,
                    proxy_password: None,
                },
            })
            .unwrap();
        for model in [FAST_KIRO_MODEL, DEFAULT_KIRO_MODEL] {
            storage
                .upsert_kiro_credential_model_availability(
                    "kiro-routing",
                    model,
                    "available",
                    None,
                    Some(10),
                )
                .unwrap();
        }

        let fast = resolve_smart_route(
            &storage,
            "key-routing",
            "fast",
            &Bytes::from_static(br#"{"model":"fast","input":"hello"}"#),
            &[],
        )
        .unwrap();
        assert_eq!(fast.selected_model, FAST_KIRO_MODEL);
        assert!(fast.explanation.contains("score="));
        let reasoning = resolve_smart_route(
            &storage,
            "key-routing",
            "fast",
            &Bytes::from_static(
                br#"{"model":"fast","input":"hello","reasoning":{"effort":"high"}}"#,
            ),
            &[],
        )
        .unwrap();
        assert_eq!(reasoning.selected_model, DEFAULT_KIRO_MODEL);

        let kiro_only = resolve_smart_route(
            &storage,
            "key-routing",
            "smart",
            &Bytes::from_static(br#"{"model":"smart","input":"hello"}"#),
            &["kiro".into()],
        )
        .unwrap();
        assert!(kiro_only.selected_model.starts_with("kiro/"));
        assert!(resolve_smart_route(
            &storage,
            "key-routing",
            "smart",
            &Bytes::from_static(br#"{"model":"smart","input":"hello"}"#),
            &["codex".into()],
        )
        .is_err());
    }
}
