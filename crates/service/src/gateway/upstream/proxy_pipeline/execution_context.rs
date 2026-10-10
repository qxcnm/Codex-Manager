use super::super::support::candidates;
use codexmanager_core::storage::Storage;

pub(in super::super) struct GatewayUpstreamExecutionContext<'a> {
    trace_id: &'a str,
    storage: &'a Storage,
    key_id: &'a str,
    original_path: &'a str,
    path: &'a str,
    request_method: &'a str,
    response_adapter: super::super::super::ResponseAdapter,
    protocol_type: &'a str,
    client_model_for_log: Option<&'a str>,
    model_for_log: Option<&'a str>,
    model_source_for_log: Option<&'a str>,
    client_reasoning_for_log: Option<&'a str>,
    reasoning_for_log: Option<&'a str>,
    reasoning_source_for_log: Option<&'a str>,
    service_tier_for_log: Option<&'a str>,
    effective_service_tier_for_log: Option<&'a str>,
    service_tier_source_for_log: Option<&'a str>,
    gateway_mode_for_log: Option<&'a str>,
    route_strategy_for_log: Option<&'a str>,
    route_source_for_log: Option<&'a str>,
    estimated_input_tokens: i64,
    candidate_count: usize,
    account_max_inflight: usize,
}
/// Owned metadata allows stream delivery to outlive the short routing worker.
/// A fresh storage handle is acquired only for final accounting.
pub(in super::super) struct OwnedGatewayUpstreamExecutionContext {
    trace_id: String,
    key_id: String,
    original_path: String,
    path: String,
    request_method: String,
    response_adapter: super::super::super::ResponseAdapter,
    protocol_type: String,
    client_model_for_log: Option<String>,
    model_for_log: Option<String>,
    model_source_for_log: Option<String>,
    client_reasoning_for_log: Option<String>,
    reasoning_for_log: Option<String>,
    reasoning_source_for_log: Option<String>,
    service_tier_for_log: Option<String>,
    effective_service_tier_for_log: Option<String>,
    service_tier_source_for_log: Option<String>,
    gateway_mode_for_log: Option<String>,
    route_strategy_for_log: Option<String>,
    route_source_for_log: Option<String>,
    estimated_input_tokens: i64,
    candidate_count: usize,
    account_max_inflight: usize,
}

impl GatewayUpstreamExecutionContext<'_> {
    pub(in super::super) fn to_owned(&self) -> OwnedGatewayUpstreamExecutionContext {
        OwnedGatewayUpstreamExecutionContext {
            trace_id: self.trace_id.to_owned(),
            key_id: self.key_id.to_owned(),
            original_path: self.original_path.to_owned(),
            path: self.path.to_owned(),
            request_method: self.request_method.to_owned(),
            response_adapter: self.response_adapter,
            protocol_type: self.protocol_type.to_owned(),
            client_model_for_log: self.client_model_for_log.map(str::to_owned),
            model_for_log: self.model_for_log.map(str::to_owned),
            model_source_for_log: self.model_source_for_log.map(str::to_owned),
            client_reasoning_for_log: self.client_reasoning_for_log.map(str::to_owned),
            reasoning_for_log: self.reasoning_for_log.map(str::to_owned),
            reasoning_source_for_log: self.reasoning_source_for_log.map(str::to_owned),
            service_tier_for_log: self.service_tier_for_log.map(str::to_owned),
            effective_service_tier_for_log: self.effective_service_tier_for_log.map(str::to_owned),
            service_tier_source_for_log: self.service_tier_source_for_log.map(str::to_owned),
            gateway_mode_for_log: self.gateway_mode_for_log.map(str::to_owned),
            route_strategy_for_log: self.route_strategy_for_log.map(str::to_owned),
            route_source_for_log: self.route_source_for_log.map(str::to_owned),
            estimated_input_tokens: self.estimated_input_tokens,
            candidate_count: self.candidate_count,
            account_max_inflight: self.account_max_inflight,
        }
    }
}

impl OwnedGatewayUpstreamExecutionContext {
    pub(in super::super) fn as_borrowed<'a>(
        &'a self,
        storage: &'a Storage,
    ) -> GatewayUpstreamExecutionContext<'a> {
        GatewayUpstreamExecutionContext {
            storage,
            trace_id: self.trace_id.as_str(),
            key_id: self.key_id.as_str(),
            original_path: self.original_path.as_str(),
            path: self.path.as_str(),
            request_method: self.request_method.as_str(),
            response_adapter: self.response_adapter,
            protocol_type: self.protocol_type.as_str(),
            client_model_for_log: self.client_model_for_log.as_deref(),
            model_for_log: self.model_for_log.as_deref(),
            model_source_for_log: self.model_source_for_log.as_deref(),
            client_reasoning_for_log: self.client_reasoning_for_log.as_deref(),
            reasoning_for_log: self.reasoning_for_log.as_deref(),
            reasoning_source_for_log: self.reasoning_source_for_log.as_deref(),
            service_tier_for_log: self.service_tier_for_log.as_deref(),
            effective_service_tier_for_log: self.effective_service_tier_for_log.as_deref(),
            service_tier_source_for_log: self.service_tier_source_for_log.as_deref(),
            gateway_mode_for_log: self.gateway_mode_for_log.as_deref(),
            route_strategy_for_log: self.route_strategy_for_log.as_deref(),
            route_source_for_log: self.route_source_for_log.as_deref(),
            estimated_input_tokens: self.estimated_input_tokens,
            candidate_count: self.candidate_count,
            account_max_inflight: self.account_max_inflight,
        }
    }
}

