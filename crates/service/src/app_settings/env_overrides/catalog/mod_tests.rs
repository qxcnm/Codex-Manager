use super::{env_override_catalog_value, ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC};

#[test]
fn catalog_marks_request_semantic_env_overrides_as_high_risk() {
    let catalog = env_override_catalog_value();
    let strict_allowlist = catalog
        .iter()
        .find(|item| {
            item.get("key").and_then(|value| value.as_str())
                == Some("CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST")
        })
        .expect("strict allowlist catalog item");

    assert_eq!(
        strict_allowlist
            .get("riskLevel")
            .and_then(|value| value.as_str()),
        Some("high")
    );
    assert_eq!(
        strict_allowlist
            .get("effectScope")
            .and_then(|value| value.as_str()),
        Some(ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC)
    );
    assert!(strict_allowlist
        .get("safetyNote")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .contains("请求语义"));

    let image_enabled = catalog
        .iter()
        .find(|item| {
            item.get("key").and_then(|value| value.as_str())
                == Some("CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED")
        })
        .expect("image generation enabled catalog item");

    assert_eq!(
        image_enabled
            .get("riskLevel")
            .and_then(|value| value.as_str()),
        Some("high")
    );
    assert_eq!(
        image_enabled
            .get("effectScope")
            .and_then(|value| value.as_str()),
        Some(ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC)
    );

    let image_main_model = catalog
        .iter()
        .find(|item| {
            item.get("key").and_then(|value| value.as_str())
                == Some("CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL")
        })
        .expect("image main model catalog item");
    assert_eq!(
        image_main_model
            .get("defaultValue")
            .and_then(|value| value.as_str()),
        Some("gpt-6-luna")
    );

    let zstd_limit = catalog
        .iter()
        .find(|item| {
            item.get("key").and_then(|value| value.as_str())
                == Some("CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES")
        })
        .expect("zstd body limit catalog item");

    assert_eq!(
        zstd_limit.get("applyMode").and_then(|value| value.as_str()),
        Some("runtime")
    );
    assert_eq!(
        zstd_limit
            .get("effectScope")
            .and_then(|value| value.as_str()),
        Some(ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC)
    );

    let text_limit = catalog
        .iter()
        .find(|item| {
            item.get("key").and_then(|value| value.as_str())
                == Some("CODEXMANAGER_MAX_TEXT_INPUT_CHARS")
        })
        .expect("text input limit catalog item");
    assert_eq!(
        text_limit
            .get("defaultValue")
            .and_then(|value| value.as_str()),
        Some("1048576")
    );
    assert_eq!(
        text_limit.get("applyMode").and_then(|value| value.as_str()),
        Some("runtime")
    );
    assert_eq!(
        text_limit.get("riskLevel").and_then(|value| value.as_str()),
        Some("high")
    );
    assert_eq!(
        text_limit
            .get("effectScope")
            .and_then(|value| value.as_str()),
        Some(ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC)
    );
}
