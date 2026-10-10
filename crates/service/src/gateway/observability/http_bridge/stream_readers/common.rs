use super::{Arc, Mutex, UpstreamResponseUsage};
use crate::gateway::upstream::attempt_flow::transport::runtime::upstream_runtime;
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
#[cfg(test)]
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::RecvTimeoutError;
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, Receiver};

const UPSTREAM_SSE_FRAME_CHANNEL_CAPACITY: usize = 128;

const STREAM_INCOMPLETE_FALLBACK_MESSAGE: &str = "连接中断（可能是网络波动或客户端主动取消）";
const STREAM_READ_FAILED_FALLBACK_MESSAGE: &str = "上游中途断开，未返回具体错误信息";
const STREAM_IDLE_TIMEOUT_FALLBACK_MESSAGE: &str = "上游流式空闲超时";

#[derive(Debug, Clone, Default)]
pub(crate) struct PassthroughSseCollector {
    pub(crate) usage: UpstreamResponseUsage,
    pub(crate) saw_terminal: bool,
    pub(crate) terminal_delivered: bool,
    #[allow(dead_code)]
    pub(crate) terminal_delivery_tail: Vec<u8>,
    pub(crate) terminal_error: Option<String>,
    pub(crate) upstream_error_hint: Option<String>,
    pub(crate) last_event_type: Option<String>,
}

pub(crate) fn mark_terminal_delivered(
    usage_collector: &Arc<Mutex<PassthroughSseCollector>>,
    chunk: &[u8],
) {
    let Ok(mut collector) = usage_collector.lock() else {
        return;
    };
    if collector.terminal_delivered {
        return;
    }
    collector.terminal_delivery_tail.extend_from_slice(chunk);
    const TAIL_LIMIT: usize = 256;
    if collector.terminal_delivery_tail.len() > TAIL_LIMIT {
        let keep_from = collector.terminal_delivery_tail.len() - TAIL_LIMIT;
        collector.terminal_delivery_tail.drain(..keep_from);
    }
    let text = String::from_utf8_lossy(&collector.terminal_delivery_tail).to_ascii_lowercase();
    if !text.contains("response.completed")
        && !text.contains("response.failed")
        && !text.contains("response.incomplete")
        && !text.contains("response.done")
        && !text.contains("data: [done]")
    {
        return;
    }
    collector.terminal_delivered = true;
}

fn elapsed_ms_since(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}

pub(super) fn mark_first_response_ms(
    usage_collector: &Arc<Mutex<PassthroughSseCollector>>,
    started_at: Instant,
) {
    if let Ok(mut collector) = usage_collector.lock() {
        if collector.usage.first_response_ms.is_none() {
            collector.usage.first_response_ms = Some(elapsed_ms_since(started_at));
        }
    }
}

