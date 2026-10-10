use super::{copy_upstream_response_headers, upstream_response_metadata};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

#[test]
fn upstream_response_metadata_extracts_debug_headers_and_content_flags() {
    let mut upstream = HeaderMap::new();
    upstream.insert("x-oai-request-id", HeaderValue::from_static(" req-123 "));
    upstream.insert("cf-ray", HeaderValue::from_static("cf-123"));
    upstream.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("quota exceeded"),
    );
    upstream.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"revoked\"}"),
    );
    upstream.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );

    let metadata = upstream_response_metadata(&upstream);

    assert_eq!(metadata.request_id.as_deref(), Some("req-123"));
    assert_eq!(metadata.cf_ray.as_deref(), Some("cf-123"));
    assert_eq!(metadata.auth_error.as_deref(), Some("quota exceeded"));
    assert_eq!(metadata.identity_error_code.as_deref(), Some("revoked"));
    assert_eq!(
        metadata.content_type.as_deref(),
        Some("text/event-stream; charset=utf-8")
    );
    assert!(metadata.is_sse);
    assert!(!metadata.is_json);
}

#[test]
fn copy_upstream_response_headers_filters_hop_by_hop_and_adds_trace_id() {
    let mut upstream = HeaderMap::new();
    upstream.insert("content-type", HeaderValue::from_static("application/json"));
    upstream.insert("transfer-encoding", HeaderValue::from_static("chunked"));
    upstream.insert("content-length", HeaderValue::from_static("12"));
    upstream.insert("connection", HeaderValue::from_static("keep-alive"));

    let headers = copy_upstream_response_headers(&upstream, Some(" trace-123 "));
    let names = headers
        .iter()
        .map(|header| header.field.as_str().to_ascii_lowercase())
        .collect::<Vec<_>>();

    assert!(names.contains(&"content-type".to_string()));
    assert!(names.contains(&crate::error_codes::TRACE_ID_HEADER_NAME.to_ascii_lowercase()));
    assert!(!names.contains(&"transfer-encoding".to_string()));
    assert!(!names.contains(&"content-length".to_string()));
    assert!(!names.contains(&"connection".to_string()));
}
