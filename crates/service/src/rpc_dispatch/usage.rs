use codexmanager_core::rpc::types::{
    JsonRpcRequest, JsonRpcResponse, UsageListResult, UsageReadResult,
};

use crate::{usage_aggregate, usage_list, usage_read, usage_refresh};

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
pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/usage/read" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            super::as_json(UsageReadResult {
                snapshot: usage_read::read_usage_snapshot(account_id),
            })
        }
        "account/usage/list" => {
            let limit = super::i64_param(req, "limit").map(|value| value.max(0) as usize);
            super::value_or_error(
                usage_list::read_usage_snapshots_limited(limit)
                    .map(|items| UsageListResult { items }),
            )
        }
        "account/usage/aggregate" => {
            super::value_or_error(usage_aggregate::read_usage_aggregate_summary())
        }
        "account/usage/refresh" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            let result = match account_id {
                Some(account_id) => usage_refresh::refresh_usage_for_account_result(account_id),
                None => usage_refresh::refresh_usage_for_all_accounts_result(),
            };
            super::value_or_error(result)
        }
        "account/usage/resetCredits" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            super::value_or_error(
                account_id
                    .ok_or_else(|| "accountId is required".to_string())
                    .and_then(crate::usage_reset_credits::read_reset_credits),
            )
        }
        "account/usage/resetCredit/consume" => {
            let account_id =
                super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
            let operation_id = super::str_param(req, "operationId")
                .or_else(|| super::str_param(req, "operation_id"));
            super::value_or_error(match (account_id, operation_id) {
                (None, _) => Err("accountId is required".to_string()),
                (_, None) => Err("operationId is required".to_string()),
                (Some(account_id), Some(operation_id)) => {
                    crate::usage_reset_credits::consume_reset_credit(account_id, operation_id)
                }
            })
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
