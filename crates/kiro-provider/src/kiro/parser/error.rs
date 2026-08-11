//! AWS Event Stream 解析错误定义

use std::fmt;

/// 解析错误类型
#[derive(Debug)]
pub enum ParseError {
    /// 数据不足，需要更多字节
    Incomplete { needed: usize, available: usize },
    /// Prelude CRC 校验失败
    PreludeCrcMismatch { expected: u32, actual: u32 },
    /// Message CRC 校验失败
    MessageCrcMismatch { expected: u32, actual: u32 },
    /// 无效的头部值类型
    InvalidHeaderType(u8),
    /// 头部解析错误
    HeaderParseFailed(String),
    /// 消息长度超限
    MessageTooLarge { length: u32, max: u32 },
    /// 消息长度过小
    MessageTooSmall { length: u32, min: u32 },
    /// 无效的消息类型
    InvalidMessageType(String),
    /// Payload 反序列化失败
    PayloadDeserialize(serde_json::Error),
    /// IO 错误
    Io(std::io::Error),
    /// 连续错误过多，解码器已停止
    TooManyErrors { count: usize, last_error: String },
    /// 缓冲区溢出
    BufferOverflow { size: usize, max: usize },
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete { needed, available } => {
                write!(f, "数据不足: 需要 {} 字节, 当前 {} 字节", needed, available)
            }
            Self::PreludeCrcMismatch { expected, actual } => {
                write!(
                    f,
                    "Prelude CRC 校验失败: 期望 0x{:08x}, 实际 0x{:08x}",
                    expected, actual
                )
            }
            Self::MessageCrcMismatch { expected, actual } => {
                write!(
                    f,
                    "Message CRC 校验失败: 期望 0x{:08x}, 实际 0x{:08x}",
                    expected, actual
                )
            }
            Self::InvalidHeaderType(t) => write!(f, "无效的头部值类型: {}", t),
            Self::HeaderParseFailed(msg) => write!(f, "头部解析失败: {}", msg),
            Self::MessageTooLarge { length, max } => {
                write!(f, "消息长度超限: {} 字节 (最大 {})", length, max)
            }
            Self::MessageTooSmall { length, min } => {
                write!(f, "消息长度过小: {} 字节 (最小 {})", length, min)
            }
            Self::InvalidMessageType(t) => write!(f, "无效的消息类型: {}", t),
            Self::PayloadDeserialize(e) => write!(f, "Payload 反序列化失败: {}", e),
            Self::Io(e) => write!(f, "IO 错误: {}", e),
            Self::TooManyErrors { count, last_error } => {
                write!(
                    f,
                    "连续错误过多 ({} 次)，解码器已停止: {}",
                    count, last_error
                )
            }
            Self::BufferOverflow { size, max } => {
                write!(f, "缓冲区溢出: {} 字节 (最大 {})", size, max)
            }
        }
    }
}

impl From<std::io::Error> for ParseError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        Self::PayloadDeserialize(e)
    }
}

/// 解析结果类型
pub type ParseResult<T> = Result<T, ParseError>;
