use std::sync::Arc;
use std::time::Instant;
use std::sync::atomic::{AtomicU64, Ordering};

use super::config;
use super::model::*;
use super::stats;
use super::upload_body::{create_fixed_upload_body, UploadProgress};

/// Intermediate result from the upload phase.
pub(crate) struct UploadRunResult {
    pub runs: Vec<CfStyleThroughputRun>,
    pub final_mbps: f64,
}

struct ParallelProgressTracker<'a, F: Fn(u64, f64) + Send + Sync> {
    start_time: Instant,
    task_bytes: Vec<AtomicU64>,
    on_progress: &'a F,
}

impl<'a, F: Fn(u64, f64) + Send + Sync> ParallelProgressTracker<'a, F> {
    fn new(num_tasks: usize, on_progress: &'a F) -> Self {
        let mut task_bytes = Vec::with_capacity(num_tasks);
        for _ in 0..num_tasks {
            task_bytes.push(AtomicU64::new(0));
        }
        Self {
            start_time: Instant::now(),
            task_bytes,
            on_progress,
        }
    }

    fn update_task_bytes(&self, idx: usize, bytes: u64) {
        self.task_bytes[idx].store(bytes, Ordering::SeqCst);
        let total_bytes: u64 = self.task_bytes.iter().map(|b| b.load(Ordering::SeqCst)).sum();
        let elapsed = self.start_time.elapsed();
        let mbps = stats::calculate_mbps(total_bytes, elapsed);
        (self.on_progress)(total_bytes, mbps);
    }
}

/// Run Cloudflare-style upload throughput measurement.
///
/// For each payload size up to `max_payload_bytes`:
///   Run `tests_per_payload` samples using fixed-length body with `Content-Length`.
///   Upload body uses a static zero buffer — no per-chunk heap allocation.
///   Aggregate final result using p90 of Mbps.
pub(crate) async fn run_upload(
    client: &reqwest::Client,
    base_url: &str,
    preset: config::CfPresetValue,
    max_payload_bytes: u64,
    tests_per_payload: usize,
    should_cancel: &(impl Fn() -> bool + Send + Sync),
    on_progress: &(impl Fn(u64, f64) + Send + Sync),
) -> UploadRunResult {


    if preset.is_all() {
        let payloads: Vec<u64> = config::CF_PAYLOADS
            .iter()
            .cloned()
            .filter(|&p| p <= max_payload_bytes)
            .collect();

        let num_tasks = payloads.len();
        let tracker = ParallelProgressTracker::new(num_tasks, on_progress);

        let mut tasks = Vec::new();
        let tracker_ref = &tracker;
        let client_ref = client;
        let base_url_ref = base_url;
        for (idx, &payload) in payloads.iter().enumerate() {
            tasks.push(async move {
                let task_on_progress = |bytes, _mbps| {
                    tracker_ref.update_task_bytes(idx, bytes);
                };
                run_single_upload(client_ref, base_url_ref, payload, &task_on_progress).await
            });
        }

        let all_runs = futures_util::future::join_all(tasks).await;

        let mut ok_mbps: Vec<f64> = all_runs
            .iter()
            .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
            .map(|r| r.raw_mbps)
            .collect();
        ok_mbps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let final_mbps = stats::percentile(&ok_mbps, 0.9).unwrap_or(0.0);

        log::info!(
            "cf_style upload parallel: completed. final_mbps={:.2}, total_runs={}",
            final_mbps,
            all_runs.len()
        );

        return UploadRunResult {
            runs: all_runs,
            final_mbps,
        };
    }

    let mut all_runs = Vec::new();
    let mut last_payload_avg_mbps: Option<f64> = None;

    for &payload in config::CF_PAYLOADS {
        if payload > max_payload_bytes {
            break;
        }
        if should_cancel() {
            break;
        }

        // Dynamic stop
        if let Some(avg) = last_payload_avg_mbps {
            if avg < config::DYNAMIC_STOP_THRESHOLD_MBPS {
                log::info!(
                    "cf_style upload: dynamic stop at {:.2} Mbps (threshold {:.1})",
                    avg,
                    config::DYNAMIC_STOP_THRESHOLD_MBPS
                );
                break;
            }
        }

        log::info!("cf_style upload: starting payload={} bytes", payload);

        let mut payload_runs = Vec::new();
        for i in 0..tests_per_payload {
            if should_cancel() {
                break;
            }

            log::info!(
                "cf_style upload: payload={} sample={}/{}",
                payload,
                i + 1,
                tests_per_payload
            );
            let run = run_single_upload(client, base_url, payload, on_progress).await;

            if matches!(run.status, CfStyleRunStatus::Ok) {
                log::info!(
                    "cf_style upload: payload={} sample={}/{} => {:.2} Mbps",
                    payload,
                    i + 1,
                    tests_per_payload,
                    run.raw_mbps
                );
            }

            payload_runs.push(run);
        }

        // Calculate average for dynamic stop decision
        let ok_runs: Vec<f64> = payload_runs
            .iter()
            .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
            .map(|r| r.raw_mbps)
            .collect();
        if !ok_runs.is_empty() {
            last_payload_avg_mbps = stats::average(&ok_runs);
        }

        all_runs.extend(payload_runs);
    }

    // Aggregate: p90 of Mbps from all successful runs
    let mut ok_mbps: Vec<f64> = all_runs
        .iter()
        .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
        .map(|r| r.raw_mbps)
        .collect();
    ok_mbps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let final_mbps = stats::percentile(&ok_mbps, 0.9).unwrap_or(0.0);

    log::info!(
        "cf_style upload: completed. final_mbps={:.2}, total_runs={}",
        final_mbps,
        all_runs.len()
    );

    UploadRunResult {
        runs: all_runs,
        final_mbps,
    }
}

