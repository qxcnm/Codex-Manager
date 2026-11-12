use super::*;
use serde_json::json;

const MAIN_MODEL: &str = "gpt-6.1-terra";
const TOOL_MODEL: &str = "gpt-image-2.5-flare";
const LEGACY_IMAGE_MODEL: &str = "gpt-image-1.5";

fn image_test_environment() -> Vec<RuntimeEnvGuard> {
    let database_path = std::env::temp_dir().join(format!(
        "cm-image-policy-{:032x}.db",
        rand::random::<u128>()
    ));
    vec![
        RuntimeEnvGuard::set("CODEXMANAGER_DB_PATH", database_path.to_str().unwrap()),
        RuntimeEnvGuard::set("CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED", "1"),
        RuntimeEnvGuard::set("CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL", MAIN_MODEL),
        RuntimeEnvGuard::set("CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL", TOOL_MODEL),
    ]
}

fn image_test_storage() -> crate::storage_helpers::StorageHandle {
    let storage = crate::storage_helpers::open_storage().expect("open isolated storage");
    storage.init().expect("initialize storage");
    if storage.get_managed_model_v2(MAIN_MODEL).unwrap().is_none() {
        let mut model = storage
            .get_managed_model_v2("gpt-6-sol")
            .unwrap()
            .expect("text model template");
        model.id.clear();
        model.slug = MAIN_MODEL.to_string();
        model.display_name = MAIN_MODEL.to_string();
        model.origin = "custom".to_string();
        model.builtin_revision = None;
        model.user_edited = false;
        model.routes.clear();
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: None,
                model,
            })
            .unwrap();
    }
    if storage
        .get_managed_model_v2(LEGACY_IMAGE_MODEL)
        .unwrap()
        .is_some()
    {
        storage.delete_managed_model_v2(LEGACY_IMAGE_MODEL).unwrap();
    }
    storage
}

fn prepare_image_request(
    storage: crate::storage_helpers::StorageHandle,
    path: &str,
    body: Bytes,
    content_type: &str,
    headers: HeaderMap,
    key: ApiKey,
) -> Result<LocalValidationResult, LocalValidationError> {
    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", content_type)
        .body(())
        .unwrap();
    request.headers_mut().extend(headers.clone());
    let (parts, ()) = request.into_parts();
    let (request, _response) = Request::new(parts, body.clone());
    build_local_validation_result(
        &request,
        "trc_image_policy_test".into(),
        crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers),
        storage,
        body,
        key,
    )
}

fn image_request_body(form: &str, model: Option<&str>, stream: bool) -> Bytes {
    if form == "multipart" {
        let model_field = model
            .map(|model| {
                format!(
                    "--image-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{model}\r\n"
                )
            })
            .unwrap_or_default();
        return Bytes::from(format!(
            concat!(
                "{model_field}",
                "--image-boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\n",
                "a red apple\r\n",
                "--image-boundary\r\nContent-Disposition: form-data; name=\"stream\"\r\n\r\n",
                "{stream}\r\n",
                "--image-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\n",
                "Content-Type: image/png\r\n\r\nPNG\r\n",
                "--image-boundary--\r\n"
            ),
            model_field = model_field,
            stream = stream,
        ));
    }
    let mut body = json!({"prompt": "a red apple", "stream": stream});
    if let Some(model) = model {
        body["model"] = json!(model);
    }
    if form == "edit-json" {
        body["images"] = json!([{"image_url":"data:image/png;base64,aW1hZ2U="}]);
    }
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

#[test]
fn account_images_apply_catalog_policies_to_main_model_after_adaptation() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for form in ["generation-json", "edit-json", "multipart"] {
        let path = if form == "generation-json" {
            "/v1/images/generations"
        } else {
            "/v1/images/edits"
        };
        for model in [Some(LEGACY_IMAGE_MODEL), None] {
            for stream in [false, true] {
                let storage = image_test_storage();
                assert!(storage
                    .get_managed_model_v2(LEGACY_IMAGE_MODEL)
                    .unwrap()
                    .is_none());
                // A text-model binding has no role in either Images model selection.
                let key = sample_api_key(
                    crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                    Some("cm-missing-bound-text-model"),
                    Some("high"),
                    Some("fast"),
                );
                let original_body = image_request_body(form, model, stream);
                let prepared = prepare_image_request(
                    storage,
                    path,
                    original_body.clone(),
                    if form == "multipart" {
                        "multipart/form-data; boundary=image-boundary"
                    } else {
                        "application/json"
                    },
                    HeaderMap::new(),
                    key,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{form}, {model:?}, stream={stream}: {} {}",
                        error.status_code, error.message
                    )
                });
                let upstream: Value = serde_json::from_slice(&prepared.body).unwrap();
                assert_eq!(prepared.path, "/v1/responses");
                assert_eq!(prepared.model_for_log.as_deref(), Some(MAIN_MODEL));
                assert_eq!(upstream["model"], MAIN_MODEL);
                assert_eq!(upstream["tools"][0]["model"], model.unwrap_or(TOOL_MODEL));
                assert_eq!(upstream["tools"][0]["type"], "image_generation");
                assert_eq!(upstream["stream"], true);
                assert_eq!(prepared.is_stream, stream);
                assert_eq!(prepared.passthrough_body, original_body);
                assert!(upstream.get("reasoning").is_none());
                assert!(upstream.get("service_tier").is_none());
            }
        }
    }
}

