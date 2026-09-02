use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

use crate::RpcActor;
use crate::{
    account_cleanup, account_delete, account_delete_many, account_export, account_import,
    account_list, account_proxy, account_test, account_update, account_warmup, auth_account,
    auth_login, auth_tokens,
};

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
        "account/list" => super::value_or_error(account_list::read_accounts()),
        "account/delete" => {
            let account_id = super::str_param(req, "accountId").unwrap_or("");
            super::ok_or_error(account_delete::delete_account(account_id))
        }
        "account/deleteMany" => {
            let account_ids = req
                .params
                .as_ref()
                .and_then(|params| params.get("accountIds"))
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            super::value_or_error(account_delete_many::delete_accounts(account_ids))
        }
        "account/deleteUnavailableFree" => {
            super::value_or_error(account_cleanup::delete_unavailable_free_accounts())
        }
        "account/deleteByStatuses" => {
            let statuses = req
                .params
                .as_ref()
                .and_then(|params| params.get("statuses"))
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            super::value_or_error(account_cleanup::delete_accounts_by_statuses(statuses))
        }
        "account/update" => {
            let account_id = super::str_param(req, "accountId").unwrap_or("");
            let sort = super::i64_param(req, "sort");
            let preferred = super::bool_param(req, "preferred");
            let status = super::string_param(req, "status");
            let label = super::string_param(req, "label");
            let has_group_name = req
                .params
                .as_ref()
                .and_then(serde_json::Value::as_object)
                .is_some_and(|params| params.contains_key("groupName"));
            let group_name = super::string_param(req, "groupName");
            let note = super::string_param(req, "note");
            let tags = super::string_param(req, "tags");
            let quota_capacity_primary_window_tokens =
                super::i64_param(req, "quotaCapacityPrimaryWindowTokens");
            let quota_capacity_secondary_window_tokens =
                super::i64_param(req, "quotaCapacitySecondaryWindowTokens");
            if has_group_name && !actor.is_admin() {
                super::ok_or_error(Err(super::permission_denied("account/update groupName")))
            } else {
                super::ok_or_error(account_update::update_account(
                    account_id,
                    sort,
                    preferred,
                    status.as_deref(),
                    label.as_deref(),
                    group_name.as_deref(),
                    has_group_name,
                    note.as_deref(),
                    tags.as_deref(),
                    quota_capacity_primary_window_tokens,
                    quota_capacity_secondary_window_tokens,
                ))
            }
        }
        "account/updateSorts" => super::value_or_error(
            account_sort_updates_param(req).and_then(account_update::update_account_sorts),
        ),
        "account/warmup" => {
            let account_ids = req
                .params
                .as_ref()
                .and_then(|params| {
                    params
                        .get("accountIds")
                        .or_else(|| params.get("account_ids"))
                })
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let message = first_string_param(req, &["message"]).unwrap_or_default();
            super::value_or_error(account_warmup::warmup_accounts(account_ids, &message))
        }
        "account/test" => {
            if !actor.is_admin() {
                super::value_or_error::<account_test::AccountTestStartResult>(Err(
                    super::permission_denied("account/test"),
                ))
            } else {
                let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
                let model =
                    first_str_param(req, &["model", "modelSlug", "model_slug"]).map(str::to_string);
                let prompt = first_str_param(req, &["prompt", "message"]).map(str::to_string);
                let kind =
                    first_str_param(req, &["kind", "testType", "test_type"]).map(str::to_string);
                let test_id = first_str_param(req, &["testId", "test_id"]).map(str::to_string);
                super::value_or_error(account_test::start_account_test(
                    account_id, model, prompt, kind, test_id,
                ))
            }
        }
        "account/test/cancel" => {
            if !actor.is_admin() {
                super::value_or_error::<bool>(Err(super::permission_denied("account/test/cancel")))
            } else {
                let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
                let test_id = first_str_param(req, &["testId", "test_id"]).unwrap_or("");
                super::value_or_error(account_test::cancel_account_test(account_id, test_id))
            }
        }
        "account/proxy/get" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::get_account_proxy_settings(account_id))
        }
        "account/proxy/set" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            let enabled = super::bool_param(req, "enabled").unwrap_or(false);
            let source = first_str_param(req, &["source", "proxySource", "proxy_source"]);
            let proxy_profile_id = first_str_param(req, &["proxyProfileId", "proxy_profile_id"]);
            let proxy_url = first_str_param(req, &["proxyUrl", "proxy_url"]);

            let status = super::str_param(req, "status");
            let latency_ms =
                super::i64_param(req, "latencyMs").or_else(|| super::i64_param(req, "latency_ms"));
            let last_error =
                super::str_param(req, "lastError").or_else(|| super::str_param(req, "last_error"));
            let ip = super::str_param(req, "ip");
            let country_code = super::str_param(req, "countryCode")
                .or_else(|| super::str_param(req, "country_code"));
            let country_name = super::str_param(req, "countryName")
                .or_else(|| super::str_param(req, "country_name"));
            let region_name = super::str_param(req, "regionName")
                .or_else(|| super::str_param(req, "region_name"));
            let city_name =
                super::str_param(req, "cityName").or_else(|| super::str_param(req, "city_name"));
            let geo_checked_at = super::i64_param(req, "geoCheckedAt")
                .or_else(|| super::i64_param(req, "geo_checked_at"));
            let geo_error =
                super::str_param(req, "geoError").or_else(|| super::str_param(req, "geo_error"));

            super::value_or_error(account_proxy::set_account_proxy_settings(
                account_id,
                enabled,
                source,
                proxy_profile_id,
                proxy_url,
                status,
                latency_ms,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
            ))
        }
        "account/proxy/clear" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::clear_account_proxy_settings(account_id))
        }
        "account/proxy/test" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            let enabled = super::bool_param(req, "enabled");
            let source = first_str_param(req, &["source", "proxySource", "proxy_source"]);
            let proxy_profile_id = first_str_param(req, &["proxyProfileId", "proxy_profile_id"]);
            let proxy_url = first_str_param(req, &["proxyUrl", "proxy_url"]);
            super::value_or_error(account_proxy::test_account_proxy_settings(
                account_id,
                enabled,
                source,
                proxy_profile_id,
                proxy_url,
            ))
        }
        "account/proxy/latency-test" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::test_account_proxy_latency(account_id))
        }
        "account/proxy/speed-test" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::test_account_proxy_speed(
                account_id,
                first_str_param(req, &["providerId", "provider_id"]),
                first_str_param(req, &["fileSizeId", "file_size_id"]),
                first_str_param(req, &["diagnosticProviderId", "diagnostic_provider_id"]),
                first_str_param(req, &["diagnosticFileSizeId", "diagnostic_file_size_id"]),
            ))
        }
        "account/proxy/cloudflare-speed-test" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            let config = req
                .params
                .as_ref()
                .and_then(|p| p.get("config"))
                .cloned()
                .map(
                    serde_json::from_value::<
                        crate::account::proxy_testing::cloudflare_style::config::CfStyleConfig,
                    >,
                )
                .transpose()
                .map_err(|err| format!("invalid config parameter: {err}"));

            match config {
                Ok(conf) => {
                    super::value_or_error(account_proxy::test_account_proxy_cloudflare_style_speed(
                        account_id,
                        conf.unwrap_or_default(),
                    ))
                }
                Err(err) => super::value_or_error(Err::<serde_json::Value, String>(err)),
            }
        }

        "account/proxy/test-job" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            let job_id = first_str_param(req, &["jobId", "job_id", "id"]).unwrap_or("");
            super::value_or_error(account_proxy::get_account_proxy_test_job(
                account_id, job_id,
            ))
        }
        "account/proxy/cancel-test" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            let job_id = first_str_param(req, &["jobId", "job_id", "id"]).unwrap_or("");
            super::ok_or_error(account_proxy::cancel_account_proxy_test_job(
                account_id, job_id,
            ))
        }
        "account/proxy/speed-test-history" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::get_account_proxy_speed_test_history(
                account_id,
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "account/proxy/latency-test-history" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::get_account_proxy_latency_test_history(
                account_id,
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "account/proxy/diagnostics-history" => {
            let account_id = first_str_param(req, &["accountId", "account_id"]).unwrap_or("");
            super::value_or_error(account_proxy::get_account_proxy_diagnostics_history(
                account_id,
                super::i64_param(req, "limit").map(|v| v as usize),
            ))
        }
        "account/import" => {
            let mut contents = req
                .params
                .as_ref()
                .and_then(|params| params.get("contents"))
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Some(content) = super::string_param(req, "content") {
                if !content.trim().is_empty() {
                    contents.push(content);
                }
            }
            super::value_or_error(account_import::import_account_auth_json(contents))
        }
        "account/export" => {
            let output_dir = super::str_param(req, "outputDir").unwrap_or("");
            let selected_account_ids = req
                .params
                .as_ref()
                .and_then(|params| {
                    params
                        .get("selectedAccountIds")
                        .or_else(|| params.get("selected_account_ids"))
                })
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let export_mode = first_string_param(req, &["exportMode", "export_mode"]);
            super::value_or_error(account_export::export_accounts_to_directory(
                output_dir,
                &selected_account_ids,
                export_mode.as_deref(),
            ))
        }
        "account/exportData" => {
            let selected_account_ids = req
                .params
                .as_ref()
                .and_then(|params| {
                    params
                        .get("selectedAccountIds")
                        .or_else(|| params.get("selected_account_ids"))
                })
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(|item| item.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let export_mode = first_string_param(req, &["exportMode", "export_mode"]);
            super::value_or_error(account_export::export_accounts_data(
                &selected_account_ids,
                export_mode.as_deref(),
            ))
        }
        "account/login/start" => {
            let login_type = super::str_param(req, "type").unwrap_or("chatgpt");
            if login_type.eq_ignore_ascii_case("chatgptAuthTokens") {
                let params = auth_account::ChatgptAuthTokensLoginInput {
                    access_token: first_string_param(req, &["accessToken", "access_token"])
                        .unwrap_or_default(),
                    refresh_token: first_string_param(req, &["refreshToken", "refresh_token"]),
                    id_token: first_string_param(req, &["idToken", "id_token"]),
                    chatgpt_account_id: first_string_param(
                        req,
                        &["chatgptAccountId", "chatgpt_account_id", "accountId"],
                    ),
                    workspace_id: first_string_param(req, &["workspaceId", "workspace_id"]),
                    chatgpt_plan_type: first_string_param(
                        req,
                        &["chatgptPlanType", "chatgpt_plan_type", "planType"],
                    ),
                };
                super::value_or_error(auth_account::login_with_chatgpt_auth_tokens(params))
            } else {
                let open_browser = super::bool_param(req, "openBrowser").unwrap_or(true);
                let note = super::string_param(req, "note");
                let tags = super::string_param(req, "tags");
                let group_name = super::string_param(req, "groupName");
                let workspace_id = super::string_param(req, "workspaceId").and_then(|v| {
                    if v.trim().is_empty() {
                        None
                    } else {
                        Some(v)
                    }
                });
                super::value_or_error(auth_login::login_start(
                    login_type,
                    open_browser,
                    note,
                    tags,
                    group_name,
                    workspace_id,
                ))
            }
        }
        "account/login/status" => {
            let login_id = super::str_param(req, "loginId").unwrap_or("");
            super::as_json(auth_login::login_status(login_id))
        }
        "account/login/cancel" => {
            let login_id = super::str_param(req, "loginId").unwrap_or("");
            super::value_or_error(auth_login::login_cancel(login_id))
        }
        "account/login/complete" => {
            let state = super::str_param(req, "state").unwrap_or("");
            let code = super::str_param(req, "code").unwrap_or("");
            let redirect_uri = super::str_param(req, "redirectUri");
            if state.is_empty() || code.is_empty() {
                serde_json::json!({"ok": false, "error": "missing code/state"})
            } else {
                super::ok_or_error(auth_tokens::complete_login_with_redirect(
                    state,
                    code,
                    redirect_uri,
                ))
            }
        }
        "account/chatgptAuthTokens/refresh" => {
            let target_account_id = first_str_param(req, &["accountId", "account_id"])
                .or_else(|| first_str_param(req, &["previousAccountId", "previous_account_id"]));
            super::value_or_error(auth_account::refresh_current_chatgpt_auth_tokens(
                target_account_id,
            ))
        }
        "account/chatgptAuthTokens/refreshAll" => {
            super::value_or_error(auth_account::refresh_all_chatgpt_auth_tokens())
        }
        "account/read" => {
            let refresh_token =
                first_bool_param(req, &["refreshToken", "refresh_token"]).unwrap_or(false);
            super::value_or_error(auth_account::read_current_account(refresh_token))
        }
        "account/logout" => super::value_or_error(auth_account::logout_current_account()),
        _ => return None,
    };

    Some(super::response(req, result))
}

fn account_sort_updates_param(
    req: &JsonRpcRequest,
) -> Result<Vec<account_update::AccountSortUpdate>, String> {
    let items = req
        .params
        .as_ref()
        .and_then(|params| params.get("updates"))
        .and_then(|value| value.as_array())
        .ok_or_else(|| "missing account sort updates".to_string())?;
    let mut updates = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let source = item
            .as_object()
            .ok_or_else(|| format!("account sort update at index {index} must be an object"))?;
        let account_id = source
            .get("accountId")
            .or_else(|| source.get("account_id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("account sort update at index {index} missing accountId"))?
            .to_string();
        let sort = source
            .get("sort")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| format!("account sort update at index {index} missing sort"))?;
        updates.push(account_update::AccountSortUpdate { account_id, sort });
    }
    Ok(updates)
}

/// 函数 `first_str_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - keys: 参数 keys
///
/// # 返回
/// 返回函数执行结果
fn first_str_param<'a>(req: &'a JsonRpcRequest, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| super::str_param(req, key))
}

