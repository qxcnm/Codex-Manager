use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

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
pub(super) fn try_handle(req: &JsonRpcRequest, actor: &crate::RpcActor) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "appSettings/get" => super::value_or_error(
            crate::app_settings_get().map(|settings| settings_for_actor(settings, actor)),
        ),
        "appSettings/set" => super::value_or_error(crate::app_settings_set(req.params.as_ref())),
        "authorContent/get" => super::value_or_error(crate::author_content_get()),
        "webAuth/status" => super::value_or_error(crate::app_auth_status_value()),
        "webAuth/password/set" => {
            let password = super::str_param(req, "password").unwrap_or("");
            super::value_or_error(
                crate::set_web_access_password(Some(password))
                    .map(|configured| serde_json::json!({ "passwordConfigured": configured })),
            )
        }
        "webAuth/password/clear" => super::value_or_error(
            crate::set_web_access_password(None)
                .map(|configured| serde_json::json!({ "passwordConfigured": configured })),
        ),
        _ => return None,
    };

    Some(super::response(req, result))
}

fn settings_for_actor(settings: serde_json::Value, actor: &crate::RpcActor) -> serde_json::Value {
    if actor.is_admin() {
        return settings;
    }
    // Members need UI/bootstrap preferences, not deployment or upstream configuration.
    // An allowlist also prevents newly added settings from becoming public by default.
    const MEMBER_FIELDS: &[&str] = &[
        "serviceAddr",
        "theme",
        "appearancePreset",
        "locale",
        "localeOptions",
        "lowTransparency",
        "zoomFactor",
        "codexCliGuideDismissed",
        "updateAutoCheck",
        "autoStartEnabled",
        "autoStartSupported",
        "showMainWindowOnStartup",
        "closeToTrayOnClose",
        "closeToTraySupported",
        "keepWindowUiMounted",
        "lightweightModeOnCloseToTray",
        "webAccessPasswordConfigured",
        "webAuthMode",
        "distributionEnabled",
        "billingModeLock",
        "appUsersConfigured",
        "runtimeTimeZone",
        "authorSponsors",
        "authorServerRecommendations",
    ];
    serde_json::Value::Object(
        MEMBER_FIELDS
            .iter()
            .filter_map(|field| {
                settings
                    .get(*field)
                    .map(|value| ((*field).to_owned(), value.clone()))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_settings_do_not_expose_new_or_nested_deployment_fields() {
        let settings = serde_json::json!({
            "theme": "tech", "serviceAddr": "localhost:48760",
            "envOverrides": { "CODEXMANAGER_GITHUB_TOKEN": "fixture-secret" },
            "envOverrideCatalog": [{ "defaultValue": "fixture-secret" }],
            "futureDeploymentSecret": "fixture-secret"
        });
        let member = crate::RpcActor::from_parts(Some("member"), Some("alice"));
        assert_eq!(
            settings_for_actor(settings.clone(), &member),
            serde_json::json!({
                "theme": "tech", "serviceAddr": "localhost:48760"
            })
        );
        assert_eq!(
            settings_for_actor(settings.clone(), &crate::RpcActor::system_admin()),
            settings
        );
    }
}
