use rusqlite::types::Value;

use super::{key_id_filters::KeyIdSqlFilter, request_log_query};

pub(super) struct RequestLogSqlFilters {
    pub(super) where_clause: String,
    pub(super) params: Vec<Value>,
    pub(super) uses_token_stats: bool,
    pub(super) uses_account_lookup: bool,
}

pub(super) fn build_request_log_filters(
    query: Option<&str>,
    status_filter: Option<&str>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    include_account_lookup: bool,
    key_filter: Option<&KeyIdSqlFilter<'_>>,
    include_route_detail_fields: bool,
) -> RequestLogSqlFilters {
    let mut clauses = Vec::new();
    let mut params = Vec::new();

    let query = request_log_query::parse_request_log_query(query);
    let uses_account_lookup = request_log_query_uses_account_lookup(&query, include_account_lookup);
    let uses_token_stats = append_request_log_query_clause(
        query,
        uses_account_lookup,
        include_route_detail_fields,
        &mut clauses,
        &mut params,
    );
    append_status_filter_clause(status_filter, &mut clauses, &mut params);
    append_time_range_clause(start_ts, end_ts, &mut clauses, &mut params);
    append_key_filter_clause(key_filter, &mut clauses, &mut params);

    RequestLogSqlFilters {
        where_clause: if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        },
        params,
        uses_token_stats,
        uses_account_lookup,
    }
}

pub(super) fn account_join_clause(include_account_lookup: bool) -> &'static str {
    if include_account_lookup {
        "LEFT JOIN accounts a ON a.id = r.account_id"
    } else {
        ""
    }
}

pub(super) fn token_stats_join_clause(include_token_stats: bool) -> &'static str {
    if include_token_stats {
        "LEFT JOIN request_token_stats t ON t.request_log_id = r.id"
    } else {
        ""
    }
}

fn append_key_filter_clause(
    key_filter: Option<&KeyIdSqlFilter<'_>>,
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    let Some(key_filter) = key_filter else {
        return;
    };
    clauses.push(key_filter.condition().to_string());
    params.extend_from_slice(key_filter.params());
}

fn request_log_query_uses_account_lookup(
    query: &request_log_query::RequestLogQuery,
    include_account_lookup: bool,
) -> bool {
    include_account_lookup
        && matches!(
            query,
            request_log_query::RequestLogQuery::AccountLike(_)
                | request_log_query::RequestLogQuery::AccountExact(_)
                | request_log_query::RequestLogQuery::GlobalLike(_)
        )
}

fn is_route_detail_query_column(column: &str) -> bool {
    matches!(
        column,
        "upstream_model" | "actual_source_kind" | "actual_source_id"
    )
}

fn append_time_range_clause(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    if let Some(start_ts) = start_ts {
        clauses.push("r.created_at >= ?".to_string());
        params.push(Value::Integer(start_ts));
    }
    if let Some(end_ts) = end_ts {
        clauses.push("r.created_at < ?".to_string());
        params.push(Value::Integer(end_ts));
    }
}

