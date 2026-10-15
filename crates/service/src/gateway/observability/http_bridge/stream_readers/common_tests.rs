use super::{
    classify_upstream_stream_read_error, stream_incomplete_message,
    stream_reader_disconnected_message,
};

#[test]
fn terminal_delivery_detects_split_marker_before_large_payload() {
    let collector = super::Arc::new(super::Mutex::new(super::PassthroughSseCollector::default()));
    super::mark_terminal_delivered(&collector, b"event: response.fa");
    assert!(!collector.lock().unwrap().terminal_delivered);
    let remainder = format!("iled\ndata: {}\n\n", "x".repeat(1024));
    super::mark_terminal_delivered(&collector, remainder.as_bytes());
    assert!(collector.lock().unwrap().terminal_delivered);
}

#[test]
fn comment_keepalive_is_not_a_semantic_event() {
    let frame = super::SseKeepAliveFrame::Comment.bytes();
    assert_eq!(frame, b": keep-alive\n\n");
    assert!(!frame.starts_with(b"data:"));
    assert!(!String::from_utf8_lossy(frame).contains("codexmanager.keepalive"));
}

/// 函数 `classify_upstream_stream_read_error_maps_body_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn classify_upstream_stream_read_error_maps_body_error() {
    assert_eq!(
        classify_upstream_stream_read_error("request or response body error"),
        "上游中途断开，未返回具体错误信息"
    );
}

/// 函数 `classify_upstream_stream_read_error_maps_disconnect`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn classify_upstream_stream_read_error_maps_disconnect() {
    assert_eq!(
        classify_upstream_stream_read_error("connection reset by peer"),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
}

/// 函数 `classify_upstream_stream_read_error_maps_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn classify_upstream_stream_read_error_maps_timeout() {
    assert_eq!(
        classify_upstream_stream_read_error("operation timed out"),
        "上游流式空闲超时"
    );
}

/// 函数 `stream_terminal_messages_are_user_friendly`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn stream_terminal_messages_are_user_friendly() {
    assert_eq!(
        stream_incomplete_message(),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
    assert_eq!(
        stream_reader_disconnected_message(),
        "连接中断（可能是网络波动或客户端主动取消）"
    );
    assert_eq!(super::stream_idle_timeout_message(), "上游流式空闲超时");
}
