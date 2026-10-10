#[derive(Debug, Clone)]
pub enum RequestLogQuery {
    All,
    GlobalLike(String),
    AccountLike(String),
    AccountExact(String),
    FieldLike {
        column: &'static str,
        pattern: String,
    },
    FieldExact {
        column: &'static str,
        value: String,
    },
    StatusExact(i64),
    StatusRange(i64, i64),
}

/// 函数 `parse_request_log_query`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub fn parse_request_log_query(query: Option<&str>) -> RequestLogQuery {
    let Some(raw) = query.map(str::trim).filter(|v| !v.is_empty()) else {
        return RequestLogQuery::All;
    };

    // 中文注释：优先解析字段前缀（如 method:/status:），不这样做会把所有搜索都退化为多列 OR LIKE，数据量上来后会明显变慢。
    if let Some(parsed) = parse_prefixed_request_log_query(raw) {
        return parsed;
    }

    RequestLogQuery::GlobalLike(format!("%{}%", raw))
}

/// 函数 `parse_prefixed_request_log_query`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_prefixed_request_log_query(raw: &str) -> Option<RequestLogQuery> {
    let (prefix, value) = raw.split_once(':')?;
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let normalized_value = value.trim();
    if normalized_value.is_empty() {
        return None;
    }
    let (is_exact, needle) = parse_match_mode(normalized_value)?;

    match normalized_prefix.as_str() {
        "account" | "account_id" => Some(parse_account_query(is_exact, needle)),
        "path" | "request_path" => Some(parse_field_query("request_path", is_exact, needle)),
        "original" | "original_path" => Some(parse_field_query("original_path", is_exact, needle)),
        "adapted" | "adapted_path" => Some(parse_field_query("adapted_path", is_exact, needle)),
        "method" => Some(parse_field_query("method", is_exact, needle)),
        "type" | "request_type" => Some(parse_field_query("request_type", is_exact, needle)),
        "route_strategy" | "strategy" => {
            Some(parse_field_query("route_strategy", is_exact, needle))
        }
        "route_source" | "route_reason" => {
            Some(parse_field_query("route_source", is_exact, needle))
        }
        "client_model" | "original_model" => {
            Some(parse_field_query("client_model", is_exact, needle))
        }
        "model" => Some(parse_field_query("model", is_exact, needle)),
        "model_source" => Some(parse_field_query("model_source", is_exact, needle)),
        "upstream_model" | "source_model" => {
            Some(parse_field_query("upstream_model", is_exact, needle))
        }
        "source_kind" | "actual_source_kind" => {
            Some(parse_field_query("actual_source_kind", is_exact, needle))
        }
        "source_id" | "actual_source_id" => {
            Some(parse_field_query("actual_source_id", is_exact, needle))
        }
        "client_reasoning" | "client_reasoning_effort" | "original_reasoning" => Some(
            parse_field_query("client_reasoning_effort", is_exact, needle),
        ),
        "reasoning" | "reason" => Some(parse_field_query("reasoning_effort", is_exact, needle)),
        "reasoning_source" | "reason_source" => {
            Some(parse_field_query("reasoning_source", is_exact, needle))
        }
        "tier" | "service_tier" => Some(parse_field_query("service_tier", is_exact, needle)),
        "effective_tier" | "effective_service_tier" => Some(parse_field_query(
            "effective_service_tier",
            is_exact,
            needle,
        )),
        "tier_source" | "service_tier_source" => {
            Some(parse_field_query("service_tier_source", is_exact, needle))
        }
        "adapter" => Some(parse_field_query("response_adapter", is_exact, needle)),
        "error" => Some(parse_field_query("error", is_exact, needle)),
        "key" | "key_id" => Some(parse_field_query("key_id", is_exact, needle)),
        "trace" | "trace_id" => Some(parse_field_query("trace_id", is_exact, needle)),
        "upstream" | "url" => Some(parse_field_query("upstream_url", is_exact, needle)),
        "status" => parse_status_query(needle),
        _ => None,
    }
}

/// 函数 `parse_match_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_match_mode(raw: &str) -> Option<(bool, &str)> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(exact) = value.strip_prefix('=') {
        let exact = exact.trim();
        if exact.is_empty() {
            return None;
        }
        return Some((true, exact));
    }
    Some((false, value))
}

/// 函数 `parse_field_query`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - column: 参数 column
/// - is_exact: 参数 is_exact
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn parse_field_query(column: &'static str, is_exact: bool, value: &str) -> RequestLogQuery {
    if is_exact {
        return RequestLogQuery::FieldExact {
            column,
            value: value.to_string(),
        };
    }
    RequestLogQuery::FieldLike {
        column,
        pattern: format!("%{}%", value),
    }
}

fn parse_account_query(is_exact: bool, value: &str) -> RequestLogQuery {
    if is_exact {
        return RequestLogQuery::AccountExact(value.to_string());
    }
    RequestLogQuery::AccountLike(format!("%{}%", value))
}

/// 函数 `parse_status_query`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_status_query(raw: &str) -> Option<RequestLogQuery> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.len() == 3 && normalized.ends_with("xx") {
        let digit = normalized.chars().next()?.to_digit(10)? as i64;
        let start = digit * 100;
        return Some(RequestLogQuery::StatusRange(start, start + 99));
    }

    normalized
        .parse::<i64>()
        .ok()
        .map(RequestLogQuery::StatusExact)
}

#[cfg(test)]
#[path = "tests/request_log_query_tests.rs"]
mod tests;
