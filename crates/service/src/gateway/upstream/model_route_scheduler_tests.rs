use super::*;

fn route(kind: &str, id: &str, upstream: &str, priority: i64, weight: i64) -> ModelRouteV2 {
    ModelRouteV2 {
        source_kind: kind.into(),
        source_id: id.into(),
        upstream_model: upstream.into(),
        priority,
        weight,
        enabled: true,
        ..Default::default()
    }
}

fn key(model: &str, weights: &[i64]) -> TierKey {
    TierKey {
        model: model.into(),
        preference: 0,
        priority: 0,
        branches: weights
            .iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    BranchKey("aggregate_api".into(), i.to_string(), "upstream".into()),
                    *w,
                )
            })
            .collect(),
    }
}

#[test]
fn smooth_wrr_has_exact_weighted_shares_and_no_bursty_expansion() {
    let mut scheduler = Scheduler::default();
    let now = Instant::now();
    let mut counts = [0; 3];
    let sequence = (0..7)
        .map(|_| scheduler.select(key("m", &[5, 1, 1]), now))
        .collect::<Vec<_>>();
    assert_eq!(sequence, [0, 0, 1, 0, 2, 0, 0]);
    for _ in 0..700 {
        counts[scheduler.select(key("m", &[5, 1, 1]), now)] += 1;
    }
    assert_eq!(counts, [500, 100, 100]);
}

#[test]
fn tiers_sort_priority_across_sources_and_keep_same_source_model_branches() {
    let routes = vec![
        route("aggregate_api", "a", "secondary", 10, 1),
        route("account_pool", "default", "pool", 20, 1),
        route("aggregate_api", "a", "primary", 20, 3),
    ];
    let weighted = tiers(ModelRoutePolicyV2::Weighted, routes.clone());
    assert_eq!(weighted.iter().map(Vec::len).collect::<Vec<_>>(), [2, 1]);
    assert!(weighted[0].iter().all(|route| route.priority == 20));
    let preferred = tiers(ModelRoutePolicyV2::AggregateFirst, routes.clone());
    assert_eq!(
        preferred
            .iter()
            .map(|g| g[0].upstream_model.as_str())
            .collect::<Vec<_>>(),
        ["primary", "secondary", "pool"]
    );
    let preferred = tiers(ModelRoutePolicyV2::AccountFirst, routes);
    assert_eq!(preferred[0][0].upstream_model, "pool");
}

#[test]
fn hard_key_restrictions_precede_model_preferences() {
    use GatewayUpstreamRouteKind::*;
    let account = route("account_pool", "default", "pool", 0, 1);
    let a = route("aggregate_api", "a", "one", 0, 1);
    let b = route("aggregate_api", "b", "two", 0, 1);
    for hybrid in [HybridAccountFirst, HybridAggregateFirst] {
        assert!(route_allowed(&account, hybrid, None));
        assert!(route_allowed(&a, hybrid, None));
        assert!(!route_allowed(&account, hybrid, Some("a")));
        assert!(!route_allowed(&b, hybrid, Some("a")));
        assert!(route_allowed(&a, hybrid, Some("a")));
    }
    assert!(!route_allowed(&a, AccountRotation, None));
    assert!(!route_allowed(&account, AggregateApi, None));
    let mut disabled = a;
    disabled.enabled = false;
    assert!(!route_allowed(&disabled, AggregateApi, None));
}

#[test]
fn candidate_set_weights_and_models_have_independent_state() {
    let mut scheduler = Scheduler::default();
    let now = Instant::now();
    assert_eq!(scheduler.select(key("a", &[1, 1]), now), 0);
    assert_eq!(scheduler.select(key("a", &[1, 1]), now), 1);
    assert_eq!(scheduler.select(key("b", &[1, 1]), now), 0);
    assert_eq!(scheduler.select(key("a", &[2, 1]), now), 0);
    assert_eq!(scheduler.select(key("a", &[1]), now), 0);
    assert_eq!(scheduler.tiers.len(), 4);
}

#[test]
fn state_is_lru_bounded_ttl_expiring_and_extreme_weights_do_not_overflow() {
    let mut scheduler = Scheduler::default();
    let now = Instant::now();
    for i in 0..MAX_STATES + 5 {
        scheduler.select(
            key(&i.to_string(), &[i64::MAX, i64::MAX]),
            now + Duration::from_nanos(i as u64),
        );
    }
    assert_eq!(scheduler.tiers.len(), MAX_STATES);
    assert!(!scheduler
        .tiers
        .contains_key(&key("0", &[i64::MAX, i64::MAX])));
    scheduler.select(
        key("new", &[1, 1]),
        now + STATE_TTL + Duration::from_secs(1),
    );
    assert_eq!(scheduler.tiers.len(), 1);
}

#[tokio::test]
async fn one_budget_is_shared_across_awaits_and_nested_transport_attempts() {
    scope_attempt_budget(async {
        for _ in 0..MAX_TRANSPORT_ATTEMPTS {
            tokio::task::yield_now().await;
            assert!(take_transport_attempt());
        }
        assert!(budget_exhausted());
        assert!(!take_transport_attempt());
    })
    .await;
    assert!(take_transport_attempt()); // Legacy requests have no new limit.
    scope_attempt_budget(async {
        assert!(!budget_exhausted());
    })
    .await;
}

#[test]
fn fallback_order_never_repeats_a_branch() {
    let routes = vec![
        route("aggregate_api", "a", "one", 0, 2),
        route("aggregate_api", "a", "two", 0, 1),
        route("aggregate_api", "b", "one", 0, 1),
    ];
    for _ in 0..12 {
        let ordered = order_tier(
            "no-repeat",
            ModelRoutePolicyV2::Weighted,
            tiers(ModelRoutePolicyV2::Weighted, routes.clone()).remove(0),
        );
        let distinct = ordered
            .iter()
            .map(branch_key)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(distinct.len(), 3);
    }
}