pub(super) fn mark_first_response_ms_on_usage(
    usage_collector: &Arc<Mutex<UpstreamResponseUsage>>,
    started_at: Instant,
) {
    if let Ok(mut usage) = usage_collector.lock() {
        if usage.first_response_ms.is_none() {
            usage.first_response_ms = Some(elapsed_ms_since(started_at));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SseKeepAliveFrame {
    Comment,
    Anthropic,
}

impl SseKeepAliveFrame {
    /// 函数 `bytes`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Comment => b": keep-alive\n\n",
            Self::Anthropic => b"event: ping\ndata: {\"type\":\"ping\"}\n\n",
        }
    }
}

#[derive(Debug)]
pub(crate) enum UpstreamSseFramePumpItem {
    Frame(Vec<String>),
    Eof,
    Error(String),
}

pub(crate) struct UpstreamSseFramePump {
    rx: Receiver<UpstreamSseFramePumpItem>,
}

impl UpstreamSseFramePump {
    #[cfg(test)]
    pub(super) fn from_receiver(rx: Receiver<UpstreamSseFramePumpItem>) -> Self {
        Self { rx }
    }

    #[cfg(test)]
    pub(crate) fn from_reader<R>(upstream: R) -> Self
    where
        R: Read + Send + 'static,
    {
        let (tx, rx) = channel::<UpstreamSseFramePumpItem>(UPSTREAM_SSE_FRAME_CHANNEL_CAPACITY);
        thread::spawn(move || {
            let mut reader = BufReader::new(upstream);
            let mut pending_frame_lines = Vec::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        if !pending_frame_lines.is_empty()
                            && tx
                                .blocking_send(UpstreamSseFramePumpItem::Frame(pending_frame_lines))
                                .is_err()
                        {
                            return;
                        }
                        let _ = tx.blocking_send(UpstreamSseFramePumpItem::Eof);
                        return;
                    }
                    Ok(_) => {
                        let is_blank = line == "\n" || line == "\r\n";
                        pending_frame_lines.push(line);
                        if is_blank {
                            let frame = std::mem::take(&mut pending_frame_lines);
                            if tx
                                .blocking_send(UpstreamSseFramePumpItem::Frame(frame))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.blocking_send(UpstreamSseFramePumpItem::Error(err.to_string()));
                        return;
                    }
                }
            }
        });
        Self { rx }
    }

    pub(crate) fn from_stream(mut upstream: GatewayByteStream) -> Self {
        let (tx, rx) = channel(UPSTREAM_SSE_FRAME_CHANNEL_CAPACITY);
        if let Ok(runtime) = upstream_runtime() {
            runtime.spawn(async move {
                let mut pending = Vec::<u8>::new();
                let mut frame = Vec::<String>::new();
                loop {
                    let next = tokio::select! {
                        _ = tx.closed() => return,
                        next = upstream.recv_async() => next,
                    };
                    match next {
                        Some(GatewayByteStreamItem::Chunk(bytes)) => {
                            pending.extend_from_slice(&bytes)
                        }
                        Some(GatewayByteStreamItem::Error(error)) => {
                            let _ = tx.send(UpstreamSseFramePumpItem::Error(error)).await;
                            return;
                        }
                        Some(GatewayByteStreamItem::Eof) | None => {
                            if !pending.is_empty() {
                                match String::from_utf8(pending) {
                                    Ok(line) => frame.push(line),
                                    Err(error) => {
                                        let _ = tx
                                            .send(UpstreamSseFramePumpItem::Error(
                                                error.to_string(),
                                            ))
                                            .await;
                                        return;
                                    }
                                }
                            }
                            if !frame.is_empty()
                                && tx
                                    .send(UpstreamSseFramePumpItem::Frame(frame))
                                    .await
                                    .is_err()
                            {
                                return;
                            }
                            let _ = tx.send(UpstreamSseFramePumpItem::Eof).await;
                            return;
                        }
                    }
                    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                        let line = match String::from_utf8(pending.drain(..=end).collect()) {
                            Ok(line) => line,
                            Err(error) => {
                                let _ = tx
                                    .send(UpstreamSseFramePumpItem::Error(error.to_string()))
                                    .await;
                                return;
                            }
                        };
                        let blank = line == "\n" || line == "\r\n";
                        frame.push(line);
                        if blank
                            && tx
                                .send(UpstreamSseFramePumpItem::Frame(std::mem::take(&mut frame)))
                                .await
                                .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        }
        Self { rx }
    }

    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    #[cfg(test)]
    pub(crate) fn new(upstream: reqwest::blocking::Response) -> Self {
        Self::from_reader(upstream)
    }

    pub(crate) async fn recv_timeout_async(
        &mut self,
        timeout: Duration,
    ) -> Result<UpstreamSseFramePumpItem, RecvTimeoutError> {
        match tokio::time::timeout(timeout, self.rx.recv()).await {
            Ok(Some(item)) => Ok(item),
            Ok(None) => Err(RecvTimeoutError::Disconnected),
            Err(_) => Err(RecvTimeoutError::Timeout),
        }
    }
}

/// 函数 `sse_keepalive_interval`
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
pub(super) fn sse_keepalive_interval() -> Duration {
    Duration::from_millis(crate::gateway::current_sse_keepalive_interval_ms())
}

pub(super) fn stream_wait_timeout(last_upstream_activity: Instant) -> Duration {
    let keepalive = sse_keepalive_interval();
    let Some(idle_timeout) = crate::gateway::upstream_stream_timeout() else {
        return keepalive;
    };
    let elapsed = last_upstream_activity.elapsed();
    if elapsed >= idle_timeout {
        return Duration::from_millis(1);
    }
    keepalive.min(
        idle_timeout
            .saturating_sub(elapsed)
            .max(Duration::from_millis(1)),
    )
}

pub(super) fn stream_idle_timed_out(last_upstream_activity: Instant) -> bool {
    crate::gateway::upstream_stream_timeout()
        .is_some_and(|idle_timeout| last_upstream_activity.elapsed() >= idle_timeout)
}

pub(super) fn stream_idle_timeout_message() -> String {
    STREAM_IDLE_TIMEOUT_FALLBACK_MESSAGE.to_string()
}

pub(super) fn should_emit_keepalive_after_first_frame(saw_upstream_frame: bool) -> bool {
    crate::gateway::current_sse_keepalive_enabled() && saw_upstream_frame
}

/// 函数 `mark_collector_terminal_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn mark_collector_terminal_success(
    usage_collector: &Arc<Mutex<PassthroughSseCollector>>,
) {
    if let Ok(mut collector) = usage_collector.lock() {
        collector.saw_terminal = true;
        collector.terminal_error = None;
    }
}

/// 函数 `stream_incomplete_message`
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
pub(super) fn stream_incomplete_message() -> String {
    STREAM_INCOMPLETE_FALLBACK_MESSAGE.to_string()
}

/// 函数 `stream_reader_disconnected_message`
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
pub(super) fn stream_reader_disconnected_message() -> String {
    STREAM_INCOMPLETE_FALLBACK_MESSAGE.to_string()
}

pub(super) fn upstream_hint_or_stream_incomplete_message(
    upstream_error_hint: Option<&str>,
) -> String {
    upstream_error_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(stream_incomplete_message)
}

/// 函数 `classify_upstream_stream_read_error`
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
pub(super) fn classify_upstream_stream_read_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return STREAM_READ_FAILED_FALLBACK_MESSAGE.to_string();
    }
    let normalized = trimmed.to_ascii_lowercase();
    if normalized == "request or response body error" || normalized == "stream read failed" {
        return STREAM_READ_FAILED_FALLBACK_MESSAGE.to_string();
    }
    if normalized.contains("timed out") || normalized.contains("timeout") {
        return STREAM_IDLE_TIMEOUT_FALLBACK_MESSAGE.to_string();
    }
    if normalized.contains("connection reset")
        || normalized.contains("connection aborted")
        || normalized.contains("connection was forcibly closed")
        || normalized.contains("broken pipe")
        || normalized.contains("unexpected eof")
        || normalized.contains("connection closed before message completed")
    {
        return STREAM_INCOMPLETE_FALLBACK_MESSAGE.to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
