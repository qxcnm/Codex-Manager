use bytes::Bytes;
use codexmanager_core::storage::Account;
use std::collections::HashMap;

use super::super::support::payload_rewrite::strip_encrypted_content_from_body;
use super::request_setup::UpstreamRequestSetup;

#[derive(Default)]
pub(in super::super) struct CandidateExecutionState {
    stripped_body: Option<Bytes>,
    rewritten_bodies: HashMap<String, Bytes>,
    stripped_rewritten_bodies: HashMap<String, Bytes>,
    first_candidate_account_scope: Option<String>,
}

impl CandidateExecutionState {
    fn existing_prompt_cache_key(body: &Bytes) -> Option<String> {
        serde_json::from_slice::<serde_json::Value>(body.as_ref())
            .ok()
            .and_then(|value| {
                value
                    .get("prompt_cache_key")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            })
    }

    /// 函数 `rewrite_cache_key`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - model_override: 参数 model_override
    /// - prompt_cache_key: 参数 prompt_cache_key
    ///
    /// # 返回
    /// 返回函数执行结果
    fn rewrite_cache_key(
        model_override: Option<&str>,
        prompt_cache_key: Option<&str>,
    ) -> Option<String> {
        let normalized_model = model_override
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let normalized_prompt_cache_key = prompt_cache_key
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if normalized_model.is_none() && normalized_prompt_cache_key.is_none() {
            return None;
        }
        Some(format!(
            "model={}|thread={}",
            normalized_model.unwrap_or("-"),
            normalized_prompt_cache_key.unwrap_or("-")
        ))
    }

    /// 函数 `strip_session_affinity`
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
    pub(in super::super) fn strip_session_affinity(
        &mut self,
        account: &Account,
        idx: usize,
        anthropic_has_thread_anchor: bool,
    ) -> bool {
        if !anthropic_has_thread_anchor {
            return idx > 0;
        }
        let candidate_scope = account
            .chatgpt_account_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .or_else(|| {
                account
                    .workspace_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            });
        if idx == 0 {
            self.first_candidate_account_scope = candidate_scope.clone();
            false
        } else {
            candidate_scope != self.first_candidate_account_scope
        }
    }

