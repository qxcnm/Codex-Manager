use serde_json::Value;
use thiserror::Error;

const DEFAULT_MAX_FRAME_BYTES: usize = 8 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrokWebEvent {
    Conversation { conversation_id: String },
    ParentResponse { response_id: String },
    ReasoningDelta(String),
    TextDelta(String),
    Image { url: String, completed: bool },
    UpstreamError { code: Option<i64>, message: String },
}

#[derive(Debug, Error)]
pub enum GrokStreamError {
    #[error("Grok stream frame exceeds {limit} bytes")]
    FrameTooLarge { limit: usize },
    #[error("Grok stream ended inside a JSON object")]
    UnexpectedEof,
    #[error("invalid Grok stream JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

/// Incrementally separates concatenated JSON objects. It accepts arbitrary
/// transport chunking and ignores non-JSON separators such as `data:` prefixes.
pub struct GrokWebStreamParser {
    frame: Vec<u8>,
    depth: usize,
    in_string: bool,
    escaped: bool,
    max_frame_bytes: usize,
}

impl Default for GrokWebStreamParser {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_BYTES)
    }
}

impl GrokWebStreamParser {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            frame: Vec::new(),
            depth: 0,
            in_string: false,
            escaped: false,
            max_frame_bytes: max_frame_bytes.max(1),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<GrokWebEvent>, GrokStreamError> {
        let mut events = Vec::new();
        for &byte in chunk {
            if self.depth == 0 {
                if byte != b'{' {
                    continue;
                }
                self.frame.clear();
                self.frame.push(byte);
                self.depth = 1;
                self.in_string = false;
                self.escaped = false;
                continue;
            }

            self.frame.push(byte);
            if self.frame.len() > self.max_frame_bytes {
                self.reset();
                return Err(GrokStreamError::FrameTooLarge {
                    limit: self.max_frame_bytes,
                });
            }
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => self.in_string = true,
                b'{' => self.depth += 1,
                b'}' => {
                    self.depth -= 1;
                    if self.depth == 0 {
                        let value: Value = serde_json::from_slice(&self.frame)?;
                        events.extend(parse_frame(&value));
                        self.frame.clear();
                    }
                }
                _ => {}
            }
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> Result<(), GrokStreamError> {
        if self.depth != 0 {
            self.reset();
            return Err(GrokStreamError::UnexpectedEof);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.frame.clear();
        self.depth = 0;
        self.in_string = false;
        self.escaped = false;
    }
}

fn parse_frame(root: &Value) -> Vec<GrokWebEvent> {
    let mut events = Vec::new();
    if let Some(error) = root.get("error") {
        events.push(error_event(error));
        return events;
    }
    let Some(result) = root.get("result") else {
        return events;
    };
    if let Some(id) = result
        .get("conversation")
        .and_then(|value| value.get("conversationId"))
        .and_then(Value::as_str)
    {
        events.push(GrokWebEvent::Conversation {
            conversation_id: id.to_owned(),
        });
    }
    let Some(response) = result.get("response") else {
        return events;
    };
    if let Some(error) = response.get("error") {
        events.push(error_event(error));
        return events;
    }
    if let Some(id) = response
        .get("userResponse")
        .and_then(|value| value.get("responseId"))
        .and_then(Value::as_str)
    {
        events.push(GrokWebEvent::ParentResponse {
            response_id: id.to_owned(),
        });
    }
    if let Some(token) = response
        .get("token")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    {
        if response
            .get("isThinking")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            events.push(GrokWebEvent::ReasoningDelta(token.to_owned()));
        } else {
            let tag = response
                .get("messageTag")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if tag.is_empty() || tag == "final" {
                events.push(GrokWebEvent::TextDelta(token.to_owned()));
            }
        }
    }
    if let Some(image) = response.get("streamingImageGenerationResponse") {
        let url = image
            .get("imageUrl")
            .or_else(|| image.get("url"))
            .and_then(Value::as_str);
        if let Some(url) = url.filter(|value| !value.is_empty()) {
            let completed = image
                .get("isFinal")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || image.get("progress").and_then(Value::as_u64) == Some(100);
            events.push(GrokWebEvent::Image {
                url: url.to_owned(),
                completed,
            });
        }
    }
    events
}

fn error_event(value: &Value) -> GrokWebEvent {
    GrokWebEvent::UpstreamError {
        code: value.get("code").and_then(Value::as_i64),
        message: value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Grok Web stream error")
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_concatenated_objects_across_transport_chunks() {
        let source = br#"data: {"result":{"conversation":{"conversationId":"conv-1"}}}
{"result":{"response":{"token":"thinking {inside}","isThinking":true}}}{"result":{"response":{"token":"done","messageTag":"final","userResponse":{"responseId":"parent-1"}}}}"#;
        let mut parser = GrokWebStreamParser::default();
        let mut events = Vec::new();
        for chunk in source.chunks(7) {
            events.extend(parser.push(chunk).unwrap());
        }
        parser.finish().unwrap();
        assert_eq!(
            events,
            [
                GrokWebEvent::Conversation {
                    conversation_id: "conv-1".to_owned()
                },
                GrokWebEvent::ReasoningDelta("thinking {inside}".to_owned()),
                GrokWebEvent::ParentResponse {
                    response_id: "parent-1".to_owned()
                },
                GrokWebEvent::TextDelta("done".to_owned()),
            ]
        );
    }

    #[test]
    fn braces_and_escaped_quotes_inside_strings_do_not_split_frames() {
        let mut parser = GrokWebStreamParser::default();
        let events = parser
            .push(br#"{"result":{"response":{"token":"a \\\"quoted\\\" } value","messageTag":"final"}}}"#)
            .unwrap();
        assert_eq!(
            events,
            [GrokWebEvent::TextDelta(
                "a \\\"quoted\\\" } value".to_owned()
            )]
        );
    }

    #[test]
    fn emits_typed_upstream_error() {
        let mut parser = GrokWebStreamParser::default();
        let events = parser
            .push(br#"{"error":{"code":7,"message":"anti-bot"}}"#)
            .unwrap();
        assert_eq!(
            events,
            [GrokWebEvent::UpstreamError {
                code: Some(7),
                message: "anti-bot".to_owned()
            }]
        );
    }

    #[test]
    fn rejects_oversized_and_truncated_frames() {
        let mut limited = GrokWebStreamParser::new(8);
        assert!(matches!(
            limited.push(br#"{"long":"value"}"#),
            Err(GrokStreamError::FrameTooLarge { limit: 8 })
        ));

        let mut truncated = GrokWebStreamParser::default();
        truncated.push(br#"{"result":{"#).unwrap();
        assert!(matches!(
            truncated.finish(),
            Err(GrokStreamError::UnexpectedEof)
        ));
    }
}