#[test]
fn account_images_still_reject_unavailable_main_model() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for missing in [false, true] {
        let storage = image_test_storage();
        if missing {
            storage.delete_managed_model_v2(MAIN_MODEL).unwrap();
        } else {
            let mut model = storage.get_managed_model_v2(MAIN_MODEL).unwrap().unwrap();
            model.enabled = false;
            storage
                .upsert_managed_model_v2(&ManagedModelV2Upsert {
                    previous_slug: None,
                    model,
                })
                .unwrap();
        }
        let result = prepare_image_request(
            storage,
            "/v1/images/generations",
            image_request_body("generation-json", Some(LEGACY_IMAGE_MODEL), false),
            "application/json",
            HeaderMap::new(),
            sample_api_key(
                crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                None,
                None,
                None,
            ),
        );
        let error = match result {
            Ok(_) => panic!("unavailable main model must remain blocked"),
            Err(error) => error,
        };
        assert_eq!(error.status_code, 404);
        assert_eq!(error.message, format!("model_not_found: {MAIN_MODEL}"));
    }
}

#[test]
fn images_passthrough_routes_still_reject_unlisted_image_models() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for (strategy, native_signal) in [
        (crate::apikey_profile::ROTATION_ACCOUNT, Some("originator")),
        (crate::apikey_profile::ROTATION_AGGREGATE_API, None),
        (crate::apikey_profile::ROTATION_HYBRID, None),
        (crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST, None),
    ] {
        let storage = image_test_storage();
        let mut headers = HeaderMap::new();
        if let Some(signal) = native_signal {
            headers.insert(signal, HeaderValue::from_static("codex_exec"));
        }
        let mut key = sample_api_key(
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
            Some(MAIN_MODEL),
            None,
            None,
        );
        key.rotation_strategy = strategy.to_string();
        let result = prepare_image_request(
            storage,
            "/v1/images/generations",
            image_request_body("generation-json", Some(LEGACY_IMAGE_MODEL), false),
            "application/json",
            headers,
            key,
        );
        let error = match result {
            Ok(_) => {
                panic!("{strategy}, {native_signal:?}: unlisted image model must remain blocked")
            }
            Err(error) => error,
        };
        assert_eq!(error.status_code, 404);
        assert_eq!(
            error.message,
            format!("model_not_found: {LEGACY_IMAGE_MODEL}")
        );
    }
}

fn cm_native_image_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("originator", HeaderValue::from_static("codex_app"));
    headers.insert(
        "x-openai-actor-authorization",
        HeaderValue::from_static("local-image-extension"),
    );
    headers.insert(
        "x-codex-image-turn-id",
        HeaderValue::from_static("turn-managed-image"),
    );
    headers
}

