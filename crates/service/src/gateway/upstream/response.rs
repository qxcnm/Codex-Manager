use super::attempt_flow::transport::runtime::upstream_runtime;
use bytes::Bytes;
use std::collections::VecDeque;
#[cfg(test)]
use std::io::Read;
#[cfg(test)]
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, Receiver};

#[cfg(test)]
const GATEWAY_STREAM_READ_CHUNK_BYTES: usize = 8 * 1024;
const GATEWAY_STREAM_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
pub(crate) enum GatewayByteStreamItem {
    Chunk(Bytes),
    Eof,
    Error(String),
}

#[derive(Debug)]
pub(crate) struct GatewayByteStream {
    rx: Receiver<GatewayByteStreamItem>,
    replay: VecDeque<GatewayByteStreamItem>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GatewayStreamPrefetchTerminal {
    Open,
    PrefixLimit,
    IdleTimeout,
    WallClockTimeout,
    Eof,
    Error(String),
    Disconnected,
}

impl GatewayByteStream {
    pub(crate) fn from_bytes(body: Bytes) -> Self {
        let (tx, rx) = channel(2);
        if !body.is_empty() {
            let _ = tx.try_send(GatewayByteStreamItem::Chunk(body));
        }
        let _ = tx.try_send(GatewayByteStreamItem::Eof);
        Self::from_receiver(rx)
    }