impl<'a> GatewayUpstreamExecutionContext<'a> {
    /// 函数 `new`
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
    pub(in super::super) fn new(
        trace_id: &'a str,
        storage: &'a Storage,
        key_id: &'a str,
        original_path: &'a str,
        path: &'a str,
        request_method: &'a str,
        response_adapter: super::super::super::ResponseAdapter,
        protocol_type: &'a str,
        client_model_for_log: Option<&'a str>,
        model_for_log: Option<&'a str>,
        model_source_for_log: Option<&'a str>,
        client_reasoning_for_log: Option<&'a str>,
        reasoning_for_log: Option<&'a str>,
        reasoning_source_for_log: Option<&'a str>,
        service_tier_for_log: Option<&'a str>,
        effective_service_tier_for_log: Option<&'a str>,
        service_tier_source_for_log: Option<&'a str>,
        gateway_mode_for_log: Option<&'a str>,
        route_strategy_for_log: Option<&'a str>,
        route_source_for_log: Option<&'a str>,
        estimated_input_tokens: i64,
        candidate_count: usize,
        account_max_inflight: usize,
    ) -> Self {
        Self {
            trace_id,
            storage,
            key_id,
            original_path,
            path,
            request_method,
            response_adapter,
            protocol_type,
            client_model_for_log,
            model_for_log,
            model_source_for_log,
            client_reasoning_for_log,
            reasoning_for_log,
            reasoning_source_for_log,
            service_tier_for_log,
            effective_service_tier_for_log,
            service_tier_source_for_log,
            gateway_mode_for_log,
            route_strategy_for_log,
            route_source_for_log,
            estimated_input_tokens,
            candidate_count,
            account_max_inflight,
        }
    }

    /// 函数 `has_more_candidates`
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
    pub(in super::super) fn has_more_candidates(&self, idx: usize) -> bool {
        idx + 1 < self.candidate_count
    }

    pub(in super::super) fn protocol_type(&self) -> &str {
        self.protocol_type
    }

