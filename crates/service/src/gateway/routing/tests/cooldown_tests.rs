use super::*;

/// 函数 `lookup_evicts_expired_target_entry_without_full_scan`
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
fn lookup_evicts_expired_target_entry_without_full_scan() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = lock.lock().expect("cooldown state lock");
    let now = now_ts();
    state.entries.insert("acc-a".to_string(), now - 1);
    state.entries.insert("acc-b".to_string(), now - 1);
    drop(state);

    assert!(!is_account_in_cooldown("acc-a"));

    let state = lock.lock().expect("cooldown state lock");
    assert!(!state.entries.contains_key("acc-a"));
    assert!(state.entries.contains_key("acc-b"));
}

/// 函数 `mark_path_cleanup_prunes_expired_entries`
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
fn mark_path_cleanup_prunes_expired_entries() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = lock.lock().expect("cooldown state lock");
    let now = now_ts();
    state.entries.insert("stale".to_string(), now - 1);
    state.last_cleanup_at = now - ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS - 1;
    drop(state);

    mark_account_cooldown("fresh", CooldownReason::Default);

    let state = lock.lock().expect("cooldown state lock");
    assert!(!state.entries.contains_key("stale"));
    assert!(state.entries.contains_key("fresh"));
}

/// 函数 `rate_limit_ladder_maps_to_expected_steps`
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
fn rate_limit_ladder_maps_to_expected_steps() {
    assert_eq!(rate_limit_cooldown_secs_for_offense(1), 45);
    assert_eq!(rate_limit_cooldown_secs_for_offense(2), 300);
    assert_eq!(rate_limit_cooldown_secs_for_offense(3), 1800);
    assert_eq!(rate_limit_cooldown_secs_for_offense(4), 7200);
    assert_eq!(rate_limit_cooldown_secs_for_offense(5), 7200);
}

/// 函数 `rate_limited_mark_increments_and_success_clear_decays_offense`
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
fn rate_limited_mark_increments_and_success_clear_decays_offense() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    mark_account_cooldown("acc", CooldownReason::RateLimited);
    {
        let state = lock.lock().expect("cooldown state lock");
        assert_eq!(state.offense_counts.get("acc"), Some(&1));
    }

    mark_account_cooldown("acc", CooldownReason::RateLimited);
    {
        let state = lock.lock().expect("cooldown state lock");
        assert_eq!(state.offense_counts.get("acc"), Some(&2));
    }

    clear_account_cooldown("acc");
    {
        let state = lock.lock().expect("cooldown state lock");
        assert_eq!(state.offense_counts.get("acc"), Some(&1));
    }

    clear_account_cooldown("acc");
    {
        let state = lock.lock().expect("cooldown state lock");
        assert!(!state.offense_counts.contains_key("acc"));
    }
}

/// 函数 `non_rate_limited_mark_keeps_existing_behavior_without_offense_count`
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
fn non_rate_limited_mark_keeps_existing_behavior_without_offense_count() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    mark_account_cooldown("acc", CooldownReason::Default);

    let state = lock.lock().expect("cooldown state lock");
    assert!(state.entries.contains_key("acc"));
    assert!(!state.offense_counts.contains_key("acc"));
}

#[test]
fn anthropic_challenge_cooldown_extends_generic_challenge_window() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let now = now_ts();

    mark_account_cooldown("acc", CooldownReason::Challenge);
    let generic_until = {
        let state = lock.lock().expect("cooldown state lock");
        *state
            .entries
            .get("acc")
            .expect("generic challenge cooldown")
    };

    mark_account_cooldown("acc", CooldownReason::AnthropicChallenge);
    let anthropic_until = {
        let state = lock.lock().expect("cooldown state lock");
        *state
            .entries
            .get("acc")
            .expect("anthropic challenge cooldown")
    };

    assert!(generic_until >= now + DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS);
    assert!(anthropic_until >= now + DEFAULT_ACCOUNT_COOLDOWN_ANTHROPIC_CHALLENGE_SECS);
    assert!(anthropic_until > generic_until);
}

/// 函数 `rate_limited_offense_resets_after_quiet_period`
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
fn rate_limited_offense_resets_after_quiet_period() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let now = now_ts();
    {
        let mut state = lock.lock().expect("cooldown state lock");
        state.offense_counts.insert("acc".to_string(), 3);
        state.offense_last_at.insert(
            "acc".to_string(),
            now - ACCOUNT_RATE_LIMIT_OFFENSE_FORGET_AFTER_SECS - 1,
        );
    }

    mark_account_cooldown("acc", CooldownReason::RateLimited);

    let state = lock.lock().expect("cooldown state lock");
    assert_eq!(state.offense_counts.get("acc"), Some(&1));
}

#[test]
fn retry_after_and_rate_limit_reset_headers_use_real_window() {
    let now = 1_800_000_000_i64;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("retry-after", "30".parse().expect("retry-after"));
    headers.insert(
        "x-ratelimit-reset-requests",
        "1m15s".parse().expect("request reset"),
    );
    headers.insert(
        "x-ratelimit-reset-tokens",
        "45.5s".parse().expect("token reset"),
    );

    assert_eq!(
        rate_limit_reset_at_from_headers(&headers, now),
        Some(now + 75)
    );
}

#[test]
fn retry_after_supports_http_date_and_reset_epoch() {
    let now = 1_445_412_000_i64;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "retry-after",
        "Wed, 21 Oct 2015 07:28:00 GMT".parse().expect("http date"),
    );
    headers.insert(
        "x-ratelimit-reset",
        "1445412605".parse().expect("reset epoch"),
    );

    assert_eq!(
        rate_limit_reset_at_from_headers(&headers, now),
        Some(1_445_412_605)
    );
}

#[test]
fn explicit_rate_limit_window_is_not_replaced_by_default_ladder() {
    let _guard = crate::test_env_guard();
    clear_account_cooldown_for_tests();
    let now = now_ts();
    mark_account_rate_limited_until("explicit-window", now + 5 * 60 * 60);

    let info = account_cooldown_info("explicit-window").expect("cooldown info");
    assert!(info.rate_limited);
    assert!(info.until >= now + 5 * 60 * 60);
    assert!(!ACCOUNT_COOLDOWN_UNTIL
        .get()
        .expect("cooldown state")
        .lock()
        .expect("cooldown lock")
        .offense_counts
        .contains_key("explicit-window"));
}
