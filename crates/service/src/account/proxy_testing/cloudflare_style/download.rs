use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::StreamExt;

use super::config;
use super::model::*;
use super::stats;

/// Intermediate result from the download phase.
pub(crate) struct DownloadRunResult {
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

/// Run Cloudflare-style download throughput measurement.
///
/// For each payload size up to `max_payload_bytes`:
///   Run `tests_per_payload` samples.
///   Track total_duration (includes TTFB), ttfb, transfer_duration.
///   Compute raw_mbps from total_duration, adjusted_mbps from transfer_duration.
///   Aggregate final result using p90 of adjusted Mbps.
pub(crate) async fn run_download(
    client: &reqwest::Client,
    base_url: &str,
    preset: config::CfPresetValue,
    max_payload_bytes: u64,
    tests_per_payload: usize,
    should_cancel: &(impl Fn() -> bool + Send + Sync),
    on_progress: &(impl Fn(u64, f64) + Send + Sync),
) -> DownloadRunResult {


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
        let should_cancel_ref = should_cancel;
        for (idx, &payload) in payloads.iter().enumerate() {
            tasks.push(async move {
                let task_on_progress = |bytes, _mbps| {
                    tracker_ref.update_task_bytes(idx, bytes);
                };
                run_single_download(client_ref, base_url_ref, payload, should_cancel_ref, &task_on_progress).await
            });
        }

        let all_runs = futures_util::future::join_all(tasks).await;

        let mut ok_mbps: Vec<f64> = all_runs
            .iter()
            .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
            .map(|r| r.adjusted_mbps)
            .collect();
        ok_mbps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let final_mbps = stats::percentile(&ok_mbps, 0.9).unwrap_or(0.0);

        log::info!(
            "cf_style download parallel: completed. final_mbps={:.2}, total_runs={}",
            final_mbps,
            all_runs.len()
        );

        return DownloadRunResult {
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

        // Dynamic stop: if previous payload throughput fell below threshold
        if let Some(avg) = last_payload_avg_mbps {
            if avg < config::DYNAMIC_STOP_THRESHOLD_MBPS {
                log::info!(
                    "cf_style download: dynamic stop at {:.2} Mbps (threshold {:.1})",
                    avg,
                    config::DYNAMIC_STOP_THRESHOLD_MBPS
                );
                break;
            }
        }

        log::info!("cf_style download: starting payload={} bytes", payload);

        let mut payload_runs = Vec::new();
        for i in 0..tests_per_payload {
            if should_cancel() {
                break;
            }

            log::info!(
                "cf_style download: payload={} sample={}/{}",
                payload,
                i + 1,
                tests_per_payload
            );
            let run = run_single_download(client, base_url, payload, should_cancel, on_progress).await;

            if matches!(run.status, CfStyleRunStatus::Ok) {
                log::info!(
                    "cf_style download: payload={} sample={}/{} => raw={:.2} adjusted={:.2} Mbps",
                    payload,
                    i + 1,
                    tests_per_payload,
                    run.raw_mbps,
                    run.adjusted_mbps
                );
            }

            payload_runs.push(run);
        }

        // Calculate average adjusted Mbps for dynamic stop decision
        let ok_runs: Vec<f64> = payload_runs
            .iter()
            .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
            .map(|r| r.adjusted_mbps)
            .collect();
        if !ok_runs.is_empty() {
            last_payload_avg_mbps = stats::average(&ok_runs);
        }

        all_runs.extend(payload_runs);
    }

    // Aggregate: p90 of adjusted Mbps from all successful runs
    let mut ok_mbps: Vec<f64> = all_runs
        .iter()
        .filter(|r| matches!(r.status, CfStyleRunStatus::Ok))
        .map(|r| r.adjusted_mbps)
        .collect();
    ok_mbps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let final_mbps = stats::percentile(&ok_mbps, 0.9).unwrap_or(0.0);

    log::info!(
        "cf_style download: completed. final_mbps={:.2}, total_runs={}",
        final_mbps,
        all_runs.len()
    );

    DownloadRunResult {
        runs: all_runs,
        final_mbps,
    }
}

async fn run_single_download(
    client: &reqwest::Client,
    base_url: &str,
    payload_bytes: u64,
    should_cancel: &(impl Fn() -> bool + Send + Sync),
    on_progress: &(impl Fn(u64, f64) + Send + Sync),
) -> CfStyleThroughputRun {
    let url = format!("{}/__down?bytes={}", base_url, payload_bytes);
    let request_started = Instant::now();

    let response = match client
        .get(&url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            let total_ms = request_started.elapsed().as_millis() as u64;
            return CfStyleThroughputRun {
                payload_bytes,
                transferred_bytes: 0,
                total_duration_ms: total_ms,
                ttfb_ms: None,
                transfer_duration_ms: 0,
                raw_mbps: 0.0,
                adjusted_mbps: 0.0,
                status: if err.is_timeout() {
                    CfStyleRunStatus::Timeout
                } else {
                    CfStyleRunStatus::Failed
                },
                error: Some(format!("request failed: {err}")),
            };
        }
    };

