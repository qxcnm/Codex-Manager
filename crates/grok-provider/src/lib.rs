//! Protocol primitives for a Grok Web provider adapter.
//!
//! This crate deliberately does not perform login, persist credentials, choose
//! proxies, or send HTTP requests. Those responsibilities stay at the service
//! boundary so the provider remains independently testable.

pub mod catalog;
pub mod headers;
pub mod quota;
pub mod request;
pub mod statsig;
pub mod stream;

pub use catalog::{
    models_for_tier, GrokModelCapability, GrokModelSpec, GrokWebTier, GROK_WEB_MODELS,
};
pub use headers::{
    build_web_headers, GrokHeaderError, GrokWebHeaderProfile, SecretSsoToken, SensitiveHeaders,
};
pub use quota::{infer_tier_from_quota, GrokQuotaMode, GrokRateLimitRequest, GrokRateLimitWindow};
pub use request::{DeviceEnvironment, GrokChatMode, GrokWebChatRequest, GrokWebConversationTarget};
pub use statsig::{
    validate_statsig_id, StatsigEnvironment, StatsigSignRequest, StatsigSignResponse,
};
pub use stream::{GrokStreamError, GrokWebEvent, GrokWebStreamParser};
