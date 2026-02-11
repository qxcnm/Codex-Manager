#[derive(Debug, Clone)]
pub(super) enum RequestLogQuery {
    All,
    GlobalLike(String),
    FieldLike { column: &'static str, pattern: String },
    StatusExact(i64),
    StatusRange(i64, i64),
}

pub(super) fn parse_request_log_query(query: Option<&str>) -> RequestLogQuery {
    let Some(raw) = query.map(str::trim).filter(|v| !v.is_empty()) else {
        return RequestLogQuery::All;
    };

    // 中文注释：优先解析字段前缀（如 method:/status:），不这样做会把所有搜索都退化为多列 OR LIKE，数据量上来后会明显变慢。
    if let Some(parsed) = parse_prefixed_request_log_query(raw) {
        return parsed;
    }

    RequestLogQuery::GlobalLike(format!("%{}%", raw))
}

fn parse_prefixed_request_log_query(raw: &str) -> Option<RequestLogQuery> {
    let (prefix, value) = raw.split_once(':')?;
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let normalized_value = value.trim();
    if normalized_value.is_empty() {
        return None;
    }

    match normalized_prefix.as_str() {
        "path" | "request_path" => Some(RequestLogQuery::FieldLike {
            column: "request_path",
            pattern: format!("%{}%", normalized_value),
        }),
        "method" => Some(RequestLogQuery::FieldLike {
            column: "method",
            pattern: format!("%{}%", normalized_value),
        }),
        "model" => Some(RequestLogQuery::FieldLike {
            column: "model",
            pattern: format!("%{}%", normalized_value),
        }),
        "reasoning" | "reason" => Some(RequestLogQuery::FieldLike {
            column: "reasoning_effort",
            pattern: format!("%{}%", normalized_value),
        }),
        "error" => Some(RequestLogQuery::FieldLike {
            column: "error",
            pattern: format!("%{}%", normalized_value),
        }),
        "key" | "key_id" => Some(RequestLogQuery::FieldLike {
            column: "key_id",
            pattern: format!("%{}%", normalized_value),
        }),
        "upstream" | "url" => Some(RequestLogQuery::FieldLike {
            column: "upstream_url",
            pattern: format!("%{}%", normalized_value),
        }),
        "status" => parse_status_query(normalized_value),
        _ => None,
    }
}

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
