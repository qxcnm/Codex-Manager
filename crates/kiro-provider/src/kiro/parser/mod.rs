//! AWS Event Stream 解析器
//!
//! 提供对 AWS Event Stream 协议的解析支持，
//! 用于处理 generateAssistantResponse 端点的流式响应

pub mod crc;
pub mod decoder;
pub mod error;
pub mod frame;
pub mod header;
