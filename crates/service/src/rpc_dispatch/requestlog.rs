use codexmanager_core::rpc::types::{
    JsonRpcRequest, JsonRpcResponse, RequestLogDetailParams, RequestLogListParams,
};
use codexmanager_core::storage::Storage;

use crate::storage_helpers::StorageHandle;
use crate::RpcActor;
use crate::{requestlog_clear, requestlog_list, requestlog_summary, requestlog_today_summary};

fn actor_key_ids_with_storage(storage: &Storage, actor: &RpcActor) -> Result<Vec<String>, String> {
    if actor.is_admin() {
        return Ok(Vec::new());
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: requestlog requires user session".to_string())?;
    crate::dashboard_storage::DashboardStorage::new(storage)
        .list_api_key_ids_for_user(user_id)
        .map_err(|err| format!("list api key ids for user failed: {err}"))
}

fn member_requestlog_scope(actor: &RpcActor) -> Result<(StorageHandle, Vec<String>), String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let key_ids = actor_key_ids_with_storage(&storage, actor)?;
    Ok((storage, key_ids))
}

/// 函数 `try_handle`
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
pub(super) fn try_handle(req: &JsonRpcRequest, actor: &RpcActor) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "requestlog/list_with_summary" => {
            let params = req
                .params
                .clone()
                .map(serde_json::from_value::<RequestLogListParams>)
                .transpose()
                .map(|params| params.unwrap_or_default())
                .map(RequestLogListParams::normalized)
                .map_err(|err| format!("invalid requestlog/list_with_summary params: {err}"));
            super::value_or_error(params.and_then(|params| {
                if actor.is_admin() {
                    let storage = crate::storage_helpers::open_storage()
                        .ok_or_else(|| "open storage failed".to_string())?;
                    let summary =
                        requestlog_summary::read_request_log_filter_summary_with_storage(
                            &storage,
                            params.clone(),
                        )?;
                    let page = if requestlog_list::request_log_page_total_matches_filter_summary(
                        &params,
                    ) {
                        requestlog_list::read_request_log_page_with_total(
                            &storage,
                            params,
                            summary.filtered_count,
                        )?
                    } else {
                        requestlog_list::read_request_log_page_with_storage(&storage, params)?
                    };
                    Ok(requestlog_list::request_log_list_with_summary_result(
                        page, summary,
                    ))
                } else {
                    let (storage, key_ids) = member_requestlog_scope(actor)?;
                    let summary =
                        requestlog_summary::read_request_log_filter_summary_for_key_ids_with_storage(
                            &storage,
                            params.clone(),
                            &key_ids,
                        )?;
                    let page = if requestlog_list::request_log_page_total_matches_filter_summary(
                        &params,
                    ) {
                        requestlog_list::read_request_log_page_for_key_ids_with_total(
                            &storage,
                            params,
                            &key_ids,
                            summary.filtered_count,
                        )?
                    } else {
                        requestlog_list::read_request_log_page_for_key_ids_with_storage(
                            &storage, params, &key_ids,
                        )?
                    };
                    Ok(requestlog_list::request_log_list_with_summary_result(
                        page, summary,
                    ))
                }
            }))
        }
        "requestlog/list" => {
            let params = req
                .params
                .clone()
                .map(serde_json::from_value::<RequestLogListParams>)
                .transpose()
                .map(|params| params.unwrap_or_default())
                .map(RequestLogListParams::normalized)
                .map_err(|err| format!("invalid requestlog/list params: {err}"));
            super::value_or_error(params.and_then(|params| {
                if actor.is_admin() {
                    requestlog_list::read_request_log_page(params)
                } else {
                    let (storage, key_ids) = member_requestlog_scope(actor)?;
                    requestlog_list::read_request_log_page_for_key_ids_with_storage(
                        &storage, params, &key_ids,
                    )
                }
            }))
        }
        "requestlog/detail" => {
            let params = req
                .params
                .clone()
                .map(serde_json::from_value::<RequestLogDetailParams>)
                .transpose()
                .map(Option::unwrap_or_default)
                .map_err(|err| format!("invalid requestlog/detail params: {err}"));
            super::value_or_error(params.and_then(|params| {
                if actor.is_admin() {
                    let storage = crate::storage_helpers::open_storage()
                        .ok_or_else(|| "open storage failed".to_string())?;
                    crate::requestlog::detail::read_request_log_detail(&storage, &params, None)
                } else {
                    let (storage, key_ids) = member_requestlog_scope(actor)?;
                    crate::requestlog::detail::read_request_log_detail(
                        &storage,
                        &params,
                        Some(key_ids.as_slice()),
                    )
                }
            }))
        }
        "requestlog/summary" => {
            let params = req
                .params
                .clone()
                .map(serde_json::from_value::<RequestLogListParams>)
                .transpose()
                .map(|params| params.unwrap_or_default())
                .map(RequestLogListParams::normalized)
                .map_err(|err| format!("invalid requestlog/summary params: {err}"));
            super::value_or_error(params.and_then(|params| {
                if actor.is_admin() {
                    requestlog_summary::read_request_log_filter_summary(params)
                } else {
                    let (storage, key_ids) = member_requestlog_scope(actor)?;
                    requestlog_summary::read_request_log_filter_summary_for_key_ids_with_storage(
                        &storage, params, &key_ids,
                    )
                }
            }))
        }
        // Service-mode HTTP dispatch handles this write through the injected
        // DomainStorage contract. Keep the synchronous path for desktop and
        // compatibility callers that do not carry AppState.
        "requestlog/clear" => super::ok_or_error(requestlog_clear::clear_request_logs()),
        "requestlog/today_summary" => {
            let day_start_ts = super::i64_param(req, "dayStartTs");
            let day_end_ts = super::i64_param(req, "dayEndTs");
            super::value_or_error(if actor.is_admin() {
                requestlog_today_summary::read_requestlog_today_summary(day_start_ts, day_end_ts)
            } else {
                member_requestlog_scope(actor).and_then(|(storage, key_ids)| {
                    requestlog_today_summary::read_requestlog_today_summary_for_key_ids_with_storage(
                        &storage,
                        day_start_ts,
                        day_end_ts,
                        &key_ids,
                    )
                })
            })
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
