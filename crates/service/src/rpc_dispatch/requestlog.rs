use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, RequestLogListParams};

use crate::RpcActor;
use crate::{requestlog_clear, requestlog_list, requestlog_summary, requestlog_today_summary};

fn actor_key_ids(actor: &RpcActor) -> Result<Vec<String>, String> {
    if actor.is_admin() {
        return Ok(Vec::new());
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: requestlog requires user session".to_string())?;
    crate::list_api_key_ids_for_user(user_id)
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
                    let key_ids = actor_key_ids(actor)?;
                    requestlog_list::read_request_log_page_for_key_ids(params, &key_ids)
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
                    let key_ids = actor_key_ids(actor)?;
                    requestlog_summary::read_request_log_filter_summary_for_key_ids(
                        params, &key_ids,
                    )
                }
            }))
        }
        "requestlog/clear" => super::ok_or_error(requestlog_clear::clear_request_logs()),
        "requestlog/today_summary" => {
            let day_start_ts = super::i64_param(req, "dayStartTs");
            let day_end_ts = super::i64_param(req, "dayEndTs");
            super::value_or_error(if actor.is_admin() {
                requestlog_today_summary::read_requestlog_today_summary(day_start_ts, day_end_ts)
            } else {
                actor_key_ids(actor).and_then(|key_ids| {
                    requestlog_today_summary::read_requestlog_today_summary_for_key_ids(
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
