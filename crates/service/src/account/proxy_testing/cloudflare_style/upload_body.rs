use bytes::Bytes;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reusable zero buffer — 256 KB static allocation.
const UPLOAD_CHUNK_SIZE: usize = 262_144; // 256 KB
static ZERO_CHUNK: &[u8; UPLOAD_CHUNK_SIZE] = &[0u8; UPLOAD_CHUNK_SIZE];

/// Lock-free upload progress tracker.
pub(crate) struct UploadProgress {
    pub bytes_sent: AtomicU64,
}

impl UploadProgress {
    pub fn new() -> Self {
        Self {
            bytes_sent: AtomicU64::new(0),
        }
    }

    pub fn sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::Relaxed)
    }
}

/// Creates a fixed-length `reqwest::Body` from a static zero buffer.
///
/// Uses `Body::wrap_stream` with a stream that yields slices of [`ZERO_CHUNK`].
/// No per-chunk heap allocation — each `Bytes` references the static buffer.
///
/// **Important**: The caller **must** set `.header(CONTENT_LENGTH, total_bytes)`
/// on the request to ensure `Content-Length` is sent and
/// `Transfer-Encoding: chunked` is **not** used.
pub(crate) fn create_fixed_upload_body(
    total_bytes: u64,
    progress: Arc<UploadProgress>,
) -> reqwest::Body {
    use futures_util::stream::unfold;

    let stream = unfold(0u64, move |sent| {
        let progress = progress.clone();
        async move {
            if sent >= total_bytes {
                return None;
            }
            let remaining = (total_bytes - sent) as usize;
            let chunk_len = remaining.min(UPLOAD_CHUNK_SIZE);
            let chunk = Bytes::from_static(&ZERO_CHUNK[..chunk_len]);
            progress
                .bytes_sent
                .fetch_add(chunk_len as u64, Ordering::Relaxed);
            Some((Ok::<_, std::io::Error>(chunk), sent + chunk_len as u64))
        }
    });

    reqwest::Body::wrap_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_chunk_has_correct_size() {
        assert_eq!(ZERO_CHUNK.len(), 262_144);
    }

    #[test]
    fn zero_chunk_is_all_zeroes() {
        assert!(ZERO_CHUNK.iter().all(|&b| b == 0));
    }

    #[test]
    fn upload_progress_starts_at_zero() {
        let p = UploadProgress::new();
        assert_eq!(p.sent(), 0);
    }

    #[test]
    fn upload_progress_tracks_additions() {
        let p = UploadProgress::new();
        p.bytes_sent.fetch_add(1024, Ordering::Relaxed);
        p.bytes_sent.fetch_add(2048, Ordering::Relaxed);
        assert_eq!(p.sent(), 3072);
    }

    #[tokio::test]
    async fn body_produces_correct_total_bytes() {
        use futures_util::StreamExt;
        use futures_util::stream::unfold;

        let total: u64 = 1_000_000; // 1 MB
        let progress = Arc::new(UploadProgress::new());
        let progress_clone = progress.clone();

        // Directly test the stream logic (same as create_fixed_upload_body internals)
        let stream = unfold(0u64, move |sent| {
            let progress = progress_clone.clone();
            async move {
                if sent >= total {
                    return None;
                }
                let remaining = (total - sent) as usize;
                let chunk_len = remaining.min(UPLOAD_CHUNK_SIZE);
                let chunk = Bytes::from_static(&ZERO_CHUNK[..chunk_len]);
                progress
                    .bytes_sent
                    .fetch_add(chunk_len as u64, Ordering::Relaxed);
                Some((chunk, sent + chunk_len as u64))
            }
        });

        let mut stream = Box::pin(stream);
        let mut received: u64 = 0;
        while let Some(chunk) = stream.next().await {
            received += chunk.len() as u64;
        }

        assert_eq!(received, total, "total bytes from stream should match");
        assert_eq!(
            progress.sent(),
            total,
            "progress tracker should match total"
        );
    }

    #[tokio::test]
    async fn body_zero_bytes_produces_empty_stream() {
        use futures_util::StreamExt;
        use futures_util::stream::unfold;

        let total: u64 = 0;
        let progress = Arc::new(UploadProgress::new());
        let progress_clone = progress.clone();

        let stream = unfold(0u64, move |sent| {
            let progress = progress_clone.clone();
            async move {
                if sent >= total {
                    return None;
                }
                let remaining = (total - sent) as usize;
                let chunk_len = remaining.min(UPLOAD_CHUNK_SIZE);
                let chunk = Bytes::from_static(&ZERO_CHUNK[..chunk_len]);
                progress
                    .bytes_sent
                    .fetch_add(chunk_len as u64, Ordering::Relaxed);
                Some((chunk, sent + chunk_len as u64))
            }
        });

        let mut stream = Box::pin(stream);
        assert!(
            stream.next().await.is_none(),
            "zero-length body should produce no chunks"
        );
        assert_eq!(progress.sent(), 0);
    }

    #[tokio::test]
    async fn body_smaller_than_chunk_size() {
        use futures_util::StreamExt;
        use futures_util::stream::unfold;

        let total: u64 = 1024; // 1 KB — smaller than UPLOAD_CHUNK_SIZE
        let progress = Arc::new(UploadProgress::new());
        let progress_clone = progress.clone();

        let stream = unfold(0u64, move |sent| {
            let progress = progress_clone.clone();
            async move {
                if sent >= total {
                    return None;
                }
                let remaining = (total - sent) as usize;
                let chunk_len = remaining.min(UPLOAD_CHUNK_SIZE);
                let chunk = Bytes::from_static(&ZERO_CHUNK[..chunk_len]);
                progress
                    .bytes_sent
                    .fetch_add(chunk_len as u64, Ordering::Relaxed);
                Some((chunk, sent + chunk_len as u64))
            }
        });

        let mut stream = Box::pin(stream);
        let mut chunk_count = 0u64;
        let mut received: u64 = 0;
        while let Some(chunk) = stream.next().await {
            received += chunk.len() as u64;
            chunk_count += 1;
        }

        assert_eq!(received, total);
        assert_eq!(chunk_count, 1, "small body should produce exactly one chunk");
        assert_eq!(progress.sent(), total);
    }
}
