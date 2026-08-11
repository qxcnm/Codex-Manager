use super::{
    apply_candidate_rotation, effective_thread_anchor, prepare_conversation_routing,
    prepare_conversation_routing_with_source, record_conversation_binding_terminal_response,
    resolve_attempt_thread, CandidateRotationSource, RouteConversationSource,
};
use codexmanager_core::storage::{Account, ConversationBinding, Storage, Token};
use std::collections::HashMap;

/// 函数 `sample_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - sort: 参数 sort
///
/// # 返回
/// 返回函数执行结果
fn sample_account(id: &str, sort: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort,
        status: "active".to_string(),
        created_at: 1,
        updated_at: 1,
    }
}

/// 函数 `sample_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn sample_token(account_id: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: String::new(),
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: 1,
    }
}

/// 函数 `sample_binding`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn sample_binding(account_id: &str) -> ConversationBinding {
    ConversationBinding {
        platform_key_hash: "key-hash-1".to_string(),
        conversation_id: "conv-1".to_string(),
        account_id: account_id.to_string(),
        thread_epoch: 1,
        thread_anchor: "thread-anchor-1".to_string(),
        status: "active".to_string(),
        last_model: Some("gpt-5.4".to_string()),
        last_switch_reason: None,
        created_at: 1,
        updated_at: 1,
        last_used_at: 1,
    }
}

/// 函数 `prepare_conversation_routing_rotates_bound_account_first`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn prepare_conversation_routing_rotates_bound_account_first() {
    let mut candidates = vec![
        (sample_account("acc-1", 0), sample_token("acc-1")),
        (sample_account("acc-2", 1), sample_token("acc-2")),
    ];
    let binding = sample_binding("acc-2");

    let actual = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        Some(&binding),
        &mut candidates,
    )
    .expect("routing context");

    assert!(actual.binding_selected);
    assert_eq!(candidates[0].0.id, "acc-2");
    assert_eq!(candidates[1].0.id, "acc-1");
}

/// 函数 `effective_thread_anchor_prefers_existing_binding_anchor`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn effective_thread_anchor_prefers_existing_binding_anchor() {
    let binding = sample_binding("acc-1");

    let actual = effective_thread_anchor(Some("conv-1"), Some(&binding));

    assert_eq!(actual.as_deref(), Some("thread-anchor-1"));
}

/// 函数 `resolve_attempt_thread_uses_next_generation_for_switched_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn resolve_attempt_thread_keeps_anchor_for_switched_account() {
    let binding = sample_binding("acc-1");
    let routing = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        Some(&binding),
        &mut vec![(sample_account("acc-2", 0), sample_token("acc-2"))],
    )
    .expect("routing context");

    let actual =
        resolve_attempt_thread(Some(&routing), &sample_account("acc-2", 0)).expect("thread");

    assert!(actual.reset_session_affinity);
    assert_eq!(actual.thread_epoch, 2);
    assert_eq!(actual.thread_anchor, binding.thread_anchor);
}

#[test]
fn prompt_cache_route_binding_does_not_create_attempt_thread() {
    let binding = sample_binding("acc-1");
    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        Some(&binding),
        &mut vec![(sample_account("acc-1", 0), sample_token("acc-1"))],
        RouteConversationSource::PromptCacheKey,
    )
    .expect("routing context");

    let actual = resolve_attempt_thread(Some(&routing), &sample_account("acc-1", 0));

    assert!(actual.is_none());
}

#[test]
fn prompt_cache_route_binding_records_without_attempt_thread() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let mut candidates = vec![(sample_account("acc-1", 0), sample_token("acc-1"))];
    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        None,
        &mut candidates,
        RouteConversationSource::PromptCacheKey,
    )
    .expect("routing context");

    record_conversation_binding_terminal_response(
        &storage,
        Some(&routing),
        &candidates[0].0,
        Some("gpt-5.5"),
        200,
    )
    .expect("record prompt cache route binding");

    let created = storage
        .get_conversation_binding("key-hash-1", "pck:v1:abcdef")
        .expect("load binding")
        .expect("binding exists");
    assert_eq!(created.account_id, "acc-1");
    assert_eq!(created.thread_anchor, "pck:v1:abcdef");
}

