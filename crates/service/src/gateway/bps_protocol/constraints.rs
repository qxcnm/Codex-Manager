//! Validate client constraints before the Codex compatibility rewrite can hide
//! them. This runs only after the selected account resolves to the BPS route.
use serde_json::Value;

pub(crate) fn validate_original_constraints(path: &str, body: &[u8]) -> Result<(), String> {
    let source: Value = serde_json::from_slice(body).map_err(|_| "invalid BPS request JSON")?;
    super::object(&source)?;
    validate_responses_constraints(&source)?;
    // Chat token limits are discarded while adapting messages to Responses.
    // Reject the original aliases too, rather than silently losing their cap.
    reject_present(&source, &["max_tokens", "max_completion_tokens"])?;
    if path
        .split('?')
        .next()
        .unwrap_or(path)
        .trim_end_matches('/')
        .ends_with("/chat/completions")
    {
        if source
            .pointer("/response_format/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "text")
        {
            return Err(
                "BPS does not support constrained response_format; disable BPS for this request"
                    .into(),
            );
        }
        reject_present(
            &source,
            &[
                "stop",
                "seed",
                "frequency_penalty",
                "presence_penalty",
                "logit_bias",
                "logprobs",
                "top_logprobs",
            ],
        )?;
        if source
            .get("n")
            .is_some_and(|v| !v.is_null() && v.as_u64() != Some(1))
        {
            return Err("BPS only supports one completion per request".into());
        }
    }
    Ok(())
}

pub(super) fn validate_responses_constraints(source: &Value) -> Result<(), String> {
    for field in ["previous_response_id", "conversation"] {
        if source
            .get(field)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        {
            return Err(format!(
                "BPS cannot resolve {field}; provide the full conversation history"
            ));
        }
    }
    if source["background"].as_bool() == Some(true) {
        return Err("BPS does not support background responses".into());
    }
    if source
        .pointer("/reasoning/mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode != "standard")
    {
        return Err("BPS only supports standard reasoning mode".into());
    }
    if source
        .pointer("/text/format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text")
    {
        return Err(
            "BPS does not support constrained structured output; disable BPS for this request"
                .into(),
        );
    }
    reject_present(
        source,
        &[
            "temperature",
            "top_p",
            "max_output_tokens",
            "max_tool_calls",
            "truncation",
        ],
    )?;
    if source["service_tier"]
        .as_str()
        .is_some_and(|tier| !matches!(tier, "auto" | "default"))
    {
        return Err("BPS does not support selecting a service tier".into());
    }
    Ok(())
}

fn reject_present(source: &Value, fields: &[&str]) -> Result<(), String> {
    for field in fields {
        if source.get(*field).is_some_and(|v| !v.is_null()) {
            return Err(format!(
                "BPS cannot preserve {field}; disable BPS for this request"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "constraints_tests.rs"]
mod tests;
