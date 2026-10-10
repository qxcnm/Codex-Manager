//! 账户×模型资格记忆。
//!
//! 背景（BUG-2026-0930-01）：某些账户可以正常使用，但**不支持某个具体模型**。
//! 上游对这种情况返回 400（例如 “The 'gpt-6-astra' model is not supported when
//! using Codex with a ChatGPT account”）。若不做资格过滤，请求会在轮转/粘性选择
//! 中反复落到不支持该模型的账户上，用户看到反复的终止性 400。
//!
//! 本模块只在收到该类**精确拒绝**后，按 (账户, 模型) 为粒度做短期记忆，供候选
//! 选择在粘性/轮转之前先过滤。记忆带 TTL（默认 30 分钟，可用环境变量覆盖），
//! 过期自动恢复；未命中过该类拒绝的账户行为完全不变。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const ENV_TTL_SECS: &str = "CODEXMANAGER_ACCOUNT_MODEL_UNSUPPORTED_TTL_SECS";
const DEFAULT_TTL_SECS: u64 = 30 * 60;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn ttl_secs() -> u64 {
    std::env::var(ENV_TTL_SECS)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TTL_SECS)
}

fn marks() -> &'static Mutex<HashMap<String, u64>> {
    static MARKS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MARKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_marks<T>(action: impl FnOnce(&mut HashMap<String, u64>) -> T) -> T {
    let mut guard = match marks().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    action(&mut guard)
}

// Upstream model IDs are case sensitive; use the exact final sent ID.
fn normalize_model(model: &str) -> String {
    model.trim().to_owned()
}

fn mark_key(account_id: &str, model: &str) -> String {
    format!("{}\u{1}{}", account_id.trim(), normalize_model(model))
}

