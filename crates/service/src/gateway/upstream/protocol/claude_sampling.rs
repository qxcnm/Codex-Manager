use bytes::Bytes;
use serde_json::Value;

// Claude 4.7 and later reject these with a 400 (`temperature` below 1 already fails), on the
// native Messages API and on Anthropic's OpenAI-compatible endpoint alike. OpenAI-style clients
// send them routinely, so they are dropped before the body reaches such a model.
const UNSUPPORTED_SAMPLING_FIELDS: [&str; 3] = ["temperature", "top_p", "top_k"];

pub(super) fn strip_unsupported_claude_sampling_params(body: &Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<Value>(body.as_ref()) else {
        return body.clone();
    };
    let Some(object) = value.as_object_mut() else {
        return body.clone();
    };
    if !object
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(claude_model_rejects_sampling_params)
    {
        return body.clone();
    }
    let mut changed = false;
    for field in UNSUPPORTED_SAMPLING_FIELDS {
        changed |= object.remove(field).is_some();
    }
    if !changed {
        return body.clone();
    }
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

fn claude_model_rejects_sampling_params(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let Some(start) = model.find("claude-") else {
        return false;
    };
    let mut parts =
        model[start + "claude-".len()..].split(|c: char| matches!(c, '-' | '.' | '@' | ':' | '/'));
    match parts.next() {
        Some("fable" | "mythos") => return true,
        Some("opus" | "sonnet" | "haiku") => {}
        // Legacy names such as claude-3-5-sonnet-20241022 predate the restriction.
        _ => return false,
    }
    let Some(major) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return false;
    };
    // A dated snapshot suffix (claude-haiku-4-5-20251001, claude-sonnet-4-20250514) is not a minor version.
    let minor = parts
        .next()
        .filter(|part| part.len() <= 2)
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    (major, minor) >= (4, 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(body: serde_json::Value) -> serde_json::Value {
        let bytes = Bytes::from(serde_json::to_vec(&body).expect("encode body"));
        serde_json::from_slice(strip_unsupported_claude_sampling_params(&bytes).as_ref())
            .expect("decode body")
    }

    #[test]
    fn claude_sampling_restriction_starts_at_4_7() {
        for model in [
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-5",
            "claude-opus-5-5",
            "claude-sonnet-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "claude-fable-5-1",
            "claude-mythos-preview",
            "anthropic.claude-opus-5-5",
            "anthropic/claude-sonnet-5-5",
            " Claude-Haiku-5-5 ",
        ] {
            assert!(claude_model_rejects_sampling_params(model), "{model}");
        }
        for model in [
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-5@20250929",
            "claude-sonnet-4-20250514",
            "claude-3-5-sonnet-20241022",
            "gpt-6-luna",
            "",
        ] {
            assert!(!claude_model_rejects_sampling_params(model), "{model}");
        }
    }

    #[test]
    fn non_stream_chat_completion_with_tools_drops_sampling_for_new_claude() {
        let value = strip(serde_json::json!({
            "model": "claude-haiku-5-5",
            "temperature": 0.2,
            "top_p": 0.9,
            "max_completion_tokens": 1600,
            "messages": [{"role": "system", "content": "s"}, {"role": "user", "content": "u"}],
            "tools": [{"type": "function", "function": {"name": "search_order", "parameters": {"type": "object"}}}],
            "tool_choice": "auto"
        }));
        assert!(value.get("temperature").is_none());
        assert!(value.get("top_p").is_none());
        assert_eq!(value["max_completion_tokens"], 1600);
        assert_eq!(value["tool_choice"], "auto");
        assert_eq!(value["tools"][0]["function"]["name"], "search_order");
        assert_eq!(value["messages"][1]["content"], "u");
    }

    #[test]
    fn stream_chat_completion_and_responses_bodies_drop_sampling_for_new_claude() {
        let chat = strip(serde_json::json!({
            "model": "claude-sonnet-5-5",
            "stream": true,
            "stream_options": {"include_usage": true},
            "temperature": 0,
            "top_k": 40,
            "messages": [{"role": "user", "content": "u"}]
        }));
        assert!(chat.get("temperature").is_none());
        assert!(chat.get("top_k").is_none());
        assert_eq!(chat["stream"], true);
        assert_eq!(chat["stream_options"]["include_usage"], true);

        let responses = strip(serde_json::json!({
            "model": "claude-opus-5-5",
            "input": "u",
            "temperature": 0.7,
            "tools": [{"type": "function", "name": "search_order", "parameters": {"type": "object"}}]
        }));
        assert!(responses.get("temperature").is_none());
        assert_eq!(responses["input"], "u");
        assert_eq!(responses["tools"][0]["name"], "search_order");
    }

    #[test]
    fn older_claude_other_models_and_non_json_bodies_are_untouched() {
        for model in ["claude-opus-4-6", "gpt-6-luna"] {
            let body = Bytes::from(
                serde_json::to_vec(
                    &serde_json::json!({"model": model, "temperature": 0.2, "messages": []}),
                )
                .expect("encode body"),
            );
            assert_eq!(strip_unsupported_claude_sampling_params(&body), body);
        }
        let raw = Bytes::from_static(b"not json");
        assert_eq!(strip_unsupported_claude_sampling_params(&raw), raw);
        let no_sampling = Bytes::from_static(br#"{"model":"claude-haiku-5-5","messages":[]}"#);
        assert_eq!(
            strip_unsupported_claude_sampling_params(&no_sampling),
            no_sampling
        );
    }
}
