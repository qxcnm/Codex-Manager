use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};
const MODEL_CACHE_SCOPE_DEFAULT: &str = "default";

#[derive(serde::Serialize)]
struct CompatibleModelsResponse<'a> {
    object: &'static str,
    data: Vec<ApiModelInfo<'a>>,
    models: Vec<&'a ModelInfo>,
}

#[derive(serde::Serialize)]
struct ApiModelInfo<'a> {
    id: &'a str,
    object: &'static str,
    created: i64,
    owned_by: &'static str,
    display_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

fn serialize_models_response_body(models: &ModelsResponse) -> String {
    let public_models = models
        .models
        .iter()
        .filter(|model| {
            model.supported_in_api
                && crate::kiro::routing::is_public_model_visibility(model.visibility.as_deref())
        })
        .collect::<Vec<_>>();
    let data = public_models
        .iter()
        .map(|model| ApiModelInfo {
            id: model.slug.as_str(),
            object: "model",
            created: 0,
            owned_by: "codexmanager",
            display_name: model.display_name.as_str(),
            description: model.description.as_deref(),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&CompatibleModelsResponse {
        object: "list",
        data,
        models: public_models,
    })
    .unwrap_or_else(|_| "{\"object\":\"list\",\"data\":[],\"models\":[]}".to_string())
}

fn serialize_models_response(models: &ModelsResponse) -> String {
    serialize_models_response_body(models)
}

fn filter_models_for_key(
    storage: &codexmanager_core::storage::Storage,
    key_id: &str,
    models: ModelsResponse,
) -> Result<(ModelsResponse, bool), String> {
    let key_policy = storage
        .find_api_key_policy(key_id)
        .map_err(|err| format!("read api key policy failed: {err}"))?
        .unwrap_or_default();
    let Some(allowed_slugs) = crate::allowed_model_slugs_for_api_key(storage, key_id)? else {
        let models = filter_models_for_policy(models, &key_policy);
        return Ok((models, true));
    };
    Ok((
        filter_models_for_policy(
            ModelsResponse {
                models: models
                    .models
                    .into_iter()
                    .filter(|model| {
                        allowed_slugs.contains(model.slug.as_str())
                            || (crate::kiro::routing::is_smart_alias(Some(model.slug.as_str()))
                                && !allowed_slugs.is_empty())
                            || model
                                .slug
                                .strip_prefix("codex/")
                                .is_some_and(|slug| allowed_slugs.contains(slug))
                    })
                    .collect(),
                extra: models.extra,
            },
            &key_policy,
        ),
        false,
    ))
}

fn filter_models_for_policy(
    models: ModelsResponse,
    policy: &codexmanager_core::storage::ApiKeyPolicy,
) -> ModelsResponse {
    ModelsResponse {
        models: models
            .models
            .into_iter()
            .filter(|model| {
                let model_allowed = policy.allowed_models.is_empty()
                    || policy.allowed_models.iter().any(|allowed| {
                        allowed == "*" || allowed.eq_ignore_ascii_case(model.slug.as_str())
                    });
                let platform_allowed = if model.slug.starts_with("kiro/") {
                    policy.allowed_platforms.is_empty()
                        || policy
                            .allowed_platforms
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case("kiro"))
                } else if model.slug.starts_with("grok/") {
                    policy.allowed_platforms.is_empty()
                        || policy
                            .allowed_platforms
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case("grok"))
                } else if model.slug.starts_with("codex/") || model.slug.starts_with("gpt-") {
                    policy.allowed_platforms.is_empty()
                        || policy
                            .allowed_platforms
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case("codex"))
                } else if crate::kiro::routing::is_smart_alias(Some(model.slug.as_str())) {
                    true
                } else {
                    true
                };
                model_allowed && platform_allowed
            })
            .collect(),
        extra: models.extra,
    }
}

fn models_etag_header(models: &ModelsResponse) -> Result<Option<tiny_http::Header>, String> {
    let Some(etag) = models.extra.get("etag").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let header = tiny_http::Header::from_bytes(b"etag".as_slice(), etag.as_bytes())
        .map_err(|_| "build etag header failed".to_string())?;
    Ok(Some(header))
}

/// 函数 `read_cached_models_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - storage: 参数 storage
///
/// # 返回
/// 返回函数执行结果
fn read_cached_models_response(
    storage: &codexmanager_core::storage::Storage,
) -> Result<ModelsResponse, String> {
    crate::apikey_models::read_model_options_from_storage(storage)
}

fn append_kiro_models(storage: &codexmanager_core::storage::Storage, models: &mut ModelsResponse) {
    super::provider_runtime::append_provider_models(storage, models);
}

