use serde::{Deserialize, Serialize};

/// Grok Web subscription level inferred by the service from upstream quota data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrokWebTier {
    Basic,
    Super,
    Heavy,
}

impl GrokWebTier {
    pub const fn supports(self, minimum: Self) -> bool {
        self as u8 >= minimum as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrokModelCapability {
    Chat,
    ImageGeneration,
    ImageEdit,
    VideoGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrokModelSpec {
    pub public_id: &'static str,
    pub upstream_model: &'static str,
    pub protocol_model: Option<&'static str>,
    pub mode: Option<&'static str>,
    pub capability: GrokModelCapability,
    pub minimum_tier: GrokWebTier,
}

pub const GROK_WEB_MODELS: &[GrokModelSpec] = &[
    GrokModelSpec {
        public_id: "grok-chat-fast",
        upstream_model: "grok-chat-fast",
        protocol_model: None,
        mode: Some("fast"),
        capability: GrokModelCapability::Chat,
        minimum_tier: GrokWebTier::Basic,
    },
    GrokModelSpec {
        public_id: "grok-chat-auto",
        upstream_model: "grok-chat-auto",
        protocol_model: None,
        mode: Some("auto"),
        capability: GrokModelCapability::Chat,
        minimum_tier: GrokWebTier::Super,
    },
    GrokModelSpec {
        public_id: "grok-chat-expert",
        upstream_model: "grok-chat-expert",
        protocol_model: None,
        mode: Some("expert"),
        capability: GrokModelCapability::Chat,
        minimum_tier: GrokWebTier::Super,
    },
    GrokModelSpec {
        public_id: "grok-chat-heavy",
        upstream_model: "grok-chat-heavy",
        protocol_model: None,
        mode: Some("heavy"),
        capability: GrokModelCapability::Chat,
        minimum_tier: GrokWebTier::Heavy,
    },
    GrokModelSpec {
        public_id: "grok-imagine-image",
        upstream_model: "grok-imagine-image",
        protocol_model: Some("imagine-lite"),
        mode: Some("fast"),
        capability: GrokModelCapability::ImageGeneration,
        minimum_tier: GrokWebTier::Basic,
    },
    GrokModelSpec {
        public_id: "grok-imagine-image-quality",
        upstream_model: "grok-imagine-image-quality",
        protocol_model: Some("imagine"),
        mode: None,
        capability: GrokModelCapability::ImageGeneration,
        minimum_tier: GrokWebTier::Super,
    },
    GrokModelSpec {
        public_id: "grok-imagine-image-edit",
        upstream_model: "imagine-image-edit",
        protocol_model: None,
        mode: None,
        capability: GrokModelCapability::ImageEdit,
        minimum_tier: GrokWebTier::Super,
    },
    GrokModelSpec {
        public_id: "grok-imagine-video",
        upstream_model: "grok-imagine-video",
        protocol_model: Some("imagine-video-gen"),
        mode: None,
        capability: GrokModelCapability::VideoGeneration,
        minimum_tier: GrokWebTier::Super,
    },
];

pub fn models_for_tier(tier: GrokWebTier) -> impl Iterator<Item = &'static GrokModelSpec> {
    GROK_WEB_MODELS
        .iter()
        .filter(move |model| tier.supports(model.minimum_tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_filter_never_exposes_higher_tier_models() {
        let basic: Vec<_> = models_for_tier(GrokWebTier::Basic)
            .map(|model| model.public_id)
            .collect();
        assert_eq!(basic, ["grok-chat-fast", "grok-imagine-image"]);

        let super_models: Vec<_> = models_for_tier(GrokWebTier::Super)
            .map(|model| model.public_id)
            .collect();
        assert!(super_models.contains(&"grok-chat-auto"));
        assert!(!super_models.contains(&"grok-chat-heavy"));

        assert_eq!(
            models_for_tier(GrokWebTier::Heavy).count(),
            GROK_WEB_MODELS.len()
        );
    }
}