    if !response.status().is_success() {
        let total_ms = request_started.elapsed().as_millis() as u64;
        return CfStyleThroughputRun {
            payload_bytes,
            transferred_bytes: 0,
            total_duration_ms: total_ms,
            ttfb_ms: None,
            transfer_duration_ms: 0,
            raw_mbps: 0.0,
            adjusted_mbps: 0.0,
            status: CfStyleRunStatus::Failed,
            error: Some(format!("HTTP {}", response.status())),
        };
    }

    // TTFB = time from request start until headers received (we are past headers here)
    let ttfb_ms = request_started.elapsed().as_millis() as u64;

    // Stream the body, measuring transfer time separately from TTFB
    let mut stream = response.bytes_stream();
    let mut bytes_read = 0_u64;
    let mut transfer_started: Option<Instant> = None;
    let mut last_progress = Instant::now();

    loop {
        if should_cancel() {
            return CfStyleThroughputRun {
                payload_bytes,
                transferred_bytes: bytes_read,
                total_duration_ms: request_started.elapsed().as_millis() as u64,
                ttfb_ms: Some(ttfb_ms),
                transfer_duration_ms: transfer_started
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0),
                raw_mbps: 0.0,
                adjusted_mbps: 0.0,
                status: CfStyleRunStatus::Cancelled,
                error: Some("cancelled".to_string()),
            };
        }

        match stream.next().await {
            Some(Ok(chunk)) => {
                if transfer_started.is_none() {
                    transfer_started = Some(Instant::now());
                }
                bytes_read += chunk.len() as u64;

                // Throttled progress update (every 150ms)
                if last_progress.elapsed() >= Duration::from_millis(150) {
                    let transfer_dur = transfer_started.unwrap().elapsed();
                    let mbps = stats::calculate_mbps(bytes_read, transfer_dur);
                    on_progress(bytes_read, mbps);
                    last_progress = Instant::now();
                }
            }
            Some(Err(err)) => {
                let total_ms = request_started.elapsed().as_millis() as u64;
                return CfStyleThroughputRun {
                    payload_bytes,
                    transferred_bytes: bytes_read,
                    total_duration_ms: total_ms,
                    ttfb_ms: Some(ttfb_ms),
                    transfer_duration_ms: transfer_started
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0),
                    raw_mbps: 0.0,
                    adjusted_mbps: 0.0,
                    status: CfStyleRunStatus::Failed,
                    error: Some(format!("stream error: {err}")),
                };
            }
            None => break,
        }
    }

    let total_duration = request_started.elapsed();
    let total_duration_ms = total_duration.as_millis().max(1) as u64;
    let transfer_duration = transfer_started
        .map(|t| t.elapsed())
        .unwrap_or(total_duration);
    let transfer_duration_ms = transfer_duration.as_millis().max(1) as u64;

    let raw_mbps = stats::calculate_mbps(bytes_read, total_duration);
    let adjusted_mbps = stats::calculate_mbps(bytes_read, transfer_duration);

    // Final progress report
    on_progress(bytes_read, adjusted_mbps);

    CfStyleThroughputRun {
        payload_bytes,
        transferred_bytes: bytes_read,
        total_duration_ms,
        ttfb_ms: Some(ttfb_ms),
        transfer_duration_ms,
        raw_mbps,
        adjusted_mbps,
        status: CfStyleRunStatus::Ok,
        error: None,
    }
}
