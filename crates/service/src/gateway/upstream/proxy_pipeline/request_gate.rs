use super::super::support::deadline;
use std::time::Instant;

/// 函数 `acquire_request_gate`
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
pub(in super::super) fn acquire_request_gate(
    trace_id: &str,
    key_id: &str,
    path: &str,
    model_for_log: Option<&str>,
    request_scope: &str,
    request_deadline: Option<Instant>,
) -> Option<super::super::super::request_gate::RequestGateGuard> {
    let request_gate_lock =
        super::super::super::request_gate_lock(key_id, path, model_for_log, request_scope);
    let request_gate_wait_timeout = super::super::super::request_gate_wait_timeout();
    super::super::super::trace_log::log_request_gate_wait(trace_id, key_id, path, model_for_log);
    let gate_wait_started_at = Instant::now();

    match request_gate_lock.try_acquire() {
        Ok(Some(guard)) => {
            super::super::super::trace_log::log_request_gate_acquired(
                trace_id,
                key_id,
                path,
                model_for_log,
                0,
            );
            Some(guard)
        }
        Ok(None) => {
            let wait_result = match request_gate_wait_timeout {
                Some(wait_timeout) => match deadline::cap_wait(wait_timeout, request_deadline) {
                    Some(effective_wait) if !effective_wait.is_zero() => {
                        request_gate_lock.acquire_with_timeout(effective_wait)
                    }
                    _ => Ok(None),
                },
                None => match deadline::remaining(request_deadline) {
                    Some(remaining) if remaining.is_zero() => Ok(None),
                    Some(remaining) => request_gate_lock.acquire_with_timeout(remaining),
                    None => request_gate_lock.acquire().map(Some),
                },
            };
            if let Ok(Some(guard)) = wait_result {
                super::super::super::trace_log::log_request_gate_acquired(
                    trace_id,
                    key_id,
                    path,
                    model_for_log,
                    gate_wait_started_at.elapsed().as_millis(),
                );
                Some(guard)
            } else {
                match wait_result {
                    Err(super::super::super::RequestGateAcquireError::Poisoned) => {
                        super::super::super::trace_log::log_request_gate_skip(
                            trace_id,
                            "lock_poisoned",
                            gate_wait_started_at.elapsed().as_millis(),
                        );
                    }
                    _ => {
                        let reason = if deadline::is_expired(request_deadline) {
                            "total_timeout"
                        } else {
                            "gate_wait_timeout"
                        };
                        super::super::super::trace_log::log_request_gate_skip(
                            trace_id,
                            reason,
                            gate_wait_started_at.elapsed().as_millis(),
                        );
                    }
                }
                None
            }
        }
        Err(super::super::super::RequestGateAcquireError::Poisoned) => {
            super::super::super::trace_log::log_request_gate_skip(trace_id, "lock_poisoned", 0);
            None
        }
    }
}

#[cfg(test)]
#[path = "request_gate_tests.rs"]
mod tests;