#[test]
fn cm_native_images_use_configured_image_model_before_all_route_policies() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for strategy in [
        crate::apikey_profile::ROTATION_ACCOUNT,
        crate::apikey_profile::ROTATION_AGGREGATE_API,
        crate::apikey_profile::ROTATION_HYBRID,
        crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST,
    ] {
        for (form, path) in [
            ("generation-json", "/v1/images/generations"),
            ("edit-json", "/v1/images/edits"),
        ] {
            for stream in [false, true] {
                let storage = image_test_storage();
                assert!(storage
                    .get_managed_model_v2(LEGACY_IMAGE_MODEL)
                    .unwrap()
                    .is_none());
                let mut key = sample_api_key(
                    crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                    Some("cm-missing-bound-text-model"),
                    Some("high"),
                    Some("fast"),
                );
                key.rotation_strategy = strategy.to_string();
                let prepared = prepare_image_request(
                    storage,
                    path,
                    image_request_body(form, Some(LEGACY_IMAGE_MODEL), stream),
                    "application/json",
                    cm_native_image_headers(),
                    key,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{strategy}, {path}, stream={stream}: {} {}",
                        error.status_code, error.message
                    )
                });
                assert_eq!(prepared.path, path);
                assert_eq!(prepared.is_stream, stream);
                assert_eq!(
                    prepared.client_model_for_log.as_deref(),
                    Some(LEGACY_IMAGE_MODEL)
                );
                assert_eq!(prepared.model_for_log.as_deref(), Some(TOOL_MODEL));
                for body in [&prepared.body, &prepared.passthrough_body] {
                    let payload: Value = serde_json::from_slice(body).unwrap();
                    assert_eq!(payload["model"], TOOL_MODEL);
                    assert_eq!(payload["stream"], stream);
                    assert!(payload.get("tools").is_none());
                    assert!(payload.get("reasoning").is_none());
                    assert!(payload.get("service_tier").is_none());
                }
            }
        }
    }
}

#[test]
fn cm_native_images_default_to_image_2_and_validate_configured_model() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for configured in ["", MAIN_MODEL, "gpt-image-config-missing", TOOL_MODEL] {
        let _tool_model = RuntimeEnvGuard::set("CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL", configured);
        let storage = image_test_storage();
        if configured == TOOL_MODEL {
            let mut model = storage.get_managed_model_v2(TOOL_MODEL).unwrap().unwrap();
            model.enabled = false;
            storage
                .upsert_managed_model_v2(&ManagedModelV2Upsert {
                    previous_slug: None,
                    model,
                })
                .unwrap();
        }
        let result = prepare_image_request(
            storage,
            "/v1/images/generations",
            image_request_body("generation-json", Some(LEGACY_IMAGE_MODEL), false),
            "application/json",
            cm_native_image_headers(),
            sample_api_key(
                crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                None,
                None,
                None,
            ),
        );
        if configured.is_empty() {
            let prepared = result.unwrap_or_else(|error| {
                panic!(
                    "default image model: {} {}",
                    error.status_code, error.message
                )
            });
            assert_eq!(prepared.model_for_log.as_deref(), Some("gpt-image-2"));
        } else {
            let error = match result {
                Ok(_) => panic!("invalid configured image model {configured} must be blocked"),
                Err(error) => error,
            };
            assert_eq!(
                error.status_code,
                if configured == MAIN_MODEL { 400 } else { 404 }
            );
            assert!(error.message.contains(configured), "{}", error.message);
        }
    }
}

