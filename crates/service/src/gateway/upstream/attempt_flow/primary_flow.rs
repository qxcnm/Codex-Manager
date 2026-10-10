use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::CONTENT_TYPE;
use std::time::Instant;

use super::super::GatewayUpstreamResponse;
use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
use super::primary_attempt::{run_primary_upstream_attempt, PrimaryAttemptResult};
use super::transport::UpstreamRequestContext;

pub(in crate::gateway::upstream) enum PrimaryFlowDecision {
    Continue {
        upstream: GatewayUpstreamResponse,
        authorization: PrimaryAuthorization,
    },
    RespondUpstream(GatewayUpstreamResponse),
    Failover,
    Terminal {
        status_code: u16,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(in crate::gateway::upstream) struct PrimaryAuthorization {
    pub(in crate::gateway::upstream) value: String,
    pub(in crate::gateway::upstream) task_id: Option<String>,
    pub(in crate::gateway::upstream) uses_agent_identity: bool,
    pub(in crate::gateway::upstream) is_fedramp: bool,
    pub(in crate::gateway::upstream) account_scope_id: Option<String>,
}

/// 函数 `resolve_chatgpt_primary_bearer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn resolve_chatgpt_primary_bearer(token: &Token) -> Option<String> {
    let access = token.access_token.trim();
    if access.is_empty() {
        None
    } else {
        Some(access.to_string())
    }
}

async fn resolve_chatgpt_primary_authorization(
    storage: &Storage,
    client: &reqwest::Client,
    account: &Account,
    token: &Token,
) -> Result<(PrimaryAuthorization, &'static str), String> {
    match crate::agent_identity::resolve_or_bootstrap_account_agent_identity_authorization_async(
        storage, client, account, token,
    )
    .await
    {
        Ok(Some(resolved)) => {
            return Ok((
                PrimaryAuthorization {
                    value: resolved.value,
                    task_id: Some(resolved.task_id),
                    uses_agent_identity: true,
                    is_fedramp: resolved.is_fedramp,
                    account_scope_id: resolved.account_scope_id,
                },
                "agent_identity",
            ));
        }
        Ok(None) => {}
        Err(err) => {
            if token.access_token.trim().is_empty() {
                return Err(err);
            }
            log::warn!(
                "event=gateway_agent_identity_resolution_failed account_id={} error={}",
                account.id,
                err
            );
        }
    }
    resolve_chatgpt_primary_bearer(token)
        .map(|access_token| {
            (
                PrimaryAuthorization {
                    value: access_token,
                    task_id: None,
                    uses_agent_identity: false,
                    is_fedramp: false,
                    account_scope_id: None,
                },
                "access_token",
            )
        })
        .ok_or_else(|| "missing chatgpt access token".to_string())
}

/// 函数 `run_primary_upstream_flow`
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
#[allow(clippy::too_many_arguments)]
pub(in crate::gateway::upstream) async fn run_primary_upstream_flow<F>(
    client: &reqwest::Client,
    storage: &Storage,
    method: &reqwest::Method,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    base: &str,
    path: &str,
    primary_url: &str,
    request_deadline: Option<Instant>,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let (authorization, token_source) =
        match resolve_chatgpt_primary_authorization(storage, client, account, token).await {
            Ok(resolved) => resolved,
            Err(err) => {
                log_gateway_result(Some(primary_url), 401, Some(err.as_str()));
                return PrimaryFlowDecision::Terminal {
                    status_code: 401,
                    message: err,
                };
            }
        };
    if debug {
        log::debug!(
            "event=gateway_upstream_token_source path={} account_id={} token_source={} upstream_base={}",
            path,
            account.id,
            token_source,
            base,
        );
    }

    let upstream = match run_primary_upstream_attempt(
        client,
        method,
        primary_url,
        request_deadline,
        request_ctx.with_fedramp(authorization.is_fedramp),
        incoming_headers,
        body,
        is_stream,
        authorization.value.as_str(),
        &account_with_authorization_scope(account, &authorization),
        strip_session_affinity,
        has_more_candidates,
        &mut log_gateway_result,
    )
    .await
    {
        PrimaryAttemptResult::Upstream(resp) => resp,
        PrimaryAttemptResult::Failover => return PrimaryFlowDecision::Failover,
        PrimaryAttemptResult::Terminal {
            status_code,
            message,
        } => {
            return PrimaryFlowDecision::Terminal {
                status_code,
                message,
            };
        }
    };

    let status = upstream.status();
    match handle_openai_fallback_branch(
        client,
        storage,
        method,
        incoming_headers,
        body,
        is_stream,
        base,
        path,
        upstream_fallback_base,
        account,
        token,
        strip_session_affinity,
        debug,
        allow_openai_fallback && !authorization.uses_agent_identity,
        status,
        upstream.headers().get(CONTENT_TYPE),
        has_more_candidates,
        request_ctx.capture,
        &mut log_gateway_result,
    )
    .await
    {
        FallbackBranchResult::NotTriggered => PrimaryFlowDecision::Continue {
            upstream,
            authorization,
        },
        FallbackBranchResult::RespondUpstream(resp) => PrimaryFlowDecision::RespondUpstream(resp),
        FallbackBranchResult::Failover => PrimaryFlowDecision::Failover,
        FallbackBranchResult::Terminal {
            status_code,
            message,
        } => PrimaryFlowDecision::Terminal {
            status_code,
            message,
        },
    }
}

pub(super) fn account_with_authorization_scope(
    account: &Account,
    authorization: &PrimaryAuthorization,
) -> Account {
    let mut account = account.clone();
    if let Some(scope) = authorization
        .account_scope_id
        .as_deref()
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
    {
        account.chatgpt_account_id = Some(scope.to_string());
        account.workspace_id = Some(scope.to_string());
    }
    account
}

#[cfg(test)]
#[path = "../tests/attempt_flow/primary_flow_tests.rs"]
mod tests;
