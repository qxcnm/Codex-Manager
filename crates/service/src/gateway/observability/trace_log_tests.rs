use super::{
    clear_trace_error, gateway_trace_stdout_enabled, has_error_text, log_failed_request,
    mark_trace_has_error, sanitize_text, should_flush_success_trace, trace_has_error,
    trace_queue_capacity,
};

/// 函数 `has_error_text_ignores_empty_and_dash`
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
fn has_error_text_ignores_empty_and_dash() {
    assert!(!has_error_text(None));
    assert!(!has_error_text(Some("")));
    assert!(!has_error_text(Some(" - ")));
    assert!(has_error_text(Some("upstream failed")));
}

/// 函数 `trace_error_state_can_mark_and_clear`
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
fn trace_error_state_can_mark_and_clear() {
    let trace_id = "trc_trace_log_unit";
    clear_trace_error(trace_id);
    assert!(!trace_has_error(trace_id));
    mark_trace_has_error(trace_id);
    assert!(trace_has_error(trace_id));
    clear_trace_error(trace_id);
    assert!(!trace_has_error(trace_id));
}

/// 函数 `request_record_ignores_success_without_error`
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
fn request_record_ignores_success_without_error() {
    log_failed_request(super::FailedRequestLog {
        ts: 1_772_000_000,
        trace_id: Some("trc_success"),
        key_id: Some("gk_success"),
        account_id: Some("acc_success"),
        method: "POST",
        request_path: "/v1/responses",
        original_path: Some("/v1/responses"),
        adapted_path: Some("/v1/responses"),
        request_type: Some("http"),
        model: Some("gpt-5.4"),
        reasoning_effort: Some("high"),
        service_tier: Some("fast"),
        upstream_url: Some("https://chatgpt.com/backend-api/codex/responses"),
        status_code: Some(200),
        error: None,
        duration_ms: Some(18),
    });
}

#[test]
fn trace_queue_capacity_zero_means_unbounded() {
    let _guard = crate::test_env_guard();
    std::env::set_var("CODEXMANAGER_TRACE_QUEUE_CAPACITY", "0");

    assert_eq!(trace_queue_capacity(), 0);
}

#[test]
fn gateway_trace_stdout_flag_accepts_error_values() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT");
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT_SLOW_MS");
    assert!(!gateway_trace_stdout_enabled());

    std::env::set_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT", "error_only");
    assert!(gateway_trace_stdout_enabled());
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT");
}

#[test]
fn success_trace_flush_requires_stdout_and_slow_threshold() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT");
    std::env::set_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT_SLOW_MS", "1000");
    assert!(!should_flush_success_trace(1500));

    std::env::set_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT", "1");
    assert!(!should_flush_success_trace(999));
    assert!(should_flush_success_trace(1000));
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT");
    std::env::remove_var("CODEXMANAGER_GATEWAY_TRACE_STDOUT_SLOW_MS");
}

#[test]
fn sanitize_text_redacts_base64_image_data_urls() {
    let sanitized = sanitize_text(
        "payload=data:image/png;base64,aGVsbG8= next=data:text/plain;base64,dmFsdWU=",
    );

    assert!(sanitized.contains("data:image/png;base64,<base64 image omitted>"));
    assert!(!sanitized.contains("aGVsbG8="));
    assert!(sanitized.contains("data:text/plain;base64,dmFsdWU="));
}

#[test]
fn sanitize_text_redacts_image_generation_result_payloads() {
    let sanitized = sanitize_text(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"image_generation_call\",\"result\":\"QUJDREVGRw==\"}}\n\
         data: {\"type\":\"response.image_generation_call.partial_image\",\"partial_image_b64\":\"cGFydA==\"}",
    );

    assert!(sanitized.contains("\"result\":\"<base64 image omitted>\""));
    assert!(sanitized.contains("\"partial_image_b64\":\"<base64 image omitted>\""));
    assert!(!sanitized.contains("QUJDREVGRw=="));
    assert!(!sanitized.contains("cGFydA=="));
}

#[tokio::test(flavor = "current_thread")]
async fn native_flush_waits_for_queued_lines_and_reports_disk_errors() {
    let path = std::env::temp_dir().join(format!(
        "codexmanager-trace-async-{}.log",
        rand::random::<u64>()
    ));
    let writer = super::TraceAsyncWriter::with_capacity(path.clone(), 8);
    writer.append_line("final-accounting-event".to_string(), false);
    writer
        .flush()
        .await
        .expect("trace barrier flushes accepted lines");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().trim(),
        "final-accounting-event"
    );

    // Opening a directory as a log file fails on all supported platforms.
    let failed = super::TraceAsyncWriter::with_capacity(std::env::temp_dir(), 8);
    failed.append_line("failed-event".to_string(), true);
    assert!(
        failed.flush().await.is_err(),
        "a failed write cannot acknowledge success"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_full_trace_queue_never_blocks_and_flush_yields_until_capacity() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let writer = super::TraceAsyncWriter {
        tx: super::TraceCommandSender::Bounded(tx),
        dropped: std::sync::atomic::AtomicU64::new(0),
        queue_capacity: 1,
    };
    writer.append_line("first".to_string(), false);
    writer.append_line("error-when-full".to_string(), true);
    assert_eq!(writer.dropped.load(std::sync::atomic::Ordering::Relaxed), 1);

    let receiver = async {
        tokio::task::yield_now().await;
        assert!(matches!(
            rx.recv().await,
            Some(super::TraceCommand::Append { .. })
        ));
        match rx.recv().await.unwrap() {
            super::TraceCommand::Flush(ack) => {
                let _ = ack.send(Ok(()));
            }
            _ => panic!("expected flush after the accepted line"),
        }
    };
    let (flushed, ()) = tokio::join!(writer.flush(), receiver);
    flushed.unwrap();
}
