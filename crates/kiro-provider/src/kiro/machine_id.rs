//! 设备指纹生成器
//!

use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kiro::model::credentials::KiroCredentials;
use crate::model::config::Config;

/// 兜底 machineId 缓存（按凭据 id 分桶，进程生命周期内稳定）
///
/// key 为 `credentials.id`；无 id 的凭据共享同一个兜底值（正常流程不会出现）。
static FALLBACK_MACHINE_IDS: OnceLock<Mutex<HashMap<Option<u64>, String>>> = OnceLock::new();

/// 标准化 machineId 格式
///
/// 支持以下格式：
/// - 64 字符十六进制字符串（直接返回）
/// - UUID 格式（如 "2582956e-cc88-4669-b546-07adbffcb894"，移除连字符后补齐到 64 字符）
fn normalize_machine_id(machine_id: &str) -> Option<String> {
    let trimmed = machine_id.trim();

    // 如果已经是 64 字符，直接返回
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed.to_string());
    }

    // 尝试解析 UUID 格式（移除连字符）
    let without_dashes: String = trimmed.chars().filter(|c| *c != '-').collect();

    // UUID 去掉连字符后是 32 字符
    if without_dashes.len() == 32 && without_dashes.chars().all(|c| c.is_ascii_hexdigit()) {
        // 补齐到 64 字符（重复一次）
        return Some(format!("{}{}", without_dashes, without_dashes));
    }

    // 无法识别的格式
    None
}

/// 根据凭证信息生成唯一的 Machine ID
///
/// 优先级：
/// 1. 凭据级 `machineId`（若配置且格式合法）
/// 2. 全局 `config.machineId`（若配置且格式合法）
/// 3. 根据凭据类型派生（互斥，由 [`KiroCredentials::is_api_key_credential`] 分流）：
///    - API Key 凭据：基于 `kiroApiKey` 派生
///    - OAuth 凭据：基于 `refreshToken` 派生
/// 4. 兜底：基于随机种子派生，按 `credentials.id` 在进程内缓存（首次触发 warn 日志）
pub fn generate_from_credentials(credentials: &KiroCredentials, config: &Config) -> String {
    // 如果配置了凭据级 machineId，优先使用
    if let Some(ref machine_id) = credentials.machine_id {
        if let Some(normalized) = normalize_machine_id(machine_id) {
            return normalized;
        }
    }

    // 如果配置了全局 machineId，作为默认值
    if let Some(ref machine_id) = config.machine_id {
        if let Some(normalized) = normalize_machine_id(machine_id) {
            return normalized;
        }
    }

    // 按凭据类型派生（API Key 与 refreshToken 两条路径互斥，不回落）
    if credentials.is_api_key_credential() {
        // API Key 凭据：基于 kiroApiKey 派生
        if let Some(ref api_key) = credentials.kiro_api_key {
            if !api_key.is_empty() {
                return sha256_hex(&format!("KiroAPIKey/{}", api_key));
            }
        }
    } else if let Some(ref refresh_token) = credentials.refresh_token {
        // OAuth 凭据：基于 refreshToken 派生
        if !refresh_token.is_empty() {
            return sha256_hex(&format!("KotlinNativeAPI/{}", refresh_token));
        }
    }

    // 兜底：走派生流程生成随机 machineId，按凭据 id 进程内稳定
    fallback_machine_id(credentials)
}

/// 为缺失派生材料的凭据生成兜底 machineId
///
/// - 仍经 `sha256("KiroFallback/<uuid>")` 派生，输出格式与正常路径一致（64 字符十六进制）
/// - 按 `credentials.id` 在进程内缓存；同一凭据多次调用返回同一值
/// - 进程重启会重新随机；不持久化
/// - 每个凭据首次生成时 warn 一次
fn fallback_machine_id(credentials: &KiroCredentials) -> String {
    let cache = FALLBACK_MACHINE_IDS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock();
    if let Some(existing) = map.get(&credentials.id) {
        return existing.clone();
    }

    let seed = Uuid::new_v4();
    let derived = sha256_hex(&format!("KiroFallback/{}", seed));
    tracing::warn!(
        credential_id = ?credentials.id,
        "凭据缺少派生材料（kiroApiKey/refreshToken 均不可用），使用随机兜底 machineId（进程内稳定）"
    );
    map.insert(credentials.id, derived.clone());
    derived
}