#[test]
fn prompt_cache_route_binding_rotates_bound_account_first() {
    let mut binding = sample_binding("acc-2");
    binding.conversation_id = "pck:v1:abcdef".to_string();
    binding.thread_anchor = "pck:v1:abcdef".to_string();
    let mut candidates = vec![
        (sample_account("acc-1", 0), sample_token("acc-1")),
        (sample_account("acc-2", 1), sample_token("acc-2")),
    ];

    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        Some(&binding),
        &mut candidates,
        RouteConversationSource::PromptCacheKey,
    )
    .expect("routing context");

    assert!(routing.binding_selected);
    assert_eq!(candidates[0].0.id, "acc-2");
    assert_eq!(candidates[1].0.id, "acc-1");
}

#[test]
fn prompt_cache_route_binding_does_not_rebind_after_selected_binding_failover_success() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let mut binding = sample_binding("acc-1");
    binding.conversation_id = "pck:v1:abcdef".to_string();
    binding.thread_anchor = "pck:v1:abcdef".to_string();
    storage
        .upsert_conversation_binding(&binding)
        .expect("seed binding");
    let mut candidates = vec![
        (sample_account("acc-1", 0), sample_token("acc-1")),
        (sample_account("acc-2", 1), sample_token("acc-2")),
    ];
    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        Some(&binding),
        &mut candidates,
        RouteConversationSource::PromptCacheKey,
    )
    .expect("routing context");
    assert!(routing.binding_selected);

    record_conversation_binding_terminal_response(
        &storage,
        Some(&routing),
        &candidates[1].0,
        Some("gpt-5.5"),
        200,
    )
    .expect("record failover success");

    let actual = storage
        .get_conversation_binding("key-hash-1", "pck:v1:abcdef")
        .expect("load binding")
        .expect("binding exists");
    assert_eq!(actual.account_id, "acc-1");
    assert_eq!(actual.thread_anchor, "pck:v1:abcdef");
    assert_eq!(actual.thread_epoch, 1);
}

#[test]
fn prompt_cache_route_binding_rebinds_when_bound_account_is_not_selected() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let mut binding = sample_binding("acc-1");
    binding.conversation_id = "pck:v1:abcdef".to_string();
    binding.thread_anchor = "pck:v1:abcdef".to_string();
    storage
        .upsert_conversation_binding(&binding)
        .expect("seed binding");
    let mut candidates = vec![(sample_account("acc-2", 0), sample_token("acc-2"))];
    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        Some(&binding),
        &mut candidates,
        RouteConversationSource::PromptCacheKey,
    )
    .expect("routing context");
    assert!(!routing.binding_selected);

    record_conversation_binding_terminal_response(
        &storage,
        Some(&routing),
        &candidates[0].0,
        Some("gpt-5.5"),
        200,
    )
    .expect("record stale rebind success");

    let actual = storage
        .get_conversation_binding("key-hash-1", "pck:v1:abcdef")
        .expect("load binding")
        .expect("binding exists");
    assert_eq!(actual.account_id, "acc-2");
    assert_eq!(actual.thread_anchor, "pck:v1:abcdef");
    assert_eq!(actual.thread_epoch, 2);
}

#[test]
fn prompt_cache_existing_only_route_binding_does_not_create_initial_binding() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    let mut candidates = vec![(sample_account("acc-1", 0), sample_token("acc-1"))];
    let routing = prepare_conversation_routing_with_source(
        "key-hash-1",
        Some("pck:v1:abcdef"),
        None,
        &mut candidates,
        RouteConversationSource::PromptCacheKeyExistingOnly,
    )
    .expect("routing context");

    record_conversation_binding_terminal_response(
        &storage,
        Some(&routing),
        &candidates[0].0,
        Some("gpt-5.5"),
        200,
    )
    .expect("record existing-only success");

    let actual = storage
        .get_conversation_binding("key-hash-1", "pck:v1:abcdef")
        .expect("load binding");
    assert!(actual.is_none());
}

