use serde_json::Value;

#[cfg(test)]
use super::openai_responses_event::OpenAIResponsesEvent;
use super::output_text;
use output_text::{
    append_output_text, collect_response_output_text, extract_error_message_from_json,
    parse_usage_from_json, UpstreamResponseUsage,
};

/// 函数 `parse_usage_from_sse_frame`
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
#[cfg(test)]
pub(in super::super) fn parse_usage_from_sse_frame(
    lines: &[String],
) -> Option<UpstreamResponseUsage> {
    let mut data_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return None;
    }
    let value = serde_json::from_str::<Value>(&data).ok()?;
    let mut usage = parse_usage_from_json(&value);
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        let mut text_out = String::new();
        for choice in choices {
            if let Some(delta) = choice
                .get("delta")
                .and_then(Value::as_object)
                .and_then(|delta| delta.get("content"))
            {
                collect_response_output_text(delta, &mut text_out);
            }
        }
        if !text_out.trim().is_empty() {
            let target = usage.output_text.get_or_insert_with(String::new);
            append_output_text(target, text_out.as_str());
        }
        return Some(usage);
    }
    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
        if !delta.is_empty() {
            let target = usage.output_text.get_or_insert_with(String::new);
            append_output_text(target, delta);
        }
        return Some(usage);
    }
    Some(usage)
}

#[derive(Debug, Clone)]
pub(in super::super) enum SseTerminal {
    Ok,
    Err(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PassthroughSseProtocol {
    #[default]
    Generic,
    AnthropicNative,
}

#[derive(Debug, Clone, Default)]
pub(in super::super) struct SseFrameInspection {
    pub saw_data: bool,
    pub usage: Option<UpstreamResponseUsage>,
    pub terminal: Option<SseTerminal>,
    pub last_event_type: Option<String>,
}

/// 函数 `classify_terminal_event_name`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn classify_terminal_event_name(
    name: &str,
    protocol: PassthroughSseProtocol,
) -> Option<SseTerminal> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if protocol == PassthroughSseProtocol::AnthropicNative && normalized == "message_stop" {
        return Some(SseTerminal::Ok);
    }
    if normalized == "done" || is_response_completed_event_name(normalized.as_str()) {
        return Some(SseTerminal::Ok);
    }
    if normalized == "error"
        || normalized == "response.failed"
        || normalized == "response.error"
        || normalized == "response.canceled"
        || normalized == "response.cancelled"
        || normalized == "response.incomplete"
    {
        return Some(SseTerminal::Err(normalized));
    }
    None
}

/// 函数 `is_response_completed_event_name`
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
pub(in super::super) fn is_response_completed_event_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "response.completed" || normalized == "response.done"
}

/// 函数 `is_chat_completion_terminal_chunk`
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
fn is_chat_completion_terminal_chunk(value: &Value) -> bool {
    if value.get("object").and_then(Value::as_str) != Some("chat.completion.chunk") {
        return false;
    }
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("finish_reason")
                    .is_some_and(|finish_reason| !finish_reason.is_null())
            })
        })
}

/// 函数 `inspect_sse_frame`
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
pub(in super::super) fn inspect_sse_frame_for_protocol(
    lines: &[String],
    protocol: PassthroughSseProtocol,
) -> SseFrameInspection {
    let mut inspection = SseFrameInspection::default();
    let mut data_lines = Vec::new();
    let mut event_name: Option<String> = None;

    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("event:") {
            if event_name.is_none() {
                let v = rest.trim();
                if !v.is_empty() {
                    event_name = Some(v.to_string());
                }
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            inspection.saw_data = true;
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if let Some(name) = event_name.as_deref() {
        inspection.last_event_type = Some(name.to_string());
    }

    if data_lines.is_empty() {
        return inspection;
    }
    let data = data_lines.join("\n");
    if data.trim().is_empty() {
        return inspection;
    }
    // Failure headers can still explain a dispatched but malformed error payload.
    // A success header alone must never mark an empty/invalid event as completed.
    inspection.terminal = event_name
        .as_deref()
        .and_then(|kind| classify_terminal_event_name(kind, protocol))
        .filter(|terminal| matches!(terminal, SseTerminal::Err(_)));

    if data.trim() == "[DONE]" {
        inspection.terminal = Some(SseTerminal::Ok);
        inspection.last_event_type = Some("[DONE]".to_string());
        return inspection;
    }

    if let Ok(value) = serde_json::from_str::<Value>(&data) {
        if inspection.last_event_type.is_none() {
            inspection.last_event_type = value
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|kind| !kind.is_empty())
                .map(str::to_string);
        }
        let kind = event_name
            .as_deref()
            .filter(|kind| !kind.eq_ignore_ascii_case("message"))
            .or_else(|| value.get("type").and_then(Value::as_str));
        let payload_failure = value
            .get("type")
            .and_then(Value::as_str)
            .and_then(|kind| classify_terminal_event_name(kind, protocol))
            .filter(|terminal| matches!(terminal, SseTerminal::Err(_)));
        let terminal = payload_failure
            .or_else(|| kind.and_then(|kind| classify_terminal_event_name(kind, protocol)));
        // A tool/MCP item can fail while the response continues. Its nested error
        // is not a response terminal; errors/statuses inside a response envelope
        // and top-level lifecycle errors remain authoritative even on completion.
        let response_envelope_error = value.get("response").and_then(|response| {
            extract_error_message_from_json(response).or_else(|| {
                response
                    .get("status_details")
                    .and_then(extract_error_message_from_json)
            })
        });
        let failed_status = value
            .pointer("/response/status")
            .and_then(Value::as_str)
            .map(|status| status.trim().to_ascii_lowercase())
            .filter(|status| {
                matches!(
                    status.as_str(),
                    "failed" | "incomplete" | "canceled" | "cancelled"
                )
            });
        let lifecycle = kind.is_some_and(|kind| {
            matches!(
                kind.trim().to_ascii_lowercase().as_str(),
                "response.created"
                    | "response.queued"
                    | "response.in_progress"
                    | "response.completed"
                    | "response.done"
            )
        });
        let response_error =
            kind.is_none() || lifecycle || matches!(&terminal, Some(SseTerminal::Err(_)));
        let message = response_envelope_error.or_else(|| {
            response_error
                .then(|| extract_error_message_from_json(&value))
                .flatten()
        });
        if let Some(message) = message.or_else(|| {
            failed_status.map(|status| format!("上游响应未成功完成（status={status}）"))
        }) {
            inspection.terminal = Some(SseTerminal::Err(message));
        } else if let Some(terminal) = terminal {
            inspection.terminal = Some(terminal);
        } else if kind.is_none() && is_chat_completion_terminal_chunk(&value) {
            inspection.terminal = Some(SseTerminal::Ok);
        }

        inspection.usage = parse_usage_from_json(&value).into();
        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            let mut text_out = String::new();
            for choice in choices {
                if let Some(delta) = choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("content"))
                {
                    collect_response_output_text(delta, &mut text_out);
                }
            }
            if !text_out.trim().is_empty() {
                let usage = inspection
                    .usage
                    .get_or_insert_with(UpstreamResponseUsage::default);
                let target = usage.output_text.get_or_insert_with(String::new);
                append_output_text(target, text_out.as_str());
            }
        } else if let Some(delta) = value.get("delta").and_then(Value::as_str) {
            if !delta.is_empty() {
                let usage = inspection
                    .usage
                    .get_or_insert_with(UpstreamResponseUsage::default);
                let target = usage.output_text.get_or_insert_with(String::new);
                append_output_text(target, delta);
            }
        }
    }

    inspection
}

