use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseAdapter {
    Passthrough,
    AnthropicMessagesFromResponses,
    ResponsesFromAnthropicMessages,
    ChatCompletionsFromResponses,
    #[allow(dead_code)]
    CompactFromChatCompletions,
    ImagesB64JsonFromResponses,
    ImagesUrlFromResponses,
    GeminiJson,
    GeminiSse,
    GeminiCliJson,
    GeminiCliSse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GeminiStreamOutputMode {
    Sse,
    Raw,
}

pub(crate) type ToolNameRestoreMap = BTreeMap<String, String>;

#[derive(Debug)]
pub(crate) struct AdaptedGatewayRequest {
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
    pub(crate) response_adapter: ResponseAdapter,
    pub(crate) gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    pub(crate) tool_name_restore_map: ToolNameRestoreMap,
}