/// 函数 `apply_candidate_rotation_reports_binding_source_when_binding_selected`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn apply_candidate_rotation_reports_binding_source_when_binding_selected() {
    let binding = sample_binding("acc-1");
    let mut routing = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        Some(&binding),
        &mut vec![
            (sample_account("acc-2", 0), sample_token("acc-2")),
            (sample_account("acc-1", 1), sample_token("acc-1")),
        ],
    )
    .expect("routing context");
    routing.manual_preferred_account_id = Some("acc-1".to_string());
    let mut candidates = vec![
        (sample_account("acc-2", 0), sample_token("acc-2")),
        (sample_account("acc-1", 1), sample_token("acc-1")),
    ];

    let plan = apply_candidate_rotation(
        &mut candidates,
        Some(&routing),
        "key-hash-1",
        Some("gpt-5.4"),
        None,
    );

    assert_eq!(plan.source, CandidateRotationSource::ConversationBinding);
    assert_eq!(plan.strategy_label, "conversation_bound");
    assert!(!plan.strategy_applied);
    assert_eq!(candidates[0].0.id, "acc-2");
}

#[test]
fn apply_candidate_rotation_reports_manual_preferred_source() {
    let mut routing = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        None,
        &mut vec![
            (sample_account("acc-2", 0), sample_token("acc-2")),
            (sample_account("acc-1", 1), sample_token("acc-1")),
        ],
    )
    .expect("routing context");
    routing.manual_preferred_account_id = Some("acc-2".to_string());
    let mut candidates = vec![
        (sample_account("acc-2", 0), sample_token("acc-2")),
        (sample_account("acc-1", 1), sample_token("acc-1")),
    ];

    let plan = apply_candidate_rotation(
        &mut candidates,
        Some(&routing),
        "key-hash-1",
        Some("gpt-5.4"),
        None,
    );

    assert_eq!(plan.source, CandidateRotationSource::ManualPreferredAccount);
    assert_eq!(plan.strategy_label, "manual_preferred_account");
    assert!(!plan.strategy_applied);
}

#[test]
fn apply_candidate_rotation_prefers_less_bound_account_for_new_thread() {
    let routing = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        None,
        &mut vec![
            (sample_account("acc-1", 0), sample_token("acc-1")),
            (sample_account("acc-2", 1), sample_token("acc-2")),
            (sample_account("acc-3", 2), sample_token("acc-3")),
        ],
    )
    .expect("routing context");
    let mut candidates = vec![
        (sample_account("acc-1", 0), sample_token("acc-1")),
        (sample_account("acc-2", 1), sample_token("acc-2")),
        (sample_account("acc-3", 2), sample_token("acc-3")),
    ];
    let account_binding_counts = HashMap::from([
        ("acc-1".to_string(), 2),
        ("acc-2".to_string(), 1),
        ("acc-3".to_string(), 0),
    ]);

    let plan = apply_candidate_rotation(
        &mut candidates,
        Some(&routing),
        "key-hash-1",
        Some("gpt-5.4"),
        Some(&account_binding_counts),
    );

    assert_eq!(
        plan.source,
        CandidateRotationSource::ThreadAwareDistribution
    );
    assert!(plan.strategy_applied);
    assert_eq!(candidates[0].0.id, "acc-3");
}

