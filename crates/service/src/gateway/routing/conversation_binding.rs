use codexmanager_core::storage::{now_ts, Account, ConversationBinding, Storage, Token};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct ConversationRoutingContext {
    pub(crate) platform_key_hash: String,
    pub(crate) conversation_id: String,
    pub(crate) source: RouteConversationSource,
    pub(crate) existing_binding: Option<ConversationBinding>,
    pub(crate) binding_selected: bool,
    pub(crate) bound_account_selectable: bool,
    pub(crate) manual_preferred_account_id: Option<String>,
    pub(crate) next_thread_epoch: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteConversationSource {
    NativeConversation,
    StickyFallback,
    PromptCacheKey,
    PromptCacheKeyExistingOnly,
}

impl RouteConversationSource {
    pub(crate) fn is_prompt_cache_key(self) -> bool {
        matches!(
            self,
            Self::PromptCacheKey | Self::PromptCacheKeyExistingOnly
        )
    }

    pub(crate) fn allows_initial_binding_create(self) -> bool {
        !matches!(self, Self::PromptCacheKeyExistingOnly)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateRotationSource {
    ConversationBinding,
    ManualPreferredAccount,
    ThreadAwareDistribution,
    RouteStrategy,
}

impl CandidateRotationSource {
    /// 函数 `as_str`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ConversationBinding => "conversation_bound",
            Self::ManualPreferredAccount => "manual_preferred_account",
            Self::ThreadAwareDistribution => "thread_aware_distribution",
            Self::RouteStrategy => "route_strategy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CandidateRotationPlan {
    pub(crate) source: CandidateRotationSource,
    pub(crate) strategy_label: &'static str,
    pub(crate) strategy_applied: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ConversationThreadAttempt {
    pub(crate) thread_anchor: String,
    pub(crate) thread_epoch: i64,
    pub(crate) reset_session_affinity: bool,
}

/// 函数 `normalize_conversation_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - conversation_id: 参数 conversation_id
///
/// # 返回
/// 返回函数执行结果
fn normalize_conversation_id(conversation_id: Option<&str>) -> Option<String> {
    conversation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 函数 `load_conversation_binding`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn load_conversation_binding(
    storage: &Storage,
    platform_key_hash: &str,
    conversation_id: Option<&str>,
) -> Result<Option<ConversationBinding>, String> {
    let Some(conversation_id) = normalize_conversation_id(conversation_id) else {
        return Ok(None);
    };
    storage
        .get_conversation_binding(platform_key_hash, conversation_id.as_str())
        .map_err(|err| format!("load conversation binding failed: {err}"))
}

/// 函数 `effective_thread_anchor`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn effective_thread_anchor(
    conversation_id: Option<&str>,
    binding: Option<&ConversationBinding>,
) -> Option<String> {
    binding
        .map(|item| item.thread_anchor.clone())
        .or_else(|| normalize_conversation_id(conversation_id))
}

/// 函数 `rotate_to_bound_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
/// - binding: 参数 binding
///
/// # 返回
/// 返回函数执行结果
fn rotate_to_bound_account(
    candidates: &mut [(Account, Token)],
    binding: &ConversationBinding,
) -> bool {
    rotate_to_account_id(candidates, binding.account_id.as_str())
}

/// 函数 `rotate_to_account_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn rotate_to_account_id(candidates: &mut [(Account, Token)], account_id: &str) -> bool {
    let Some(index) = candidates
        .iter()
        .position(|(account, _)| account.id == account_id)
    else {
        return false;
    };
    if index > 0 {
        candidates.rotate_left(index);
    }
    true
}

/// 函数 `derive_next_thread_epoch`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - existing_binding: 参数 existing_binding
///
/// # 返回
/// 返回函数执行结果
fn derive_next_thread_epoch(existing_binding: Option<&ConversationBinding>) -> Option<i64> {
    existing_binding.map(|binding| binding.thread_epoch + 1)
}

/// 函数 `switch_reason_for_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - routing: 参数 routing
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn switch_reason_for_account(
    routing: &ConversationRoutingContext,
    account_id: &str,
) -> &'static str {
    if routing
        .manual_preferred_account_id
        .as_deref()
        .is_some_and(|manual_id| manual_id == account_id)
    {
        "manual_account_switch"
    } else {
        "automatic_account_switch"
    }
}

fn thread_distribution_load(
    account_id: &str,
    account_binding_counts: &HashMap<String, usize>,
) -> (usize, usize) {
    // 正在执行的长流比历史线程绑定更影响新会话体验。先把新线程分给空闲账号，
    // 只有并发数相同时才用历史绑定数做均衡，避免 `0 绑定 + 1 运行中` 与
    // `1 绑定 + 0 运行中` 被错误视为相同负载。
    (
        super::account_inflight_count(account_id),
        account_binding_counts
            .get(account_id)
            .copied()
            .unwrap_or_default(),
    )
}

fn should_apply_thread_aware_distribution(
    routing: Option<&ConversationRoutingContext>,
    account_binding_counts: Option<&HashMap<String, usize>>,
) -> bool {
    if account_binding_counts.is_none() {
        return false;
    }
    routing.is_some_and(|routing| {
        routing.existing_binding.is_none()
            && routing.manual_preferred_account_id.is_none()
            && routing.source.allows_initial_binding_create()
    })
}

fn rotate_to_least_loaded_thread_account(
    candidates: &mut [(Account, Token)],
    account_binding_counts: &HashMap<String, usize>,
) -> bool {
    if candidates.len() <= 1 {
        return false;
    }

    let mut best_index = 0;
    let mut best_load =
        thread_distribution_load(candidates[0].0.id.as_str(), account_binding_counts);
    for (index, (account, _)) in candidates.iter().enumerate().skip(1) {
        let load = thread_distribution_load(account.id.as_str(), account_binding_counts);
        if load < best_load {
            best_index = index;
            best_load = load;
        }
    }

    if best_index > 0 {
        candidates.rotate_left(best_index);
        return true;
    }
    false
}

/// 函数 `prepare_conversation_routing`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
pub(crate) fn prepare_conversation_routing(
    platform_key_hash: &str,
    conversation_id: Option<&str>,
    existing_binding: Option<&ConversationBinding>,
    candidates: &mut Vec<(Account, Token)>,
) -> Option<ConversationRoutingContext> {
    prepare_conversation_routing_with_source(
        platform_key_hash,
        conversation_id,
        existing_binding,
        candidates,
        RouteConversationSource::StickyFallback,
    )
}

pub(crate) fn prepare_conversation_routing_with_source(
    platform_key_hash: &str,
    conversation_id: Option<&str>,
    existing_binding: Option<&ConversationBinding>,
    candidates: &mut Vec<(Account, Token)>,
    source: RouteConversationSource,
) -> Option<ConversationRoutingContext> {
    let conversation_id = normalize_conversation_id(conversation_id)?;
    let existing_binding = existing_binding.cloned();
    let bound_account_selectable = existing_binding.as_ref().is_some_and(|binding| {
        candidates
            .iter()
            .any(|(account, _)| account.id == binding.account_id)
    });
    let manual_preferred_account_id = super::manual_preferred_account()
        .filter(|account_id| rotate_to_account_id(candidates.as_mut_slice(), account_id));
    let binding_selected = if let Some(account_id) = manual_preferred_account_id.as_deref() {
        existing_binding
            .as_ref()
            .is_some_and(|binding| binding.account_id == account_id)
    } else {
        existing_binding
            .as_ref()
            .is_some_and(|binding| rotate_to_bound_account(candidates.as_mut_slice(), binding))
    };
    let next_thread_epoch = derive_next_thread_epoch(existing_binding.as_ref());

    Some(ConversationRoutingContext {
        platform_key_hash: platform_key_hash.to_string(),
        conversation_id,
        source,
        existing_binding,
        binding_selected,
        bound_account_selectable,
        manual_preferred_account_id,
        next_thread_epoch,
    })
}

/// 函数 `apply_candidate_rotation`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn apply_candidate_rotation(
    candidates: &mut Vec<(Account, Token)>,
    routing: Option<&ConversationRoutingContext>,
    key_id: &str,
    model_for_log: Option<&str>,
    account_binding_counts: Option<&HashMap<String, usize>>,
) -> CandidateRotationPlan {
    if routing
        .as_ref()
        .is_some_and(|routing| routing.binding_selected)
    {
        return CandidateRotationPlan {
            source: CandidateRotationSource::ConversationBinding,
            strategy_label: "conversation_bound",
            strategy_applied: false,
        };
    }
    if routing
        .as_ref()
        .and_then(|routing| routing.manual_preferred_account_id.as_deref())
        .is_some()
    {
        return CandidateRotationPlan {
            source: CandidateRotationSource::ManualPreferredAccount,
            strategy_label: "manual_preferred_account",
            strategy_applied: false,
        };
    }
    let manual_preferred_account_id = super::manual_preferred_account();
    super::apply_route_strategy(candidates, key_id, model_for_log);
    if manual_preferred_account_id
        .as_deref()
        .is_some_and(|account_id| {
            candidates
                .iter()
                .any(|(account, _)| account.id == account_id)
        })
    {
        return CandidateRotationPlan {
            source: CandidateRotationSource::ManualPreferredAccount,
            strategy_label: "manual_preferred_account",
            strategy_applied: false,
        };
    }
    if should_apply_thread_aware_distribution(routing, account_binding_counts) {
        let strategy_applied = account_binding_counts
            .is_some_and(|counts| rotate_to_least_loaded_thread_account(candidates, counts));
        return CandidateRotationPlan {
            source: CandidateRotationSource::ThreadAwareDistribution,
            strategy_label: "thread_aware_distribution",
            strategy_applied,
        };
    }
    CandidateRotationPlan {
        source: CandidateRotationSource::RouteStrategy,
        strategy_label: super::current_route_strategy(),
        strategy_applied: true,
    }
}

/// 函数 `resolve_attempt_thread`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_attempt_thread(
    routing: Option<&ConversationRoutingContext>,
    account: &Account,
) -> Option<ConversationThreadAttempt> {
    let routing = routing?;
    if routing.source.is_prompt_cache_key() {
        return None;
    }
    match routing.existing_binding.as_ref() {
        Some(binding) if binding.account_id == account.id => Some(ConversationThreadAttempt {
            thread_anchor: binding.thread_anchor.clone(),
            thread_epoch: binding.thread_epoch,
            reset_session_affinity: false,
        }),
        Some(binding) => Some(ConversationThreadAttempt {
            // 同一会话的 prompt_cache_key 必须稳定；账号切换只重置 session affinity。
            thread_anchor: binding.thread_anchor.clone(),
            thread_epoch: routing
                .next_thread_epoch
                .unwrap_or(binding.thread_epoch + 1),
            reset_session_affinity: true,
        }),
        None => Some(ConversationThreadAttempt {
            thread_anchor: routing.conversation_id.clone(),
            thread_epoch: 1,
            reset_session_affinity: false,
        }),
    }
}

