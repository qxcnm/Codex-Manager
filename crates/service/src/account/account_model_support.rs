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
const MAX_MESSAGE_CHARS: usize = 2000;

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

fn normalize_model(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn mark_key(account_id: &str, model: &str) -> String {
    format!("{}\u{1}{}", account_id.trim(), normalize_model(model))
}

/// 把上游错误正文裁剪到有界长度，避免把整个响应体带进日志或记忆。
pub(crate) fn bound_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX_MESSAGE_CHARS {
        return trimmed.to_string();
    }
    trimmed.chars().take(MAX_MESSAGE_CHARS).collect()
}

/// 识别“该账户不支持该模型”这一类上游拒绝。
///
/// 只匹配同时出现“模型不被支持”与“账户/Codex 范围”的文案，避免把参数错误
/// （例如 “images[].file_id is not supported (use image_url)”）误判为资格问题。
pub(crate) fn looks_like_account_model_unsupported(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    let mentions_model = lower.contains("model") || lower.contains("gpt-");
    let mentions_unsupported = lower.contains("is not supported")
        || lower.contains("not supported for")
        || lower.contains("does not support")
        || lower.contains("unsupported model");
    let mentions_account_scope = lower.contains("chatgpt account")
        || lower.contains("using codex")
        || lower.contains("this account")
        || lower.contains("account does not")
        || lower.contains("account is not");
    mentions_model && mentions_unsupported && mentions_account_scope
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
    if !looks_like_account_model_unsupported(message) {
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
        assert!(is_unsupported(&account, "GPT-6-ASTRA"));
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
    fn bound_message_limits_size() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 50);
        assert_eq!(bound_message(&long).chars().count(), MAX_MESSAGE_CHARS);
        assert_eq!(bound_message("  short  "), "short");
    }
}
