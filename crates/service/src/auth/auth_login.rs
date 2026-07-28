use codexmanager_core::auth::{
    build_authorize_url, generate_pkce, generate_state, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use codexmanager_core::rpc::types::LoginStartResult;
use codexmanager_core::storage::{now_ts, Event, LoginSession};

use crate::auth_callback::{ensure_login_server, resolve_redirect_uri};
use crate::storage_helpers::open_storage;

const LOGIN_SESSION_TTL_SECONDS: i64 = 15 * 60;
// A completion performs at most two auth HTTP requests, each bounded to 60
// seconds. Five minutes leaves ample margin while allowing an interrupted
// process to release the verifier on a later status check.
const LOGIN_COMPLETION_STALE_SECONDS: i64 = 5 * 60;

/// 函数 `is_device_login_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - login_type: 参数 login_type
///
/// # 返回
/// 返回函数执行结果
fn is_device_login_type(login_type: &str) -> bool {
    login_type.eq_ignore_ascii_case("chatgptDeviceCode")
        || login_type.eq_ignore_ascii_case("device")
}

/// 函数 `is_supported_chatgpt_login_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - login_type: 参数 login_type
///
/// # 返回
/// 返回函数执行结果
fn is_supported_chatgpt_login_type(login_type: &str) -> bool {
    let normalized = login_type.trim();
    normalized.eq_ignore_ascii_case("chatgpt")
        || normalized.eq_ignore_ascii_case("chatgptDeviceCode")
        || normalized.eq_ignore_ascii_case("device")
}

/// 函数 `login_start`
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
pub(crate) fn login_start(
    login_type: &str,
    open_browser: bool,
    note: Option<String>,
    tags: Option<String>,
    group_name: Option<String>,
    workspace_id: Option<String>,
) -> Result<LoginStartResult, String> {
    // 读取登录相关配置
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let originator = crate::gateway::current_wire_originator();
    let normalized_login_type = login_type.trim();
    if normalized_login_type.eq_ignore_ascii_case("apiKey") {
        return Ok(LoginStartResult::ApiKey {});
    }
    if !is_supported_chatgpt_login_type(normalized_login_type) {
        return Err(format!("unsupported login type: {normalized_login_type}"));
    }
    let is_device = is_device_login_type(normalized_login_type);
    if !is_device {
        ensure_login_server()?;
    }
    let redirect_uri = if is_device {
        std::env::var("CODEXMANAGER_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:1455/auth/callback".to_string())
    } else {
        resolve_redirect_uri().unwrap_or_else(|| "http://localhost:1455/auth/callback".to_string())
    };

    // 生成登录状态。Device Code 的 verifier 由 token 轮询接口返回。
    let state = generate_state();
    let login_id = if is_device {
        generate_state()
    } else {
        state.clone()
    };

    if is_device {
        let device = crate::auth_tokens::request_device_code(&issuer, &client_id)?;
        let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
        storage
            .insert_login_session(&LoginSession {
                login_id: login_id.clone(),
                code_verifier: String::new(),
                state: login_id.clone(),
                status: "pending".to_string(),
                error: None,
                workspace_id: workspace_id.clone(),
                note,
                tags,
                group_name,
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .map_err(|err| err.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: None,
                event_type: "login_start".to_string(),
                message: serde_json::json!({
                    "login_id": &login_id,
                    "login_type": "chatgptDeviceCode"
                })
                .to_string(),
                created_at: now_ts(),
            },
            "auth_login.device_code_start",
        );
        drop(storage);
        crate::auth_tokens::spawn_device_code_login_completion(
            issuer.clone(),
            login_id.clone(),
            device.clone(),
        )?;

        return Ok(LoginStartResult::ChatgptDeviceCode {
            login_id,
            verification_url: device.verification_url,
            user_code: device.user_code,
        });
    }

    let pkce = generate_pkce();

    // 写入登录会话
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .insert_login_session(&LoginSession {
            login_id: login_id.clone(),
            code_verifier: pkce.code_verifier.clone(),
            state: state.clone(),
            status: "pending".to_string(),
            error: None,
            workspace_id: workspace_id.clone(),
            note,
            tags,
            group_name,
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .map_err(|err| err.to_string())?;

    // 构造登录地址
    let auth_url = build_authorize_url(
        &issuer,
        &client_id,
        &redirect_uri,
        &pkce.code_challenge,
        &state,
        &originator,
        workspace_id.as_deref(),
    );

    // 写入事件日志
    crate::storage_helpers::insert_event_best_effort(
        &storage,
        &Event {
            account_id: None,
            event_type: "login_start".to_string(),
            message: serde_json::json!({
                "login_id": &state,
                "login_type": "chatgpt"
            })
            .to_string(),
            created_at: now_ts(),
        },
        "auth_login.oauth_start",
    );
    drop(storage);

    // 可选自动打开浏览器
    if open_browser {
        if let Err(err) = webbrowser::open(&auth_url) {
            log::warn!("event=auth_browser_open_failed error={err}");
        }
    }

    Ok(LoginStartResult::Chatgpt {
        login_id: state,
        auth_url,
    })
}

/// 函数 `login_status`
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
pub(crate) fn login_status(login_id: &str) -> serde_json::Value {
    // 查询登录会话状态
    if login_id.is_empty() {
        return serde_json::json!({ "status": "unknown" });
    }
    let storage = match open_storage() {
        Some(storage) => storage,
        None => return serde_json::json!({ "status": "unknown" }),
    };
    let mut session = match storage.get_login_session(login_id) {
        Ok(Some(session)) => session,
        _ => return serde_json::json!({ "status": "unknown" }),
    };
    let now = now_ts();
    let expiration_message = match session.status.as_str() {
        "pending" if now.saturating_sub(session.created_at) >= LOGIN_SESSION_TTL_SECONDS => {
            Some("login session expired after 15 minutes")
        }
        "completing"
            if now.saturating_sub(session.updated_at) >= LOGIN_COMPLETION_STALE_SECONDS =>
        {
            Some("login completion expired after 5 minutes without progress")
        }
        _ => None,
    };
    if let Some(message) = expiration_message {
        if storage
            .finish_login_session(login_id, "expired", Some(message))
            .unwrap_or(false)
        {
            crate::auth_tokens::cancel_device_code_login(login_id);
        }
        if let Ok(Some(updated)) = storage.get_login_session(login_id) {
            session = updated;
        }
    }
    serde_json::json!({
        "status": session.status,
        "error": session.error,
        "updatedAt": session.updated_at
    })
}

pub(crate) fn login_cancel(login_id: &str) -> Result<serde_json::Value, String> {
    if login_id.trim().is_empty() {
        return Err("missing login id".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let cancelled = storage
        .cancel_login_session(login_id)
        .map_err(|err| err.to_string())?;
    if cancelled {
        crate::auth_tokens::cancel_device_code_login(login_id);
    }
    let status = storage
        .get_login_session(login_id)
        .map_err(|err| err.to_string())?
        .map(|session| session.status)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(serde_json::json!({
        "cancelled": cancelled,
        "status": status
    }))
}
