//! Opt-in model-route scheduling. Kept in capabilities JSON so old catalogs and
//! all storage backends retain the same schema and default behavior.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoutePolicyV2 {
    #[default]
    Legacy,
    AccountFirst,
    AggregateFirst,
    Weighted,
}

impl ModelRoutePolicyV2 {
    pub fn from_capabilities(capabilities: &Value) -> Result<Self, String> {
        match capabilities.get("route_policy") {
            None => Ok(Self::Legacy),
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| {
                "invalid route_policy: expected legacy, account_first, aggregate_first or weighted"
                    .to_string()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_policy_is_opt_in_and_strict() {
        assert_eq!(
            ModelRoutePolicyV2::from_capabilities(&serde_json::json!({})).unwrap(),
            ModelRoutePolicyV2::Legacy
        );
        for policy in ["legacy", "account_first", "aggregate_first", "weighted"] {
            assert!(ModelRoutePolicyV2::from_capabilities(
                &serde_json::json!({"route_policy": policy})
            )
            .is_ok());
        }
        for value in [
            serde_json::json!(null),
            serde_json::json!(true),
            serde_json::json!("typo"),
        ] {
            assert!(ModelRoutePolicyV2::from_capabilities(
                &serde_json::json!({"route_policy": value})
            )
            .is_err());
        }
    }
}