#[test]
fn apply_candidate_rotation_prefers_idle_account_over_unbound_busy_account() {
    let busy_account_id = "thread-distribution-busy-account";
    let idle_account_id = "thread-distribution-idle-account";
    let routing = prepare_conversation_routing(
        "key-hash-thread-distribution-inflight",
        Some("conv-thread-distribution-inflight"),
        None,
        &mut vec![
            (
                sample_account(busy_account_id, 0),
                sample_token(busy_account_id),
            ),
            (
                sample_account(idle_account_id, 1),
                sample_token(idle_account_id),
            ),
        ],
    )
    .expect("routing context");
    let mut candidates = vec![
        (
            sample_account(busy_account_id, 0),
            sample_token(busy_account_id),
        ),
        (
            sample_account(idle_account_id, 1),
            sample_token(idle_account_id),
        ),
    ];
    let account_binding_counts = HashMap::from([
        (busy_account_id.to_string(), 0),
        (idle_account_id.to_string(), 1),
    ]);
    let _busy_guard = crate::gateway::acquire_account_inflight(busy_account_id);

    let plan = apply_candidate_rotation(
        &mut candidates,
        Some(&routing),
        "key-hash-thread-distribution-inflight",
        Some("gpt-5.4"),
        Some(&account_binding_counts),
    );

    assert_eq!(
        plan.source,
        CandidateRotationSource::ThreadAwareDistribution
    );
    assert!(plan.strategy_applied);
    assert_eq!(candidates[0].0.id, idle_account_id);
}

#[test]
fn apply_candidate_rotation_keeps_existing_binding_before_thread_distribution() {
    let binding = sample_binding("acc-1");
    let routing = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        Some(&binding),
        &mut vec![
            (sample_account("acc-1", 0), sample_token("acc-1")),
            (sample_account("acc-2", 1), sample_token("acc-2")),
        ],
    )
    .expect("routing context");
    let mut candidates = vec![
        (sample_account("acc-1", 0), sample_token("acc-1")),
        (sample_account("acc-2", 1), sample_token("acc-2")),
    ];
    let account_binding_counts = HashMap::from([("acc-1".to_string(), 4)]);

    let plan = apply_candidate_rotation(
        &mut candidates,
        Some(&routing),
        "key-hash-1",
        Some("gpt-5.4"),
        Some(&account_binding_counts),
    );

    assert_eq!(plan.source, CandidateRotationSource::ConversationBinding);
    assert_eq!(candidates[0].0.id, "acc-1");
}

/// 函数 `terminal_response_creates_and_rebinds_conversation_binding_on_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn terminal_response_creates_and_rebinds_conversation_binding_on_success() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let mut candidates = vec![(sample_account("acc-1", 0), sample_token("acc-1"))];
    let routing = prepare_conversation_routing("key-hash-1", Some("conv-1"), None, &mut candidates)
        .expect("routing context");
    record_conversation_binding_terminal_response(
        &storage,
        Some(&routing),
        &candidates[0].0,
        Some("gpt-5.4"),
        200,
    )
    .expect("create binding");

    let created = storage
        .get_conversation_binding("key-hash-1", "conv-1")
        .expect("load binding")
        .expect("binding exists");
    assert_eq!(created.account_id, "acc-1");

    let rebound_context = prepare_conversation_routing(
        "key-hash-1",
        Some("conv-1"),
        Some(&created),
        &mut vec![(sample_account("acc-2", 0), sample_token("acc-2"))],
    )
    .expect("rebound routing");
    record_conversation_binding_terminal_response(
        &storage,
        Some(&rebound_context),
        &sample_account("acc-2", 0),
        Some("gpt-5.5"),
        200,
    )
    .expect("rebind binding");

    let rebound = storage
        .get_conversation_binding("key-hash-1", "conv-1")
        .expect("reload binding")
        .expect("binding exists");
    assert_eq!(rebound.account_id, "acc-2");
    assert_eq!(rebound.thread_epoch, 2);
    assert_eq!(rebound.thread_anchor, created.thread_anchor);
    assert_eq!(rebound.last_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        rebound.last_switch_reason.as_deref(),
        Some("automatic_account_switch")
    );
}
