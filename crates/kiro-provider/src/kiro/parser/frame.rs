//! AWS Event Stream 消息帧解析
//!
//! ## 消息格式
//!
//! ```text
//! ┌──────────────┬──────────────┬──────────────┬──────────┬──────────┬───────────┐
//! │ Total Length │ Header Length│ Prelude CRC  │ Headers  │ Payload  │ Msg CRC   │
//! │   (4 bytes)  │   (4 bytes)  │   (4 bytes)  │ (变长)    │ (变长)    │ (4 bytes) │
//! └──────────────┴──────────────┴──────────────┴──────────┴──────────┴───────────┘
//! ```
//!
//! - Total Length: 整个消息的总长度（包括自身 4 字节）
//! - Header Length: 头部数据的长度
//! - Prelude CRC: 前 8 字节（Total Length + Header Length）的 CRC32 校验
//! - Headers: 头部数据
//! - Payload: 载荷数据（通常是 JSON）
//! - Message CRC: 整个消息（不含 Message CRC 自身）的 CRC32 校验

use super::crc::crc32;
use super::error::{ParseError, ParseResult};
use super::header::{parse_headers, Headers};

/// Prelude 固定大小 (12 字节)
pub const PRELUDE_SIZE: usize = 12;

/// 最小消息大小 (Prelude + Message CRC)
pub const MIN_MESSAGE_SIZE: usize = PRELUDE_SIZE + 4;

/// 最大消息大小限制 (16 MB)
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// 解析后的消息帧
#[derive(Debug, Clone)]
pub struct Frame {
    /// 消息头部
    pub headers: Headers,
    /// 消息负载
    pub payload: Vec<u8>,
}

impl Frame {
    /// 获取消息类型
    pub fn message_type(&self) -> Option<&str> {
        self.headers.message_type()
    }

    /// 获取事件类型
    pub fn event_type(&self) -> Option<&str> {
        self.headers.event_type()
    }

    /// 将 payload 解析为 JSON
    pub fn payload_as_json<T: serde::de::DeserializeOwned>(&self) -> ParseResult<T> {
        serde_json::from_slice(&self.payload).map_err(ParseError::PayloadDeserialize)
    }

    /// 将 payload 解析为字符串
    pub fn payload_as_str(&self) -> String {
        String::from_utf8_lossy(&self.payload).to_string()
    }
}

/// 尝试从缓冲区解析一个完整的帧
///
/// 这是一个无状态的纯函数，每次调用独立解析。
/// 缓冲区管理由上层 `EventStreamDecoder` 负责。
///
/// # Arguments
/// * `buffer` - 输入缓冲区
///
/// # Returns
/// - `Ok(Some((frame, consumed)))` - 成功解析，返回帧和消费的字节数
/// - `Ok(None)` - 数据不足，需要更多数据
/// - `Err(e)` - 解析错误
pub fn parse_frame(buffer: &[u8]) -> ParseResult<Option<(Frame, usize)>> {
    // 检查是否有足够的数据读取 prelude
    if buffer.len() < PRELUDE_SIZE {
        return Ok(None);
    }

    // 读取 prelude
    let total_length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let header_length = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
    let prelude_crc = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);

    // 验证消息长度范围
    if total_length < MIN_MESSAGE_SIZE as u32 {
        return Err(ParseError::MessageTooSmall {
            length: total_length,
            min: MIN_MESSAGE_SIZE as u32,
        });
    }

    if total_length > MAX_MESSAGE_SIZE {
        return Err(ParseError::MessageTooLarge {
            length: total_length,
            max: MAX_MESSAGE_SIZE,
        });
    }

    let total_length = total_length as usize;
    let header_length = header_length as usize;

    // 检查是否有完整的消息
    if buffer.len() < total_length {
        return Ok(None);
    }

    // 验证 Prelude CRC
    let actual_prelude_crc = crc32(&buffer[..8]);
    if actual_prelude_crc != prelude_crc {
        return Err(ParseError::PreludeCrcMismatch {
            expected: prelude_crc,
            actual: actual_prelude_crc,
        });
    }

    // 读取 Message CRC
    let message_crc = u32::from_be_bytes([
        buffer[total_length - 4],
        buffer[total_length - 3],
        buffer[total_length - 2],
        buffer[total_length - 1],
    ]);

    // 验证 Message CRC (对整个消息不含最后4字节)
    let actual_message_crc = crc32(&buffer[..total_length - 4]);
    if actual_message_crc != message_crc {
        return Err(ParseError::MessageCrcMismatch {
            expected: message_crc,
            actual: actual_message_crc,
        });
    }

    // 解析头部
    let headers_start = PRELUDE_SIZE;
    let headers_end = headers_start + header_length;

    // 验证头部边界
    if headers_end > total_length - 4 {
        return Err(ParseError::HeaderParseFailed(
            "头部长度超出消息边界".to_string(),
        ));
    }

    let headers = parse_headers(&buffer[headers_start..headers_end], header_length)?;

    // 提取 payload (去除最后4字节的 message_crc)
    let payload_start = headers_end;
    let payload_end = total_length - 4;
    let payload = buffer[payload_start..payload_end].to_vec();

    Ok(Some((Frame { headers, payload }, total_length)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_insufficient_data() {
        let buffer = [0u8; 10]; // 小于 PRELUDE_SIZE
        assert!(matches!(parse_frame(&buffer), Ok(None)));
    }

    #[test]
    fn test_frame_message_too_small() {
        // 构造一个 total_length = 10 的 prelude (小于最小值)
        let mut buffer = vec![0u8; 16];
        buffer[0..4].copy_from_slice(&10u32.to_be_bytes()); // total_length
        buffer[4..8].copy_from_slice(&0u32.to_be_bytes()); // header_length
        let prelude_crc = crc32(&buffer[0..8]);
        buffer[8..12].copy_from_slice(&prelude_crc.to_be_bytes());

        let result = parse_frame(&buffer);
        assert!(matches!(result, Err(ParseError::MessageTooSmall { .. })));
    }
}
