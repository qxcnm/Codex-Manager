/// 函数 `run_gateway_keepalive_once`
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
pub(crate) fn run_gateway_keepalive_once() -> Result<(), String> {
    // Keep the background task local-only after the V2 cutover. It verifies that
    // the catalog remains readable without contacting any upstream `/models` API.
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .list_api_models_v2()
        .map_err(|err| format!("model catalog V2 keepalive failed: {err}"))?;
    Ok(())
}

/// 函数 `is_keepalive_error_ignorable`
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
pub(crate) fn is_keepalive_error_ignorable(err: &str) -> bool {
    let normalized = err.trim().to_ascii_lowercase();
    normalized.contains("no available account") || normalized.contains("storage unavailable")
}

#[cfg(test)]
#[path = "tests/usage_keepalive_tests.rs"]
mod tests;