fn append_smart_aliases(models: &mut ModelsResponse) {
    for (slug, display_name, description) in [
        (
            "smart",
            "Smart Route",
            "Balanced capability, health, quota, and latency routing",
        ),
        (
            "coding",
            "Coding Route",
            "Coding-first intelligent model route",
        ),
        ("fast", "Fast Route", "Low-latency intelligent model route"),
        ("cheap", "Cheap Route", "Cost-aware intelligent model route"),
    ] {
        if models.models.iter().any(|model| model.slug == slug) {
            continue;
        }
        models.models.push(ModelInfo {
            slug: slug.into(),
            display_name: display_name.into(),
            description: Some(description.into()),
            supported_in_api: true,
            priority: 110,
            supports_reasoning_summaries: Some(true),
            supports_parallel_tool_calls: Some(true),
            supports_image_detail_original: Some(true),
            context_window: Some(1_000_000),
            experimental_supported_tools: vec!["tools".into(), "web_search".into()],
            input_modalities: vec!["text".into(), "image".into()],
            ..Default::default()
        });
    }
}

fn append_codex_namespaced_models(models: &mut ModelsResponse) {
    let aliases = models
        .models
        .iter()
        .filter(|model| model.slug.starts_with("gpt-"))
        .cloned()
        .map(|mut model| {
            model.slug = format!("codex/{}", model.slug);
            model.display_name = format!("Codex {}", model.display_name);
            model.description = Some("Codex exact model via the unified OpenAI gateway".into());
            model
        })
        .collect::<Vec<_>>();
    for alias in aliases {
        if !models.models.iter().any(|model| model.slug == alias.slug) {
            models.models.push(alias);
        }
    }
}

fn has_active_codex_account(storage: &codexmanager_core::storage::Storage) -> bool {
    storage.list_accounts().ok().is_some_and(|accounts| {
        accounts
            .iter()
            .any(|account| matches!(account.status.trim(), "active" | "available"))
    })
}

/// 函数 `maybe_respond_local_models`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn maybe_respond_local_models(
    request: tiny_http::Request,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    original_path: &str,
    path: &str,
    response_adapter: super::ResponseAdapter,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<tiny_http::Request>, String> {
    let is_models_list = request_method.eq_ignore_ascii_case("GET")
        && (path == "/v1/models" || path.starts_with("/v1/models?"));
    if !is_models_list {
        return Ok(Some(request));
    }
    let context = super::local_response::LocalResponseContext {
        trace_id,
        key_id,
        protocol_type,
        original_path,
        path,
        response_adapter,
        request_method,
        model_for_log,
        reasoning_for_log,
        storage,
    };
    let cached = match read_cached_models_response(storage) {
        Ok(models) => models,
        Err(err) => {
            let message = crate::gateway::bilingual_error(
                "读取模型缓存失败",
                format!("model options cache read failed: {err}"),
            );
            super::local_response::respond_local_terminal_error(request, &context, 503, message)?;
            return Ok(None);
        }
    };

    // Kiro is a first-class provider and must not depend on a Codex account being
    // available. Seed its local catalog before deciding whether a remote Codex
    // model refresh is required; otherwise a Kiro-only installation makes
    // `GET /v1/models` fail while chat requests themselves remain usable.
    let mut locally_available = cached.clone();
    append_kiro_models(storage, &mut locally_available);
    let should_refresh_codex = cached.is_empty() && has_active_codex_account(storage);
    let mut models = if !should_refresh_codex && !locally_available.is_empty() {
        locally_available
    } else {
        match super::fetch_models_for_picker() {
            Ok(fetched) if !fetched.is_empty() => {
                let remote_merged =
                    crate::apikey_models::merge_models_response(cached.clone(), fetched);
                if let Err(err) =
                    crate::apikey_models::save_model_options_with_storage(storage, &remote_merged)
                {
                    log::warn!(
                        "event=gateway_model_catalog_upsert_failed scope={} err={}",
                        MODEL_CACHE_SCOPE_DEFAULT,
                        err
                    );
                }
                crate::apikey_models::merge_models_response(locally_available, remote_merged)
            }
            Ok(_) if locally_available.is_empty() => {
                let message = crate::gateway::bilingual_error(
                    "模型刷新后返回空目录",
                    "models refresh returned empty catalog",
                );
                super::local_response::respond_local_terminal_error(
                    request, &context, 503, message,
                )?;
                return Ok(None);
            }
            Err(err) if locally_available.is_empty() => {
                let message = crate::gateway::bilingual_error(
                    "模型刷新失败",
                    format!("models refresh failed: {err}"),
                );
                super::local_response::respond_local_terminal_error(
                    request, &context, 503, message,
                )?;
                return Ok(None);
            }
            Ok(_) => locally_available,
            Err(err) => {
                log::warn!(
                    "event=gateway_codex_model_catalog_refresh_failed_fallback_to_local err={}",
                    err
                );
                locally_available
            }
        }
    };
    append_codex_namespaced_models(&mut models);
    append_kiro_models(storage, &mut models);
    append_smart_aliases(&mut models);

    let (output_models, include_implicit_models) = filter_models_for_key(storage, key_id, models)?;
    let output = if include_implicit_models {
        serialize_models_response(&output_models)
    } else {
        serialize_models_response_body(&output_models)
    };
    let extra_headers = models_etag_header(&output_models)?.into_iter().collect();
    super::local_response::respond_local_json_with_headers(
        request,
        &context,
        output,
        super::request_log::RequestLogUsage::default(),
        extra_headers,
    )?;
    Ok(None)
}

#[cfg(test)]
#[path = "tests/local_models_tests.rs"]
mod tests;
