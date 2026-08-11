//! 上下文使用率事件
//!
//! 处理 contextUsageEvent 类型的事件

use serde::Deserialize;

use crate::kiro::parser::error::ParseResult;
use crate::kiro::parser::frame::Frame;

use super::base::EventPayload;

/// 上下文使用率事件
///
/// 包含当前上下文窗口的使用百分比
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsageEvent {
    /// 上下文使用百分比 (0-100)
    #[serde(default)]
    pub context_usage_percentage: f64,
}

impl EventPayload for ContextUsageEvent {
    fn from_frame(frame: &Frame) -> ParseResult<Self> {
        frame.payload_as_json()
    }
}

impl ContextUsageEvent {
    /// 获取格式化的百分比字符串
    pub fn formatted_percentage(&self) -> String {
        format!("{:.2}%", self.context_usage_percentage)
    }
}

impl std::fmt::Display for ContextUsageEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.formatted_percentage())
    }
}
