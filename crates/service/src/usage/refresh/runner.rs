use chrono::Local;
use croner::parser::{CronParser, Seconds};
use rand::Rng;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use super::settings::{current_mutex_string, warmup_cron_signal_version};
use super::{
    is_keepalive_error_ignorable, parse_interval_secs,
    refresh_tokens_before_expiry_for_all_accounts,
    refresh_usage_and_aggregate_balances_for_polling_cycle, run_gateway_keepalive_once,
    COMMON_POLL_FAILURE_BACKOFF_MAX_ENV, COMMON_POLL_JITTER_ENV,
    DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS, DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
    DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS, DEFAULT_USAGE_POLL_JITTER_SECS,
    GATEWAY_KEEPALIVE_ENABLED, GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV,
    GATEWAY_KEEPALIVE_INTERVAL_SECS, GATEWAY_KEEPALIVE_JITTER_ENV,
    TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS, TOKEN_REFRESH_POLLING_ENABLED,
    TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC, USAGE_POLLING_ENABLED,
    USAGE_POLL_FAILURE_BACKOFF_MAX_ENV, USAGE_POLL_INTERVAL_SECS, USAGE_POLL_JITTER_ENV,
    WARMUP_CRON_ENABLED, WARMUP_CRON_EXPRESSION, WARMUP_CRON_SIGNAL,
};

/// 函数 `usage_polling_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn usage_polling_loop() {
    run_dynamic_poll_loop(
        "usage polling",
        || USAGE_POLLING_ENABLED.load(Ordering::Relaxed),
        || USAGE_POLL_INTERVAL_SECS.load(Ordering::Relaxed),
        || {
            parse_interval_with_fallback(
                USAGE_POLL_JITTER_ENV,
                COMMON_POLL_JITTER_ENV,
                DEFAULT_USAGE_POLL_JITTER_SECS,
                0,
            )
        },
        |interval_secs| {
            parse_interval_with_fallback(
                USAGE_POLL_FAILURE_BACKOFF_MAX_ENV,
                COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
                DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS,
                interval_secs,
            )
        },
        refresh_usage_and_aggregate_balances_for_polling_cycle,
        |_| true,
    );
}

/// 函数 `gateway_keepalive_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn gateway_keepalive_loop() {
    run_dynamic_poll_loop(
        "gateway keepalive",
        || GATEWAY_KEEPALIVE_ENABLED.load(Ordering::Relaxed),
        || GATEWAY_KEEPALIVE_INTERVAL_SECS.load(Ordering::Relaxed),
        || {
            parse_interval_with_fallback(
                GATEWAY_KEEPALIVE_JITTER_ENV,
                COMMON_POLL_JITTER_ENV,
                DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
                0,
            )
        },
        |interval_secs| {
            parse_interval_with_fallback(
                GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV,
                COMMON_POLL_FAILURE_BACKOFF_MAX_ENV,
                DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS,
                interval_secs,
            )
        },
        run_gateway_keepalive_once,
        |err| !is_keepalive_error_ignorable(err),
    );
}

/// 函数 `token_refresh_polling_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn token_refresh_polling_loop() {
    run_dynamic_poll_loop(
        "token refresh polling",
        || TOKEN_REFRESH_POLLING_ENABLED.load(Ordering::Relaxed),
        || TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC.load(Ordering::Relaxed),
        || 0,
        |interval_secs| TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS.max(interval_secs),
        refresh_tokens_before_expiry_for_all_accounts,
        |_| true,
    );
}