    pub(in super::super) fn payload_capture(
        &self,
    ) -> super::super::super::OutboundPayloadContext<'_> {
        super::super::super::OutboundPayloadContext {
            trace_id: self.trace_id,
            key_id: self.key_id,
        }
    }

    pub(in super::super) fn record_completed_response_id(&self, response_id: &str) {
        if let Err(err) =
            self.storage
                .record_request_log_response_id(self.key_id, response_id, self.trace_id)
        {
            log::warn!(
                "event=request_log_response_id_insert_failed trace_id={} err={err}",
                self.trace_id
            );
        }
    }

    /// 函数 `should_skip_candidate`
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
    pub(in super::super) fn should_skip_candidate(
        &self,
        account_id: &str,
        idx: usize,
    ) -> Option<candidates::CandidateSkipReason> {
        candidates::candidate_skip_reason_for_proxy(
            account_id,
            idx,
            self.candidate_count,
            self.account_max_inflight,
            self.protocol_type == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        )
    }

    /// 函数 `log_candidate_start`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - in super: 参数 in super
    ///
    /// # 返回
    /// 无
    pub(in super::super) fn log_candidate_start(
        &self,
        account_id: &str,
        idx: usize,
        strip_session_affinity: bool,
    ) {
        super::super::super::trace_log::log_candidate_start(
            self.trace_id,
            idx,
            self.candidate_count,
            account_id,
            strip_session_affinity,
        );
    }

    /// 函数 `log_candidate_skip`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - in super: 参数 in super
    ///
    /// # 返回
    /// 无
    pub(in super::super) fn log_candidate_skip(
        &self,
        account_id: &str,
        idx: usize,
        reason: candidates::CandidateSkipReason,
    ) {
        let reason_text = match reason {
            candidates::CandidateSkipReason::Cooldown => "cooldown",
            candidates::CandidateSkipReason::Inflight => "inflight",
        };
        super::super::super::trace_log::log_candidate_skip(
            self.trace_id,
            idx,
            self.candidate_count,
            account_id,
            reason_text,
        );
    }

    /// 函数 `log_attempt_result`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 无
    pub(super) fn log_attempt_result(
        &self,
        account_id: &str,
        upstream_url: Option<&str>,
        status_code: u16,
        error: Option<&str>,
    ) {
        super::super::super::trace_log::log_attempt_result(
            self.trace_id,
            account_id,
            upstream_url,
            status_code,
            error,
        );
    }

    /// 函数 `mark_account_unavailable_for_gateway_error`
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
    pub(in super::super) fn mark_account_unavailable_for_gateway_error(
        &self,
        account_id: &str,
        err: &str,
    ) -> bool {
        crate::account_status::mark_account_unavailable_for_gateway_error(
            self.storage,
            account_id,
            err,
        )
    }

    pub(in super::super) fn apply_gateway_error_follow_up(
        &self,
        account_id: &str,
        err: &str,
        has_more_candidates: bool,
    ) -> crate::account_status::GatewayErrorFollowUp {
        let follow_up = crate::account_status::analyze_gateway_error(err, has_more_candidates);
        if follow_up.should_mark_default_cooldown {
            super::super::super::mark_account_cooldown(
                account_id,
                super::super::super::CooldownReason::Default,
            );
        }
        if follow_up.should_mark_account_unavailable {
            let _ = self.mark_account_unavailable_for_gateway_error(account_id, err);
        }
        follow_up
    }

    /// 函数 `log_final_result`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - in super: 参数 in super
    ///
    /// # 返回
    /// 无
    pub(in super::super) fn log_final_result(
        &self,
        final_account_id: Option<&str>,
        upstream_url: Option<&str>,
        status_code: u16,
        usage: super::super::super::request_log::RequestLogUsage,
        error: Option<&str>,
        elapsed_ms: u128,
        attempted_account_ids: Option<&[String]>,
    ) {
        self.log_final_result_with_model(
            final_account_id,
            upstream_url,
            self.model_for_log,
            status_code,
            usage,
            error,
            elapsed_ms,
            attempted_account_ids,
        );
    }

    /// 函数 `log_final_result_with_model`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - in super: 参数 in super
    ///
    /// # 返回
    /// 无
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn log_final_result_with_model(
        &self,
        final_account_id: Option<&str>,
        upstream_url: Option<&str>,
        model_for_log: Option<&str>,
        status_code: u16,
        mut usage: super::super::super::request_log::RequestLogUsage,
        error: Option<&str>,
        elapsed_ms: u128,
        attempted_account_ids: Option<&[String]>,
    ) {
        if usage.estimated_input_tokens.is_none() {
            usage.estimated_input_tokens = Some(self.estimated_input_tokens);
        }
        let platform_model_for_log = self.model_for_log.or(model_for_log);
        let direct_upstream_model =
            resolve_direct_upstream_model_for_log(platform_model_for_log, model_for_log);
        super::super::super::request_log::write_request_log_with_attempts(
            self.storage,
            super::super::super::request_log::RequestLogTraceContext {
                trace_id: Some(self.trace_id),
                original_path: Some(self.original_path),
                adapted_path: Some(self.path),
                gateway_mode: self.gateway_mode_for_log,
                route_strategy: self.route_strategy_for_log,
                route_source: self.route_source_for_log,
                response_adapter: Some(self.response_adapter),
                request_type: Some("http"),
                client_model: self.client_model_for_log,
                model_source: self.model_source_for_log,
                client_reasoning_effort: self.client_reasoning_for_log,
                reasoning_source: self.reasoning_source_for_log,
                service_tier: self.service_tier_for_log,
                effective_service_tier: self.effective_service_tier_for_log,
                service_tier_source: self.service_tier_source_for_log,
                upstream_model: direct_upstream_model,
                actual_source_kind: final_account_id.map(|_| "openai_account"),
                actual_source_id: final_account_id,
                ..Default::default()
            },
            Some(self.key_id),
            final_account_id,
            self.path,
            self.request_method,
            platform_model_for_log,
            self.reasoning_for_log,
            upstream_url,
            Some(status_code),
            usage,
            error,
            Some(elapsed_ms),
            attempted_account_ids,
        );
        super::super::super::trace_log::log_request_final(
            self.trace_id,
            status_code,
            final_account_id,
            upstream_url,
            error,
            elapsed_ms,
        );
        super::super::super::record_gateway_request_outcome(
            self.path,
            status_code,
            Some(self.protocol_type),
        );
    }
}

fn resolve_direct_upstream_model_for_log<'a>(
    platform_model_for_log: Option<&'a str>,
    model_for_log: Option<&'a str>,
) -> Option<&'a str> {
    let platform_model = platform_model_for_log
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let candidate_model = model_for_log
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    (candidate_model != platform_model).then_some(candidate_model)
}

#[cfg(test)]
#[path = "execution_context_tests.rs"]
mod tests;
