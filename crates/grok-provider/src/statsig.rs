use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsigEnvironment {
    pub meta_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsigSignRequest {
    pub method: String,
    pub path: String,
    pub environment: StatsigEnvironment,
}

impl StatsigSignRequest {
    pub fn new(method: &str, path: &str, meta_content: impl Into<String>) -> Self {
        Self {
            method: method.trim().to_ascii_uppercase(),
            path: normalize_path(path),
            environment: StatsigEnvironment {
                meta_content: meta_content.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsigSignResponse {
    #[serde(rename = "x-statsig-id")]
    pub statsig_id: String,
}

impl StatsigSignResponse {
    pub fn validated_id(&self) -> Option<&str> {
        validate_statsig_id(&self.statsig_id).then_some(self.statsig_id.trim())
    }
}

/// Current Grok Web signatures are base64-encoded 70-byte envelopes.
pub fn validate_statsig_id(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    general_purpose::STANDARD_NO_PAD
        .decode(value)
        .or_else(|_| general_purpose::STANDARD.decode(value))
        .is_ok_and(|decoded| decoded.len() == 70)
}

fn normalize_path(value: &str) -> String {
    let trimmed = value.trim();
    let path = trimmed
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|index| &rest[index..]))
        .unwrap_or(trimmed);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.is_empty() {
        "/".into()
    } else if path.starts_with('/') {
        path.into()
    } else {
        format!("/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signer_payload_uses_uppercase_method_and_path_only() {
        let request = StatsigSignRequest::new(
            " post ",
            "https://grok.com/rest/rate-limits?ignored=1",
            "verification",
        );
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "method":"POST",
                "path":"/rest/rate-limits",
                "environment":{"metaContent":"verification"}
            })
        );
    }

    #[test]
    fn validates_only_current_seventy_byte_envelope_shape() {
        let valid = general_purpose::STANDARD_NO_PAD.encode([7_u8; 70]);
        assert!(validate_statsig_id(&valid));
        assert!(!validate_statsig_id("not-base64"));
        assert!(!validate_statsig_id(
            &general_purpose::STANDARD_NO_PAD.encode([7_u8; 69])
        ));
    }
}
