//! Model-route branches, not accounts: opt-in priority tiers and smooth WRR.
//! State is process-local, shared by model + effective tier, and bounded in both
//! entry count and tier width. Failed branches are never reinserted in a turn.
use codexmanager_core::storage::{ModelRoutePolicyV2, ModelRouteV2};
use std::collections::{BTreeMap, HashMap};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use super::executor::GatewayUpstreamRouteKind;

#[path = "model_route_execution.rs"]
mod execution;
pub(super) use execution::proxy_scheduled_request;

const MAX_STATES: usize = 512;
const STATE_TTL: Duration = Duration::from_secs(30 * 60);
pub(super) const MAX_TRANSPORT_ATTEMPTS: usize = 16;

tokio::task_local! {
    static ATTEMPTS_LEFT: AtomicUsize;
    static REQUEST_DEADLINE: Option<Instant>;
}

pub(super) async fn scope_request_deadline<F: std::future::Future>(
    deadline: Option<Instant>,
    future: F,
) -> F::Output {
    REQUEST_DEADLINE.scope(deadline, future).await
}

pub(super) fn remaining_request_deadline() -> Option<Duration> {
    REQUEST_DEADLINE
        .try_with(|deadline| {
            deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
        })
        .ok()
        .flatten()
}

pub(super) fn is_scheduled_request() -> bool {
    ATTEMPTS_LEFT.try_with(|_| ()).is_ok()
}

pub(super) async fn scope_attempt_budget<F: std::future::Future>(future: F) -> F::Output {
    ATTEMPTS_LEFT
        .scope(AtomicUsize::new(MAX_TRANSPORT_ATTEMPTS), future)
        .await
}

/// Called at the actual send boundary, including executor-local retries.
pub(super) fn take_transport_attempt() -> bool {
    ATTEMPTS_LEFT
        .try_with(|left| {
            let mut current = left.load(Ordering::Relaxed);
            while current > 0 {
                match left.compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return true,
                    Err(actual) => current = actual,
                }
            }
            false
        })
        .unwrap_or(true)
}

pub(super) fn budget_exhausted() -> bool {
    ATTEMPTS_LEFT
        .try_with(|left| left.load(Ordering::Relaxed) == 0)
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BranchKey(String, String, String);

fn branch_key(route: &ModelRouteV2) -> BranchKey {
    BranchKey(
        route.source_kind.clone(),
        route.source_id.clone(),
        route.upstream_model.clone(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TierKey {
    model: String,
    preference: u8,
    priority: i64,
    branches: Vec<(BranchKey, i64)>,
}

struct TierState {
    current: Vec<i128>,
    touched: Instant,
}

#[derive(Default)]
struct Scheduler {
    tiers: HashMap<TierKey, TierState>,
}

impl Scheduler {
    fn select(&mut self, key: TierKey, now: Instant) -> usize {
        self.tiers
            .retain(|_, state| now.saturating_duration_since(state.touched) < STATE_TTL);
        if !self.tiers.contains_key(&key) && self.tiers.len() >= MAX_STATES {
            if let Some(oldest) = self
                .tiers
                .iter()
                .min_by_key(|(_, state)| state.touched)
                .map(|(key, _)| key.clone())
            {
                self.tiers.remove(&oldest);
            }
        }
        let state = self.tiers.entry(key.clone()).or_insert_with(|| TierState {
            current: vec![0; key.branches.len()],
            touched: now,
        });
        state.touched = now;
        let mut winner = 0;
        let mut total = 0i128;
        for (idx, (_, weight)) in key.branches.iter().enumerate() {
            let weight = i128::from((*weight).max(1));
            total += weight;
            state.current[idx] += weight;
            if state.current[idx] > state.current[winner] {
                winner = idx;
            }
        }
        state.current[winner] -= total;
        winner
    }
}

fn source_preference(policy: ModelRoutePolicyV2, route: &ModelRouteV2) -> u8 {
    match policy {
        ModelRoutePolicyV2::AccountFirst => u8::from(route.source_kind != "account_pool"),
        ModelRoutePolicyV2::AggregateFirst => u8::from(route.source_kind != "aggregate_api"),
        _ => 0,
    }
}

pub(super) fn route_allowed(
    route: &ModelRouteV2,
    kind: GatewayUpstreamRouteKind,
    explicit: Option<&str>,
) -> bool {
    if !route.enabled {
        return false;
    }
    if let Some(id) = explicit.filter(|id| !id.trim().is_empty()) {
        return kind != GatewayUpstreamRouteKind::AccountRotation
            && route.source_kind == "aggregate_api"
            && route.source_id == id.trim();
    }
    match route.source_kind.as_str() {
        "account_pool" => {
            route.source_id == "default" && kind != GatewayUpstreamRouteKind::AggregateApi
        }
        "aggregate_api" => kind != GatewayUpstreamRouteKind::AccountRotation,
        _ => false,
    }
}

/// Build tiers only from eligible branches. A disappeared source/changed weight
/// gets its own state. Lower tiers advance only when the caller reaches them.
pub(super) fn tiers(
    policy: ModelRoutePolicyV2,
    routes: Vec<ModelRouteV2>,
) -> Vec<Vec<ModelRouteV2>> {
    let mut groups = BTreeMap::new();
    for route in routes {
        groups
            .entry((
                source_preference(policy, &route),
                std::cmp::Reverse(route.priority),
            ))
            .or_insert_with(Vec::new)
            .push(route);
    }
    groups
        .into_values()
        .map(|mut group| {
            group.sort_by_key(branch_key);
            group.dedup_by_key(|route| branch_key(route));
            group
        })
        .collect()
}

pub(super) fn order_tier(
    model: &str,
    policy: ModelRoutePolicyV2,
    mut tier: Vec<ModelRouteV2>,
) -> Vec<ModelRouteV2> {
    if tier.len() > 1 {
        static SCHEDULER: OnceLock<Mutex<Scheduler>> = OnceLock::new();
        let key = TierKey {
            model: model.to_string(),
            preference: source_preference(policy, &tier[0]),
            priority: tier[0].priority,
            branches: tier
                .iter()
                .map(|route| (branch_key(route), route.weight))
                .collect(),
        };
        let winner = SCHEDULER
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .select(key, Instant::now());
        tier.rotate_left(winner);
    }
    tier
}

#[cfg(test)]
#[path = "model_route_scheduler_tests.rs"]
mod tests;