pub(super) fn warmup_cron_loop() {
    let mut last_invalid_expression = String::new();
    let mut signal_version = warmup_cron_signal_version();
    loop {
        if crate::shutdown_requested() {
            break;
        }
        if !WARMUP_CRON_ENABLED.load(Ordering::Relaxed) {
            signal_version = wait_for_warmup_cron_change(signal_version, None).0;
            continue;
        }

        let expression = current_mutex_string(&WARMUP_CRON_EXPRESSION);
        let next_run_at = match next_cron_after(expression.as_str(), Local::now()) {
            Ok(next_run_at) => next_run_at,
            Err(err) => {
                if last_invalid_expression != expression {
                    log::warn!("account warmup cron disabled by invalid expression: {err}");
                    last_invalid_expression = expression;
                }
                signal_version =
                    wait_for_warmup_cron_change(signal_version, Some(Duration::from_secs(60))).0;
                continue;
            }
        };
        last_invalid_expression.clear();
        log::info!(
            "account warmup cron scheduled: expression=\"{}\" next_run_at={}",
            expression,
            next_run_at.to_rfc3339()
        );

        let delay = delay_until(next_run_at);
        let (next_signal_version, timed_out) =
            wait_for_warmup_cron_change(signal_version, Some(delay));
        signal_version = next_signal_version;
        if !timed_out {
            continue;
        }
        if Local::now() < next_run_at {
            continue;
        }
        if !WARMUP_CRON_ENABLED.load(Ordering::Relaxed)
            || current_mutex_string(&WARMUP_CRON_EXPRESSION) != expression
        {
            continue;
        }

        match crate::account_warmup::warmup_accounts(Vec::new(), "") {
            Ok(result) => log::info!(
                "account warmup cron finished: requested={} succeeded={} failed={}",
                result.requested,
                result.succeeded,
                result.failed
            ),
            Err(err) => log::warn!("account warmup cron error: {err}"),
        }
    }
}

/// 函数 `parse_interval_with_fallback`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - primary_env: 参数 primary_env
/// - fallback_env: 参数 fallback_env
/// - default_secs: 参数 default_secs
/// - min_secs: 参数 min_secs
///
/// # 返回
/// 返回函数执行结果
fn parse_interval_with_fallback(
    primary_env: &str,
    fallback_env: &str,
    default_secs: u64,
    min_secs: u64,
) -> u64 {
    let primary = std::env::var(primary_env).ok();
    let fallback = std::env::var(fallback_env).ok();
    let raw = primary.as_deref().or(fallback.as_deref());
    parse_interval_secs(raw, default_secs, min_secs)
}

fn delay_until(next: chrono::DateTime<Local>) -> Duration {
    let millis = next
        .signed_duration_since(Local::now())
        .num_milliseconds()
        .max(1) as u64;
    Duration::from_millis(millis)
}

fn wait_for_warmup_cron_change(last_seen: u64, timeout: Option<Duration>) -> (u64, bool) {
    let effective_timeout = match timeout {
        Some(d) => d.min(Duration::from_millis(500)),
        None => Duration::from_millis(500),
    };
    let (lock, cvar) =
        WARMUP_CRON_SIGNAL.get_or_init(|| (std::sync::Mutex::new(0), std::sync::Condvar::new()));
    let guard = crate::lock_utils::lock_recover(lock, "warmup_cron_signal");
    if *guard != last_seen {
        return (*guard, false);
    }

    let (guard, timed_out) = match cvar.wait_timeout(guard, effective_timeout) {
        Ok((guard, result)) => (guard, result.timed_out()),
        Err(poisoned) => {
            let (guard, result) = poisoned.into_inner();
            (guard, result.timed_out())
        }
    };
    (*guard, timed_out)
}

pub(super) fn validate_warmup_cron_expression(expression: &str) -> Result<(), String> {
    next_cron_after(expression, Local::now()).map(|_| ())
}

fn next_cron_after(
    expression: &str,
    after: chrono::DateTime<Local>,
) -> Result<chrono::DateTime<Local>, String> {
    let schedules = parse_cron_schedules(expression)?;
    let mut next_match: Option<chrono::DateTime<Local>> = None;

    for schedule in schedules {
        let candidate = next_cron_after_schedule(&schedule, after)?;
        next_match = match next_match {
            Some(current) if current <= candidate => Some(current),
            _ => Some(candidate),
        };
    }

    next_match.ok_or_else(|| "cron expression has no schedule".to_string())
}

fn parse_cron_schedules(expression: &str) -> Result<Vec<CronSchedule>, String> {
    let mut schedules = Vec::new();
    for item in expression.split('|') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let schedule = CronParser::builder()
            .seconds(Seconds::Optional)
            .build()
            .parse(trimmed)
            .map_err(|err| err.to_string())?;
        schedules.push(CronSchedule { schedule });
    }
    if schedules.is_empty() {
        return Err("cron expression has no schedule".to_string());
    }
    Ok(schedules)
}

fn next_cron_after_schedule(
    schedule: &CronSchedule,
    after: chrono::DateTime<Local>,
) -> Result<chrono::DateTime<Local>, String> {
    schedule
        .schedule
        .find_next_occurrence(&after, false)
        .map_err(|err| err.to_string())
}

