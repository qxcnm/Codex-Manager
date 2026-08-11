use super::*;
use std::time::{Duration, Instant};

const ENV_REQUEST_GATE_WAIT_TIMEOUT_MS: &str = "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS";

fn restore_request_gate_wait_timeout(previous: Option<String>) {
    match previous {
        Some(value) => std::env::set_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS, value),
        None => std::env::remove_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS),
    }
    crate::gateway::reload_runtime_config_from_env();
}

#[test]
fn acquire_request_gate_times_out_before_request_deadline_when_busy() {
    let _guard = crate::test_env_guard();
    let previous = std::env::var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS).ok();
    std::env::set_var(ENV_REQUEST_GATE_WAIT_TIMEOUT_MS, "30");
    crate::gateway::reload_runtime_config_from_env();

    let first_guard = acquire_request_gate(
        "trc_gate_first",
        "gk_gate_bounded",
        "/v1/responses",
        Some("gpt-5.5"),
        "conversation-gate-bounded",
        Some(Instant::now() + Duration::from_secs(5)),
    )
    .expect("first request should acquire the gate immediately");

    let started_at = Instant::now();
    let second_guard = acquire_request_gate(
        "trc_gate_second",
        "gk_gate_bounded",
        "/v1/responses",
        Some("gpt-5.5"),
        "conversation-gate-bounded",
        Some(Instant::now() + Duration::from_secs(5)),
    );
    let waited = started_at.elapsed();

    drop(first_guard);
    restore_request_gate_wait_timeout(previous);

    assert!(
        second_guard.is_none(),
        "busy gate should time out and let the request proceed without a guard"
    );
    assert!(
        waited >= Duration::from_millis(20),
        "waited less than configured timeout: {waited:?}"
    );
    assert!(
        waited < Duration::from_millis(500),
        "gate wait should not consume the request deadline: {waited:?}"
    );
}