#[cfg(test)]
pub(in super::super) fn inspect_openai_responses_sse_frame(lines: &[String]) -> SseFrameInspection {
    let mut inspection = inspect_sse_frame_for_protocol(lines, PassthroughSseProtocol::Generic);
    let Some(event) = OpenAIResponsesEvent::parse(lines) else {
        return inspection;
    };

    if let Some(event_type) = event.event_type {
        inspection.last_event_type = Some(event_type);
    }
    inspection.terminal = event.terminal;
    inspection.usage = Some(event.usage);
    inspection
}

pub(in super::super) fn inspect_sse_frame(lines: &[String]) -> SseFrameInspection {
    inspect_sse_frame_for_protocol(lines, PassthroughSseProtocol::Generic)
}

/// 函数 `extract_sse_event_name`
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
pub(in super::super) fn extract_sse_event_name(lines: &[String]) -> Option<String> {
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("event:") {
            let event_name = rest.trim();
            if !event_name.is_empty() {
                return Some(event_name.to_string());
            }
        }
    }
    None
}

/// 函数 `normalize_sse_event_name_for_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - event_name: 参数 event_name
///
/// # 返回
/// 返回函数执行结果
fn normalize_sse_event_name_for_type(event_name: &str) -> Option<&str> {
    let normalized = event_name.trim();
    if normalized.is_empty() || normalized.eq_ignore_ascii_case("message") {
        return None;
    }
    Some(normalized)
}

/// 函数 `extract_sse_frame_payload`
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
pub(in super::super) fn extract_sse_frame_payload(lines: &[String]) -> Option<String> {
    let mut data_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if !data_lines.is_empty() {
        return Some(data_lines.join("\n"));
    }

    let mut raw_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(':')
            || trimmed.starts_with("event:")
            || trimmed.starts_with("id:")
            || trimmed.starts_with("retry:")
        {
            continue;
        }
        raw_lines.push(trimmed.to_string());
    }
    if raw_lines.is_empty() {
        None
    } else {
        Some(raw_lines.join("\n"))
    }
}

#[cfg(test)]
#[path = "sse_frame_tests.rs"]
mod tests;

/// 函数 `ensure_value_has_sse_event_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - lines: 参数 lines
/// - value: 参数 value
///
/// # 返回
/// 无
fn ensure_value_has_sse_event_type(lines: &[String], value: &mut Value) {
    let Some(event_name) = extract_sse_event_name(lines) else {
        return;
    };
    let Some(event_type) = normalize_sse_event_name_for_type(event_name.as_str()) else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let has_type = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| !kind.trim().is_empty());
    if !has_type {
        object.insert("type".to_string(), Value::String(event_type.to_string()));
    }
}

/// 函数 `parse_sse_frame_json`
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
pub(in super::super) fn parse_sse_frame_json(lines: &[String]) -> Option<Value> {
    let payload = extract_sse_frame_payload(lines)?;
    let mut value = serde_json::from_str::<Value>(&payload).ok()?;
    ensure_value_has_sse_event_type(lines, &mut value);
    Some(value)
}