struct CronSchedule {
    schedule: croner::Cron,
}

/// 函数 `run_dynamic_poll_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - loop_name: 参数 loop_name
/// - enabled: 参数 enabled
/// - interval_secs: 参数 interval_secs
/// - jitter_secs: 参数 jitter_secs
/// - failure_backoff_cap_secs: 参数 failure_backoff_cap_secs
/// - task: 参数 task
/// - should_log_error: 参数 should_log_error
///
/// # 返回
/// 无
fn run_dynamic_poll_loop<F, L, E, I, J, B>(
    loop_name: &str,
    enabled: E,
    interval_secs: I,
    jitter_secs: J,
    failure_backoff_cap_secs: B,
    mut task: F,
    mut should_log_error: L,
) where
    F: FnMut() -> Result<(), String>,
    L: FnMut(&str) -> bool,
    E: Fn() -> bool,
    I: Fn() -> u64,
    J: Fn() -> u64,
    B: Fn(u64) -> u64,
{
    let mut rng = rand::thread_rng();
    let mut consecutive_failures = 0u32;
    loop {
        if crate::shutdown_requested() {
            break;
        }
        if !enabled() {
            consecutive_failures = 0;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(1) {
                if crate::shutdown_requested() {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            continue;
        }

        let succeeded = match task() {
            Ok(_) => true,
            Err(err) => {
                if should_log_error(err.as_str()) {
                    log::warn!("{loop_name} error: {err}");
                }
                false
            }
        };

        if succeeded {
            consecutive_failures = 0;
        } else {
            consecutive_failures = consecutive_failures.saturating_add(1);
        }

        let base_interval_secs = interval_secs().max(1);
        let jitter_cap_secs = jitter_secs();
        let sampled_jitter = if jitter_cap_secs == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs(rng.gen_range(0..=jitter_cap_secs))
        };
        let delay = next_dynamic_poll_delay(
            Duration::from_secs(base_interval_secs),
            Duration::from_secs(jitter_cap_secs),
            Duration::from_secs(
                failure_backoff_cap_secs(base_interval_secs).max(base_interval_secs),
            ),
            consecutive_failures,
            sampled_jitter,
        );
        let start = std::time::Instant::now();
        while start.elapsed() < delay {
            if crate::shutdown_requested() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// 函数 `next_dynamic_poll_delay`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - interval: 参数 interval
/// - jitter_cap: 参数 jitter_cap
/// - failure_backoff_cap: 参数 failure_backoff_cap
/// - consecutive_failures: 参数 consecutive_failures
/// - sampled_jitter: 参数 sampled_jitter
///
/// # 返回
/// 返回函数执行结果
fn next_dynamic_poll_delay(
    interval: Duration,
    jitter_cap: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
    sampled_jitter: Duration,
) -> Duration {
    let base_delay =
        next_dynamic_failure_backoff(interval, failure_backoff_cap, consecutive_failures);
    let bounded_jitter = if jitter_cap.is_zero() {
        Duration::ZERO
    } else {
        sampled_jitter.min(jitter_cap)
    };
    base_delay
        .checked_add(bounded_jitter)
        .unwrap_or(Duration::MAX)
}

/// 函数 `next_dynamic_failure_backoff`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - interval: 参数 interval
/// - failure_backoff_cap: 参数 failure_backoff_cap
/// - consecutive_failures: 参数 consecutive_failures
///
/// # 返回
/// 返回函数执行结果
fn next_dynamic_failure_backoff(
    interval: Duration,
    failure_backoff_cap: Duration,
    consecutive_failures: u32,
) -> Duration {
    if consecutive_failures == 0 {
        return interval;
    }

    let base_ms = interval.as_millis();
    if base_ms == 0 {
        return interval;
    }

    let cap_ms = failure_backoff_cap.max(interval).as_millis();
    let shift = (consecutive_failures.saturating_sub(1)).min(20);
    let multiplier = 1u128 << shift;
    let scaled_ms = base_ms.saturating_mul(multiplier);
    let bounded_ms = scaled_ms.min(cap_ms).max(base_ms);
    if bounded_ms > u64::MAX as u128 {
        Duration::from_millis(u64::MAX)
    } else {
        Duration::from_millis(bounded_ms as u64)
    }
}

#[cfg(test)]
#[path = "../tests/usage_refresh_runner_tests.rs"]
mod tests;
