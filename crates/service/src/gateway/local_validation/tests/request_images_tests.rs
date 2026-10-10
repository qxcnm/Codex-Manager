use super::{
    adapt_openai_images_edits_body_to_responses, adapt_openai_images_generations_body_to_responses,
    ensure_non_text_model_not_used_for_text_request,
};
use crate::gateway::ResponseAdapter;
use codexmanager_core::storage::{ManagedModelV2, ManagedModelV2Upsert, ModelPriceV2, Storage};
use serde_json::json;

fn storage() -> Storage {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    storage
}

#[test]
fn images_generation_request_builds_responses_image_generation_tool() {
    let body = json!({
        "model": "gpt-image-2",
        "prompt": "画一张极简风格的猫",
        "size": "1024x1024",
        "quality": "high",
        "background": "transparent",
        "output_format": "png",
        "response_format": "url",
        "stream": true,
        "partial_images": 1
    });

    let (mapped, adapter) =
        adapt_openai_images_generations_body_to_responses(serde_json::to_vec(&body).expect("body"))
            .expect("adapt images request");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(adapter, ResponseAdapter::ImagesUrlFromResponses);
    assert_eq!(value["model"], "gpt-6-luna");
    assert_eq!(value["stream"], true);
    assert_eq!(value["store"], false);
    assert_eq!(value["tool_choice"]["type"], "image_generation");
    assert_eq!(value["tools"][0]["type"], "image_generation");
    assert_eq!(value["tools"][0]["model"], "gpt-image-2");
    assert_eq!(value["tools"][0]["size"], "1024x1024");
    assert_eq!(value["tools"][0]["quality"], "high");
    assert_eq!(value["tools"][0]["background"], "transparent");
    assert_eq!(value["tools"][0]["partial_images"], 1);
    assert_eq!(
        value["input"][0]["content"][0]["text"],
        "画一张极简风格的猫"
    );
}

#[test]
fn image_25_generation_request_forwards_selected_tool_model() {
    for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
        let body = json!({
            "model": slug,
            "prompt": "render a product mockup"
        });

        let (mapped, adapter) = adapt_openai_images_generations_body_to_responses(
            serde_json::to_vec(&body).expect("body"),
        )
        .expect("adapt Image 2.5 request");
        let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

        assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
        assert_eq!(value["model"], "gpt-6-luna");
        assert_eq!(value["tools"][0]["model"], slug);
    }
}

#[test]
fn images_generation_request_defaults_b64_json_and_tool_model() {
    let body = json!({ "prompt": "cat" });

    let (mapped, adapter) =
        adapt_openai_images_generations_body_to_responses(serde_json::to_vec(&body).expect("body"))
            .expect("adapt images request");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
    assert_eq!(value["tools"][0]["model"], "gpt-image-2");
    assert_eq!(value["tools"][0]["output_format"], "png");
}

#[test]
fn images_generation_request_requires_prompt() {
    let body = json!({ "model": "gpt-image-2" });

    let err =
        adapt_openai_images_generations_body_to_responses(serde_json::to_vec(&body).expect("body"))
            .expect_err("prompt should be required");

    assert!(err.contains("prompt is required"));
}

#[test]
fn images_edits_json_request_builds_responses_with_input_images_and_mask() {
    let body = json!({
        "model": "gpt-image-2",
        "prompt": "把背景改成透明",
        "images": [{
            "image_url": "data:image/png;base64,aW1hZ2U="
        }],
        "mask": {
            "image_url": "data:image/png;base64,bWFzaw=="
        },
        "response_format": "b64_json"
    });

    let (mapped, adapter) = adapt_openai_images_edits_body_to_responses(
        serde_json::to_vec(&body).expect("body"),
        Some("application/json"),
    )
    .expect("adapt edits json request");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
    assert_eq!(value["tools"][0]["type"], "image_generation");
    assert_eq!(value["tools"][0]["model"], "gpt-image-2");
    assert_eq!(
        value["tools"][0]["input_image_mask"]["image_url"],
        "data:image/png;base64,bWFzaw=="
    );
    assert_eq!(value["input"][0]["content"][0]["text"], "把背景改成透明");
    assert_eq!(
        value["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aW1hZ2U="
    );
}

#[test]
fn image_25_edits_requests_forward_selected_tool_model() {
    for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
        let json_body = json!({
            "model": slug,
            "prompt": "edit the product photo",
            "images": [{
                "image_url": "data:image/png;base64,aW1hZ2U="
            }]
        });
        let (mapped, _) = adapt_openai_images_edits_body_to_responses(
            serde_json::to_vec(&json_body).expect("body"),
            Some("application/json"),
        )
        .expect("adapt Image 2.5 JSON edit request");
        let value: serde_json::Value =
            serde_json::from_slice(&mapped).expect("parse mapped JSON edit body");
        assert_eq!(value["tools"][0]["model"], slug);

        let multipart_body = format!(
            concat!(
                "--test-boundary\r\n",
                "Content-Disposition: form-data; name=\"model\"\r\n\r\n",
                "{}\r\n",
                "--test-boundary\r\n",
                "Content-Disposition: form-data; name=\"prompt\"\r\n\r\n",
                "edit the product photo\r\n",
                "--test-boundary\r\n",
                "Content-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\n",
                "Content-Type: image/png\r\n\r\n",
                "IMG\r\n",
                "--test-boundary--\r\n"
            ),
            slug
        )
        .into_bytes();
        let (mapped, _) = adapt_openai_images_edits_body_to_responses(
            multipart_body,
            Some("multipart/form-data; boundary=test-boundary"),
        )
        .expect("adapt Image 2.5 multipart edit request");
        let value: serde_json::Value =
            serde_json::from_slice(&mapped).expect("parse mapped multipart edit body");
        assert_eq!(value["tools"][0]["model"], slug);
    }
}