/// Match only complete, explicit account/model rejection templates.
/// Tool capability and parameter errors must not poison the primary model.
fn rejected_model(message: &str) -> Option<&str> {
    let message = message.trim().trim_end_matches('.');
    let model = if let Some(rest) = message.strip_prefix("The '") {
        rest.strip_suffix("' model is not supported when using Codex with a ChatGPT account")?
    } else if let Some(rest) = message.strip_prefix("model ") {
        rest.strip_suffix(" is not supported for this account")?
    } else {
        return None;
    };
    (!model.is_empty()
        && model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then_some(model)
}

pub(crate) fn account_model_rejection_message(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    // A parameter/tool-scoped error never establishes primary-model eligibility.
    if error
        .get("param")
        .is_some_and(|param| !param.is_null() && param.as_str() != Some("model"))
    {
        return None;
    }
    let message = error.get("message")?.as_str()?;
    rejected_model(message).map(|_| message.to_owned())
}

#[cfg(test)]
fn looks_like_account_model_unsupported(message: &str) -> bool {
    rejected_model(message).is_some()
}

/// Use the same configured account-pool model for routing and the final request.
pub(crate) fn account_model_override_for_request(
    storage: &codexmanager_core::storage::Storage,
    requested_model: Option<&str>,
) -> Option<String> {
    requested_model
        .and_then(|model| {
            crate::models_v2::enabled_model(storage, crate::models_v2::policy_catalog_slug(model))
                .ok()
                .flatten()
        })
        .and_then(|model| {
            model
                .routes
                .into_iter()
                .filter(|route| {
                    route.enabled
                        && route.source_kind == "account_pool"
                        && route.source_id == "default"
                })
                .max_by_key(|route| route.priority)
                .map(|route| route.upstream_model)
        })
        .filter(|model| {
            !crate::models_v2::should_preserve_luna_reserve_alias(requested_model, Some(model))
        })
}

/// 记录 (账户, 模型) 不支持，TTL 取环境变量或默认值。
pub(crate) fn mark_unsupported(account_id: &str, model: &str) {
    mark_unsupported_with_ttl(account_id, model, ttl_secs());
}

/// 记录 (账户, 模型) 不支持，使用显式 TTL（0 表示立即过期，便于测试）。
pub(crate) fn mark_unsupported_with_ttl(account_id: &str, model: &str, ttl_secs: u64) {
    if account_id.trim().is_empty() || model.trim().is_empty() {
        return;
    }
    let expires_at = now_secs().saturating_add(ttl_secs);
    let key = mark_key(account_id, model);
    with_marks(|marks| {
        marks.insert(key, expires_at);
    });
}

/// 清除记录（例如同一账户同一模型后来成功了）。
pub(crate) fn clear_unsupported(account_id: &str, model: &str) {
    if account_id.trim().is_empty() || model.trim().is_empty() {
        return;
    }
    let key = mark_key(account_id, model);
    with_marks(|marks| {
        marks.remove(&key);
    });
}

/// 查询 (账户, 模型) 是否在有效期内被标记为不支持。
pub(crate) fn is_unsupported(account_id: &str, model: &str) -> bool {
    if account_id.trim().is_empty() || model.trim().is_empty() {
        return false;
    }
    let key = mark_key(account_id, model);
    let now = now_secs();
    with_marks(|marks| match marks.get(&key).copied() {
        Some(expires_at) if expires_at > now => true,
        Some(_) => {
            marks.remove(&key);
            false
        }
        None => false,
    })
}

/// 从上游错误正文学习：命中该类拒绝时记录 (账户, 模型)，返回是否命中。
pub(crate) fn note_account_model_unsupported(
    account_id: &str,
    model: Option<&str>,
    message: &str,
) -> bool {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if !rejected_model(message)
        .is_some_and(|rejected| normalize_model(rejected) == normalize_model(model))
    {
        return false;
    }
    log::warn!(
        "event=account_model_unsupported_noted account_id={} model={}",
        account_id,
        model
    );
    mark_unsupported(account_id, model);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(prefix: &str) -> String {
        format!("{prefix}-{}", now_secs())
    }

    #[test]
    fn classify_matches_upstream_model_rejection() {
        assert!(looks_like_account_model_unsupported(
            "The 'gpt-6-astra' model is not supported when using Codex with a ChatGPT account."
        ));
        assert!(looks_like_account_model_unsupported(
            "model gpt-6-astra is not supported for this account"
        ));
    }

    #[test]
    fn classify_rejects_unrelated_400_messages() {
        assert!(!looks_like_account_model_unsupported(
            "Invalid request: images[].file_id is not supported (use image_url)"
        ));
        assert!(!looks_like_account_model_unsupported(
            "No tool output found for function call"
        ));
        assert!(!looks_like_account_model_unsupported(""));
    }

    #[test]
    fn marks_are_scoped_to_account_and_model() {
        let account = unique("acct");
        let other_account = unique("other");
        mark_unsupported(&account, "gpt-6-astra");

        assert!(is_unsupported(&account, "gpt-6-astra"));
        assert!(!is_unsupported(&account, "GPT-6-ASTRA"));
        assert!(!is_unsupported(&account, "gpt-5.6-luna"));
        assert!(!is_unsupported(&other_account, "gpt-6-astra"));

        clear_unsupported(&account, "gpt-6-astra");
        assert!(!is_unsupported(&account, "gpt-6-astra"));
    }

    #[test]
    fn expired_marks_stop_matching() {
        let account = unique("expiring");
        mark_unsupported_with_ttl(&account, "gpt-6-astra", 0);
        assert!(!is_unsupported(&account, "gpt-6-astra"));
    }

    #[test]
    fn success_clears_mark() {
        let account = unique("cleared");
        mark_unsupported(&account, "gpt-6-astra");
        assert!(is_unsupported(&account, "gpt-6-astra"));
        // 同一账户同一模型后来成功了：记忆必须立即清除（不等 TTL）。
        clear_unsupported(&account, "gpt-6-astra");
        assert!(!is_unsupported(&account, "gpt-6-astra"));
        // 清除后仍可被再次学习。
        assert!(note_account_model_unsupported(
            &account,
            Some("gpt-6-astra"),
            "The 'gpt-6-astra' model is not supported when using Codex with a ChatGPT account."
        ));
        assert!(is_unsupported(&account, "gpt-6-astra"));
    }

    #[test]
    fn note_requires_model_and_matching_message() {
        let account = unique("noted");
        assert!(!note_account_model_unsupported(&account, None, "anything"));
        assert!(!note_account_model_unsupported(
            &account,
            Some("gpt-6-astra"),
            "Invalid request: temperature must be <= 2"
        ));
        assert!(!is_unsupported(&account, "gpt-6-astra"));

        assert!(note_account_model_unsupported(
            &account,
            Some("gpt-6-astra"),
            "The 'gpt-6-astra' model is not supported when using Codex with a ChatGPT account."
        ));
        assert!(is_unsupported(&account, "gpt-6-astra"));
    }

    #[test]
    fn learning_requires_the_rejected_primary_model_and_explicit_error_field() {
        let account = unique("dimension");
        let tool_error =
            "The 'gpt-image-2' model is not supported when using Codex with a ChatGPT account.";
        assert!(!note_account_model_unsupported(
            &account,
            Some("gpt-6-luna"),
            tool_error
        ));
        assert!(!is_unsupported(&account, "gpt-6-luna"));
        let parameter_error = "The 'gpt-6-luna' model parameter temperature is not supported when using Codex with a ChatGPT account.";
        assert!(!note_account_model_unsupported(
            &account,
            Some("gpt-6-luna"),
            parameter_error
        ));
        let precise =
            "The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account.";
        let parameter_body = serde_json::to_vec(
            &serde_json::json!({"error": {"message": precise, "param": "tools[0].model"}}),
        )
        .unwrap();
        assert!(account_model_rejection_message(&parameter_body).is_none());
        let metadata_body =
            serde_json::to_vec(&serde_json::json!({"error": {"context": {"message": precise}}}))
                .unwrap();
        assert!(account_model_rejection_message(&metadata_body).is_none());
        let body = serde_json::to_vec(
            &serde_json::json!({"error": {"message": precise, "param": "model"}}),
        )
        .unwrap();
        let message = account_model_rejection_message(&body).unwrap();
        assert!(note_account_model_unsupported(
            &account,
            Some("gpt-6-luna"),
            &message
        ));
        clear_unsupported(&account, "gpt-6-luna");
    }

    #[test]
    fn ttl_configuration_defaults_for_zero_invalid_and_missing_values() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var(ENV_TTL_SECS).ok();
        for raw in ["0", "-1", "invalid", "", "18446744073709551616"] {
            std::env::set_var(ENV_TTL_SECS, raw);
            assert_eq!(ttl_secs(), DEFAULT_TTL_SECS);
        }
        std::env::remove_var(ENV_TTL_SECS);
        assert_eq!(ttl_secs(), DEFAULT_TTL_SECS);
        std::env::set_var(ENV_TTL_SECS, " 10 ");
        assert_eq!(ttl_secs(), 10);
        if let Some(previous) = previous {
            std::env::set_var(ENV_TTL_SECS, previous);
        } else {
            std::env::remove_var(ENV_TTL_SECS);
        }
    }
}
