use serde_json::Value;
use std::time::Duration;

use super::super::response::GatewayStreamPrefetchTerminal;
use super::super::GatewayUpstreamResponse;

const STREAM_PREFLIGHT_MAX_BYTES: usize = 64 * 1024;
// Keep response headers below the default 15-second SSE keepalive window. A longer
// transparent failover window requires moving candidate coordination into the body.
const STREAM_PREFLIGHT_WALL_CLOCK_TIMEOUT: Duration = Duration::from_secs(10);
const USAGE_LIMIT_NOTICE_PREFIXES: [&str; 6] = [
    "you've hit your usage limit",
    "you have hit your usage limit",
    "the usage limit has been reached",
    "usage limit has been reached",
    "quota exceeded",
    "usage exhausted",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrefixDecision {
    NeedMore,
    Deliver,
    Failover(String),
    RetryUsageNotice(String),
}

pub(in super::super) enum StreamPreflightOutcome {
    Ready(GatewayUpstreamResponse),
    Failover(String),
    StatusFailover { status_code: u16, message: String },
    RetryUsageNotice(String),
    TransportFailover(String),
}

fn should_prefetch_actionable_error_body(status_code: u16) -> bool {
    matches!(status_code, 401 | 403 | 429)
}

fn is_actionable_gateway_error(message: &str) -> bool {
    crate::account_status::usage_limit_reason_from_message(message).is_some()
        || crate::account_status::deactivation_reason_from_message(message).is_some()
}

fn actionable_message_from_error_value(error: &Value) -> Option<String> {
    if let Some(message) = error.as_str() {
        return is_actionable_gateway_error(message).then(|| message.to_string());
    }

    let error = error.as_object()?;
    ["message", "code", "type"]
        .into_iter()
        .filter_map(|key| error.get(key).and_then(Value::as_str))
        .find(|message| is_actionable_gateway_error(message))
        .map(str::to_string)
}

fn actionable_message_from_explicit_error(value: &Value) -> Option<String> {
    let top_level_error = value.get("error");
    let response = value.get("response");
    let response_error = response.and_then(|response| response.get("error"));
    let status_details_error = response
        .and_then(|response| response.get("status_details"))
        .and_then(|details| details.get("error"));

    [top_level_error, response_error, status_details_error]
        .into_iter()
        .flatten()
        .find_map(actionable_message_from_error_value)
}

fn is_error_event(event_type: &str) -> bool {
    matches!(
        event_type.trim().to_ascii_lowercase().as_str(),
        "error" | "response.failed" | "response.incomplete"
    )
}

fn actionable_message_from_error_event(value: &Value) -> Option<String> {
    actionable_message_from_explicit_error(value).or_else(|| {
        ["message", "code", "type"]
            .into_iter()
            .filter_map(|key| value.get(key).and_then(Value::as_str))
            .find(|message| is_actionable_gateway_error(message))
            .map(str::to_string)
    })
}

fn actionable_message_from_error_body(body: &[u8]) -> Option<String> {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    if let Some(message) = parsed
        .as_ref()
        .and_then(actionable_message_from_error_event)
    {
        return Some(message);
    }

    let text = std::str::from_utf8(body).ok()?.trim();
    (!text.is_empty() && is_actionable_gateway_error(text)).then(|| text.to_string())
}

fn summarize_non_200_status_failover(status_code: u16, body: Option<&[u8]>) -> String {
    let body_hint = body
        .and_then(|body| crate::gateway::summarize_upstream_error_hint_from_body(status_code, body))
        .map(|hint| format!(" body={hint}"))
        .unwrap_or_default();
    format!("upstream non-200 status={status_code}{body_hint}")
}

fn is_strong_usage_limit_delta(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    let looks_like_notice = USAGE_LIMIT_NOTICE_PREFIXES.iter().any(|prefix| {
        normalized.strip_prefix(prefix).is_some_and(|suffix| {
            suffix.is_empty()
                || suffix
                    .chars()
                    .next()
                    .is_some_and(|ch| matches!(ch, '.' | '!' | ':' | ',' | '\n' | '\r'))
        })
    }) || matches!(
        normalized.as_str(),
        "usage_limit_reached"
            | "usage_limit_exceeded"
            | "usage_limit_exhausted"
            | "insufficient_quota"
    );

    looks_like_notice && crate::account_status::usage_limit_reason_from_message(message).is_some()
}

fn is_possible_usage_limit_delta_prefix(message: &str) -> bool {
    let normalized = message.trim_start().to_ascii_lowercase();
    !normalized.is_empty()
        && USAGE_LIMIT_NOTICE_PREFIXES
            .iter()
            .chain(
                [
                    "usage_limit_reached",
                    "usage_limit_exceeded",
                    "usage_limit_exhausted",
                    "insufficient_quota",
                ]
                .iter(),
            )
            .any(|notice| notice.starts_with(normalized.as_str()))
}

fn normalized_frames(prefix: &[u8], include_incomplete_frame: bool) -> (Vec<String>, bool) {
    let normalized = String::from_utf8_lossy(prefix).replace("\r\n", "\n");
    let has_incomplete_trailing_frame = include_incomplete_frame && !normalized.ends_with("\n\n");
    let mut parts = normalized.split("\n\n").collect::<Vec<_>>();
    if !include_incomplete_frame && !normalized.ends_with("\n\n") {
        let _ = parts.pop();
    }
    (
        parts.into_iter().map(str::to_string).collect(),
        has_incomplete_trailing_frame,
    )
}

fn frame_event_and_data(frame: &str) -> (Option<String>, Option<String>) {
    let mut event_type = None;
    let mut data = String::new();
    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    let data = (!data.is_empty()).then_some(data);
    (event_type, data)
}

fn is_metadata_only_event(event_type: &str) -> bool {
    matches!(
        event_type.trim().to_ascii_lowercase().as_str(),
        "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added"
            | "ping"
    )
}

fn is_usage_notice_followup_event(event_type: &str) -> bool {
    matches!(
        event_type.trim().to_ascii_lowercase().as_str(),
        "response.output_text.done" | "response.output_item.done" | "response.content_part.done"
    )
}

fn is_usage_notice_terminal_event(event_type: &str) -> bool {
    matches!(
        event_type.trim().to_ascii_lowercase().as_str(),
        "error" | "response.failed" | "response.incomplete"
    )
}

fn is_sse_stream_response(response: &GatewayUpstreamResponse) -> bool {
    matches!(response, GatewayUpstreamResponse::Stream(_))
        && response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

fn classify_prefix(prefix: &[u8], include_incomplete_frame: bool) -> PrefixDecision {
    let (frames, has_incomplete_trailing_frame) =
        normalized_frames(prefix, include_incomplete_frame);
    if frames.is_empty() {
        return PrefixDecision::NeedMore;
    }

    let frame_count = frames.len();
    let mut pending_usage_notice = None;
    let mut usage_notice_candidate = String::new();
    for (index, frame) in frames.into_iter().enumerate() {
        let (declared_event_type, data) = frame_event_and_data(frame.as_str());
        let Some(data) = data else {
            continue;
        };
        if data.trim() == "[DONE]" {
            return pending_usage_notice
                .map(PrefixDecision::RetryUsageNotice)
                .unwrap_or(PrefixDecision::Deliver);
        }

        let parsed = serde_json::from_str::<Value>(data.as_str()).ok();
        let is_incomplete_trailing_frame =
            has_incomplete_trailing_frame && index + 1 == frame_count;
        if is_incomplete_trailing_frame
            && parsed.is_none()
            && data
                .trim_start()
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(*byte, b'{' | b'['))
        {
            return PrefixDecision::NeedMore;
        }
        let payload_event_type = parsed
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let event_type = payload_event_type.clone().or(declared_event_type.clone());

        if let Some(message) = parsed
            .as_ref()
            .and_then(actionable_message_from_explicit_error)
        {
            return PrefixDecision::Failover(message);
        }

        let has_error_event = declared_event_type.as_deref().is_some_and(is_error_event)
            || payload_event_type.as_deref().is_some_and(is_error_event);
        if has_error_event {
            let message = match parsed.as_ref() {
                Some(value) => actionable_message_from_error_event(value),
                None => is_actionable_gateway_error(data.as_str()).then(|| data.clone()),
            };
            if let Some(message) = message {
                return PrefixDecision::Failover(message);
            }
        }

        if event_type.as_deref() == Some("response.output_text.delta") {
            if let Some(message) = parsed
                .as_ref()
                .and_then(|value| value.get("delta"))
                .and_then(Value::as_str)
            {
                usage_notice_candidate.push_str(message);
                if is_strong_usage_limit_delta(usage_notice_candidate.as_str()) {
                    pending_usage_notice = Some(usage_notice_candidate.clone());
                    continue;
                }
                if pending_usage_notice.is_none()
                    && is_possible_usage_limit_delta_prefix(usage_notice_candidate.as_str())
                {
                    continue;
                }
            }
        }

        match event_type.as_deref() {
            Some(event_type) if is_metadata_only_event(event_type) => {}
            Some(event_type)
                if pending_usage_notice.is_some() && is_usage_notice_terminal_event(event_type) =>
            {
                return PrefixDecision::RetryUsageNotice(
                    pending_usage_notice.expect("pending usage notice checked above"),
                );
            }
            Some(event_type)
                if pending_usage_notice.is_some() && is_usage_notice_followup_event(event_type) => {
            }
            // Any non-metadata event means the upstream has begun producing a real response.
            // At that point retrying another account could duplicate visible output/tool work.
            Some(_) | None => return PrefixDecision::Deliver,
        }
    }

    if include_incomplete_frame {
        if let Some(message) = pending_usage_notice {
            return PrefixDecision::RetryUsageNotice(message);
        }
    }
    PrefixDecision::NeedMore
}

pub(in super::super) async fn preflight_stream_response(
    response: GatewayUpstreamResponse,
    request_path: &str,
    upstream_is_stream: bool,
    has_more_candidates: bool,
) -> StreamPreflightOutcome {
    preflight_stream_response_with_idle_timeout(
        response,
        request_path,
        upstream_is_stream,
        has_more_candidates,
        crate::gateway::upstream_stream_timeout(),
    )
    .await
}

async fn preflight_stream_response_with_idle_timeout(
    response: GatewayUpstreamResponse,
    request_path: &str,
    upstream_is_stream: bool,
    has_more_candidates: bool,
    idle_timeout: Option<std::time::Duration>,
) -> StreamPreflightOutcome {
    preflight_stream_response_with_timeouts(
        response,
        request_path,
        upstream_is_stream,
        has_more_candidates,
        idle_timeout,
        Some(STREAM_PREFLIGHT_WALL_CLOCK_TIMEOUT),
    )
    .await
}

async fn preflight_stream_response_with_timeouts(
    response: GatewayUpstreamResponse,
    request_path: &str,
    upstream_is_stream: bool,
    has_more_candidates: bool,
    idle_timeout: Option<Duration>,
    wall_clock_timeout: Option<Duration>,
) -> StreamPreflightOutcome {
    let status_code = response.status().as_u16();
    if has_more_candidates && !(200..=299).contains(&status_code) {
        if should_prefetch_actionable_error_body(status_code) {
            return match response.into_buffered_async().await {
                Ok((body, _response)) => actionable_message_from_error_body(body.as_ref())
                    .map(StreamPreflightOutcome::Failover)
                    .unwrap_or_else(|| StreamPreflightOutcome::StatusFailover {
                        status_code,
                        message: summarize_non_200_status_failover(
                            status_code,
                            Some(body.as_ref()),
                        ),
                    }),
                Err(err) => StreamPreflightOutcome::StatusFailover {
                    status_code,
                    message: format!(
                        "upstream non-200 status={status_code}; read response body failed: {err}"
                    ),
                },
            };
        }
        return StreamPreflightOutcome::StatusFailover {
            status_code,
            message: summarize_non_200_status_failover(status_code, None),
        };
    }

    if !upstream_is_stream
        || !has_more_candidates
        || !request_path.starts_with("/v1/responses")
        || status_code >= 400
        || !is_sse_stream_response(&response)
    {
        return StreamPreflightOutcome::Ready(response);
    }

    let (prefix, response, terminal) = response
        .prefetch_stream_prefix_async(
            STREAM_PREFLIGHT_MAX_BYTES,
            idle_timeout,
            wall_clock_timeout,
            |prefix| !matches!(classify_prefix(prefix, false), PrefixDecision::NeedMore),
        )
        .await;
    let include_incomplete_frame = matches!(
        terminal,
        GatewayStreamPrefetchTerminal::Eof
            | GatewayStreamPrefetchTerminal::Error(_)
            | GatewayStreamPrefetchTerminal::Disconnected
    );
    match classify_prefix(prefix.as_ref(), include_incomplete_frame) {
        PrefixDecision::Failover(message) => StreamPreflightOutcome::Failover(message),
        PrefixDecision::RetryUsageNotice(message) => {
            StreamPreflightOutcome::RetryUsageNotice(message)
        }
        PrefixDecision::Deliver => StreamPreflightOutcome::Ready(response),
        PrefixDecision::NeedMore => match terminal {
            GatewayStreamPrefetchTerminal::Open => StreamPreflightOutcome::TransportFailover(
                "upstream response stream preflight stopped before producing deliverable content"
                    .to_string(),
            ),
            GatewayStreamPrefetchTerminal::PrefixLimit => {
                // Large response.created metadata can legitimately exceed the
                // classification buffer (for example with a large tools list).
                // Commit and replay it instead of exhausting every account with
                // the same request-shaped prefix.
                StreamPreflightOutcome::Ready(response)
            }
            GatewayStreamPrefetchTerminal::IdleTimeout => {
                StreamPreflightOutcome::TransportFailover(
                    "upstream response stream idle timeout before producing deliverable content"
                        .to_string(),
                )
            }
            GatewayStreamPrefetchTerminal::WallClockTimeout => {
                // A normal response may have a slow first semantic event. Commit the
                // buffered stream instead of treating the account as unhealthy and
                // delaying downstream response headers for the full idle timeout.
                StreamPreflightOutcome::Ready(response)
            }
            GatewayStreamPrefetchTerminal::Eof => StreamPreflightOutcome::TransportFailover(
                "upstream response stream ended before producing deliverable content".to_string(),
            ),
            GatewayStreamPrefetchTerminal::Error(err) => {
                StreamPreflightOutcome::TransportFailover(format!(
                    "upstream response stream failed before producing deliverable content: {err}"
                ))
            }
            GatewayStreamPrefetchTerminal::Disconnected => {
                StreamPreflightOutcome::TransportFailover(
                    "upstream response stream disconnected before producing deliverable content"
                        .to_string(),
                )
            }
        },
    }
}

#[cfg(test)]
#[path = "stream_preflight_tests.rs"]
mod tests;
