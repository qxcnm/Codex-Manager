use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const FAILURE_THRESHOLD: usize = 2;
const ISOLATION_DURATION: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Default)]
struct ProxyStreamHealth {
    failures: VecDeque<Instant>,
    isolated_until: Option<Instant>,
}

#[derive(Debug, Default)]
struct ProxyStreamCircuitState {
    routes: HashMap<String, ProxyStreamHealth>,
}

impl ProxyStreamCircuitState {
    fn is_open(&mut self, route: &str, now: Instant) -> bool {
        let Some(health) = self.routes.get_mut(route) else {
            return false;
        };
        if health.isolated_until.is_some_and(|until| until > now) {
            return true;
        }
        health.isolated_until = None;
        prune_failures(health, now);
        false
    }

    fn record_failure(&mut self, route: &str, now: Instant) -> bool {
        let health = self.routes.entry(route.to_string()).or_default();
        if health.isolated_until.is_some_and(|until| until > now) {
            return true;
        }
        health.isolated_until = None;
        prune_failures(health, now);
        health.failures.push_back(now);
        if health.failures.len() < FAILURE_THRESHOLD {
            return false;
        }
        health.failures.clear();
        health.isolated_until = Some(now + ISOLATION_DURATION);
        true
    }

    fn record_success(&mut self, route: &str, now: Instant) {
        let Some(health) = self.routes.get_mut(route) else {
            return;
        };
        if !health.isolated_until.is_some_and(|until| until > now) {
            health.failures.clear();
            health.isolated_until = None;
        }
    }
}

fn prune_failures(health: &mut ProxyStreamHealth, now: Instant) {
    while health
        .failures
        .front()
        .is_some_and(|failed_at| now.saturating_duration_since(*failed_at) > FAILURE_WINDOW)
    {
        health.failures.pop_front();
    }
}

fn circuit_state() -> &'static Mutex<ProxyStreamCircuitState> {
    static STATE: OnceLock<Mutex<ProxyStreamCircuitState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ProxyStreamCircuitState::default()))
}

fn route_key_for_account(account_id: &str) -> String {
    crate::gateway::runtime_config::upstream_proxy_url_for_account(account_id)
        .unwrap_or_else(|| "<direct>".to_string())
}

fn route_label(route: &str) -> String {
    if route == "<direct>" {
        return route.to_string();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    route.hash(&mut hasher);
    format!("proxy:{:016x}", hasher.finish())
}

pub(crate) fn is_account_proxy_stream_circuit_open(account_id: &str) -> bool {
    let route = route_key_for_account(account_id);
    crate::lock_utils::lock_recover(circuit_state(), "proxy_stream_circuit")
        .is_open(route.as_str(), Instant::now())
}

pub(crate) fn record_account_proxy_stream_failure(account_id: &str) {
    let route = route_key_for_account(account_id);
    let opened = crate::lock_utils::lock_recover(circuit_state(), "proxy_stream_circuit")
        .record_failure(route.as_str(), Instant::now());
    if opened {
        log::warn!(
            "event=gateway_proxy_stream_circuit_open account_id={} route={} isolation_secs={}",
            account_id,
            route_label(route.as_str()),
            ISOLATION_DURATION.as_secs()
        );
    }
}

pub(crate) fn record_account_proxy_stream_success(account_id: &str) {
    let route = route_key_for_account(account_id);
    crate::lock_utils::lock_recover(circuit_state(), "proxy_stream_circuit")
        .record_success(route.as_str(), Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_failures_inside_window_open_route_for_ten_minutes() {
        let mut state = ProxyStreamCircuitState::default();
        let started = Instant::now();
        assert!(!state.record_failure("route-a", started));
        assert!(state.record_failure("route-a", started + Duration::from_secs(30)));
        assert!(state.is_open("route-a", started + Duration::from_secs(9 * 60)));
        assert!(!state.is_open("route-a", started + Duration::from_secs(11 * 60)));
    }

    #[test]
    fn failures_outside_window_do_not_open_route() {
        let mut state = ProxyStreamCircuitState::default();
        let started = Instant::now();
        assert!(!state.record_failure("route-a", started));
        assert!(!state.record_failure("route-a", started + Duration::from_secs(61)));
        assert!(!state.is_open("route-a", started + Duration::from_secs(62)));
    }

    #[test]
    fn healthy_completed_stream_clears_pending_failure() {
        let mut state = ProxyStreamCircuitState::default();
        let started = Instant::now();
        assert!(!state.record_failure("route-a", started));
        state.record_success("route-a", started + Duration::from_secs(5));
        assert!(!state.record_failure("route-a", started + Duration::from_secs(10)));
        assert!(!state.is_open("route-a", started + Duration::from_secs(11)));
    }
}
