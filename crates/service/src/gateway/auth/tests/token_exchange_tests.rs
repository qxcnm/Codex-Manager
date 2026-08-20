use super::*;
use base64::Engine as _;

/// 函数 `same_account_reuses_exchange_lock`
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
fn same_account_reuses_exchange_lock() {
    let _guard = crate::test_env_guard();
    clear_account_token_exchange_locks_for_tests();
    let first = account_token_exchange_lock("acc-1");
    let second = account_token_exchange_lock("acc-1");
    assert!(Arc::ptr_eq(&first, &second));
}

/// 函数 `stale_unshared_exchange_lock_entry_is_reclaimed`
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
fn stale_unshared_exchange_lock_entry_is_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_account_token_exchange_locks_for_tests();
    let first = account_token_exchange_lock("acc-1");
    let weak = Arc::downgrade(&first);
    drop(first);

    let lock = ACCOUNT_TOKEN_EXCHANGE_LOCKS
        .get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    let mut table = lock.lock().expect("token exchange table lock");
    let now = now_ts();
    table
        .entries
        .get_mut("acc-1")
        .expect("token exchange entry")
        .last_seen_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let _second = account_token_exchange_lock("acc-1");
    assert!(weak.upgrade().is_none());
}

/// 函数 `stale_shared_exchange_lock_entry_is_not_reclaimed`
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
fn stale_shared_exchange_lock_entry_is_not_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_account_token_exchange_locks_for_tests();
    let first = account_token_exchange_lock("acc-1");

    let lock = ACCOUNT_TOKEN_EXCHANGE_LOCKS
        .get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    let mut table = lock.lock().expect("token exchange table lock");
    let now = now_ts();
    table
        .entries
        .get_mut("acc-1")
        .expect("token exchange entry")
        .last_seen_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let second = account_token_exchange_lock("acc-1");
    assert!(Arc::ptr_eq(&first, &second));
}

/// 函数 `fallback_to_access_token_uses_runtime_access_token_when_exchange_fails`
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
fn fallback_to_access_token_uses_runtime_access_token_when_exchange_fails() {
    let token = Token {
        account_id: "acc-2".to_string(),
        id_token: "runtime-id-token".to_string(),
        access_token: "runtime-access-token".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    let bearer =
        fallback_to_access_token(&token, "api key exchange failed").expect("fallback bearer");
    assert_eq!(bearer, "runtime-access-token");
}

/// 函数 `api_key_exchange_subject_token_omits_access_token_without_id_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn api_key_exchange_subject_token_omits_access_token_without_id_token() {
    let token = Token {
        account_id: "acc-import-session".to_string(),
        id_token: String::new(),
        access_token: " imported-session-access ".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert_eq!(api_key_exchange_subject_token(&token), None);
}

/// 函数 `api_key_exchange_subject_token_uses_id_token_only`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn api_key_exchange_subject_token_uses_id_token_only() {
    let token = Token {
        account_id: "acc-login".to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert_eq!(
        api_key_exchange_subject_token(&token),
        Some("id-token".to_string())
    );
}

#[test]
fn api_key_exchange_client_id_prefers_id_token_claim() {
    let jwt = |client_id: &str| {
        let payload = serde_json::json!({"sub":"user-test","client_id": client_id}).to_string();
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
        )
    };
    let token = Token {
        account_id: "acc-client-id".to_string(),
        id_token: jwt("id-token-client"),
        access_token: jwt("access-token-client"),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert_eq!(
        api_key_exchange_client_id(&token, "fallback-client"),
        "id-token-client"
    );
}

/// 函数 `usable_api_key_access_token_rejects_expired_jwt`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn usable_api_key_access_token_rejects_expired_jwt() {
    assert_eq!(
        usable_api_key_access_token("a.eyJleHAiOjE3MDAwMDAwMDB9.s"),
        None
    );
}

/// 函数 `usable_api_key_access_token_keeps_future_jwt_and_opaque_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn usable_api_key_access_token_keeps_future_jwt_and_opaque_token() {
    assert_eq!(
        usable_api_key_access_token("a.eyJleHAiOjQxMDI0NDQ4MDB9.s").as_deref(),
        Some("a.eyJleHAiOjQxMDI0NDQ4MDB9.s")
    );
    assert_eq!(
        usable_api_key_access_token("opaque-api-token").as_deref(),
        Some("opaque-api-token")
    );
}

/// 函数 `valid_access_token_skips_unavailable_mark_for_bearer_exchange_refresh_failure`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-14
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn valid_access_token_skips_unavailable_mark_for_bearer_exchange_refresh_failure() {
    let token = Token {
        account_id: "acc-valid-access".to_string(),
        id_token: "runtime-id-token".to_string(),
        access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert!(!should_mark_account_unavailable_after_refresh_failure_for_bearer_exchange(&token));
}

/// 函数 `expired_access_token_keeps_unavailable_mark_for_bearer_exchange_refresh_failure`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-14
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn expired_access_token_keeps_unavailable_mark_for_bearer_exchange_refresh_failure() {
    let token = Token {
        account_id: "acc-expired-access".to_string(),
        id_token: "runtime-id-token".to_string(),
        access_token: "a.eyJleHAiOjE3MDAwMDAwMDB9.s".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert!(should_mark_account_unavailable_after_refresh_failure_for_bearer_exchange(&token));
}
