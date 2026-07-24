use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};

#[derive(serde::Serialize)]
struct CompatibleModelsResponse<'a> {
    object: &'static str,
    data: Vec<ApiModelInfo<'a>>,
    models: &'a [ModelInfo],
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
    let data = models
        .models
        .iter()
        .filter(|model| model.supported_in_api)
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
        // Codex CLI consumes the private `models` catalog while generic OpenAI
        // clients consume `data`, so both representations must stay populated.
        models: &models.models,
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
    let Some(allowed_slugs) = crate::allowed_model_slugs_for_api_key(storage, key_id)? else {
        return Ok((models, true));
    };
    Ok((
        ModelsResponse {
            models: models
                .models
                .into_iter()
                .filter(|model| allowed_slugs.contains(model.slug.as_str()))
                .collect(),
            extra: models.extra,
        },
        false,
    ))
}

fn filter_models_for_catalog_policy(
    storage: &codexmanager_core::storage::Storage,
    key_id: &str,
    models: ModelsResponse,
    policy: crate::codex_model_catalog::GatewayCatalogPolicy,
) -> Result<(ModelsResponse, bool), String> {
    match policy {
        crate::codex_model_catalog::GatewayCatalogPolicy::OfficialAccountPool => Ok((models, true)),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed => {
            filter_models_for_key(storage, key_id, models)
        }
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
    key_id: &str,
) -> Result<
    (
        ModelsResponse,
        crate::codex_model_catalog::GatewayCatalogPolicy,
    ),
    String,
> {
    crate::codex_model_catalog::models_response_for_gateway_key(storage, key_id)
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
    let (cached, catalog_policy) = match read_cached_models_response(storage, key_id) {
        Ok(result) => result,
        Err(err) => {
            let message = crate::gateway::bilingual_error(
                "读取模型缓存失败",
                format!("model options cache read failed: {err}"),
            );
            super::local_response::respond_local_terminal_error(request, &context, 503, message)?;
            return Ok(None);
        }
    };

    let (output_models, include_implicit_models) =
        filter_models_for_catalog_policy(storage, key_id, cached, catalog_policy)?;
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