/// 函数 `first_string_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - keys: 参数 keys
///
/// # 返回
/// 返回函数执行结果
fn first_string_param(req: &JsonRpcRequest, keys: &[&str]) -> Option<String> {
    first_str_param(req, keys).map(|value| value.to_string())
}

/// 函数 `first_bool_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - keys: 参数 keys
///
/// # 返回
/// 返回函数执行结果
fn first_bool_param(req: &JsonRpcRequest, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| super::bool_param(req, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            id: 1.into(),
            method: method.to_string(),
            params: Some(params),
            trace: None,
        }
    }

    fn error_message(resp: &JsonRpcResponse) -> String {
        resp.result
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn update_sorts_rpc_rejects_malformed_updates() {
        let actor = RpcActor::system_admin();
        let missing_sort = try_handle(
            &rpc_request(
                "account/updateSorts",
                serde_json::json!({ "updates": [{ "accountId": "acc-a" }] }),
            ),
            &actor,
        )
        .expect("response");
        assert_eq!(
            error_message(&missing_sort),
            "account sort update at index 0 missing sort"
        );

        let missing_account_id = try_handle(
            &rpc_request(
                "account/updateSorts",
                serde_json::json!({ "updates": [{ "sort": 1 }] }),
            ),
            &actor,
        )
        .expect("response");
        assert_eq!(
            error_message(&missing_account_id),
            "account sort update at index 0 missing accountId"
        );
    }

    #[test]
    fn member_cannot_change_account_group_name() {
        let response = try_handle(
            &rpc_request(
                "account/update",
                serde_json::json!({ "accountId": "acc-a", "groupName": "team-a" }),
            ),
            &RpcActor::from_parts(Some(crate::ROLE_MEMBER), Some("member-a")),
        )
        .expect("response");
        assert_eq!(
            error_message(&response),
            "permission_denied: account/update groupName"
        );
    }

    #[test]
    fn member_cannot_start_or_cancel_account_tests() {
        let actor = RpcActor::from_parts(Some(crate::ROLE_MEMBER), Some("member-a"));
        for method in ["account/test", "account/test/cancel"] {
            let response = try_handle(
                &rpc_request(
                    method,
                    serde_json::json!({ "accountId": "acc-a", "testId": "test-a" }),
                ),
                &actor,
            )
            .expect("response");
            assert_eq!(
                error_message(&response),
                format!("permission_denied: {method}")
            );
        }
    }
}