/// 函数 `record_conversation_binding_terminal_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn record_conversation_binding_terminal_response(
    storage: &Storage,
    routing: Option<&ConversationRoutingContext>,
    account: &Account,
    model: Option<&str>,
    status_code: u16,
) -> Result<(), String> {
    let Some(routing) = routing else {
        return Ok(());
    };
    let attempt_thread = resolve_attempt_thread(Some(routing), account);

    let now = now_ts();
    match routing.existing_binding.as_ref() {
        Some(binding) if binding.account_id == account.id => storage
            .touch_conversation_binding(
                routing.platform_key_hash.as_str(),
                routing.conversation_id.as_str(),
                account.id.as_str(),
                model,
                now,
            )
            .map(|_| ())
            .map_err(|err| format!("touch conversation binding failed: {err}")),
        Some(_) if routing.source.is_prompt_cache_key() && routing.bound_account_selectable => {
            Ok(())
        }
        Some(binding) if status_code < 400 => {
            let (thread_epoch, thread_anchor) = if routing.source.is_prompt_cache_key() {
                (binding.thread_epoch + 1, binding.thread_anchor.clone())
            } else {
                let attempt_thread = attempt_thread
                    .ok_or_else(|| "missing conversation thread for rebound account".to_string())?;
                (attempt_thread.thread_epoch, attempt_thread.thread_anchor)
            };
            let mut next = binding.clone();
            next.account_id = account.id.clone();
            next.thread_epoch = thread_epoch;
            next.thread_anchor = thread_anchor;
            next.last_model = model.map(str::to_string);
            next.last_switch_reason =
                Some(switch_reason_for_account(routing, account.id.as_str()).to_string());
            next.updated_at = now;
            next.last_used_at = now;
            storage
                .upsert_conversation_binding(&next)
                .map_err(|err| format!("rebind conversation binding failed: {err}"))
        }
        None if status_code < 400 && routing.source.allows_initial_binding_create() => {
            let (thread_epoch, thread_anchor) = if routing.source.is_prompt_cache_key() {
                (1, routing.conversation_id.clone())
            } else {
                let attempt_thread = attempt_thread
                    .ok_or_else(|| "missing conversation thread for initial binding".to_string())?;
                (attempt_thread.thread_epoch, attempt_thread.thread_anchor)
            };
            let binding = ConversationBinding {
                platform_key_hash: routing.platform_key_hash.clone(),
                conversation_id: routing.conversation_id.clone(),
                account_id: account.id.clone(),
                thread_epoch,
                thread_anchor,
                status: "active".to_string(),
                last_model: model.map(str::to_string),
                last_switch_reason: None,
                created_at: now,
                updated_at: now,
                last_used_at: now,
            };
            storage
                .upsert_conversation_binding(&binding)
                .map_err(|err| format!("create conversation binding failed: {err}"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "conversation_binding_tests.rs"]
mod tests;
