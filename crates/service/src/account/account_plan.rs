use codexmanager_core::auth::parse_id_token_claims;
use serde_json::Value;

pub(crate) fn extract_plan_type_from_id_token(id_token: &str) -> Option<String> {
    parse_id_token_claims(id_token)
        .ok()
        .and_then(|claims| claims.auth)
        .and_then(|auth| auth.chatgpt_plan_type)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub(crate) fn is_free_plan_type(plan_type: Option<&str>) -> bool {
    let Some(plan_type) = plan_type else {
        return false;
    };
    let normalized = plan_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("free")
}

pub(crate) fn is_free_plan_from_credits_json(raw_credits_json: Option<&str>) -> bool {
    let Some(raw_credits_json) = raw_credits_json else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_credits_json) else {
        return false;
    };
    let keys = [
        "plan_type",
        "planType",
        "subscription_tier",
        "subscriptionTier",
        "tier",
        "account_type",
        "accountType",
        "type",
    ];
    let extracted = extract_string_by_keys_recursive(&value, &keys);
    is_free_plan_type(extracted.as_deref())
}

fn extract_string_by_keys_recursive(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(object) = value.as_object() {
        for key in keys {
            let candidate = object
                .get(*key)
                .and_then(Value::as_str)
                .map(|text| text.trim().to_ascii_lowercase())
                .filter(|text| !text.is_empty());
            if candidate.is_some() {
                return candidate;
            }
        }
        for child in object.values() {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
        return None;
    }
    if let Some(array) = value.as_array() {
        for child in array {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        extract_plan_type_from_id_token, is_free_plan_from_credits_json, is_free_plan_type,
    };

    fn encode_base64url(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        let mut index = 0;
        while index + 3 <= bytes.len() {
            let chunk = ((bytes[index] as u32) << 16)
                | ((bytes[index + 1] as u32) << 8)
                | (bytes[index + 2] as u32);
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
            out.push(TABLE[(chunk & 0x3f) as usize] as char);
            index += 3;
        }
        match bytes.len().saturating_sub(index) {
            1 => {
                let chunk = (bytes[index] as u32) << 16;
                out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
                out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            }
            2 => {
                let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
                out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
                out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
                out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
            }
            _ => {}
        }
        out
    }

    #[test]
    fn free_plan_detection_accepts_common_variants() {
        assert!(is_free_plan_type(Some("free")));
        assert!(is_free_plan_type(Some("ChatGPT_Free")));
        assert!(is_free_plan_type(Some("free_tier")));
    }

    #[test]
    fn free_plan_detection_rejects_paid_or_unknown_variants() {
        assert!(!is_free_plan_type(None));
        assert!(!is_free_plan_type(Some("")));
        assert!(!is_free_plan_type(Some("plus")));
        assert!(!is_free_plan_type(Some("pro")));
        assert!(!is_free_plan_type(Some("team")));
    }

    #[test]
    fn free_plan_detection_accepts_credits_json_marker() {
        let credits_json = r#"{"planType":"free"}"#;
        assert!(is_free_plan_from_credits_json(Some(credits_json)));
    }

    #[test]
    fn extract_plan_type_from_id_token_reads_chatgpt_claim() {
        let header = encode_base64url(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = encode_base64url(
            serde_json::json!({
                "sub": "acc-plan-free",
                "https://api.openai.com/auth": {
                    "chatgpt_plan_type": "free"
                }
            })
            .to_string()
            .as_bytes(),
        );
        let token = format!("{header}.{payload}.sig");
        assert_eq!(
            extract_plan_type_from_id_token(&token).as_deref(),
            Some("free")
        );
    }
}
