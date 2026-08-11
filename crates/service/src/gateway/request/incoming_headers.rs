use tiny_http::Request;

#[derive(Clone, Default)]
pub(crate) struct IncomingHeaderSnapshot {
    authorization_present: bool,
    x_api_key_present: bool,
    authorization_bearer_strict: Option<String>,
    authorization_bearer_case_insensitive: Option<String>,
    x_api_key: Option<String>,
    user_agent: Option<String>,
    originator: Option<String>,
    session_id: Option<String>,
    session_affinity: Option<String>,
    client_request_id: Option<String>,
    idempotency_key: Option<String>,
    subagent: Option<String>,
    beta_features: Option<String>,
    window_id: Option<String>,
    turn_metadata: Option<String>,
    turn_state: Option<String>,
    parent_thread_id: Option<String>,
    codex_installation_id: Option<String>,
    responsesapi_include_timing_metrics: Option<String>,
    codex_inference_call_id: Option<String>,
    oai_attestation: Option<String>,
    passthrough_codex_headers: Vec<(String, String)>,
    conversation_id: Option<String>,
}

impl IncomingHeaderSnapshot {
    /// 函数 `from_http_headers`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-05
    ///
    /// # 参数
    /// - headers: 参数 headers
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn from_http_headers(headers: &axum::http::HeaderMap) -> Self {
        let mut snapshot = IncomingHeaderSnapshot::default();
        for (name, value) in headers.iter() {
            let Ok(raw_value) = value.to_str() else {
                continue;
            };
            let name = name.as_str();
            let value = raw_value.trim();
            if name.eq_ignore_ascii_case("Authorization") {
                snapshot.authorization_present = true;
                if snapshot.authorization_bearer_strict.is_none() {
                    snapshot.authorization_bearer_strict = strict_bearer_token(value);
                }
                if snapshot.authorization_bearer_case_insensitive.is_none() {
                    snapshot.authorization_bearer_case_insensitive =
                        case_insensitive_bearer_token(value);
                }
                continue;
            }
            if name.eq_ignore_ascii_case("x-api-key") || name.eq_ignore_ascii_case("x-goog-api-key")
            {
                snapshot.x_api_key_present = true;
                if snapshot.x_api_key.is_none() && !value.is_empty() {
                    snapshot.x_api_key = Some(value.to_string());
                }
                continue;
            }
            if snapshot.idempotency_key.is_none() && name.eq_ignore_ascii_case("idempotency-key") {
                if !value.is_empty() {
                    snapshot.idempotency_key = Some(value.to_string());
                }
                continue;
            }
            if snapshot.user_agent.is_none() && name.eq_ignore_ascii_case("User-Agent") {
                if !value.is_empty() {
                    snapshot.user_agent = Some(value.to_string());
                }
                continue;
            }
            if snapshot.originator.is_none() && name.eq_ignore_ascii_case("originator") {
                if !value.is_empty() {
                    snapshot.originator = Some(value.to_string());
                }
                continue;
            }
            if snapshot.session_id.is_none() && name.eq_ignore_ascii_case("session_id") {
                if !value.is_empty() {
                    snapshot.session_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.session_affinity.is_none()
                && name.eq_ignore_ascii_case("x-session-affinity")
            {
                if !value.is_empty() {
                    snapshot.session_affinity = Some(value.to_string());
                }
                continue;
            }
            if snapshot.client_request_id.is_none()
                && name.eq_ignore_ascii_case("x-client-request-id")
            {
                if !value.is_empty() {
                    snapshot.client_request_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.subagent.is_none() && name.eq_ignore_ascii_case("x-openai-subagent") {
                if !value.is_empty() {
                    snapshot.subagent = Some(value.to_string());
                }
                continue;
            }
            if snapshot.beta_features.is_none()
                && name.eq_ignore_ascii_case("x-codex-beta-features")
            {
                if !value.is_empty() {
                    snapshot.beta_features = Some(value.to_string());
                }
                continue;
            }
            if snapshot.window_id.is_none() && name.eq_ignore_ascii_case("x-codex-window-id") {
                if !value.is_empty() {
                    snapshot.window_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.turn_metadata.is_none()
                && name.eq_ignore_ascii_case("x-codex-turn-metadata")
            {
                if !value.is_empty() {
                    snapshot.turn_metadata = Some(value.to_string());
                }
                continue;
            }
            if snapshot.turn_state.is_none() && name.eq_ignore_ascii_case("x-codex-turn-state") {
                if !value.is_empty() {
                    snapshot.turn_state = Some(value.to_string());
                }
                continue;
            }
            if snapshot.parent_thread_id.is_none()
                && name.eq_ignore_ascii_case("x-codex-parent-thread-id")
            {
                if !value.is_empty() {
                    snapshot.parent_thread_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.codex_installation_id.is_none()
                && name.eq_ignore_ascii_case("x-codex-installation-id")
            {
                if !value.is_empty() {
                    snapshot.codex_installation_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.responsesapi_include_timing_metrics.is_none()
                && name.eq_ignore_ascii_case("x-responsesapi-include-timing-metrics")
            {
                if !value.is_empty() {
                    snapshot.responsesapi_include_timing_metrics = Some(value.to_string());
                }
                continue;
            }
            if snapshot.codex_inference_call_id.is_none()
                && name.eq_ignore_ascii_case("x-codex-inference-call-id")
            {
                if !value.is_empty() {
                    snapshot.codex_inference_call_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.oai_attestation.is_none() && name.eq_ignore_ascii_case("x-oai-attestation")
            {
                if !value.is_empty() {
                    snapshot.oai_attestation = Some(value.to_string());
                }
                continue;
            }
            if should_capture_passthrough_codex_header(name) && !value.is_empty() {
                remember_passthrough_header(&mut snapshot.passthrough_codex_headers, name, value);
                continue;
            }
            if snapshot.conversation_id.is_none() && name.eq_ignore_ascii_case("conversation_id") {
                if !value.is_empty() {
                    snapshot.conversation_id = Some(value.to_string());
                }
            }
        }
        snapshot.derive_session_id_from_turn_metadata();
        snapshot
    }

    /// 函数 `from_request`
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
    pub(crate) fn from_request(request: &Request) -> Self {
        let mut snapshot = IncomingHeaderSnapshot::default();
        for header in request.headers() {
            if header.field.equiv("Authorization") {
                snapshot.authorization_present = true;
                let value = header.value.as_str().trim();
                if snapshot.authorization_bearer_strict.is_none() {
                    snapshot.authorization_bearer_strict = strict_bearer_token(value);
                }
                if snapshot.authorization_bearer_case_insensitive.is_none() {
                    snapshot.authorization_bearer_case_insensitive =
                        case_insensitive_bearer_token(value);
                }
                continue;
            }
            if header.field.equiv("x-api-key") || header.field.equiv("x-goog-api-key") {
                snapshot.x_api_key_present = true;
                if snapshot.x_api_key.is_none() {
                    let value = header.value.as_str().trim();
                    if !value.is_empty() {
                        snapshot.x_api_key = Some(value.to_string());
                    }
                }
                continue;
            }
            if snapshot.idempotency_key.is_none() && header.field.equiv("idempotency-key") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.idempotency_key = Some(value.to_string());
                }
                continue;
            }
            if snapshot.user_agent.is_none() && header.field.equiv("User-Agent") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.user_agent = Some(value.to_string());
                }
                continue;
            }
            if snapshot.originator.is_none() && header.field.equiv("originator") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.originator = Some(value.to_string());
                }
                continue;
            }
            if snapshot.session_id.is_none() && header.field.equiv("session_id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.session_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.session_affinity.is_none() && header.field.equiv("x-session-affinity") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.session_affinity = Some(value.to_string());
                }
                continue;
            }
            if snapshot.client_request_id.is_none() && header.field.equiv("x-client-request-id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.client_request_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.subagent.is_none() && header.field.equiv("x-openai-subagent") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.subagent = Some(value.to_string());
                }
                continue;
            }
            if snapshot.beta_features.is_none() && header.field.equiv("x-codex-beta-features") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.beta_features = Some(value.to_string());
                }
                continue;
            }
            if snapshot.window_id.is_none() && header.field.equiv("x-codex-window-id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.window_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.turn_metadata.is_none() && header.field.equiv("x-codex-turn-metadata") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.turn_metadata = Some(value.to_string());
                }
                continue;
            }
            if snapshot.turn_state.is_none() && header.field.equiv("x-codex-turn-state") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.turn_state = Some(value.to_string());
                }
                continue;
            }
            if snapshot.parent_thread_id.is_none() && header.field.equiv("x-codex-parent-thread-id")
            {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.parent_thread_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.codex_installation_id.is_none()
                && header.field.equiv("x-codex-installation-id")
            {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.codex_installation_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.responsesapi_include_timing_metrics.is_none()
                && header.field.equiv("x-responsesapi-include-timing-metrics")
            {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.responsesapi_include_timing_metrics = Some(value.to_string());
                }
                continue;
            }
            if snapshot.codex_inference_call_id.is_none()
                && header.field.equiv("x-codex-inference-call-id")
            {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.codex_inference_call_id = Some(value.to_string());
                }
                continue;
            }
            if snapshot.oai_attestation.is_none() && header.field.equiv("x-oai-attestation") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.oai_attestation = Some(value.to_string());
                }
                continue;
            }
            let header_name = header.field.to_string();
            if should_capture_passthrough_codex_header(header_name.as_str()) {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    remember_passthrough_header(
                        &mut snapshot.passthrough_codex_headers,
                        header_name.as_str(),
                        value,
                    );
                }
                continue;
            }
            if snapshot.conversation_id.is_none() && header.field.equiv("conversation_id") {
                let value = header.value.as_str().trim();
                if !value.is_empty() {
                    snapshot.conversation_id = Some(value.to_string());
                }
            }
        }
        snapshot.derive_session_id_from_turn_metadata();
        snapshot
    }

    fn derive_session_id_from_turn_metadata(&mut self) {
        if self.session_id.is_some() {
            return;
        }
        let Some(turn_metadata) = self.turn_metadata.as_deref() else {
            return;
        };
        self.session_id = session_id_from_turn_metadata(turn_metadata);
    }

    /// 函数 `platform_key`
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
    pub(crate) fn platform_key(&self) -> Option<&str> {
        self.x_api_key
            .as_deref()
            .or(self.authorization_bearer_strict.as_deref())
    }

    /// 函数 `sticky_key_material`
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
    pub(crate) fn sticky_key_material(&self) -> Option<&str> {
        self.session_id
            .as_deref()
            .or(self.x_api_key.as_deref())
            .or(self.authorization_bearer_case_insensitive.as_deref())
    }

    /// 函数 `user_agent`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-16
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }

    /// 函数 `originator`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-16
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn originator(&self) -> Option<&str> {
        self.originator.as_deref()
    }

    /// 函数 `has_authorization`
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
    pub(crate) fn has_authorization(&self) -> bool {
        self.authorization_present
    }

    /// 函数 `has_x_api_key`
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
    pub(crate) fn has_x_api_key(&self) -> bool {
        self.x_api_key_present
    }

    /// 函数 `session_id`
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
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 函数 `session_affinity`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-16
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn session_affinity(&self) -> Option<&str> {
        self.session_affinity.as_deref()
    }

    /// 函数 `client_request_id`
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
    pub(crate) fn client_request_id(&self) -> Option<&str> {
        self.client_request_id.as_deref()
    }

    /// 函数 `subagent`
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
    pub(crate) fn subagent(&self) -> Option<&str> {
        self.subagent.as_deref()
    }

    /// 函数 `beta_features`
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
    pub(crate) fn beta_features(&self) -> Option<&str> {
        self.beta_features.as_deref()
    }

    /// 函数 `window_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-11
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn window_id(&self) -> Option<&str> {
        self.window_id.as_deref()
    }

    /// 函数 `turn_metadata`
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
    pub(crate) fn turn_metadata(&self) -> Option<&str> {
        self.turn_metadata.as_deref()
    }

    /// 函数 `turn_state`
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
    pub(crate) fn turn_state(&self) -> Option<&str> {
        self.turn_state.as_deref()
    }

    /// 函数 `parent_thread_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-11
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn parent_thread_id(&self) -> Option<&str> {
        self.parent_thread_id.as_deref()
    }

    /// 函数 `codex_installation_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-05-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn codex_installation_id(&self) -> Option<&str> {
        self.codex_installation_id.as_deref()
    }

    /// 函数 `responsesapi_include_timing_metrics`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-05-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn responsesapi_include_timing_metrics(&self) -> Option<&str> {
        self.responsesapi_include_timing_metrics.as_deref()
    }

    pub(crate) fn codex_inference_call_id(&self) -> Option<&str> {
        self.codex_inference_call_id.as_deref()
    }

    pub(crate) fn oai_attestation(&self) -> Option<&str> {
        self.oai_attestation.as_deref()
    }

    pub(crate) fn idempotency_key(&self) -> Option<&str> {
        self.idempotency_key.as_deref()
    }

    /// 函数 `passthrough_codex_headers`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-11
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn passthrough_codex_headers(&self) -> &[(String, String)] {
        self.passthrough_codex_headers.as_slice()
    }

    /// 函数 `conversation_id`
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
    pub(crate) fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// 函数 `with_conversation_id_override`
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
    pub(crate) fn with_conversation_id_override(&self, conversation_id: Option<&str>) -> Self {
        self.with_thread_affinity_override(conversation_id, false)
    }

    /// 函数 `with_thread_affinity_override`
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
    pub(crate) fn with_thread_affinity_override(
        &self,
        conversation_id: Option<&str>,
        reset_session_affinity: bool,
    ) -> Self {
        let mut next = self.clone();
        next.conversation_id = conversation_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if reset_session_affinity {
            next.session_id = None;
            next.window_id = None;
            next.turn_state = None;
        }
        next
    }
}

fn should_capture_passthrough_codex_header(name: &str) -> bool {
    // 中文注释：Codex 上游只接受源码里明确构造的那组头；未知 x-codex-* 一律不做透传。
    // 这里保留入口只是为了让调用点语义清晰，但当前实现不允许任何额外透传头。
    let _ = name;
    false
}

fn remember_passthrough_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    if headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        return;
    }
    headers.push((name.to_string(), value.to_string()));
}

fn session_id_from_turn_metadata(turn_metadata: &str) -> Option<String> {
    let payload = serde_json::from_str::<serde_json::Value>(turn_metadata).ok()?;
    let session_id = payload.get("session_id")?.as_str()?.trim();
    if is_safe_derived_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn is_safe_derived_session_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 {
        return false;
    }
    value.bytes().all(|byte| matches!(byte, b'!'..=b'~'))
}

/// 函数 `strict_bearer_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn strict_bearer_token(value: &str) -> Option<String> {
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// 函数 `case_insensitive_bearer_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn case_insensitive_bearer_token(value: &str) -> Option<String> {
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

#[cfg(test)]
#[path = "tests/incoming_headers_tests.rs"]
mod tests;
