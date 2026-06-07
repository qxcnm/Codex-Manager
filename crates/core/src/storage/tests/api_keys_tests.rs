use super::{ApiKey, Storage};

/// 函数 `make_test_api_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - index: 参数 index
///
/// # 返回
/// 返回函数执行结果
fn make_test_api_key(index: usize) -> ApiKey {
    ApiKey {
        id: format!("key-{index:04}"),
        name: Some(format!("Key {index}")),
        model_slug: Some("gpt-5".to_string()),
        reasoning_effort: Some("medium".to_string()),
        service_tier: Some("priority".to_string()),
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{index:04}"),
        status: "active".to_string(),
        created_at: index as i64,
        last_used_at: Some(index as i64),
    }
}

/// 函数 `large_key_sets_are_chunked_for_api_key_and_quota_queries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn large_key_sets_are_chunked_for_api_key_and_quota_queries() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut selected = Vec::new();
    for index in 0..901 {
        let key = make_test_api_key(index);
        selected.push(key.id.clone());
        storage.insert_api_key(&key).expect("insert api key");
        storage
            .upsert_api_key_quota_limit(&key.id, Some(1000 + index as i64))
            .expect("insert quota limit");
    }

    let requested = selected.iter().rev().cloned().collect::<Vec<_>>();
    let keys = storage
        .list_api_keys_for_ids(&requested)
        .expect("list api keys");
    assert_eq!(keys.len(), selected.len());
    assert_eq!(keys.first().map(|item| item.id.as_str()), Some("key-0900"));
    assert_eq!(keys.last().map(|item| item.id.as_str()), Some("key-0000"));

    let quota_limits = storage
        .list_api_key_quota_limits_for_ids(&requested)
        .expect("list quota limits");
    assert_eq!(quota_limits.len(), selected.len());
    assert_eq!(quota_limits.get("key-0000"), Some(&1000));
    assert_eq!(quota_limits.get("key-0900"), Some(&1900));
}