async fn run_single_upload(
    client: &reqwest::Client,
    base_url: &str,
    payload_bytes: u64,
    on_progress: &(impl Fn(u64, f64) + Send + Sync),
) -> CfStyleThroughputRun {
    let progress = Arc::new(UploadProgress::new());
    let body = create_fixed_upload_body(payload_bytes, progress.clone());

    let request_started = Instant::now();

    // CRITICAL: Set Content-Length header explicitly to avoid chunked encoding.
    // This is the key fix for the upload throughput bottleneck.
    let result = client
        .post(format!("{}/__up", base_url))
        .header(reqwest::header::CONTENT_LENGTH, payload_bytes)
        .body(body)
        .send()
        .await;

    let total_duration = request_started.elapsed();
    let total_duration_ms = total_duration.as_millis().max(1) as u64;
    let bytes_sent = progress.sent();

    match result {
        Ok(response) => {
            if !response.status().is_success() {
                return CfStyleThroughputRun {
                    payload_bytes,
                    transferred_bytes: bytes_sent,
                    total_duration_ms,
                    ttfb_ms: None,
                    transfer_duration_ms: total_duration_ms,
                    raw_mbps: 0.0,
                    adjusted_mbps: 0.0,
                    status: CfStyleRunStatus::Failed,
                    error: Some(format!("HTTP {}", response.status())),
                };
            }

            let raw_mbps = stats::calculate_mbps(bytes_sent, total_duration);
            // For upload, adjusted ≈ raw (no meaningful TTFB separation)
            let adjusted_mbps = raw_mbps;

            on_progress(bytes_sent, adjusted_mbps);

            CfStyleThroughputRun {
                payload_bytes,
                transferred_bytes: bytes_sent,
                total_duration_ms,
                ttfb_ms: None,
                transfer_duration_ms: total_duration_ms,
                raw_mbps,
                adjusted_mbps,
                status: CfStyleRunStatus::Ok,
                error: None,
            }
        }
        Err(err) => {
            let status = if err.is_timeout() {
                CfStyleRunStatus::Timeout
            } else {
                CfStyleRunStatus::Failed
            };
            CfStyleThroughputRun {
                payload_bytes,
                transferred_bytes: bytes_sent,
                total_duration_ms,
                ttfb_ms: None,
                transfer_duration_ms: total_duration_ms,
                raw_mbps: 0.0,
                adjusted_mbps: 0.0,
                status,
                error: Some(format!("upload failed: {err}")),
            }
        }
    }
}
