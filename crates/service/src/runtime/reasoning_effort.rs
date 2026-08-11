/// 函数 `normalize_reasoning_effort`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn normalize_reasoning_effort(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some("none"),
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        "ultra" => Some("ultra"),
        // 兼容历史写法；统一改写为官方使用的 xhigh，避免不同拼写在上游行为不一致。
        "extra_high" => Some("xhigh"),
        _ => None,
    }
}

/// 函数 `normalize_reasoning_effort_owned`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn normalize_reasoning_effort_owned(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(normalize_reasoning_effort)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{normalize_reasoning_effort, normalize_reasoning_effort_owned};

    #[test]
    fn accepts_all_official_codex_reasoning_efforts() {
        for effort in [
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ] {
            assert_eq!(normalize_reasoning_effort(effort), Some(effort));
        }
    }

    #[test]
    fn normalizes_case_whitespace_and_legacy_extra_high() {
        assert_eq!(normalize_reasoning_effort(" MAX "), Some("max"));
        assert_eq!(normalize_reasoning_effort("Ultra"), Some("ultra"));
        assert_eq!(
            normalize_reasoning_effort_owned(Some("extra_high".to_string())).as_deref(),
            Some("xhigh")
        );
        assert_eq!(normalize_reasoning_effort("unsupported"), None);
    }
}
