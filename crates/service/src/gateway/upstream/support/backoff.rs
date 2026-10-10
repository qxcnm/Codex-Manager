use rand::Rng;
use std::time::{Duration, Instant};

/// 函数 `as_millis_u64`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - duration: 参数 duration
///
/// # 返回
/// 返回函数执行结果
fn as_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

/// 函数 `exponential_jitter_delay`
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
pub(in super::super) fn exponential_jitter_delay(
    base: Duration,
    cap: Duration,
    attempt: u32,
) -> Duration {
    let base_ms = as_millis_u64(base);
    let cap_ms = as_millis_u64(cap);
    if base_ms == 0 || cap_ms == 0 {
        return Duration::from_millis(0);
    }
    let multiplier = 1_u64 << attempt.min(10);
    let max_delay_ms = base_ms.saturating_mul(multiplier).min(cap_ms).max(1);
    let jitter_ms = rand::thread_rng().gen_range(0..=max_delay_ms);
    Duration::from_millis(jitter_ms)
}

/// 函数 `sleep_with_exponential_jitter`
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
#[cfg(test)]
pub(in super::super) fn sleep_with_exponential_jitter(
    base: Duration,
    cap: Duration,
    attempt: u32,
    deadline: Option<Instant>,
) -> bool {
    let delay = exponential_jitter_delay(base, cap, attempt);
    let Some(delay) = super::deadline::cap_wait(delay, deadline) else {
        return false;
    };
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    true
}

pub(in super::super) async fn sleep_with_exponential_jitter_async(
    base: Duration,
    cap: Duration,
    attempt: u32,
    deadline: Option<Instant>,
) -> bool {
    let Some(delay) =
        super::deadline::cap_wait(exponential_jitter_delay(base, cap, attempt), deadline)
    else {
        return false;
    };
    crate::http::gateway_request::with_response_cancellation(tokio::time::sleep(delay))
        .await
        .is_ok()
}

#[cfg(test)]
#[path = "../tests/support/backoff_tests.rs"]
mod tests;
