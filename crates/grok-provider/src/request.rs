use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrokChatMode {
    Fast,
    Auto,
    Expert,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrokWebConversationTarget {
    New,
    Continue { conversation_id: String },
}

impl GrokWebConversationTarget {
    pub fn path(&self) -> Cow<'_, str> {
        match self {
            Self::New => Cow::Borrowed("/rest/app-chat/conversations/new"),
            Self::Continue { conversation_id } => Cow::Owned(format!(
                "/rest/app-chat/conversations/{}/responses",
                encode_path_segment(conversation_id)
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEnvironment {
    pub dark_mode_enabled: bool,
    pub device_pixel_ratio: u16,
    pub screen_height: u32,
    pub screen_width: u32,
    pub viewport_height: u32,
    pub viewport_width: u32,
}

impl Default for DeviceEnvironment {
    fn default() -> Self {
        Self {
            dark_mode_enabled: false,
            device_pixel_ratio: 2,
            screen_height: 1328,
            screen_width: 2056,
            viewport_height: 1083,
            viewport_width: 2056,
        }
    }
}

/// Typed representation of the Grok Web chat payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokWebChatRequest {
    pub collection_ids: Vec<String>,
    pub disabled_connector_ids: Vec<String>,
    pub device_env_info: DeviceEnvironment,
    pub disable_memory: bool,
    pub disable_search: bool,
    pub disable_self_harm_short_circuit: bool,
    pub disable_text_follow_ups: bool,
    pub enable_image_generation: bool,
    pub enable_image_streaming: bool,
    pub enable_side_by_side: bool,
    pub file_attachments: Vec<String>,
    pub force_concise: bool,
    pub force_side_by_side: bool,
    pub image_attachments: Vec<String>,
    pub image_generation_count: u8,
    pub is_async_chat: bool,
    pub message: String,
    pub mode_id: GrokChatMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub response_metadata: serde_json::Value,
    pub return_image_bytes: bool,
    pub return_raw_grok_in_xai_request: bool,
    pub send_final_metadata: bool,
    pub temporary: bool,
}

impl GrokWebChatRequest {
    pub fn new(message: impl Into<String>, mode: GrokChatMode) -> Self {
        Self {
            collection_ids: Vec::new(),
            disabled_connector_ids: Vec::new(),
            device_env_info: DeviceEnvironment::default(),
            disable_memory: true,
            disable_search: false,
            disable_self_harm_short_circuit: false,
            disable_text_follow_ups: false,
            enable_image_generation: true,
            enable_image_streaming: true,
            enable_side_by_side: true,
            file_attachments: Vec::new(),
            force_concise: false,
            force_side_by_side: false,
            image_attachments: Vec::new(),
            image_generation_count: 2,
            is_async_chat: false,
            message: message.into(),
            mode_id: mode,
            response_id: None,
            response_metadata: serde_json::json!({}),
            return_image_bytes: false,
            return_raw_grok_in_xai_request: false,
            send_final_metadata: true,
            temporary: true,
        }
    }

    pub fn with_file_attachments(mut self, attachments: Vec<String>) -> Self {
        self.file_attachments = attachments;
        self
    }

    pub fn with_previous_response(mut self, response_id: impl Into<String>) -> Self {
        self.response_id = Some(response_id.into());
        self
    }
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_current_web_shape() {
        let request = GrokWebChatRequest::new("hello", GrokChatMode::Fast)
            .with_file_attachments(vec!["file-1".to_owned()])
            .with_previous_response("response-1");
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["message"], "hello");
        assert_eq!(value["modeId"], "fast");
        assert_eq!(value["responseId"], "response-1");
        assert_eq!(value["fileAttachments"][0], "file-1");
        assert_eq!(value["deviceEnvInfo"]["devicePixelRatio"], 2);
        assert_eq!(value["temporary"], true);
    }

    #[test]
    fn continuation_path_encodes_untrusted_identifier() {
        let target = GrokWebConversationTarget::Continue {
            conversation_id: "id/with space".to_owned(),
        };
        assert_eq!(
            target.path(),
            "/rest/app-chat/conversations/id%2Fwith%20space/responses"
        );
    }
}