/// SHA256 哈希实现（返回十六进制字符串）
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let result = sha256_hex("test");
        assert_eq!(result.len(), 64);
        assert_eq!(
            result,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn test_generate_with_custom_machine_id() {
        let credentials = KiroCredentials::default();
        let mut config = Config::default();
        config.machine_id = Some("a".repeat(64));

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result, "a".repeat(64));
    }

    #[test]
    fn test_generate_with_credential_machine_id_overrides_config() {
        let mut credentials = KiroCredentials::default();
        credentials.machine_id = Some("b".repeat(64));

        let mut config = Config::default();
        config.machine_id = Some("a".repeat(64));

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result, "b".repeat(64));
    }

    #[test]
    fn test_generate_with_refresh_token() {
        let mut credentials = KiroCredentials::default();
        credentials.refresh_token = Some("test_refresh_token".to_string());
        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_generate_without_credentials_uses_fallback() {
        // 完全空凭据会走兜底分支，返回派生后的随机 machineId
        let credentials = KiroCredentials::default();
        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result.len(), 64);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_with_api_key() {
        let mut credentials = KiroCredentials::default();
        credentials.kiro_api_key = Some("ksk_test_api_key".to_string());
        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result.len(), 64);
        // 应与 KiroAPIKey/<api_key> 的哈希一致
        assert_eq!(result, sha256_hex("KiroAPIKey/ksk_test_api_key"));
    }

    #[test]
    fn test_api_key_and_refresh_token_are_mutually_exclusive() {
        // 同时存在 kiroApiKey 和 refreshToken 时，应走 API Key 分支
        let mut credentials = KiroCredentials::default();
        credentials.kiro_api_key = Some("ksk_test".to_string());
        credentials.refresh_token = Some("should_not_be_used".to_string());
        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result, sha256_hex("KiroAPIKey/ksk_test"));
    }

    #[test]
    fn test_api_key_auth_method_empty_uses_fallback_not_refresh_token() {
        // auth_method=api_key 但 kiro_api_key 为空：不回落到 refreshToken，走兜底分支
        let mut credentials = KiroCredentials::default();
        credentials.id = Some(u64::MAX - 1);
        credentials.auth_method = Some("api_key".to_string());
        credentials.refresh_token = Some("should_not_be_used".to_string());
        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result.len(), 64);
        // 必须不是基于 refresh_token 派生的值（互斥性验证）
        assert_ne!(result, sha256_hex("KotlinNativeAPI/should_not_be_used"));
    }

    #[test]
    fn test_fallback_is_stable_per_credential() {
        // 同一凭据（按 id 区分）多次调用兜底应返回同一值
        let mut credentials = KiroCredentials::default();
        credentials.id = Some(u64::MAX - 10);
        let config = Config::default();

        let first = generate_from_credentials(&credentials, &config);
        let second = generate_from_credentials(&credentials, &config);
        assert_eq!(first, second);
    }

    #[test]
    fn test_fallback_differs_across_credentials() {
        // 不同凭据（不同 id）的兜底值应互不相同
        let mut cred_a = KiroCredentials::default();
        cred_a.id = Some(u64::MAX - 20);
        let mut cred_b = KiroCredentials::default();
        cred_b.id = Some(u64::MAX - 21);
        let config = Config::default();

        let id_a = generate_from_credentials(&cred_a, &config);
        let id_b = generate_from_credentials(&cred_b, &config);
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn test_normalize_uuid_format() {
        // UUID 格式应该被转换为 64 字符
        let uuid = "2582956e-cc88-4669-b546-07adbffcb894";
        let result = normalize_machine_id(uuid);
        assert!(result.is_some());
        let normalized = result.unwrap();
        assert_eq!(normalized.len(), 64);
        // UUID 去掉连字符后重复一次
        assert_eq!(
            normalized,
            "2582956ecc884669b54607adbffcb8942582956ecc884669b54607adbffcb894"
        );
    }

    #[test]
    fn test_normalize_64_char_hex() {
        // 64 字符十六进制应该直接返回
        let hex64 = "a".repeat(64);
        let result = normalize_machine_id(&hex64);
        assert_eq!(result, Some(hex64));
    }

    #[test]
    fn test_normalize_invalid_format() {
        // 无效格式应该返回 None
        assert!(normalize_machine_id("invalid").is_none());
        assert!(normalize_machine_id("too-short").is_none());
        assert!(normalize_machine_id(&"g".repeat(64)).is_none()); // 非十六进制
    }

    #[test]
    fn test_generate_with_uuid_machine_id() {
        let mut credentials = KiroCredentials::default();
        credentials.machine_id = Some("2582956e-cc88-4669-b546-07adbffcb894".to_string());

        let config = Config::default();

        let result = generate_from_credentials(&credentials, &config);
        assert_eq!(result.len(), 64);
    }
}
