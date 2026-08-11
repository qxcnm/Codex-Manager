use serde_json::{Map, Value};

/// Provider-neutral request used after public protocol adapters have normalized
/// Chat Completions, Responses, Anthropic, or Gemini input to the Responses
/// shape. Unknown fields are retained so newer OpenAI request features can
/// traverse the gateway without waiting for a schema release.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CanonicalRequest {
    pub model: String,
    pub input: Option<Value>,
    pub instructions: Option<Value>,
    pub tools: Option<Value>,
    pub tool_choice: Option<Value>,
    pub reasoning: Option<Value>,
    pub stream: bool,
    pub max_output_tokens: Option<Value>,
    pub temperature: Option<Value>,
    pub top_p: Option<Value>,
    pub parallel_tool_calls: Option<Value>,
    extra: Map<String, Value>,
}

impl CanonicalRequest {
    pub(crate) fn from_responses_bytes(body: &[u8]) -> Result<Self, String> {
        let value = serde_json::from_slice::<Value>(body)
            .map_err(|error| format!("invalid canonical request JSON: {error}"))?;
        let mut object = value
            .as_object()
            .cloned()
            .ok_or_else(|| "canonical request must be a JSON object".to_string())?;
        let model = take_text(&mut object, "model")
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| "canonical request model is required".to_string())?;
        let stream = object
            .remove("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        Ok(Self {
            model,
            input: object.remove("input"),
            instructions: object.remove("instructions"),
            tools: object.remove("tools"),
            tool_choice: object.remove("tool_choice"),
            reasoning: object.remove("reasoning"),
            stream,
            max_output_tokens: object.remove("max_output_tokens"),
            temperature: object.remove("temperature"),
            top_p: object.remove("top_p"),
            parallel_tool_calls: object.remove("parallel_tool_calls"),
            extra: object,
        })
    }

    pub(crate) fn to_responses_value(&self) -> Value {
        let mut object = self.extra.clone();
        object.insert("model".into(), Value::String(self.model.clone()));
        object.insert("stream".into(), Value::Bool(self.stream));
        insert_optional(&mut object, "input", &self.input);
        insert_optional(&mut object, "instructions", &self.instructions);
        insert_optional(&mut object, "tools", &self.tools);
        insert_optional(&mut object, "tool_choice", &self.tool_choice);
        insert_optional(&mut object, "reasoning", &self.reasoning);
        insert_optional(&mut object, "max_output_tokens", &self.max_output_tokens);
        insert_optional(&mut object, "temperature", &self.temperature);
        insert_optional(&mut object, "top_p", &self.top_p);
        insert_optional(
            &mut object,
            "parallel_tool_calls",
            &self.parallel_tool_calls,
        );
        Value::Object(object)
    }

    pub(crate) fn to_responses_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.to_responses_value())
            .map_err(|error| format!("serialize canonical request failed: {error}"))
    }
}

/// A provider implements only the conversion from the canonical request to its
/// own wire request. Public API protocol handling remains outside providers.
pub(crate) trait ProviderAdapter {
    type ProviderRequest;

    fn adapt_request(&self, request: &CanonicalRequest) -> Result<Self::ProviderRequest, String>;
}

/// Codex already speaks the canonical Responses protocol, so its adapter is an
/// intentional identity serialization rather than a separate conversion path.
pub(crate) struct CodexProviderAdapter;

impl ProviderAdapter for CodexProviderAdapter {
    type ProviderRequest = Vec<u8>;

    fn adapt_request(&self, request: &CanonicalRequest) -> Result<Self::ProviderRequest, String> {
        request.to_responses_bytes()
    }
}

fn take_text(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    object.remove(key)?.as_str().map(str::to_string)
}

fn insert_optional(object: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
    if let Some(value) = value {
        object.insert(key.into(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_request_round_trip_preserves_known_multimodal_and_future_fields() {
        let original = serde_json::json!({
            "model": "kiro/claude-sonnet-4.6",
            "instructions": "system",
            "input": [{"role":"user","content":[
                {"type":"input_text","text":"hello"},
                {"type":"input_image","image_url":"data:image/png;base64,AA=="}
            ]}],
            "tools": [{"type":"function","name":"lookup","parameters":{"type":"object"}}],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "reasoning": {"effort":"high","summary":"auto"},
            "max_output_tokens": 2048,
            "temperature": 0.2,
            "top_p": 0.9,
            "stream": true,
            "future_parameter": {"enabled":true}
        });
        let body = serde_json::to_vec(&original).unwrap();

        let canonical = CanonicalRequest::from_responses_bytes(&body).unwrap();
        let round_trip = canonical.to_responses_value();

        assert_eq!(round_trip, original);
    }

    #[test]
    fn codex_provider_is_the_canonical_identity_adapter() {
        let canonical = CanonicalRequest::from_responses_bytes(
            br#"{"model":"codex/gpt-5.4","input":"hello","stream":false}"#,
        )
        .unwrap();
        let bytes = CodexProviderAdapter.adapt_request(&canonical).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["model"], "codex/gpt-5.4");
        assert_eq!(value["input"], "hello");
    }
}