#[test]
fn configured_native_image_model_uses_image_turn_without_requiring_cm_marker() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for (marker, turn, originator) in [
        (None, false, None),
        (None, true, None),
        (None, true, Some("codex_app")),
        (Some("local-image-extension"), false, Some("codex_exec")),
        (Some("local-image-extension"), true, Some("codex_app")),
        (Some("Bearer unrelated-actor"), true, Some("codex_app")),
    ] {
        let storage = image_test_storage();
        let mut headers = HeaderMap::new();
        if let Some(value) = marker {
            headers.insert(
                "x-openai-actor-authorization",
                HeaderValue::from_static(value),
            );
        }
        if turn {
            headers.insert(
                "x-codex-image-turn-id",
                HeaderValue::from_static("other-turn"),
            );
        }
        if let Some(value) = originator {
            headers.insert("originator", HeaderValue::from_static(value));
        }
        let prepared = prepare_image_request(
            storage,
            "/v1/images/generations",
            image_request_body("generation-json", Some("gpt-image-2"), false),
            "application/json",
            headers,
            sample_api_key(
                crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                None,
                None,
                None,
            ),
        )
        .unwrap_or_else(|error| {
            panic!(
                "image request with turn={turn}, marker={marker:?}: {} {}",
                error.status_code, error.message
            )
        });
        let payload: Value = serde_json::from_slice(&prepared.body).unwrap();
        let selected_image_model = if prepared.path == "/v1/responses" {
            &payload["tools"][0]["model"]
        } else {
            &payload["model"]
        };
        assert_eq!(
            selected_image_model,
            if turn { TOOL_MODEL } else { "gpt-image-2" }
        );
        if turn {
            assert_eq!(prepared.path, "/v1/images/generations");
            assert_eq!(prepared.model_for_log.as_deref(), Some(TOOL_MODEL));
        }
    }
}

#[test]
fn configured_native_image_model_does_not_override_responses_or_tools() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    let storage = image_test_storage();
    let tools = json!([{
        "type": "namespace", "name": "image_gen", "description": "Image generation",
        "tools": [{"type": "function", "name": "text2im", "description": "Generate an image",
                   "parameters": {"type": "object", "properties": {}}}]
    }, {"type": "image_generation", "model": "gpt-image-2"}]);
    let prepared = prepare_image_request(
        storage,
        "/v1/responses",
        Bytes::from(
            serde_json::to_vec(&json!({
                "model": MAIN_MODEL, "input": "a red apple", "stream": true,
                "tools": tools, "tool_choice": "auto"
            }))
            .unwrap(),
        ),
        "application/json",
        cm_native_image_headers(),
        sample_api_key(
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
            None,
            None,
            None,
        ),
    )
    .unwrap_or_else(|error| panic!("Responses tools: {} {}", error.status_code, error.message));
    let payload: Value = serde_json::from_slice(&prepared.body).unwrap();
    assert_eq!(payload["model"], MAIN_MODEL);
    assert_eq!(payload["tools"], tools);
}

#[test]
fn cm_native_images_validate_generation_and_edit_capabilities_independently() {
    let _guard = crate::test_env_guard();
    let _environment = image_test_environment();
    for generation_only in [true, false] {
        for (edit, form, path) in [
            (false, "generation-json", "/v1/images/generations"),
            (true, "edit-json", "/v1/images/edits"),
        ] {
            let storage = image_test_storage();
            let mut model = storage.get_managed_model_v2(TOOL_MODEL).unwrap().unwrap();
            model.capabilities["supports_image_generation"] = json!(generation_only);
            model.capabilities["supports_image_editing"] = json!(!generation_only);
            storage
                .upsert_managed_model_v2(&ManagedModelV2Upsert {
                    previous_slug: None,
                    model,
                })
                .unwrap();
            let result = prepare_image_request(
                storage,
                path,
                image_request_body(form, Some(LEGACY_IMAGE_MODEL), false),
                "application/json",
                cm_native_image_headers(),
                sample_api_key(
                    crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                    None,
                    None,
                    None,
                ),
            );
            if edit != generation_only {
                let prepared = result.unwrap_or_else(|error| {
                    panic!(
                        "supported endpoint {path}, generation_only={generation_only}: {} {}",
                        error.status_code, error.message
                    )
                });
                assert_eq!(prepared.path, path);
                assert_eq!(prepared.model_for_log.as_deref(), Some(TOOL_MODEL));
            } else {
                let error = match result {
                    Ok(_) => panic!(
                        "unsupported endpoint {path}, generation_only={generation_only} must fail"
                    ),
                    Err(error) => error,
                };
                assert_eq!(error.status_code, 400);
                assert!(error.message.contains(TOOL_MODEL), "{}", error.message);
            }
        }
    }
}
