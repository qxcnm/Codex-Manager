use super::{derive_final_error, derive_status_for_log, is_client_disconnect_error};

#[test]
fn derive_final_error_prefers_upstream_hint_then_http_error_then_bridge_error() {
    assert_eq!(
        derive_final_error(
            429,
            Some("last attempt"),
            Some("upstream hint"),
            Some("bridge error".to_string()),
        )
        .as_deref(),
        Some("upstream hint")
    );
    assert_eq!(
        derive_final_error(
            429,
            Some("last attempt"),
            None,
            Some("bridge error".to_string())
        )
        .as_deref(),
        Some("last attempt")
    );
    assert_eq!(
        derive_final_error(200, None, None, Some("bridge error".to_string())).as_deref(),
        Some("bridge error")
    );
}

#[test]
fn derive_status_for_log_respects_disconnect_delivery_and_bridge_fallbacks() {
    assert_eq!(
        derive_status_for_log(200, None, true, false, false, true),
        499
    );
    assert_eq!(
        derive_status_for_log(200, Some(207), true, false, false, false),
        207
    );
    assert_eq!(
        derive_status_for_log(404, None, true, false, false, false),
        404
    );
    assert_eq!(
        derive_status_for_log(200, None, true, true, false, false),
        502
    );
    assert_eq!(
        derive_status_for_log(200, None, false, false, false, false),
        502
    );
    assert_eq!(
        derive_status_for_log(200, None, true, false, false, false),
        200
    );
}

#[test]
fn client_disconnect_error_matches_common_socket_messages() {
    assert!(is_client_disconnect_error("broken pipe"));
    assert!(is_client_disconnect_error("connection reset by peer"));
    assert!(is_client_disconnect_error(
        "你的主机中的软件中止了一个已建立的连接。 (os error 10053)"
    ));
    assert!(!is_client_disconnect_error("upstream timeout"));
}

#[test]
fn rejection_cache_only_clears_after_a_confirmed_completed_response() {
    use crate::account::model_support::{clear_unsupported, is_unsupported, mark_unsupported};
    use crate::gateway::http_bridge::UpstreamResponseBridgeResult;
    let account = "finalized-model-support-account";
    let model = "gpt-6-luna";
    for is_stream in [false, true] {
        let mut result = UpstreamResponseBridgeResult::default();
        mark_unsupported(account, model);
        super::clear_model_rejection_after_success(account, Some(model), 200, is_stream, &result);
        assert!(
            is_unsupported(account, model),
            "headers or missing completion are insufficient"
        );
        result.stream_terminal_seen = true;
        result.stream_terminal_delivered = true;
        result.usage.completed_successfully = true;
        result.stream_terminal_error = Some("response.failed".to_owned());
        result.upstream_error_hint = Some("response.failed".to_owned());
        super::clear_model_rejection_after_success(account, Some(model), 200, is_stream, &result);
        assert!(is_unsupported(account, model));
        result.stream_terminal_error = None;
        result.upstream_error_hint = None;
        result.delivery_error = Some("body truncated".to_owned());
        super::clear_model_rejection_after_success(account, Some(model), 200, is_stream, &result);
        assert!(is_unsupported(account, model));
        result.delivery_error = None;
        super::clear_model_rejection_after_success(account, Some(model), 200, is_stream, &result);
        assert!(!is_unsupported(account, model));
    }
    clear_unsupported(account, model);
}

#[test]
fn real_bridge_outcomes_clear_qualification_only_after_success_for_both_endpoints_and_modes() {
    use crate::account::model_support::{clear_unsupported, is_unsupported, mark_unsupported};
    use crate::gateway::upstream::{
        GatewayByteStream, GatewayStreamResponse, GatewayUpstreamResponse,
    };
    use crate::http::gateway_request::GatewayRequest;
    use bytes::Bytes;
    crate::gateway::response_test_runtime().unwrap().block_on(async {
        for (path, is_stream, body, success) in [
            ("/v1/responses", false, r#"{"object":"response","status":"failed","error":{"message":"failed tool"},"output":[]}"#, false),
            ("/v1/responses", false, r#"{"object":"response","status":"incomplete","output":[]}"#, false),
            ("/v1/responses", false, r#"{"object":"response","status":"completed","output":[{"type":"function_call","name":"weather","arguments":"{}"}]}"#, true),
            ("/v1/chat/completions", false, r#"{"error":{"message":"failed tool"}}"#, false),
            ("/v1/chat/completions", false, r#"{"object":"chat.completion","choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}"#, false),
            ("/v1/chat/completions", false, r#"{"object":"chat.completion","choices":[{"message":{"tool_calls":[{"type":"function","function":{"name":"weather","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#, true),
            ("/v1/responses", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"failed\",\"output\":[]}}\n\n", false),
            ("/v1/responses", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"incomplete\",\"output\":[]}}\n\n", false),
            ("/v1/chat/completions", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"failed\",\"output\":[]}}\n\n", false),
            ("/v1/chat/completions", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"incomplete\",\"output\":[]}}\n\n", false),
            ("/v1/responses", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"canceled\",\"output\":[]}}\n\n", false),
            ("/v1/chat/completions", false, r#"{"object":"chat.completion","status":"canceled","choices":[{"message":{},"finish_reason":"stop"}]}"#, false),
            ("/v1/responses", true, "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"failed tool\"}}}\n\n", false),
            ("/v1/responses", true, "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n", false),
            ("/v1/responses", true, "data: {\"type\":\"response.completed\",\"response\":{\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"name\":\"weather\",\"arguments\":\"{}\"}]}}\n\n", true),
            ("/v1/chat/completions", true, "data: {\"error\":{\"message\":\"failed tool\"}}\n\n", false),
            ("/v1/chat/completions", true, "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n", false),
            ("/v1/chat/completions", true, "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"type\":\"function\",\"function\":{\"name\":\"weather\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n", true),
        ] {
            let account = format!("bridge-qualification-{path}-{is_stream}");
            let model = "gpt-6-luna";
            mark_unsupported(&account, model);
            let (parts, ()) = axum::http::Request::builder().method("POST").uri(path).body(()).unwrap().into_parts();
            let (request, receiver) = GatewayRequest::new(parts, Bytes::from_static(b"{}"));
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("content-type", if is_stream { "text/event-stream" } else { "application/json" }.parse().unwrap());
            let upstream = GatewayUpstreamResponse::Stream(GatewayStreamResponse::new(reqwest::StatusCode::OK, headers,
                GatewayByteStream::from_bytes(Bytes::copy_from_slice(body.as_bytes()))));
            let owned_path = path.to_owned();
            let owned_account = account.clone();
            let delivery = tokio::spawn(async move {
                crate::gateway::http_bridge::respond_with_upstream_async(request, upstream,
                    crate::gateway::acquire_account_inflight(&owned_account), crate::gateway::ResponseAdapter::Passthrough,
                    None, None, &owned_path, None, is_stream, false, None, Some("gpt-6-luna"), std::time::Instant::now()).await
            });
            let response = receiver.await.unwrap();
            let _ = axum::body::to_bytes(response.into_body(), usize::MAX).await;
            let bridge = delivery.await.unwrap().unwrap();
            super::clear_model_rejection_after_success(&account, Some(model), 200, is_stream, &bridge);
            assert_eq!(!is_unsupported(&account, model), success, "{path} stream={is_stream} body={body} bridge={bridge:?}");
            clear_unsupported(&account, model);
        }
    });
}
