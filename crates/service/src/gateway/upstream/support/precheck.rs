use crate::http::gateway_request::GatewayRequest as Request;
use codexmanager_core::storage::{Account, Storage, Token};

pub(in super::super) enum CandidatePrecheckResult {
    Ready {
        request: Request,
        candidates: Vec<(Account, Token)>,
    },
    Empty {
        request: Request,
    },
    Responded,
}

/// 函数 `prepare_candidates_for_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn prepare_candidates_for_proxy(
    request: Request,
    storage: &Storage,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    response_adapter: super::super::super::ResponseAdapter,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    account_group_filter: Option<&str>,
    account_plan_filter: Option<&str>,
    low_quota_mode: super::super::super::LowQuotaCandidateMode,
    respond_when_empty: bool,
) -> CandidatePrecheckResult {
    let candidates: Vec<(Account, Token)> = match super::candidates::prepare_gateway_candidates(
        storage,
        model_for_log,
        account_group_filter,
        account_plan_filter,
        low_quota_mode,
    ) {
        Ok(v) => v,
        Err(err) => {
            let err_text = super::super::super::bilingual_error(
                "候选账号解析失败",
                format!("candidate resolve failed: {err}"),
            );
            super::super::super::write_request_log(
                storage,
                super::super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    original_path: Some(original_path),
                    adapted_path: Some(path),
                    response_adapter: Some(response_adapter),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                None,
                Some(500),
                super::super::super::request_log::RequestLogUsage::default(),
                Some(err_text.as_str()),
                None,
            );
            let response = super::super::super::error_response::terminal_text_response(
                500,
                super::super::super::error_message_for_client(
                    super::super::super::prefers_raw_errors_for_gateway_request(&request),
                    err_text.clone(),
                ),
                Some(trace_id),
            );
            let _ = request.respond(response);
            super::super::super::trace_log::log_request_final(
                trace_id,
                500,
                None,
                None,
                Some(err_text.as_str()),
                0,
            );
            return CandidatePrecheckResult::Responded;
        }
    };

    if candidates.is_empty() && !respond_when_empty {
        return CandidatePrecheckResult::Empty { request };
    }

    if candidates.is_empty() {
        super::super::super::write_request_log(
            storage,
            super::super::super::request_log::RequestLogTraceContext {
                trace_id: Some(trace_id),
                original_path: Some(original_path),
                adapted_path: Some(path),
                response_adapter: Some(response_adapter),
                ..Default::default()
            },
            Some(key_id),
            None,
            path,
            request_method,
            model_for_log,
            reasoning_for_log,
            None,
            Some(503),
            super::super::super::request_log::RequestLogUsage::default(),
            Some("无可用账号(no available account)"),
            None,
        );
        let response = super::super::super::error_response::terminal_text_response(
            503,
            super::super::super::error_message_for_client(
                super::super::super::prefers_raw_errors_for_gateway_request(&request),
                "无可用账号(no available account)",
            ),
            Some(trace_id),
        );
        let _ = request.respond(response);
        super::super::super::trace_log::log_request_final(
            trace_id,
            503,
            None,
            None,
            Some("无可用账号(no available account)"),
            0,
        );
        return CandidatePrecheckResult::Responded;
    }

    CandidatePrecheckResult::Ready {
        request,
        candidates,
    }
}
