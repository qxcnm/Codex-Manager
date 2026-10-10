use super::{collect_free_account_max_model_options, normalize_market_mode};

/// 函数 `free_account_max_model_options_fallback_to_curated_defaults`
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
fn free_account_max_model_options_fallback_to_curated_defaults() {
    let actual = collect_free_account_max_model_options("auto", &[]);
    assert_eq!(
        actual,
        [
            "auto",
            "gpt-6-astra",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );
}

/// 函数 `free_account_max_model_options_reuse_cached_model_picker_options`
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
fn free_account_max_model_options_reuse_cached_model_picker_options() {
    let actual = collect_free_account_max_model_options(
        "gpt-6-luna",
        &[
            "gpt-5".to_string(),
            "gpt-5.1-codex".to_string(),
            "gpt-5.4-pro".to_string(),
            "gpt-5.4".to_string(),
            "gpt-6-sol".to_string(),
            "o3".to_string(),
            "gpt-6-sol".to_string(),
        ],
    );

    assert_eq!(
        actual,
        vec![
            "auto".to_string(),
            "gpt-6-sol".to_string(),
            "gpt-6-luna".to_string()
        ]
    );
}

/// 函数 `plugin_market_mode_normalization_defaults_to_builtin`
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
fn plugin_market_mode_normalization_defaults_to_builtin() {
    assert_eq!(normalize_market_mode(""), "builtin");
    assert_eq!(normalize_market_mode("private"), "private");
    assert_eq!(normalize_market_mode("custom"), "custom");
    assert_eq!(normalize_market_mode("unknown"), "builtin");
}
