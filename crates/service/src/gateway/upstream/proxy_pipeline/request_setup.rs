use codexmanager_core::storage::{Account, ConversationBinding, Storage, Token};

use super::super::super::IncomingHeaderSnapshot;
use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;
use crate::gateway::conversation_binding::{ConversationRoutingContext, RouteConversationSource};

pub(in super::super) struct UpstreamRequestSetup {
    pub(in super::super) upstream_base: String,
    pub(in super::super) upstream_fallback_base: Option<String>,
    pub(in super::super) url: String,
    pub(in super::super) url_alt: Option<String>,
    pub(in super::super) candidate_count: usize,
    pub(in super::super) account_max_inflight: usize,
    pub(in super::super) anthropic_has_thread_anchor: bool,
    pub(in super::super) has_sticky_fallback_session: bool,
    pub(in super::super) has_sticky_fallback_conversation: bool,
    pub(in super::super) has_body_encrypted_content: bool,
    pub(in super::super) conversation_routing: Option<ConversationRoutingContext>,
    pub(in super::super) route_strategy_for_log: &'static str,
    pub(in super::super) route_source_for_log: &'static str,
}

/// 函数 `prepare_request_setup`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn prepare_request_setup(
    storage: &Storage,
    path: &str,
    protocol_type: &str,
    has_prompt_cache_key: bool,
    incoming_headers: &IncomingHeaderSnapshot,
    body: &bytes::Bytes,
    candidates: &mut Vec<(Account, Token)>,
    key_id: &str,
    platform_key_hash: &str,
    route_conversation_id: Option<&str>,
    route_conversation_source: RouteConversationSource,
    conversation_binding: Option<&ConversationBinding>,
    model_for_log: Option<&str>,
    trace_id: &str,
) -> UpstreamRequestSetup {
    let upstream_base = super::super::super::resolve_upstream_base_url();
    let upstream_fallback_base =
        super::super::super::resolve_upstream_fallback_base_url(upstream_base.as_str());
    let (url, url_alt) =
        super::super::super::request_rewrite::compute_upstream_url(upstream_base.as_str(), path);
    let candidate_count = candidates.len();
    let account_max_inflight = super::super::super::account_max_inflight_limit();
    let mut conversation_routing =
        super::super::super::conversation_binding::prepare_conversation_routing_with_source(
            platform_key_hash,
            route_conversation_id,
            conversation_binding,
            candidates,
            route_conversation_source,
        );
    let account_binding_counts = if super::super::super::thread_aware_account_distribution_enabled()
        && conversation_routing.as_ref().is_some_and(|routing| {
            routing.existing_binding.is_none() && routing.source.allows_initial_binding_create()
        }) {
        match crate::account::remote_storage::AccountStorage::new(storage)
            .active_conversation_binding_account_counts(platform_key_hash)
        {
            Ok(counts) => Some(counts),
            Err(err) => {
                log::warn!("load conversation binding account counts failed: {err}");
                None
            }
        }
    } else {
        None
    };
    let anthropic_has_thread_anchor = protocol_type == PROTOCOL_ANTHROPIC_NATIVE
        && (has_prompt_cache_key || conversation_routing.is_some());
    let mut rotation_plan = super::super::super::conversation_binding::apply_candidate_rotation(
        candidates,
        conversation_routing.as_ref(),
        key_id,
        model_for_log,
        account_binding_counts.as_ref(),
    );
    match super::super::super::conversation_binding::claim_initial_conversation_binding(
        storage,
        conversation_routing.as_mut(),
        candidates,
        model_for_log,
    ) {
        Ok(claim) if claim.selected_binding() => {
            rotation_plan =
                super::super::super::conversation_binding::CandidateRotationPlan {
                    source: super::super::super::conversation_binding::CandidateRotationSource::ConversationBinding,
                    strategy_label: claim.strategy_label(),
                    strategy_applied: true,
                };
        }
        Ok(_) => {}
        Err(err) => {
            log::warn!(
                "event=gateway_conversation_claim_failed trace_id={} err={}",
                trace_id,
                err
            );
        }
    }
    let candidate_order = candidates
        .iter()
        .map(|(account, _)| format!("{}#sort={}", account.id, account.sort))
        .collect::<Vec<_>>();
    super::super::super::trace_log::log_candidate_pool(
        trace_id,
        key_id,
        rotation_plan.strategy_label,
        rotation_plan.source.as_str(),
        rotation_plan.strategy_applied,
        candidate_order.as_slice(),
    );

    UpstreamRequestSetup {
        upstream_base,
        upstream_fallback_base,
        url,
        url_alt,
        candidate_count,
        account_max_inflight,
        anthropic_has_thread_anchor,
        has_sticky_fallback_session: false,
        has_sticky_fallback_conversation:
            super::super::header_profile::derive_sticky_conversation_id_from_headers(
                incoming_headers,
            )
            .is_some(),
        has_body_encrypted_content:
            super::super::support::payload_rewrite::body_has_encrypted_content_hint(body.as_ref()),
        conversation_routing,
        route_strategy_for_log: rotation_plan.strategy_label,
        route_source_for_log: rotation_plan.source.as_str(),
    }
}