    #[cfg(test)]
    pub(crate) fn from_blocking_response(mut response: reqwest::blocking::Response) -> Self {
        let (tx, rx) = channel::<GatewayByteStreamItem>(GATEWAY_STREAM_CHANNEL_CAPACITY);
        thread::spawn(move || loop {
            let mut buffer = vec![0_u8; GATEWAY_STREAM_READ_CHUNK_BYTES];
            match response.read(&mut buffer) {
                Ok(0) => {
                    let _ = tx.blocking_send(GatewayByteStreamItem::Eof);
                    return;
                }
                Ok(read) => {
                    buffer.truncate(read);
                    if tx
                        .blocking_send(GatewayByteStreamItem::Chunk(Bytes::from(buffer)))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(err) => {
                    let _ = tx.blocking_send(GatewayByteStreamItem::Error(err.to_string()));
                    return;
                }
            }
        });
        Self::from_receiver(rx)
    }

    pub(crate) fn from_receiver(rx: Receiver<GatewayByteStreamItem>) -> Self {
        Self::from_receiver_with_cancel(rx, None)
    }

    pub(crate) fn from_receiver_with_cancel(
        rx: Receiver<GatewayByteStreamItem>,
        cancel: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Self {
        Self {
            rx,
            replay: VecDeque::new(),
            cancel,
        }
    }

    pub(crate) async fn recv_async(&mut self) -> Option<GatewayByteStreamItem> {
        if let Some(item) = self.replay.pop_front() {
            return Some(item);
        }
        self.rx.recv().await
    }

    pub(crate) async fn recv_timeout_async(
        &mut self,
        timeout: Duration,
    ) -> Result<GatewayByteStreamItem, RecvTimeoutError> {
        match tokio::time::timeout(timeout, self.recv_async()).await {
            Ok(Some(item)) => Ok(item),
            Ok(None) => Err(RecvTimeoutError::Disconnected),
            Err(_) => Err(RecvTimeoutError::Timeout),
        }
    }

    pub(crate) async fn read_all_bytes_async(mut self) -> Result<Bytes, String> {
        let mut buffer = Vec::new();
        loop {
            match self.recv_async().await {
                Some(GatewayByteStreamItem::Chunk(bytes)) => buffer.extend_from_slice(&bytes),
                Some(GatewayByteStreamItem::Eof) | None => return Ok(Bytes::from(buffer)),
                Some(GatewayByteStreamItem::Error(error)) => return Err(error),
            }
        }
    }

    // Synchronous access exists only for legacy test fixtures.
    #[cfg(test)]
    pub(crate) fn recv(&mut self) -> Result<GatewayByteStreamItem, mpsc::RecvError> {
        if let Some(item) = self.replay.pop_front() {
            return Ok(item);
        }
        upstream_runtime()
            .map_err(|_| mpsc::RecvError)?
            .block_on(crate::http::gateway_request::with_response_cancellation(
                self.rx.recv(),
            ))
            .map_err(|_| mpsc::RecvError)?
            .ok_or(mpsc::RecvError)
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<GatewayByteStreamItem, RecvTimeoutError> {
        if let Some(item) = self.replay.pop_front() {
            return Ok(item);
        }
        upstream_runtime()
            .map_err(|_| RecvTimeoutError::Disconnected)?
            .block_on(async {
                match crate::http::gateway_request::with_response_cancellation(
                    tokio::time::timeout(timeout, self.rx.recv()),
                )
                .await
                {
                    Ok(Ok(Some(item))) => Ok(item),
                    Ok(Ok(None)) | Err(()) => Err(RecvTimeoutError::Disconnected),
                    Ok(Err(_)) => Err(RecvTimeoutError::Timeout),
                }
            })
    }

    #[cfg(test)]
    fn prefetch_until<F>(
        self,
        max_bytes: usize,
        idle_timeout: Option<Duration>,
        wall_clock_timeout: Option<Duration>,
        should_stop: F,
    ) -> (Bytes, Self, GatewayStreamPrefetchTerminal)
    where
        F: Fn(&[u8]) -> bool,
    {
        upstream_runtime()
            .expect("gateway test runtime")
            .block_on(self.prefetch_until_async(
                max_bytes,
                idle_timeout,
                wall_clock_timeout,
                should_stop,
            ))
    }

    async fn prefetch_until_async<F>(
        mut self,
        max_bytes: usize,
        idle_timeout: Option<Duration>,
        wall_clock_timeout: Option<Duration>,
        should_stop: F,
    ) -> (Bytes, Self, GatewayStreamPrefetchTerminal)
    where
        F: Fn(&[u8]) -> bool,
    {
        let mut prefix = Vec::new();
        let mut replay = VecDeque::new();
        let mut terminal = GatewayStreamPrefetchTerminal::Open;
        let started_at = Instant::now();

        loop {
            if prefix.len() >= max_bytes {
                terminal = GatewayStreamPrefetchTerminal::PrefixLimit;
                break;
            }
            if should_stop(prefix.as_slice()) {
                break;
            }
            let wall_clock_remaining =
                wall_clock_timeout.map(|timeout| timeout.saturating_sub(started_at.elapsed()));
            if wall_clock_remaining.is_some_and(|remaining| remaining.is_zero()) {
                terminal = GatewayStreamPrefetchTerminal::WallClockTimeout;
                break;
            }
            let recv_timeout = match (idle_timeout, wall_clock_remaining) {
                (Some(idle), Some(wall_clock)) => Some(idle.min(wall_clock)),
                (Some(idle), None) => Some(idle),
                (None, Some(wall_clock)) => Some(wall_clock),
                (None, None) => None,
            };
            let next_item = match recv_timeout {
                Some(timeout) => self.recv_timeout_async(timeout).await,
                None => self
                    .recv_async()
                    .await
                    .ok_or(RecvTimeoutError::Disconnected),
            };
            match next_item {
                Ok(item @ GatewayByteStreamItem::Chunk(_)) => {
                    if let GatewayByteStreamItem::Chunk(bytes) = &item {
                        let remaining_bytes = max_bytes.saturating_sub(prefix.len());
                        let copy_len = remaining_bytes.min(bytes.len());
                        prefix.extend_from_slice(&bytes[..copy_len]);
                    }
                    replay.push_back(item);
                }
                Ok(item @ GatewayByteStreamItem::Eof) => {
                    terminal = GatewayStreamPrefetchTerminal::Eof;
                    replay.push_back(item);
                    break;
                }
                Ok(item @ GatewayByteStreamItem::Error(_)) => {
                    if let GatewayByteStreamItem::Error(err) = &item {
                        terminal = GatewayStreamPrefetchTerminal::Error(err.clone());
                    }
                    replay.push_back(item);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    terminal = if wall_clock_timeout
                        .is_some_and(|timeout| started_at.elapsed() >= timeout)
                    {
                        GatewayStreamPrefetchTerminal::WallClockTimeout
                    } else {
                        GatewayStreamPrefetchTerminal::IdleTimeout
                    };
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    terminal = GatewayStreamPrefetchTerminal::Disconnected;
                    break;
                }
            }
        }

        replay.append(&mut self.replay);
        self.replay = replay;
        (Bytes::from(prefix), self, terminal)
    }

    pub(crate) fn close(&mut self) {
        self.close_input();
        self.replay.clear();
    }

    fn close_input(&mut self) {
        self.rx.close();
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }

    /// The left consumer owns provider cancellation. Closing it stops new
    /// provider bytes, while the observer receives every already queued item.
    pub(crate) fn tee(mut self) -> (Self, Self) {
        let (left_tx, left_rx) = channel(GATEWAY_STREAM_CHANNEL_CAPACITY);
        let (right_tx, right_rx) = channel(GATEWAY_STREAM_CHANNEL_CAPACITY);
        let primary_cancel = self.cancel.take();
        if let Ok(runtime) = upstream_runtime() {
            runtime.spawn(async move {
                loop {
                    let item = tokio::select! {
                        _ = left_tx.closed() => break,
                        item = self.recv_async() => item.unwrap_or(GatewayByteStreamItem::Eof),
                    };
                    let terminal = matches!(
                        item,
                        GatewayByteStreamItem::Eof | GatewayByteStreamItem::Error(_)
                    );
                    let left = left_tx.send(item.clone()).await;
                    // Finish the observer send even if the primary consumer
                    // closed during it; dropping right_tx then provides EOF.
                    let _ = right_tx.send(item).await;
                    if terminal {
                        return;
                    }
                    if left.is_err() {
                        break;
                    }
                }
                // Freeze input and flush bytes the HTTP producer had already
                // queued before cancellation. No further network wait occurs.
                self.close_input();
                while let Some(item) = self.recv_async().await {
                    let terminal = matches!(
                        item,
                        GatewayByteStreamItem::Eof | GatewayByteStreamItem::Error(_)
                    );
                    if right_tx.send(item).await.is_err() || terminal {
                        break;
                    }
                }
            });
        }
        (
            Self::from_receiver_with_cancel(left_rx, primary_cancel),
            Self::from_receiver(right_rx),
        )
    }

    #[cfg(test)]
    pub(crate) fn read_all_bytes(self) -> Result<Bytes, String> {
        upstream_runtime()
            .map_err(|error| error.to_string())?
            .block_on(self.read_all_bytes_async())
    }
}

#[derive(Debug)]
pub(crate) struct GatewayStreamResponse {
    status: reqwest::StatusCode,
    headers: reqwest::header::HeaderMap,
    body: GatewayByteStream,
}

impl GatewayStreamResponse {
    pub(crate) fn new(
        status: reqwest::StatusCode,
        headers: reqwest::header::HeaderMap,
        body: GatewayByteStream,
    ) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_blocking_response(response: reqwest::blocking::Response) -> Self {
        let status = response.status();
        let headers = response.headers().clone();
        let body = GatewayByteStream::from_blocking_response(response);
        Self::new(status, headers, body)
    }

    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.status
    }

    pub(crate) fn headers(&self) -> &reqwest::header::HeaderMap {
        &self.headers
    }

    #[cfg(test)]
    pub(crate) fn read_all_bytes(self) -> Result<Bytes, String> {
        upstream_runtime()
            .map_err(|error| error.to_string())?
            .block_on(self.read_all_bytes_async())
    }

    pub(crate) async fn read_all_bytes_async(self) -> Result<Bytes, String> {
        self.body.read_all_bytes_async().await
    }

    pub(crate) async fn read_all_bytes_cancellable(
        self,
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Bytes, String> {
        tokio::select! {
            biased;
            _ = cancellation.wait_for(|cancelled| *cancelled) => Err("broken pipe: downstream HTTP body closed".to_owned()),
            body = self.read_all_bytes_async() => body,
        }
    }

    pub(crate) fn into_body(self) -> GatewayByteStream {
        self.body
    }

    async fn prefetch_until_async<F>(
        self,
        max_bytes: usize,
        idle_timeout: Option<Duration>,
        wall_clock_timeout: Option<Duration>,
        should_stop: F,
    ) -> (Bytes, Self, GatewayStreamPrefetchTerminal)
    where
        F: Fn(&[u8]) -> bool,
    {
        let Self {
            status,
            headers,
            body,
        } = self;
        let (prefix, body, terminal) = body
            .prefetch_until_async(max_bytes, idle_timeout, wall_clock_timeout, should_stop)
            .await;
        (prefix, Self::new(status, headers, body), terminal)
    }
}

impl Drop for GatewayByteStream {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

#[derive(Debug)]
pub(crate) enum GatewayUpstreamResponse {
    #[cfg(test)]
    Blocking(reqwest::blocking::Response),
    Stream(GatewayStreamResponse),
}

impl GatewayUpstreamResponse {
    pub(crate) fn status(&self) -> reqwest::StatusCode {
        match self {
            #[cfg(test)]
            Self::Blocking(response) => response.status(),
            Self::Stream(response) => response.status(),
        }
    }

    pub(crate) fn headers(&self) -> &reqwest::header::HeaderMap {
        match self {
            #[cfg(test)]
            Self::Blocking(response) => response.headers(),
            Self::Stream(response) => response.headers(),
        }
    }

    #[cfg(test)]
    pub(crate) fn into_buffered(self) -> Result<(Bytes, Self), String> {
        upstream_runtime()
            .map_err(|error| error.to_string())?
            .block_on(self.into_buffered_async())
    }

    pub(crate) async fn into_buffered_async(self) -> Result<(Bytes, Self), String> {
        let status = self.status();
        let headers = self.headers().clone();
        let body = match self {
            #[cfg(test)]
            Self::Blocking(response) => {
                GatewayByteStream::from_blocking_response(response)
                    .read_all_bytes_async()
                    .await?
            }
            Self::Stream(response) => response.read_all_bytes_async().await?,
        };
        let rebuilt = Self::Stream(GatewayStreamResponse::new(
            status,
            headers,
            GatewayByteStream::from_bytes(body.clone()),
        ));
        Ok((body, rebuilt))
    }

    #[cfg(test)]
    pub(crate) fn prefetch_stream_prefix<F>(
        self,
        max_bytes: usize,
        idle_timeout: Option<Duration>,
        wall_clock_timeout: Option<Duration>,
        should_stop: F,
    ) -> (Bytes, Self, GatewayStreamPrefetchTerminal)
    where
        F: Fn(&[u8]) -> bool,
    {
        upstream_runtime().expect("gateway test runtime").block_on(
            self.prefetch_stream_prefix_async(
                max_bytes,
                idle_timeout,
                wall_clock_timeout,
                should_stop,
            ),
        )
    }

    pub(crate) async fn prefetch_stream_prefix_async<F>(
        self,
        max_bytes: usize,
        idle_timeout: Option<Duration>,
        wall_clock_timeout: Option<Duration>,
        should_stop: F,
    ) -> (Bytes, Self, GatewayStreamPrefetchTerminal)
    where
        F: Fn(&[u8]) -> bool,
    {
        let response = match self {
            #[cfg(test)]
            Self::Blocking(response) => GatewayStreamResponse::from_blocking_response(response),
            Self::Stream(response) => response,
        };
        let (prefix, response, terminal) = response
            .prefetch_until_async(max_bytes, idle_timeout, wall_clock_timeout, should_stop)
            .await;
        (prefix, Self::Stream(response), terminal)
    }
}

#[cfg(test)]
impl From<reqwest::blocking::Response> for GatewayUpstreamResponse {
    fn from(response: reqwest::blocking::Response) -> Self {
        Self::Blocking(response)
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
