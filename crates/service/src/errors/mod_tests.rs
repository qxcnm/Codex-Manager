use super::{classify_message, rpc_action_error_payload, rpc_error_payload, ErrorCode};

#[test]
fn classify_permission_denied_without_misclassifying_upstream_errors() {
    for message in [
        "permission_denied",
        "permission_denied: accountManager/users/list",
        " PERMISSION_DENIED: apikey requires user session ",
        "无权限(permission_denied: accountManager/wallet/topUp)",
    ] {
        assert_eq!(classify_message(message), ErrorCode::PermissionDenied);
    }
    assert_eq!(
        classify_message("upstream returned permission_denied"),
        ErrorCode::UnknownError
    );
    assert_eq!(
        classify_message("permission_denied_extra"),
        ErrorCode::UnknownError
    );
}

#[test]
fn permission_denied_rpc_payloads_preserve_existing_error_contract() {
    let message = "permission_denied: accountManager/wallet/topUp";
    let expected = serde_json::json!({
        "error": message,
        "errorCode": "permission_denied",
        "errorDetail": {
            "code": "permission_denied",
            "message": message,
        }
    });
    assert_eq!(rpc_error_payload(message.to_owned()), expected);

    let mut expected_action = expected;
    expected_action["ok"] = serde_json::Value::Bool(false);
    assert_eq!(
        rpc_action_error_payload(message.to_owned()),
        expected_action
    );
}

/// 函数 `classify_known_messages`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn classify_known_messages() {
    assert_eq!(
        classify_message("invalid app settings payload: missing field"),
        ErrorCode::InvalidSettingsPayload
    );
    assert_eq!(
        classify_message("Input exceeds the maximum length of 1048576 characters."),
        ErrorCode::InputTooLarge
    );
    assert_eq!(
        classify_message("upstream total timeout exceeded"),
        ErrorCode::UpstreamTimeout
    );
    assert_eq!(
        classify_message("invalid upstream json payload"),
        ErrorCode::ProtocolMappingError
    );
    assert_eq!(
        classify_message("backend proxy error: connection refused"),
        ErrorCode::BackendProxyError
    );
    assert_eq!(
        classify_message("claude request body must be an object"),
        ErrorCode::InvalidRequestPayload
    );
    assert_eq!(
        classify_message("Claude 请求体必须是对象(claude request body must be an object)"),
        ErrorCode::InvalidRequestPayload
    );
    assert_eq!(classify_message("上游请求超时"), ErrorCode::UpstreamTimeout);
    assert_eq!(
        classify_message("upstream request timed out"),
        ErrorCode::UpstreamTimeout
    );
    assert_eq!(
        classify_message("上游流式空闲超时"),
        ErrorCode::UpstreamTimeout
    );
    assert_eq!(
        classify_message("上游被安全验证拦截（Cloudflare/WAF）"),
        ErrorCode::UpstreamChallengeBlocked
    );
    assert_eq!(
        classify_message("stream disconnected before completion"),
        ErrorCode::StreamInterrupted
    );
    assert_eq!(
        classify_message("上游中途断开，未返回具体错误信息"),
        ErrorCode::StreamInterrupted
    );
    assert_eq!(
        classify_message("response.incomplete"),
        ErrorCode::StreamInterrupted
    );
    assert_eq!(classify_message("网络抖动"), ErrorCode::StreamInterrupted);
    assert_eq!(
        classify_message("连接中断（可能是网络波动或客户端主动取消）"),
        ErrorCode::StreamInterrupted
    );
    assert_eq!(
        classify_message("上游请求失败，未返回具体错误信息"),
        ErrorCode::UpstreamNonSuccess
    );
    assert_eq!(
        classify_message("无可用账号(no available account)"),
        ErrorCode::NoAvailableAccount
    );
    assert_eq!(
        classify_message(
            "code=model_not_found type=invalid_request_error The model 'gpt-5.4' does not exist"
        ),
        ErrorCode::UpstreamNonSuccess
    );
}
