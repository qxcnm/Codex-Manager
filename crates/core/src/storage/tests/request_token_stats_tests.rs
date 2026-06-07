use rusqlite::params;

use super::{RequestTokenStat, Storage};

/// 函数 `insert_rollup_row`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - storage: 参数 storage
/// - key_id: 参数 key_id
/// - account_id: 参数 account_id
/// - model: 参数 model
/// - total_tokens: 参数 total_tokens
/// - estimated_cost_usd: 参数 estimated_cost_usd
/// - updated_at: 参数 updated_at
///
/// # 返回
/// 无
fn insert_rollup_row(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    reasoning_output_tokens: i64,
    estimated_cost_usd: f64,
    source_rows: i64,
    updated_at: i64,
) {
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_rollups (
                key_id,
                account_id,
                model,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                total_tokens,
                reasoning_output_tokens,
                estimated_cost_usd,
                source_rows,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                key_id,
                account_id,
                model,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                total_tokens,
                reasoning_output_tokens,
                estimated_cost_usd,
                source_rows,
                updated_at,
            ],
        )
        .expect("insert rollup row");
}

/// 函数 `assert_float_close`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - left: 参数 left
/// - right: 参数 right
///
/// # 返回
/// 无
fn assert_float_close(left: f64, right: f64) {
    assert!(
        (left - right).abs() < 1e-9,
        "expected {left} to be close to {right}"
    );
}

/// 函数 `summaries_for_selected_keys_include_rollups_and_respect_time_ranges`
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
fn summaries_for_selected_keys_include_rollups_and_respect_time_ranges() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    // 明细行和 rollup 行分属不同 key，便于验证过滤是否真的落在数据库层。
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(1),
            output_tokens: Some(2),
            total_tokens: Some(12),
            reasoning_output_tokens: Some(3),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
        })
        .expect("insert raw key a");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 2,
            key_id: Some("key-b".to_string()),
            account_id: Some("acc-b".to_string()),
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(20),
            cached_input_tokens: Some(0),
            output_tokens: Some(0),
            total_tokens: Some(20),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.20),
            created_at: 110,
        })
        .expect("insert raw key b");

    // Rollup 只写入 key-a，用来验证无时间范围时会把 rollup 一并纳入。
    insert_rollup_row(
        &storage, "key-a", "acc-a", "gpt-5", 5, 0, 0, 5, 0, 0.05, 1, 999,
    );

    let selected = vec!["key-a".to_string()];
    let by_key = storage
        .summarize_request_token_stats_by_key_for_keys(&selected)
        .expect("summarize by key");
    assert_eq!(by_key.len(), 1);
    assert_eq!(by_key[0].key_id, "key-a");
    assert_eq!(by_key[0].total_tokens, 17);
    assert_float_close(by_key[0].estimated_cost_usd, 0.15);

    let by_model = storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &selected)
        .expect("summarize by model");
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-5");
    assert_eq!(by_model[0].total_tokens, 17);

    let by_key_and_model = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(Some(90), Some(110), &selected)
        .expect("summarize by key and model");
    assert_eq!(by_key_and_model.len(), 1);
    assert_eq!(by_key_and_model[0].key_id, "key-a");
    assert_eq!(by_key_and_model[0].model, "gpt-5");
    assert_eq!(by_key_and_model[0].total_tokens, 12);
    assert_float_close(by_key_and_model[0].estimated_cost_usd, 0.10);
}

#[test]
fn summaries_for_empty_key_lists_return_empty_results() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(0),
            output_tokens: Some(5),
            total_tokens: Some(15),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
        })
        .expect("insert raw key a");

    let empty = Vec::<String>::new();
    assert!(storage
        .summarize_request_token_stats_by_key_for_keys(&empty)
        .expect("summarize by key")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &empty)
        .expect("summarize by model")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_key_and_model_for_keys(None, None, &empty)
        .expect("summarize by key and model")
        .is_empty());
}

/// 函数 `summaries_for_large_key_lists_use_temp_filter`
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
fn summaries_for_large_key_lists_use_temp_filter() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut selected = Vec::new();
    for index in 0..901 {
        let key_id = format!("key-{index:04}");
        selected.push(key_id.clone());
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id: index as i64 + 1,
                key_id: Some(key_id),
                account_id: Some(format!("acc-{index:04}")),
                model: Some("gpt-5".to_string()),
                input_tokens: Some(1),
                cached_input_tokens: Some(0),
                output_tokens: Some(0),
                total_tokens: Some(1),
                reasoning_output_tokens: Some(0),
                estimated_cost_usd: Some(0.01),
                created_at: 1_000 + index as i64,
            })
            .expect("insert request token stat");
    }

    let by_key = storage
        .summarize_request_token_stats_by_key_for_keys(&selected)
        .expect("summarize by key");
    assert_eq!(by_key.len(), selected.len());
    assert_eq!(
        by_key.first().map(|item| item.key_id.as_str()),
        Some("key-0000")
    );
    assert_eq!(
        by_key.last().map(|item| item.key_id.as_str()),
        Some("key-0900")
    );

    let by_model = storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &selected)
        .expect("summarize by model");
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-5");
    assert_eq!(by_model[0].total_tokens, selected.len() as i64);
    assert_float_close(by_model[0].estimated_cost_usd, 9.01);

    let by_key_and_model = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(None, None, &selected)
        .expect("summarize by key and model");
    assert_eq!(by_key_and_model.len(), selected.len());
    assert_eq!(
        by_key_and_model.first().map(|item| item.key_id.as_str()),
        Some("key-0000")
    );
    assert_eq!(
        by_key_and_model.last().map(|item| item.key_id.as_str()),
        Some("key-0900")
    );
}
