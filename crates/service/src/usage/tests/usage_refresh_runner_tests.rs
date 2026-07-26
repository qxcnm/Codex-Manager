use super::*;
use chrono::{Duration as ChronoDuration, TimeZone, Timelike};

#[test]
fn warmup_cron_accepts_five_field_expression() {
    let after = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");

    let next = next_cron_after("0 */4 * * *", after).expect("next run");

    assert_eq!(next.hour(), 12);
    assert_eq!(next.minute(), 0);
    assert_eq!(next.second(), 0);
}

#[test]
fn warmup_cron_accepts_six_field_expression() {
    let after = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");

    let next = next_cron_after("45 15 10 * * *", after).expect("next run");

    assert_eq!(next.hour(), 10);
    assert_eq!(next.minute(), 15);
    assert_eq!(next.second(), 45);
}

#[test]
fn warmup_cron_uses_earliest_pipe_separated_schedule() {
    let after = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");

    let next = next_cron_after("0 18 * * *|30 10 * * *", after).expect("next run");

    assert_eq!(next.hour(), 10);
    assert_eq!(next.minute(), 30);
}

#[test]
fn warmup_cron_heartbeat_before_deadline_keeps_waiting() {
    let now = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");
    let next_run_at = now + ChronoDuration::seconds(10);

    assert_eq!(
        classify_warmup_cron_wait(7, 7, &now, &next_run_at, false),
        None,
    );
}

#[test]
fn warmup_cron_setting_change_interrupts_current_schedule() {
    let now = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");
    let next_run_at = now + ChronoDuration::hours(1);

    assert_eq!(
        classify_warmup_cron_wait(7, 8, &now, &next_run_at, false),
        Some(WarmupCronWaitOutcome::SettingsChanged),
    );
}

#[test]
fn warmup_cron_deadline_allows_scheduled_task_to_run() {
    let next_run_at = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");

    assert_eq!(
        classify_warmup_cron_wait(7, 7, &next_run_at, &next_run_at, false),
        Some(WarmupCronWaitOutcome::DeadlineReached),
    );
}

#[test]
fn warmup_cron_shutdown_interrupts_current_schedule() {
    let now = Local
        .with_ymd_and_hms(2026, 5, 23, 10, 15, 30)
        .single()
        .expect("local timestamp");
    let next_run_at = now + ChronoDuration::hours(1);

    assert_eq!(
        classify_warmup_cron_wait(7, 7, &now, &next_run_at, true),
        Some(WarmupCronWaitOutcome::ShutdownRequested),
    );
}

#[test]
fn dynamic_poll_delay_recalculates_when_disabled() {
    assert!(should_recalculate_dynamic_poll_delay(
        false,
        600,
        600,
        std::time::Duration::from_secs(1),
    ));
}

#[test]
fn dynamic_poll_delay_recalculates_when_interval_is_shortened_and_due() {
    assert!(should_recalculate_dynamic_poll_delay(
        true,
        600,
        60,
        std::time::Duration::from_secs(60),
    ));
}

#[test]
fn dynamic_poll_delay_keeps_sleeping_when_interval_is_shortened_but_not_due() {
    assert!(!should_recalculate_dynamic_poll_delay(
        true,
        600,
        60,
        std::time::Duration::from_secs(30),
    ));
}
