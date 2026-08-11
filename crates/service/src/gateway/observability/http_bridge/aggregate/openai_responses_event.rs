use serde_json::Value;
use std::collections::BTreeSet;

use super::output_text::{
    append_output_text, append_output_text_raw, collect_output_text_from_event_fields,
    collect_response_output_text, extract_error_message_from_json, merge_usage,
    parse_usage_from_json, UpstreamResponseUsage,
};
use super::{parse_sse_frame_json, SseTerminal};

const STREAM_INCOMPLETE_FALLBACK_MESSAGE: &str = "连接中断（可能是网络波动或客户端主动取消）";
const STREAM_IDLE_TIMEOUT_FALLBACK_MESSAGE: &str = "上游流式空闲超时";
const UPSTREAM_NON_SUCCESS_FALLBACK_MESSAGE: &str = "上游请求失败，未返回具体错误信息";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum OpenAIResponsesEventKind {
    Completed,
    Done,
    Failed,
    Incomplete,
    OutputTextDelta,
    OutputTextDone,
    OutputItemAdded,
    OutputItemDone,
    ImageGenerationPartialImage,
    ContentPartAdded,
    ContentPartDelta,
    ContentPartDone,
    Other,
}

impl OpenAIResponsesEventKind {
    fn from_type(kind: &str) -> Self {
        match kind.trim() {
            "response.completed" => Self::Completed,
            "response.done" => Self::Done,
            "response.failed" => Self::Failed,
            "response.incomplete" => Self::Incomplete,
            "response.output_text.delta" => Self::OutputTextDelta,
            "response.output_text.done" => Self::OutputTextDone,
            "response.output_item.added" => Self::OutputItemAdded,
            "response.output_item.done" => Self::OutputItemDone,
            "response.image_generation_call.partial_image" => Self::ImageGenerationPartialImage,
            "response.content_part.added" => Self::ContentPartAdded,
            "response.content_part.delta" => Self::ContentPartDelta,
            "response.content_part.done" => Self::ContentPartDone,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAIResponsesOutputTextKind {
    Delta,
    Snapshot,
    TerminalSnapshot,
}

#[derive(Debug, Clone, Default)]
pub(in super::super) struct OpenAIResponsesOutputTextState {
    saw_delta: bool,
    seen_snapshot_keys: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct OpenAIResponsesEvent {
    pub(in super::super) event_type: Option<String>,
    pub(in super::super) usage: UpstreamResponseUsage,
    output_text_kind: Option<OpenAIResponsesOutputTextKind>,
    output_text_snapshot_key: Option<String>,
    pub(in super::super) terminal: Option<SseTerminal>,
    pub(in super::super) upstream_error_hint: Option<String>,
}

/// Canonical provider-neutral event consumed by the downstream Chat and
/// Responses output adapters. Providers normalize their wire events to this
/// OpenAI Responses event representation before delivery.
pub(in super::super) type CanonicalEvent = OpenAIResponsesEvent;

impl OpenAIResponsesEvent {
    pub(in super::super) fn parse(lines: &[String]) -> Option<Self> {
        let value = parse_sse_frame_json(lines)?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|kind: &&str| !kind.is_empty())
            .map(str::to_string);
        let kind = event_type
            .as_deref()
            .map(OpenAIResponsesEventKind::from_type)
            .unwrap_or(OpenAIResponsesEventKind::Other);

        let upstream_error_hint = extract_error_message_from_json(&value);
        let terminal =
            terminal_for_event(kind, event_type.as_deref(), upstream_error_hint.as_deref());

        let mut usage = parse_usage_from_json(&value);
        usage.output_text = None;
        let mut output_text_snapshot_key = None;
        let output_text_kind = collect_event_output_text(&value, kind).map(
            |(extra_output_text, output_text_kind, snapshot_key)| {
                usage.output_text = Some(extra_output_text);
                output_text_snapshot_key = snapshot_key;
                output_text_kind
            },
        );

        Some(Self {
            event_type,
            usage,
            output_text_kind,
            output_text_snapshot_key,
            terminal,
            upstream_error_hint,
        })
    }

    pub(in super::super) fn merge_usage_into(
        &self,
        target: &mut UpstreamResponseUsage,
        text_state: &mut OpenAIResponsesOutputTextState,
    ) {
        let mut usage = self.usage.clone();
        let output_text = usage.output_text.take();
        merge_usage(target, usage);

        let Some(output_text) = output_text else {
            return;
        };
        if output_text.trim().is_empty() {
            return;
        }

        match self.output_text_kind {
            Some(OpenAIResponsesOutputTextKind::Delta) => {
                let target_text = target.output_text.get_or_insert_with(String::new);
                append_output_text_raw(target_text, output_text.as_str());
                text_state.saw_delta = true;
            }
            Some(OpenAIResponsesOutputTextKind::Snapshot) => {
                let snapshot_key = self
                    .output_text_snapshot_key
                    .clone()
                    .unwrap_or_else(|| output_text.trim().to_string());
                if !text_state.saw_delta && text_state.seen_snapshot_keys.insert(snapshot_key) {
                    let target_text = target.output_text.get_or_insert_with(String::new);
                    append_output_text_raw(target_text, output_text.as_str());
                }
            }
            Some(OpenAIResponsesOutputTextKind::TerminalSnapshot) => {
                if !text_state.saw_delta
                    && target
                        .output_text
                        .as_deref()
                        .is_none_or(|text| text.trim().is_empty())
                {
                    let target_text = target.output_text.get_or_insert_with(String::new);
                    append_output_text_raw(target_text, output_text.as_str());
                }
            }
            None => {}
        }
    }
}

fn terminal_for_event(
    kind: OpenAIResponsesEventKind,
    event_type: Option<&str>,
    upstream_error_hint: Option<&str>,
) -> Option<SseTerminal> {
    if let Some(raw) = upstream_error_hint {
        return Some(SseTerminal::Err(normalize_terminal_error_hint(raw)));
    }

    match kind {
        OpenAIResponsesEventKind::Completed | OpenAIResponsesEventKind::Done => {
            Some(SseTerminal::Ok)
        }
        OpenAIResponsesEventKind::Failed => Some(SseTerminal::Err(
            UPSTREAM_NON_SUCCESS_FALLBACK_MESSAGE.to_string(),
        )),
        OpenAIResponsesEventKind::Incomplete => Some(SseTerminal::Err(
            event_type
                .filter(|value| !value.trim().is_empty())
                .map(|_| STREAM_INCOMPLETE_FALLBACK_MESSAGE.to_string())
                .unwrap_or_else(|| STREAM_INCOMPLETE_FALLBACK_MESSAGE.to_string()),
        )),
        OpenAIResponsesEventKind::OutputTextDelta
        | OpenAIResponsesEventKind::OutputTextDone
        | OpenAIResponsesEventKind::OutputItemAdded
        | OpenAIResponsesEventKind::OutputItemDone
        | OpenAIResponsesEventKind::ImageGenerationPartialImage
        | OpenAIResponsesEventKind::ContentPartAdded
        | OpenAIResponsesEventKind::ContentPartDelta
        | OpenAIResponsesEventKind::ContentPartDone
        | OpenAIResponsesEventKind::Other => None,
    }
}

fn normalize_terminal_error_hint(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return UPSTREAM_NON_SUCCESS_FALLBACK_MESSAGE.to_string();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("stream_timeout")
        || normalized.contains("stream idle timeout")
        || normalized.contains("idle timeout")
    {
        return STREAM_IDLE_TIMEOUT_FALLBACK_MESSAGE.to_string();
    }

    trimmed.to_string()
}

fn collect_completed_response_text(value: &Value, text_out: &mut String) {
    let response = value.get("response").unwrap_or(value);
    if let Some(output_text) = response.get("output_text").and_then(Value::as_str) {
        append_output_text(text_out, output_text);
        return;
    }
    if let Some(output) = response.get("output") {
        collect_response_output_text(output, text_out);
    }
}

fn collect_event_string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            value
                .get("item")
                .and_then(|item| item.get(field))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .or_else(|| {
            value
                .get("output_item")
                .and_then(|item| item.get(field))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
}

fn snapshot_dedupe_key(value: &Value, text: &str) -> String {
    let text = text.trim();
    if let Some(output_index) = value.get("output_index").and_then(Value::as_i64) {
        return format!("output_index={output_index};text={text}");
    }
    if let Some(item_id) = collect_event_string_field(value, "item_id")
        .or_else(|| collect_event_string_field(value, "id"))
    {
        return format!("item_id={item_id};text={text}");
    }
    format!("text={text}")
}

fn collect_event_output_text(
    value: &Value,
    kind: OpenAIResponsesEventKind,
) -> Option<(String, OpenAIResponsesOutputTextKind, Option<String>)> {
    let mut text_out = String::new();
    let mut output_text_kind = None;

    match kind {
        OpenAIResponsesEventKind::OutputTextDelta | OpenAIResponsesEventKind::ContentPartDelta => {
            if let Some(delta) = value.get("delta") {
                collect_response_output_text(delta, &mut text_out);
            }
            output_text_kind = Some(OpenAIResponsesOutputTextKind::Delta);
        }
        OpenAIResponsesEventKind::OutputTextDone => {
            if let Some(text) = value.get("text") {
                collect_response_output_text(text, &mut text_out);
            } else if let Some(delta) = value.get("delta") {
                collect_response_output_text(delta, &mut text_out);
            }
            output_text_kind = Some(OpenAIResponsesOutputTextKind::Snapshot);
        }
        OpenAIResponsesEventKind::ContentPartAdded | OpenAIResponsesEventKind::ContentPartDone => {
            collect_output_text_from_event_fields(value, &mut text_out);
            if text_out.trim().is_empty() {
                if let Some(delta) = value.get("delta") {
                    collect_response_output_text(delta, &mut text_out);
                }
            }
            output_text_kind = Some(OpenAIResponsesOutputTextKind::Snapshot);
        }
        OpenAIResponsesEventKind::OutputItemAdded | OpenAIResponsesEventKind::OutputItemDone => {
            collect_output_text_from_event_fields(value, &mut text_out);
            output_text_kind = Some(OpenAIResponsesOutputTextKind::Snapshot);
        }
        OpenAIResponsesEventKind::Completed | OpenAIResponsesEventKind::Done => {
            collect_completed_response_text(value, &mut text_out);
            output_text_kind = Some(OpenAIResponsesOutputTextKind::TerminalSnapshot);
        }
        OpenAIResponsesEventKind::Failed
        | OpenAIResponsesEventKind::Incomplete
        | OpenAIResponsesEventKind::ImageGenerationPartialImage
        | OpenAIResponsesEventKind::Other => {}
    }

    let text = text_out.trim();
    if text.is_empty() {
        None
    } else {
        let snapshot_key = matches!(
            output_text_kind,
            Some(OpenAIResponsesOutputTextKind::Snapshot)
        )
        .then(|| snapshot_dedupe_key(value, text));
        output_text_kind.map(|kind| (text_out, kind, snapshot_key))
    }
}

#[cfg(test)]
#[path = "openai_responses_event_tests.rs"]
mod tests;
