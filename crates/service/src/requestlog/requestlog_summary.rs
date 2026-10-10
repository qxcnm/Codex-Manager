use codexmanager_core::rpc::types::{RequestLogFilterSummaryResult, RequestLogListParams};
use codexmanager_core::storage::{RequestLogQuerySummary, Storage};

use crate::storage_helpers::open_storage;

use super::list::NormalizedRequestLogParams;

/// 函数 `read_request_log_filter_summary`
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
pub(crate) fn read_request_log_filter_summary(
    params: RequestLogListParams,
) -> Result<RequestLogFilterSummaryResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_request_log_filter_summary_with_storage(&storage, params)
}

pub(crate) fn read_request_log_filter_summary_with_storage(
    storage: &Storage,
    params: RequestLogListParams,
) -> Result<RequestLogFilterSummaryResult, String> {
    let params = NormalizedRequestLogParams::from_params(params);
    if crate::storage_helpers::seaorm_enabled() {
        let filter = super::seaorm::filter(&params, None);
        let filtered = super::seaorm::summary(filter.clone())?;
        let total = if params.status_filter.is_some() {
            let mut all = filter;
            all.status = None;
            super::seaorm::count(all)?
        } else {
            filtered.count
        };
        return Ok(map_filter_summary(total, filtered));
    }
    let filtered = storage
        .summarize_request_logs_filtered(
            params.query.as_deref(),
            params.status_filter.as_deref(),
            params.start_ts,
            params.end_ts,
        )
        .map_err(|err| format!("summarize request logs failed: {err}"))?;
    let total_count = match needs_unfiltered_total_count(params.status_filter.as_deref()) {
        true => storage
            .count_request_logs(
                params.query.as_deref(),
                None,
                params.start_ts,
                params.end_ts,
            )
            .map_err(|err| format!("count request logs failed: {err}"))?,
        false => filtered.count,
    };

    Ok(map_filter_summary(total_count, filtered))
}

pub(crate) fn read_request_log_filter_summary_for_key_ids_with_storage(
    storage: &Storage,
    params: RequestLogListParams,
    key_ids: &[String],
) -> Result<RequestLogFilterSummaryResult, String> {
    let params = NormalizedRequestLogParams::from_params(params);
    if crate::storage_helpers::seaorm_enabled() {
        let filter = super::seaorm::filter(&params, Some(key_ids));
        let filtered = super::seaorm::summary(filter.clone())?;
        let total = if params.status_filter.is_some() {
            let mut all = filter;
            all.status = None;
            super::seaorm::count(all)?
        } else {
            filtered.count
        };
        return Ok(map_filter_summary(total, filtered));
    }
    if key_ids.is_empty() {
        return Ok(RequestLogFilterSummaryResult::default());
    }
    let filtered = storage
        .summarize_request_logs_filtered_for_keys(
            params.query.as_deref(),
            params.status_filter.as_deref(),
            params.start_ts,
            params.end_ts,
            key_ids,
        )
        .map_err(|err| format!("summarize request logs failed: {err}"))?;
    let total_count = match needs_unfiltered_total_count(params.status_filter.as_deref()) {
        true => storage
            .count_request_logs_for_keys(
                params.query.as_deref(),
                None,
                params.start_ts,
                params.end_ts,
                key_ids,
            )
            .map_err(|err| format!("count request logs failed: {err}"))?,
        false => filtered.count,
    };

    Ok(map_filter_summary(total_count, filtered))
}

fn needs_unfiltered_total_count(status_filter: Option<&str>) -> bool {
    status_filter.is_some()
}

fn map_filter_summary(
    total_count: i64,
    filtered: RequestLogQuerySummary,
) -> RequestLogFilterSummaryResult {
    RequestLogFilterSummaryResult {
        total_count: total_count.max(0),
        filtered_count: filtered.count.max(0),
        success_count: filtered.success_count.max(0),
        error_count: filtered.error_count.max(0),
        total_tokens: filtered.total_tokens.max(0),
        total_cost_usd: filtered.estimated_cost_usd.max(0.0),
    }
}

#[cfg(test)]
#[path = "requestlog_summary_tests.rs"]
mod tests;