    /// 函数 `rewrite_body_for_model`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - path: 参数 path
    /// - body: 参数 body
    /// - setup: 参数 setup
    /// - model_override: 参数 model_override
    /// - prompt_cache_key: 参数 prompt_cache_key
    ///
    /// # 返回
    /// 返回函数执行结果
    fn rewrite_body_for_model(
        &mut self,
        path: &str,
        body: &Bytes,
        setup: &UpstreamRequestSetup,
        model_override: Option<&str>,
        prompt_cache_key: Option<&str>,
    ) -> Bytes {
        let existing_prompt_cache_key = Self::existing_prompt_cache_key(body);
        let effective_prompt_cache_key = existing_prompt_cache_key.as_deref().or(prompt_cache_key);
        let Some(cache_key) = Self::rewrite_cache_key(model_override, effective_prompt_cache_key)
        else {
            return body.clone();
        };

        self.rewritten_bodies
            .entry(cache_key)
            .or_insert_with(|| {
                let has_local_thread_anchor = setup.has_sticky_fallback_session
                    || setup.has_sticky_fallback_conversation
                    || setup.conversation_routing.is_some();
                let should_force_prompt_cache_key =
                    effective_prompt_cache_key.is_some() && has_local_thread_anchor;
                let prompt_cache_key_for_rewrite = if should_force_prompt_cache_key {
                    effective_prompt_cache_key
                } else {
                    None
                };
                let rewritten = if should_force_prompt_cache_key {
                    super::super::super::apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
                        path,
                        body.to_vec(),
                        model_override,
                        None,
                        None,
                        Some(setup.upstream_base.as_str()),
                        prompt_cache_key_for_rewrite,
                        false,
                    )
                } else {
                    super::super::super::apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
                        path,
                        body.to_vec(),
                        model_override,
                        None,
                        None,
                        Some(setup.upstream_base.as_str()),
                        prompt_cache_key_for_rewrite,
                        false,
                    )
                };
                Bytes::from(rewritten)
            })
            .clone()
    }

    /// 函数 `body_for_attempt`
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
    pub(in super::super) fn body_for_attempt(
        &mut self,
        path: &str,
        body: &Bytes,
        strip_session_affinity: bool,
        setup: &UpstreamRequestSetup,
        model_override: Option<&str>,
        prompt_cache_key: Option<&str>,
    ) -> Bytes {
        let rewritten =
            self.rewrite_body_for_model(path, body, setup, model_override, prompt_cache_key);
        if strip_session_affinity && setup.has_body_encrypted_content {
            if let Some(cache_key) = Self::rewrite_cache_key(model_override, prompt_cache_key) {
                return self
                    .stripped_rewritten_bodies
                    .entry(cache_key)
                    .or_insert_with(|| {
                        strip_encrypted_content_from_body(rewritten.as_ref())
                            .map(Bytes::from)
                            .unwrap_or_else(|| rewritten.clone())
                    })
                    .clone();
            }
            if self.stripped_body.is_none() {
                self.stripped_body = strip_encrypted_content_from_body(rewritten.as_ref())
                    .map(Bytes::from)
                    .or_else(|| Some(rewritten.clone()));
            }
            self.stripped_body
                .as_ref()
                .expect("stripped body should be initialized")
                .clone()
        } else {
            rewritten
        }
    }

    /// 函数 `retry_body`
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
    pub(in super::super) fn retry_body(
        &mut self,
        path: &str,
        body: &Bytes,
        setup: &UpstreamRequestSetup,
        model_override: Option<&str>,
        prompt_cache_key: Option<&str>,
    ) -> Bytes {
        let rewritten =
            self.rewrite_body_for_model(path, body, setup, model_override, prompt_cache_key);
        if setup.has_body_encrypted_content {
            if let Some(cache_key) = Self::rewrite_cache_key(model_override, prompt_cache_key) {
                return self
                    .stripped_rewritten_bodies
                    .entry(cache_key)
                    .or_insert_with(|| {
                        strip_encrypted_content_from_body(rewritten.as_ref())
                            .map(Bytes::from)
                            .unwrap_or_else(|| rewritten.clone())
                    })
                    .clone();
            }
            if self.stripped_body.is_none() {
                self.stripped_body = strip_encrypted_content_from_body(rewritten.as_ref())
                    .map(Bytes::from)
                    .or_else(|| Some(rewritten.clone()));
            }
            self.stripped_body
                .as_ref()
                .expect("stripped body should be initialized")
                .clone()
        } else {
            rewritten
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CandidateExecutionState;
    use bytes::Bytes;
    use codexmanager_core::storage::Account;

    fn sample_setup() -> super::super::request_setup::UpstreamRequestSetup {
        super::super::request_setup::UpstreamRequestSetup {
            upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
            upstream_fallback_base: None,
            url: "https://chatgpt.com/backend-api/codex/responses".to_string(),
            url_alt: None,
            candidate_count: 1,
            account_max_inflight: 1,
            anthropic_has_thread_anchor: false,
            has_sticky_fallback_session: false,
            has_sticky_fallback_conversation: false,
            has_body_encrypted_content: false,
            conversation_routing: None,
        }
    }

    /// 函数 `body_for_attempt_rewrites_model_override`
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
    fn body_for_attempt_rewrites_model_override() {
        let mut state = CandidateExecutionState::default();
        let body = Bytes::from_static(br#"{"model":"gpt-5.4","input":"hello"}"#);
        let setup = sample_setup();

        let actual = state.body_for_attempt(
            "/v1/responses",
            &body,
            false,
            &setup,
            Some("gpt-5.2"),
            Some("thread-2"),
        );
        let value: serde_json::Value =
            serde_json::from_slice(actual.as_ref()).expect("parse rewritten body");

        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-5.2")
        );
        assert_eq!(
            value
                .get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            None
        );
    }

    #[test]
    fn body_for_attempt_keeps_native_codex_retry_shape() {
        let _guard = crate::test_env_guard();
        let mut state = CandidateExecutionState::default();
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.4","input":"hello","stream":false,"store":true}"#,
        );
        let setup = sample_setup();

        let actual = state.body_for_attempt(
            "/v1/responses",
            &body,
            false,
            &setup,
            Some("gpt-5.2"),
            Some("thread-2"),
        );
        let value: serde_json::Value =
            serde_json::from_slice(actual.as_ref()).expect("parse rewritten body");

        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-5.2")
        );
        assert_eq!(
            value.get("stream").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            value.get("store").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(value.get("prompt_cache_key").is_none());
        assert!(value.get("instructions").is_none());
        assert!(value.get("tool_choice").is_none());
        assert!(value.get("include").is_none());
    }

    #[test]
    fn body_for_attempt_injects_local_thread_anchor_without_compat_shape() {
        let mut state = CandidateExecutionState::default();
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.4","input":"hello","stream":false,"store":true}"#,
        );
        let mut setup = sample_setup();
        setup.has_sticky_fallback_conversation = true;

        let actual = state.body_for_attempt(
            "/v1/responses",
            &body,
            false,
            &setup,
            Some("gpt-5.2"),
            Some("thread-2"),
        );
        let value: serde_json::Value =
            serde_json::from_slice(actual.as_ref()).expect("parse rewritten body");

        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-5.2")
        );
        assert_eq!(
            value
                .get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            Some("thread-2")
        );
        assert_eq!(
            value.get("stream").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            value.get("store").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(value.get("instructions").is_none());
        assert!(value.get("tool_choice").is_none());
        assert!(value.get("include").is_none());
    }

    #[test]
    fn body_for_attempt_preserves_existing_prompt_cache_key() {
        let mut state = CandidateExecutionState::default();
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.4","input":"hello","prompt_cache_key":"client-thread"}"#,
        );
        let setup = sample_setup();

        let actual = state.body_for_attempt(
            "/v1/responses",
            &body,
            false,
            &setup,
            None,
            Some("thread-from-conversation"),
        );
        let value: serde_json::Value =
            serde_json::from_slice(actual.as_ref()).expect("parse rewritten body");

        assert_eq!(
            value
                .get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            Some("client-thread")
        );
    }

    #[test]
    fn compact_body_for_attempt_preserves_existing_prompt_cache_key() {
        let mut state = CandidateExecutionState::default();
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.5","input":"hello","prompt_cache_key":"client-thread"}"#,
        );
        let mut setup = sample_setup();
        setup.has_sticky_fallback_conversation = true;

        let actual = state.body_for_attempt(
            "/v1/responses/compact",
            &body,
            true,
            &setup,
            None,
            Some("thread-from-conversation"),
        );
        let value: serde_json::Value =
            serde_json::from_slice(actual.as_ref()).expect("parse rewritten body");

        assert_eq!(
            value
                .get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            Some("client-thread")
        );
    }

    #[test]
    fn strip_session_affinity_preserves_same_workspace_when_thread_anchor_exists() {
        let mut state = CandidateExecutionState::default();
        let first = Account {
            id: "acc-1".to_string(),
            label: "acc-1".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws-same".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        let second = Account {
            id: "acc-2".to_string(),
            label: "acc-2".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws-same".to_string()),
            group_name: None,
            sort: 2,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };

        assert!(!state.strip_session_affinity(&first, 0, true));
        assert!(!state.strip_session_affinity(&second, 1, true));
    }

    #[test]
    fn strip_session_affinity_strips_cross_workspace_when_thread_anchor_exists() {
        let mut state = CandidateExecutionState::default();
        let first = Account {
            id: "acc-1".to_string(),
            label: "acc-1".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws-a".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        let second = Account {
            id: "acc-2".to_string(),
            label: "acc-2".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws-b".to_string()),
            group_name: None,
            sort: 2,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };

        assert!(!state.strip_session_affinity(&first, 0, true));
        assert!(state.strip_session_affinity(&second, 1, true));
    }
}
