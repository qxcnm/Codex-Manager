use codexmanager_core::rpc::types::{
    InitializeResult, JsonRpcError, JsonRpcErrorObject, JsonRpcMessage, JsonRpcRequest,
    JsonRpcResponse,
};
use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;
use serde_json::Value;

use crate::storage_helpers;
use crate::RpcActor;

mod account;
mod account_manager;
mod aggregate_api;
mod apikey;
mod app_settings;
mod codex_profile;
mod codex_skills;
mod dashboard;
mod gateway;
mod model_groups;
mod quota;
mod requestlog;
mod service_config;
mod startup;
mod system;
mod usage;

/// 函数 `response`
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
pub(super) fn response(req: &JsonRpcRequest, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        id: req.id.clone(),
        result,
    }
}

/// 函数 `as_json`
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
pub(super) fn as_json<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// 函数 `str_param`
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
pub(super) fn str_param<'a>(req: &'a JsonRpcRequest, key: &str) -> Option<&'a str> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
}

/// 函数 `string_param`
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
pub(super) fn string_param(req: &JsonRpcRequest, key: &str) -> Option<String> {
    str_param(req, key).map(|v| v.to_string())
}

/// 函数 `i64_param`
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
pub(super) fn i64_param(req: &JsonRpcRequest, key: &str) -> Option<i64> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_i64())
}

/// 函数 `bool_param`
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
pub(super) fn bool_param(req: &JsonRpcRequest, key: &str) -> Option<bool> {
    req.params
        .as_ref()
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_bool())
}

/// 函数 `ok_result`
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
pub(super) fn ok_result() -> Value {
    serde_json::json!({ "ok": true })
}

/// 函数 `ok_or_error`
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
pub(super) fn ok_or_error(result: Result<(), String>) -> Value {
    match result {
        Ok(_) => ok_result(),
        Err(err) => crate::error_codes::rpc_action_error_payload(err),
    }
}

/// 函数 `value_or_error`
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
pub(super) fn value_or_error<T: Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => as_json(value),
        Err(err) => crate::error_codes::rpc_error_payload(err),
    }
}

fn permission_denied(method: &str) -> String {
    format!("permission_denied: {method}")
}

const MEMBER_METHOD_ALLOWLIST: &[&str] = &[
    "account/chatgptAuthTokens/refresh",
    "account/chatgptAuthTokens/refreshAll",
    "account/list",
    "account/proxy/clear",
    "account/proxy/cancel-test",
    "account/proxy/get",
    "account/proxy/latency-test",
    "account/proxy/set",
    "account/proxy/speed-test",
    "account/proxy/cloudflare-speed-test",
    "account/proxy/speed-test-history",
    "account/proxy/latency-test-history",
    "account/proxy/diagnostics-history",
    "account/proxy/test",
    "account/proxy/test-job",
    "account/read",
    "account/update",
    "account/updateSorts",
    "account/usage/aggregate",
    "account/usage/list",
    "account/usage/read",
    "account/usage/resetCredits",
    "account/usage/refresh",
    "account/warmup",
    "accountManager/password/change",
    "accountManager/profile/update",
    "accountManager/session/current",
    "accountManager/status",
    "apikey/create",
    "apikey/delete",
    "apikey/disable",
    "apikey/enable",
    "apikey/list",
    "apikey/managedModelGetV2",
    "apikey/managedModelListV2",
    "apikey/readSecret",
    "apikey/updateModel",
    "apikey/usageStats",
    "appSettings/get",
    "dashboard/memberSummary",
    "requestlog/list",
    "requestlog/list_with_summary",
    "requestlog/summary",
    "requestlog/today_summary",
    "startup/snapshot",
];

fn member_method_allowed(method: &str) -> bool {
    if crate::current_web_auth_mode() == "password" {
        return true;
    }
    MEMBER_METHOD_ALLOWLIST.contains(&method)
}

fn ensure_method_allowed(actor: &RpcActor, method: &str) -> Result<(), String> {
    if actor.is_admin() || member_method_allowed(method) {
        return Ok(());
    }
    Err(permission_denied(method))
}

/// 函数 `handle_request`
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
#[cfg(test)]
pub(crate) fn handle_request(req: JsonRpcRequest) -> JsonRpcMessage {
    handle_request_with_actor(req, RpcActor::system_admin())
}

pub(crate) fn handle_request_with_actor(req: JsonRpcRequest, actor: RpcActor) -> JsonRpcMessage {
    if req.method == "initialize" {
        let _ = storage_helpers::initialize_storage();
        if let Some(storage) = storage_helpers::open_storage() {
            let _ = storage.insert_event(&Event {
                account_id: None,
                event_type: "initialize".to_string(),
                message: "service initialized".to_string(),
                created_at: now_ts(),
            });
        }
        let result = InitializeResult {
            version: codexmanager_core::core_version().to_string(),
            user_agent: crate::gateway::current_codex_user_agent(),
            codex_home: crate::process_env::db_dir().to_string_lossy().to_string(),
            platform_family: std::env::consts::FAMILY.to_string(),
            platform_os: std::env::consts::OS.to_string(),
        };
        return JsonRpcMessage::Response(response(&req, as_json(result)));
    }

    if let Err(err) = ensure_method_allowed(&actor, &req.method) {
        return JsonRpcMessage::Response(response(&req, value_or_error::<()>(Err(err))));
    }

    if let Some(resp) = account::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = account_manager::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = aggregate_api::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = apikey::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = app_settings::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = codex_profile::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = codex_skills::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = dashboard::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = usage::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = service_config::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = system::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = startup::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = gateway::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = model_groups::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = quota::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = crate::plugin::try_handle(&req) {
        return JsonRpcMessage::Response(resp);
    }
    if let Some(resp) = requestlog::try_handle(&req, &actor) {
        return JsonRpcMessage::Response(resp);
    }

    JsonRpcMessage::Error(JsonRpcError {
        id: req.id,
        error: JsonRpcErrorObject {
            code: -32601,
            data: None,
            message: "unknown_method".to_string(),
        },
    })
}