fn append_request_log_query_clause(
    query: request_log_query::RequestLogQuery,
    include_account_lookup: bool,
    include_route_detail_fields: bool,
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
) -> bool {
    match query {
        request_log_query::RequestLogQuery::All => false,
        request_log_query::RequestLogQuery::AccountLike(pattern) => {
            append_account_query_clause(pattern, false, include_account_lookup, clauses, params);
            false
        }
        request_log_query::RequestLogQuery::AccountExact(value) => {
            append_account_query_clause(value, true, include_account_lookup, clauses, params);
            false
        }
        request_log_query::RequestLogQuery::FieldLike { column, pattern } => {
            if is_route_detail_query_column(column) && !include_route_detail_fields {
                clauses.push("1 = 0".to_string());
                return false;
            }
            clauses.push(format!("IFNULL(r.{column}, '') LIKE ?"));
            params.push(Value::Text(pattern));
            false
        }
        request_log_query::RequestLogQuery::FieldExact { column, value } => {
            if is_route_detail_query_column(column) && !include_route_detail_fields {
                clauses.push("1 = 0".to_string());
                return false;
            }
            clauses.push(format!("r.{column} = ?"));
            params.push(Value::Text(value));
            false
        }
        request_log_query::RequestLogQuery::StatusExact(status) => {
            clauses.push("r.status_code = ?".to_string());
            params.push(Value::Integer(status));
            false
        }
        request_log_query::RequestLogQuery::StatusRange(start, end) => {
            clauses.push("r.status_code >= ? AND r.status_code <= ?".to_string());
            params.push(Value::Integer(start));
            params.push(Value::Integer(end));
            false
        }
        request_log_query::RequestLogQuery::GlobalLike(pattern) => {
            let mut global_fields = vec![
                "r.request_path LIKE ?",
                "IFNULL(r.initial_account_id,'') LIKE ?",
                "IFNULL(r.attempted_account_ids_json,'') LIKE ?",
                "IFNULL(r.initial_aggregate_api_id,'') LIKE ?",
                "IFNULL(r.attempted_aggregate_api_ids_json,'') LIKE ?",
                "IFNULL(r.aggregate_api_supplier_name,'') LIKE ?",
                "IFNULL(r.aggregate_api_url,'') LIKE ?",
                "IFNULL(r.original_path,'') LIKE ?",
                "IFNULL(r.adapted_path,'') LIKE ?",
                "r.method LIKE ?",
                "IFNULL(r.request_type,'') LIKE ?",
                "IFNULL(r.route_strategy,'') LIKE ?",
                "IFNULL(r.route_source,'') LIKE ?",
                "IFNULL(r.account_id,'') LIKE ?",
                "IFNULL(r.client_model,'') LIKE ?",
                "IFNULL(r.model,'') LIKE ?",
                "IFNULL(r.model_source,'') LIKE ?",
                "IFNULL(r.client_reasoning_effort,'') LIKE ?",
                "IFNULL(r.reasoning_effort,'') LIKE ?",
                "IFNULL(r.reasoning_source,'') LIKE ?",
                "IFNULL(r.service_tier,'') LIKE ?",
                "IFNULL(r.effective_service_tier,'') LIKE ?",
                "IFNULL(r.service_tier_source,'') LIKE ?",
                "IFNULL(r.response_adapter,'') LIKE ?",
                "IFNULL(r.error,'') LIKE ?",
                "IFNULL(r.key_id,'') LIKE ?",
                "IFNULL(r.trace_id,'') LIKE ?",
                "IFNULL(r.upstream_url,'') LIKE ?",
                "IFNULL(CAST(r.status_code AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.input_tokens AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.cached_input_tokens AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.output_tokens AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.total_tokens AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.reasoning_output_tokens AS TEXT),'') LIKE ?",
                "IFNULL(CAST(t.estimated_cost_usd AS TEXT),'') LIKE ?",
            ];
            if include_route_detail_fields {
                global_fields.extend([
                    "IFNULL(r.upstream_model,'') LIKE ?",
                    "IFNULL(r.actual_source_kind,'') LIKE ?",
                    "IFNULL(r.actual_source_id,'') LIKE ?",
                ]);
            }
            if include_account_lookup {
                global_fields.extend([
                    "IFNULL(a.label,'') LIKE ?",
                    "IFNULL(a.chatgpt_account_id,'') LIKE ?",
                    "IFNULL(a.workspace_id,'') LIKE ?",
                ]);
            }
            clauses.push(format!(
                "({})",
                global_fields.join("\n                    OR ")
            ));
            for _ in 0..global_fields.len() {
                params.push(Value::Text(pattern.clone()));
            }
            true
        }
    }
}

fn append_account_query_clause(
    value: String,
    is_exact: bool,
    include_account_lookup: bool,
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    if include_account_lookup {
        let comparator = if is_exact { "=" } else { "LIKE" };
        clauses.push(format!(
            "(IFNULL(r.account_id, '') {comparator} ?
                OR IFNULL(a.label, '') {comparator} ?
                OR IFNULL(a.chatgpt_account_id, '') {comparator} ?
                OR IFNULL(a.workspace_id, '') {comparator} ?)"
        ));
        for _ in 0..4 {
            params.push(Value::Text(value.clone()));
        }
        return;
    }

    let comparator = if is_exact { "=" } else { "LIKE" };
    clauses.push(format!("IFNULL(r.account_id, '') {comparator} ?"));
    params.push(Value::Text(value));
}

fn append_status_filter_clause(
    status_filter: Option<&str>,
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    let normalized = status_filter
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "" | "all" => {}
        "2xx" => {
            clauses.push("r.status_code >= ? AND r.status_code <= ?".to_string());
            params.push(Value::Integer(200));
            params.push(Value::Integer(299));
        }
        "4xx" => {
            clauses.push("r.status_code >= ? AND r.status_code <= ?".to_string());
            params.push(Value::Integer(400));
            params.push(Value::Integer(499));
        }
        "5xx" => {
            clauses.push("r.status_code >= ?".to_string());
            params.push(Value::Integer(500));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_log_filter_builder_marks_token_stats_usage_only_when_needed() {
        let exact_filters = build_request_log_filters(
            Some("model:=gpt-5"),
            Some("2xx"),
            Some(1000),
            Some(2000),
            false,
            None,
            true,
        );
        assert!(!exact_filters.uses_token_stats);
        assert!(!exact_filters.uses_account_lookup);

        let global_filters =
            build_request_log_filters(Some("42"), None, None, None, true, None, true);
        assert!(global_filters.uses_token_stats);
        assert!(global_filters.uses_account_lookup);

        let account_filters =
            build_request_log_filters(Some("account:team"), None, None, None, true, None, true);
        assert!(!account_filters.uses_token_stats);
        assert!(account_filters.uses_account_lookup);

        let account_without_table_filters =
            build_request_log_filters(Some("account:team"), None, None, None, false, None, true);
        assert!(!account_without_table_filters.uses_token_stats);
        assert!(!account_without_table_filters.uses_account_lookup);
    }
}
