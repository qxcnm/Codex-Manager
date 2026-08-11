//! CRC32 校验实现
//!
//! AWS Event Stream 使用 CRC32 (ISO-HDLC/以太网/ZIP 标准)

use crc::{Crc, CRC_32_ISO_HDLC};

/// CRC32 计算器实例 (ISO-HDLC 标准，多项式 0xEDB88320)
const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

/// 计算 CRC32 校验和 (ISO-HDLC 标准)
///
/// # Arguments
/// * `data` - 要计算校验和的数据
///
/// # Returns
/// CRC32 校验和值
pub fn crc32(data: &[u8]) -> u32 {
    CRC32.checksum(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_empty() {
        // 空数据的 CRC32 应该是 0
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn test_crc32_known_value() {
        // "123456789" 的 CRC32 (ISO-HDLC) 值是 0xCBF43926
        let data = b"123456789";
        assert_eq!(crc32(data), 0xCBF43926);
    }
}
