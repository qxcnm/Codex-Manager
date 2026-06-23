use std::time::{Duration, Instant};

use super::model::CfStyleLatencyResult;
use super::stats;

/// Run Cloudflare-style unloaded latency measurement.
///
/// 1. Warmup request (result discarded)
/// 2. `sample_count` sample requests to `/__down?bytes=0`
/// 3. Compute raw stats (min, avg, median, p90, p95, jitter)
pub(crate) async fn run_latency(
    client: &reqwest::Client,
    base_url: &str,
    sample_count: usize,
    should_cancel: &(impl Fn() -> bool + Send + Sync),
) -> Result<CfStyleLatencyResult, String> {
    let download_url = format!("{}/__down", base_url);

    // Warmup — single request, result discarded
    let _ = client
        .get(format!("{}?bytes=0", download_url))
        .send()
        .await;

    let mut raw_samples = Vec::with_capacity(sample_count);

    for _ in 0..sample_count {
        if should_cancel() {
            return Err("cancelled".to_string());
        }

        let start = Instant::now();
        match client
            .get(format!("{}?bytes=0", download_url))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                // Consume body to complete the round-trip
                let _ = resp.bytes().await;
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                raw_samples.push(elapsed_ms);
            }
            Ok(resp) => {
                log::warn!("cf_style latency: probe returned HTTP {}", resp.status());
            }
            Err(err) => {
                log::warn!("cf_style latency: probe failed: {}", err);
            }
        }

        // Brief pause between samples to avoid back-pressure artefacts
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    if raw_samples.len() < 2 {
        return Err("insufficient latency samples".to_string());
    }

    let jitter_val = stats::jitter(&raw_samples);
    let summary = stats::compute_summary(&raw_samples)
        .ok_or_else(|| "failed to compute latency stats".to_string())?;

    Ok(CfStyleLatencyResult {
        raw_samples_ms: raw_samples,
        min_ms: summary.min,
        avg_ms: summary.avg,
        median_ms: summary.median,
        p90_ms: summary.p90,
        p95_ms: summary.p95,
        jitter_ms: jitter_val,
    })
}
