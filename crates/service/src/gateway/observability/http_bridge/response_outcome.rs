use serde_json::{json, Value};

pub(super) fn is_output_limit_incomplete(value: &Value) -> bool {
    let response = value.get("response").unwrap_or(value);
    let has_error = value.get("error").is_some_and(|v| !v.is_null())
        || response.get("error").is_some_and(|v| !v.is_null())
        || response
            .pointer("/status_details/error")
            .is_some_and(|v| !v.is_null());
    !has_error
        && (response.get("status").and_then(Value::as_str) == Some("incomplete")
            || value.get("type").and_then(Value::as_str) == Some("response.incomplete"))
        && matches!(
            response
                .pointer("/incomplete_details/reason")
                .and_then(Value::as_str),
            Some("max_output_tokens" | "max_tokens" | "length")
        )
}

pub(super) fn output_limit_has_visible_text(value: &Value) -> bool {
    let mut text = String::new();
    super::collect_response_output_text(value.get("response").unwrap_or(value), &mut text);
    is_output_limit_incomplete(value) && !text.trim().is_empty()
}

pub(super) fn response_failure(value: &Value) -> Option<Value> {
    let response = value.get("response").unwrap_or(value);
    let error = [
        value.get("error"),
        response.get("error"),
        response.pointer("/status_details/error"),
    ]
    .into_iter()
    .flatten()
    .find(|v| !v.is_null());
    let status = response.get("status").and_then(Value::as_str);
    let event = value.get("type").and_then(Value::as_str);
    if error.is_none()
        && !matches!(
            status,
            Some("failed" | "error" | "incomplete" | "cancelled" | "canceled")
        )
        && !matches!(
            event,
            Some(
                "error"
                    | "response.failed"
                    | "response.error"
                    | "response.incomplete"
                    | "response.cancelled"
                    | "response.canceled"
            )
        )
    {
        return None;
    }
    let error = error.unwrap_or(value);
    let incomplete = status == Some("incomplete") || event == Some("response.incomplete");
    let code = if incomplete {
        "upstream_response_incomplete"
    } else {
        "upstream_response_failed"
    };
    let field = |key: &str| error.get(key).and_then(Value::as_str);
    Some(json!({
        "type": field("type").filter(|kind| !kind.starts_with("response.") && *kind != "error").unwrap_or("upstream_error"),
        "code": field("code").unwrap_or(code),
        "message": field("message").or_else(|| error.as_str()).or_else(|| response.pointer("/incomplete_details/reason").and_then(Value::as_str)).unwrap_or(code),
        "param": field("param")
    }))
}

/// Bound retained delivery evidence; after overflow, skip snapshot repair rather
/// than compare a truncated prefix and risk duplicating already delivered text.
#[derive(Default)]
pub(super) struct TextDeliveryTracker {
    text: String,
    overflow: bool,
    visible: bool,
}

impl TextDeliveryTracker {
    pub(super) fn record(&mut self, fragment: &str) {
        self.visible |= !fragment.trim().is_empty();
        if self.overflow {
            return;
        }
        if self.text.len().saturating_add(fragment.len()) > 1024 * 1024 {
            self.text.clear();
            self.overflow = true;
        } else {
            self.text.push_str(fragment);
        }
    }

    pub(super) fn missing_suffix<'a>(&self, snapshot: &'a str) -> Option<&'a str> {
        if self.overflow {
            None
        } else {
            snapshot.strip_prefix(&self.text)
        }
    }

    pub(super) fn has_visible_text(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_prefix_evidence_is_bounded_and_never_guesses_a_mismatch() {
        let mut tracker = TextDeliveryTracker::default();
        tracker.record("上海");
        assert_eq!(tracker.missing_suffix("上海23°C"), Some("23°C"));
        assert_eq!(tracker.missing_suffix("北京23°C"), None);
        tracker.record(&"x".repeat(1024 * 1024));
        assert!(tracker.text.is_empty());
        assert!(tracker.missing_suffix("anything").is_none());
        assert!(tracker.has_visible_text());
    }
}