#[test]
fn images_edits_json_rejects_file_id() {
    let body = json!({
        "prompt": "edit",
        "images": [{ "file_id": "file_123" }]
    });

    let err = adapt_openai_images_edits_body_to_responses(
        serde_json::to_vec(&body).expect("body"),
        Some("application/json"),
    )
    .expect_err("file_id should be rejected");

    assert!(err.contains("file_id is not supported"));
}

#[test]
fn images_edits_json_rejects_invalid_base64_data_url() {
    let body = json!({
        "prompt": "edit",
        "images": [{ "image_url": "data:image/png;base64,***" }]
    });

    let err = adapt_openai_images_edits_body_to_responses(
        serde_json::to_vec(&body).expect("body"),
        Some("application/json"),
    )
    .expect_err("invalid base64 should be rejected");

    assert!(err.contains("invalid base64 image data"));
}

#[test]
fn images_edits_multipart_request_builds_data_urls() {
    let body = concat!(
        "--test-boundary\r\n",
        "Content-Disposition: form-data; name=\"prompt\"\r\n\r\n",
        "修图\r\n",
        "--test-boundary\r\n",
        "Content-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\n",
        "Content-Type: image/png\r\n\r\n",
        "IMG\r\n",
        "--test-boundary\r\n",
        "Content-Disposition: form-data; name=\"mask\"; filename=\"m.png\"\r\n",
        "Content-Type: image/png\r\n\r\n",
        "MSK\r\n",
        "--test-boundary--\r\n"
    )
    .as_bytes()
    .to_vec();

    let (mapped, adapter) = adapt_openai_images_edits_body_to_responses(
        body,
        Some("multipart/form-data; boundary=test-boundary"),
    )
    .expect("adapt edits multipart request");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(adapter, ResponseAdapter::ImagesB64JsonFromResponses);
    assert_eq!(value["input"][0]["content"][0]["text"], "修图");
    assert_eq!(
        value["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,SU1H"
    );
    assert_eq!(
        value["tools"][0]["input_image_mask"]["image_url"],
        "data:image/png;base64,TVNL"
    );
}

#[test]
fn rejects_gpt_image_model_on_text_generation_paths() {
    let storage = storage();
    let err = ensure_non_text_model_not_used_for_text_request(
        &storage,
        "/v1/chat/completions",
        Some("gpt-image-2"),
    )
    .expect_err("text path should reject image tool model");

    assert_eq!(err.status_code, 400);
    assert!(err.message.contains("/v1/images/generations"));
}

#[test]
fn allows_gpt_image_model_on_images_paths() {
    let storage = storage();
    assert!(ensure_non_text_model_not_used_for_text_request(
        &storage,
        "/v1/images/generations",
        Some("gpt-image-2"),
    )
    .is_ok());
    assert!(ensure_non_text_model_not_used_for_text_request(
        &storage,
        "/v1/images/edits",
        Some("gpt-image-2"),
    )
    .is_ok());
}

#[test]
fn image_25_models_are_case_insensitive_and_only_allowed_on_images_paths() {
    let storage = storage();
    for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
        for request_slug in [slug.to_string(), slug.to_ascii_uppercase()] {
            for path in ["/v1/images/generations", "/v1/images/edits"] {
                assert!(
                    ensure_non_text_model_not_used_for_text_request(
                        &storage,
                        path,
                        Some(request_slug.as_str()),
                    )
                    .is_ok(),
                    "{request_slug} should be accepted on {path}"
                );
            }

            let err = ensure_non_text_model_not_used_for_text_request(
                &storage,
                "/v1/responses",
                Some(request_slug.as_str()),
            )
            .expect_err("image-only Image 2.5 model must be rejected on text paths");
            assert_eq!(err.status_code, 400);
            assert!(err.message.contains("images/generations"));
        }
    }
}

#[test]
fn rejects_catalog_non_text_model_on_text_generation_paths() {
    let storage = storage();
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            model: ManagedModelV2 {
                slug: "custom-image-model".to_string(),
                display_name: "Custom Image Model".to_string(),
                origin: "custom".to_string(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".to_string(),
                capabilities: json!({
                    "supports_text_generation": false,
                    "output_modalities": ["image"]
                }),
                instructions_mode: "passthrough".to_string(),
                price: ModelPriceV2 {
                    price_status: "missing".to_string(),
                    ..Default::default()
                },
                ..ManagedModelV2::default()
            },
            ..ManagedModelV2Upsert::default()
        })
        .expect("save custom image model");

    let err = ensure_non_text_model_not_used_for_text_request(
        &storage,
        "/v1/responses",
        Some("custom-image-model"),
    )
    .expect_err("catalog non-text model should be rejected");
    assert_eq!(err.status_code, 400);
    assert!(err.message.contains("does not support text generation"));

    assert!(ensure_non_text_model_not_used_for_text_request(
        &storage,
        "/v1/responses",
        Some("external-unknown-model"),
    )
    .is_ok());
}
