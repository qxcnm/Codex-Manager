//! BPS diagnostics report only BPS results; they never mutate Codex account health.
use super::{AccountTestEvent, AccountTestOutcome, TestKind, ACCOUNT_TEST_OVERALL_TIMEOUT};
use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

pub(super) async fn execute(
    account_id: &str,
    test_id: &str,
    model: &str,
    prompt: &str,
    kind: TestKind,
    credentials: crate::account::bps::BpsCredentials,
) -> AccountTestOutcome {
    let secrets = vec![credentials.access_token.clone()];
    if kind.is_image() {
        return failed(test_id, "BPS 转发暂不支持图片生成测试", &secrets);
    }
    super::notify_account_test_event(
        AccountTestEvent::new(test_id, "status").with_status("正在连接 BPS 上游…"),
    );
    let body =
        Bytes::from(json!({"model":model,"input":prompt,"stream":true,"store":false}).to_string());
    let stream = match crate::gateway::bps_account_test_request(
        account_id,
        credentials,
        body,
        std::time::Instant::now() + ACCOUNT_TEST_OVERALL_TIMEOUT,
    )
    .await
    {
        Ok(stream) => stream,
        Err(error) => return failed(test_id, &error, &secrets),
    };
    super::notify_account_test_event(
        AccountTestEvent::new(test_id, "status").with_status("已连接 BPS 上游"),
    );
    consume(stream, test_id, &secrets).await
}

fn failed(test_id: &str, message: &str, secrets: &[String]) -> AccountTestOutcome {
    let message = super::redact(message, secrets);
    super::emit_redacted_error(test_id, &message, secrets);
    // Even HTTP 401/403/429 from BPS must not change the Codex account state.
    AccountTestOutcome::Failed(message)
}

async fn consume<S>(stream: S, test_id: &str, secrets: &[String]) -> AccountTestOutcome
where
    S: Stream<Item = Result<Bytes, String>> + Send,
{
    let stream = Box::pin(stream.map_err(std::io::Error::other));
    let mut reader = BufReader::new(StreamReader::new(stream));
    let mut line = String::new();
    let mut data = String::new();
    let mut received_text = false;
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => return failed(test_id, "BPS 连接在完成事件前中断", secrets),
            Ok(_) => {}
            Err(error) => return failed(test_id, &format!("BPS 测试流读取失败: {error}"), secrets),
        }
        if line.len() + data.len() > 16 * 1024 * 1024 {
            return failed(test_id, "BPS 测试事件超出大小限制", secrets);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(part) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(part.trim_start());
            continue;
        }
        if !line.is_empty() || data.is_empty() {
            continue;
        }
        let event: Value = match serde_json::from_str(&data) {
            Ok(event) => event,
            Err(_) => return failed(test_id, "BPS 测试返回无效事件", secrets),
        };
        data.clear();
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(text) = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    received_text = true;
                    super::notify_account_test_event(
                        AccountTestEvent::new(test_id, "content").with_text(text),
                    );
                }
            }
            Some("response.completed") => {
                if event.pointer("/response/status").and_then(Value::as_str) != Some("completed") {
                    return failed(test_id, "BPS 返回的最终状态不是 completed", secrets);
                }
                if !received_text {
                    let text = event
                        .pointer("/response/output")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
                        .filter_map(|item| item.get("content").and_then(Value::as_array))
                        .flatten()
                        .filter(|part| {
                            part.get("type").and_then(Value::as_str) == Some("output_text")
                        })
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<String>();
                    if !text.is_empty() {
                        received_text = true;
                        super::notify_account_test_event(
                            AccountTestEvent::new(test_id, "content").with_text(text),
                        );
                    }
                }
                if !received_text {
                    return failed(test_id, "BPS 已结束但没有返回文字内容", secrets);
                }
                super::notify_account_test_event(
                    AccountTestEvent::new(test_id, "test_complete").with_success(true),
                );
                return AccountTestOutcome::Success;
            }
            Some("response.failed" | "response.incomplete" | "error") => {
                return failed(
                    test_id,
                    &super::extract_stream_error_message(&event),
                    secrets,
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stream(events: &[Value]) -> impl Stream<Item = Result<Bytes, String>> + Send {
        let wire = events
            .iter()
            .map(|value| format!("data: {value}\n\n"))
            .collect::<String>();
        futures_util::stream::iter(
            wire.as_bytes()
                .chunks(7)
                .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                .collect::<Vec<_>>(),
        )
    }

    #[tokio::test]
    async fn bps_account_test_accepts_completed_snapshot_without_deltas() {
        let result = consume(stream(&[json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}})]), "bps-snapshot-test", &[]).await;
        assert!(matches!(result, AccountTestOutcome::Success));
    }

    #[tokio::test]
    async fn bps_account_test_failure_is_never_a_codex_auth_or_rate_limit_outcome() {
        for code in ["account_deactivated", "rate_limit_exceeded"] {
            let result = consume(
                stream(&[json!({"type":"response.failed","response":{"error":{"message":code}}})]),
                "bps-error-test",
                &[],
            )
            .await;
            assert!(matches!(result, AccountTestOutcome::Failed(_)));
        }
        let result = consume(
            stream(&[json!({"type":"response.output_text.delta","delta":"unfinished"})]),
            "bps-truncated-test",
            &[],
        )
        .await;
        assert!(matches!(result, AccountTestOutcome::Failed(_)));
    }

    #[tokio::test]
    async fn bps_account_test_rejects_image_generation_without_network() {
        let result = execute(
            "local",
            "bps-image-test",
            "gpt-image-2",
            "cat",
            TestKind::Image,
            crate::account::bps::BpsCredentials {
                access_token: "not-used".into(),
                account_id: "not-used".into(),
            },
        )
        .await;
        assert!(matches!(result, AccountTestOutcome::Failed(_)));
    }
}
