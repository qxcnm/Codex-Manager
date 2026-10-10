use super::{OpenAIResponsesEvent, SseTerminal};

#[test]
fn response_id_only_comes_from_successful_completed_event() {
    let parsed = |kind: &str, id: &str| {
        let event = format!("data: {{\"type\":\"{kind}\",\"response\":{{\"id\":\"{id}\"}}}}\n");
        OpenAIResponsesEvent::parse(&[event, "\n".to_string()]).unwrap()
    };
    assert_eq!(
        parsed("response.completed", "resp_real_1")
            .usage
            .response_id
            .as_deref(),
        Some("resp_real_1")
    );
    assert!(parsed("response.failed", "resp_real_1")
        .usage
        .response_id
        .is_none());
    assert!(parsed("response.created", "resp_real_1")
        .usage
        .response_id
        .is_none());
    assert!(parsed("response.completed", "resp_proxy")
        .usage
        .response_id
        .is_none());
}

#[test]
fn parse_openai_responses_event_maps_bare_incomplete_to_user_friendly_terminal() {
    let lines = vec![
        "event: response.incomplete\n".to_string(),
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\"}}\n"
            .to_string(),
        "\n".to_string(),
    ];

    let event = OpenAIResponsesEvent::parse(&lines).expect("parsed event");
    assert_eq!(event.event_type.as_deref(), Some("response.incomplete"));
    assert!(matches!(
        event.terminal,
        Some(SseTerminal::Err(ref message))
            if message == "连接中断（可能是网络波动或客户端主动取消）"
    ));
}

#[test]
fn parse_openai_responses_event_maps_stream_timeout_hint_to_idle_timeout() {
    let lines = vec![
        "event: response.incomplete\n".to_string(),
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"status_details\":{\"error\":{\"message\":\"stream timeout at upstream\",\"code\":\"stream_timeout\"}}}}\n".to_string(),
        "\n".to_string(),
    ];

    let event = OpenAIResponsesEvent::parse(&lines).expect("parsed event");
    assert_eq!(
        event.upstream_error_hint.as_deref(),
        Some("code=stream_timeout stream timeout at upstream")
    );
    assert!(matches!(
        event.terminal,
        Some(SseTerminal::Err(ref message)) if message == "上游流式空闲超时"
    ));
}

#[test]
fn parse_openai_responses_event_treats_partial_image_as_non_terminal() {
    let lines = vec![
        "event: response.image_generation_call.partial_image\n".to_string(),
        "data: {\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"ig_1\",\"partial_image_b64\":\"cGFydA==\",\"partial_image_index\":0}\n".to_string(),
        "\n".to_string(),
    ];

    let event = OpenAIResponsesEvent::parse(&lines).expect("parsed event");
    assert_eq!(
        event.event_type.as_deref(),
        Some("response.image_generation_call.partial_image")
    );
    assert!(event.terminal.is_none());
}

#[test]
fn completed_event_with_failed_status_never_succeeds_or_links_a_response_id() {
    for status in ["failed", "incomplete", "cancelled", "canceled"] {
        let lines = vec![format!("data: {{\"type\":\"response.completed\",\"response\":{{\"object\":\"response\",\"id\":\"resp_contradictory\",\"status\":\"{status}\",\"output\":[]}}}}")];
        let event = super::OpenAIResponsesEvent::parse(&lines).unwrap();
        assert!(matches!(event.terminal, Some(super::SseTerminal::Err(_))));
        assert!(event.usage.response_id.is_none());
        assert!(event.usage.explicit_failure);
    }
}
