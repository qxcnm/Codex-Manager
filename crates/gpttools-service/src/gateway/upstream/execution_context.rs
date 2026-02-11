use gpttools_core::storage::Storage;

pub(super) struct GatewayUpstreamExecutionContext<'a> {
    storage: &'a Storage,
    key_id: &'a str,
    path: &'a str,
    request_method: &'a str,
    model_for_log: Option<&'a str>,
    reasoning_for_log: Option<&'a str>,
    candidate_count: usize,
    account_max_inflight: usize,
}

impl<'a> GatewayUpstreamExecutionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        storage: &'a Storage,
        key_id: &'a str,
        path: &'a str,
        request_method: &'a str,
        model_for_log: Option<&'a str>,
        reasoning_for_log: Option<&'a str>,
        candidate_count: usize,
        account_max_inflight: usize,
    ) -> Self {
        Self {
            storage,
            key_id,
            path,
            request_method,
            model_for_log,
            reasoning_for_log,
            candidate_count,
            account_max_inflight,
        }
    }

    pub(super) fn has_more_candidates(&self, idx: usize) -> bool {
        idx + 1 < self.candidate_count
    }

    pub(super) fn should_skip_candidate(&self, account_id: &str, idx: usize) -> bool {
        super::candidates::should_skip_candidate_for_proxy(
            account_id,
            idx,
            self.candidate_count,
            self.account_max_inflight,
        )
    }

    pub(super) fn log_result(
        &self,
        upstream_url: Option<&str>,
        status_code: u16,
        error: Option<&str>,
    ) {
        super::super::write_request_log(
            self.storage,
            Some(self.key_id),
            self.path,
            self.request_method,
            self.model_for_log,
            self.reasoning_for_log,
            upstream_url,
            Some(status_code),
            error,
        );
    }
}


