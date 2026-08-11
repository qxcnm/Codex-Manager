use serde::{Deserialize, Serialize};

use crate::catalog::GrokWebTier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrokQuotaMode {
    Auto,
    Fast,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokRateLimitRequest {
    pub model_name: GrokQuotaMode,
}

impl GrokRateLimitRequest {
    pub const fn new(model_name: GrokQuotaMode) -> Self {
        Self { model_name }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokRateLimitWindow {
    pub window_size_seconds: i64,
    pub remaining_queries: i64,
    pub total_queries: i64,
}

impl GrokRateLimitWindow {
    pub fn normalized(mut self) -> Self {
        self.total_queries = self.total_queries.max(0);
        self.window_size_seconds = self.window_size_seconds.max(1);
        self.remaining_queries = self.remaining_queries.clamp(0, self.total_queries);
        self
    }
}

/// Infers the minimum safe tier from observed upstream quota shapes. Unknown or
/// contradictory shapes return `None` rather than exposing models speculatively.
pub fn infer_tier_from_quota(
    windows: &[(GrokQuotaMode, GrokRateLimitWindow)],
) -> Option<GrokWebTier> {
    let mut detected = None;
    for (mode, window) in windows {
        let candidate = match (mode, window.total_queries) {
            (GrokQuotaMode::Auto, 7 | 20) | (GrokQuotaMode::Fast, 30) => Some(GrokWebTier::Basic),
            (GrokQuotaMode::Auto, 50) | (GrokQuotaMode::Fast, 140) => Some(GrokWebTier::Super),
            (GrokQuotaMode::Auto, 150) | (GrokQuotaMode::Fast, 400) => Some(GrokWebTier::Heavy),
            (GrokQuotaMode::Heavy, total) if total > 0 => Some(GrokWebTier::Heavy),
            _ => None,
        };
        if let Some(candidate) = candidate {
            detected = Some(match detected {
                Some(current) if current < candidate => current,
                _ => candidate,
            });
        }
    }
    detected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(total_queries: i64) -> GrokRateLimitWindow {
        GrokRateLimitWindow {
            window_size_seconds: 18_000,
            remaining_queries: total_queries,
            total_queries,
        }
    }

    #[test]
    fn serializes_upstream_rate_limit_request_shape() {
        assert_eq!(
            serde_json::to_value(GrokRateLimitRequest::new(GrokQuotaMode::Fast)).unwrap(),
            serde_json::json!({"modelName":"fast"})
        );
    }

    #[test]
    fn recognizes_known_tiers_and_uses_lower_conflicting_tier() {
        assert_eq!(
            infer_tier_from_quota(&[(GrokQuotaMode::Fast, window(30))]),
            Some(GrokWebTier::Basic)
        );
        assert_eq!(
            infer_tier_from_quota(&[
                (GrokQuotaMode::Auto, window(50)),
                (GrokQuotaMode::Fast, window(140)),
            ]),
            Some(GrokWebTier::Super)
        );
        assert_eq!(
            infer_tier_from_quota(&[
                (GrokQuotaMode::Auto, window(150)),
                (GrokQuotaMode::Fast, window(30)),
            ]),
            Some(GrokWebTier::Basic)
        );
        assert_eq!(
            infer_tier_from_quota(&[(GrokQuotaMode::Fast, window(999))]),
            None
        );
    }
}
