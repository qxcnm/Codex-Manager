use crate::http::gateway_request::GatewayRequest as Request;
use sha2::{Digest, Sha256};

use crate::gateway::IncomingHeaderSnapshot;

/// 函数 `find_incoming_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
#[allow(dead_code)]
pub(crate) fn find_incoming_header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

/// 函数 `derive_sticky_conversation_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
#[allow(dead_code)]
pub(crate) fn derive_sticky_conversation_id(request: &Request) -> Option<String> {
    let incoming_headers = IncomingHeaderSnapshot::from_request(request);
    derive_sticky_conversation_id_from_headers(&incoming_headers)
}

/// 函数 `derive_sticky_conversation_id_from_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn derive_sticky_conversation_id_from_headers(
    incoming_headers: &IncomingHeaderSnapshot,
) -> Option<String> {
    derive_sticky_id_from_material(incoming_headers.sticky_key_material(), "conversation")
}

/// 函数 `stable_session_id_from_material`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn stable_session_id_from_material(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// 函数 `derive_sticky_id_from_material`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_material: 参数 key_material
/// - salt: 参数 salt
///
/// # 返回
/// 返回函数执行结果
fn derive_sticky_id_from_material(key_material: Option<&str>, salt: &str) -> Option<String> {
    let key_material = key_material?;
    Some(stable_session_id_from_material(&format!(
        "{salt}:{key_material}"
    )))
}
