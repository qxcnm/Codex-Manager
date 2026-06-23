use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::unfold;

use crate::account::proxy_testing::client::{build_proxy_test_client, ProxyTestRedirectPolicy};
use crate::account::proxy_testing::jobs::{SpeedSample, SpeedMetricSummary};

const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);
const PAYLOADS: &[u64] = &[100_000, 1_000_000, 10_000_000, 25_000_000, 100_000_000];

#[derive(Debug, Clone)]
pub struct CloudflareTestOutcome {
    pub status: String,
    pub observed_ip: Option<String>,
    pub observed_country: Option<String>,
    pub observed_colo: Option<String>,
    pub download_samples: Vec<SpeedSample>,
    pub upload_samples: Vec<SpeedSample>,
    pub download_summary: Option<SpeedMetricSummary>,
    pub upload_summary: Option<SpeedMetricSummary>,
    pub error: Option<String>,
    pub latency_ms: Option<u64>,
    pub jitter_ms: Option<u64>,
}

pub async fn run_cloudflare_speed_test<F, PDl, PUl, SDl, SUl, PF, PL>(
    proxy_url: &str,
    max_payload_bytes: u64,
    should_cancel: F,
    on_download_progress: PDl,
    on_upload_progress: PUl,
    on_download_sample: SDl,
    on_upload_sample: SUl,
    on_preflight: PF,
    on_payload_start: PL,
) -> CloudflareTestOutcome
where
    F: Fn() -> bool + Clone + Send + Sync + 'static,
    PDl: Fn(u64, f64) + Clone + Send + Sync + 'static,
    PUl: Fn(u64, f64) + Clone + Send + Sync + 'static,
    SDl: Fn(SpeedSample) + Clone + Send + Sync + 'static,
    SUl: Fn(SpeedSample) + Clone + Send + Sync + 'static,
    PF: Fn(String, String, String) + Clone + Send + Sync + 'static,
    PL: Fn(u64, bool) + Clone + Send + Sync + 'static,
{
    let proxy_url_str = proxy_url.to_string();
    log::info!("Cloudflare speed test: started for proxy: {}", crate::account_proxy::redact_proxy_url_for_log(&proxy_url_str));

    let mut outcome = CloudflareTestOutcome {
        status: "failed".to_string(),
        observed_ip: None,
        observed_country: None,
        observed_colo: None,
        download_samples: Vec::new(),
        upload_samples: Vec::new(),
        download_summary: None,
        upload_summary: None,
        error: None,
        latency_ms: None,
        jitter_ms: None,
    };

    if should_cancel() {
        log::warn!("Cloudflare speed test: cancelled before starting");
        outcome.status = "cancelled".to_string();
        outcome.error = Some("cancelled before start".to_string());
        return outcome;
    }

    // Build client
    let (client, _) = match build_proxy_test_client(
        &proxy_url_str,
        ProxyTestRedirectPolicy::Limited(5),
        false,
    ) {
        Ok(res) => res,
        Err(err) => {
            log::error!("Cloudflare speed test: failed to build proxy client: {}", err.message);
            outcome.error = Some(format!("failed to build proxy client: {}", err.message));
            return outcome;
        }
    };

    // 1. Preflight
    if should_cancel() {
        log::warn!("Cloudflare speed test: cancelled before preflight");
        outcome.status = "cancelled".to_string();
        return outcome;
    }

    log::info!("Cloudflare speed test: starting preflight check...");
    match do_preflight(&client, &should_cancel).await {
        Ok((ip, country, colo)) => {
            log::info!("Cloudflare speed test: preflight success! IP={}, country={}, colo={}", ip, country, colo);
            outcome.observed_ip = Some(ip.clone());
            outcome.observed_country = Some(country.clone());
            outcome.observed_colo = Some(colo.clone());
            on_preflight(ip, country, colo);
        }
        Err(err) => {
            log::error!("Cloudflare speed test: preflight check failed: {}", err);
            outcome.error = Some(format!("Preflight check failed: {err}"));
            return outcome;
        }
    }

    // 1.5. Latency phase
    if should_cancel() {
        log::warn!("Cloudflare speed test: cancelled before latency check");
        outcome.status = "cancelled".to_string();
        return outcome;
    }

    match measure_cloudflare_latency(&client, &should_cancel).await {
        Ok((latency, jitter)) => {
            outcome.latency_ms = Some(latency);
            outcome.jitter_ms = Some(jitter);
        }
        Err(err) => {
            log::warn!("Cloudflare speed test: latency measurement failed: {}", err);
        }
    }

    // 2. Download phase
    log::info!("Cloudflare speed test: starting download phase. Max payload bytes={}", max_payload_bytes);
    let mut last_payload_mbps = None;
    for &payload in PAYLOADS {
        if payload > max_payload_bytes {
            log::info!("Cloudflare speed test: payload {} exceeds max_payload_bytes {}, stopping download loop", payload, max_payload_bytes);
            break;
        }
        if should_cancel() {
            log::warn!("Cloudflare speed test: cancelled in download loop");
            outcome.status = "cancelled".to_string();
            return outcome;
        }

        // Dynamic stop: if previous payload was too slow (< 1.5 Mbps), don't increase size
        if let Some(mbps) = last_payload_mbps {
            if mbps < 1.5 {
                log::info!("Cloudflare speed test: previous download payload speed {:.2} Mbps is below threshold 1.5 Mbps, dynamic stop triggered", mbps);
                break;
            }
        }

        log::info!("Cloudflare speed test: starting download payload={}", payload);
        on_payload_start(payload, true);

        let mut samples_for_payload = Vec::new();
        for i in 0..3 {
            if should_cancel() {
                log::warn!("Cloudflare speed test: cancelled in download sample loop");
                outcome.status = "cancelled".to_string();
                return outcome;
            }
            log::info!("Cloudflare speed test: running download payload={} sample={}/3...", payload, i + 1);
            match run_download_sample(&client, payload, &should_cancel, &on_download_progress).await {
                Ok(sample) => {
                    log::info!("Cloudflare speed test: download payload={} sample={}/3 success: {:.2} Mbps in {}ms", payload, i + 1, sample.mbps, sample.duration_ms);
                    on_download_sample(sample.clone());
                    outcome.download_samples.push(sample.clone());
                    samples_for_payload.push(sample);
                }
                Err(err) => {
                    log::error!("Cloudflare speed test: download payload={} sample={}/3 failed: {}", payload, i + 1, err);
                    outcome.error = Some(format!("Download test failed: {err}"));
                    return outcome;
                }
            }
        }

        // Calculate average speed of current payload for dynamic stop decision
        if !samples_for_payload.is_empty() {
            let sum: f64 = samples_for_payload.iter().map(|s| s.mbps).sum();
            let avg = sum / samples_for_payload.len() as f64;
            log::info!("Cloudflare speed test: download payload={} average speed={:.2} Mbps", payload, avg);
            last_payload_mbps = Some(avg);
        }
    }

    // Calculate download summary
    outcome.download_summary = calculate_summary(&outcome.download_samples);
    if let Some(ref sum) = outcome.download_summary {
        log::info!("Cloudflare speed test: download phase completed. Median={:.2} Mbps, Best={:.2} Mbps", sum.median, sum.best);
    }

    // 3. Upload phase
    log::info!("Cloudflare speed test: starting upload phase. Max payload bytes={}", max_payload_bytes);
    last_payload_mbps = None;
    for &payload in PAYLOADS {
        if payload > max_payload_bytes {
            log::info!("Cloudflare speed test: upload payload {} exceeds max_payload_bytes {}, stopping upload loop", payload, max_payload_bytes);
            break;
        }
        if should_cancel() {
            log::warn!("Cloudflare speed test: cancelled in upload loop");
            outcome.status = "cancelled".to_string();
            return outcome;
        }

        // Dynamic stop: if previous payload was too slow (< 1.5 Mbps), don't increase size
        if let Some(mbps) = last_payload_mbps {
            if mbps < 1.5 {
                log::info!("Cloudflare speed test: previous upload payload speed {:.2} Mbps is below threshold 1.5 Mbps, dynamic stop triggered", mbps);
                break;
            }
        }

        log::info!("Cloudflare speed test: starting upload payload={}", payload);
        on_payload_start(payload, false);

        let mut samples_for_payload = Vec::new();
        for i in 0..3 {
            if should_cancel() {
                log::warn!("Cloudflare speed test: cancelled in upload sample loop");
                outcome.status = "cancelled".to_string();
                return outcome;
            }
            log::info!("Cloudflare speed test: running upload payload={} sample={}/3...", payload, i + 1);
            match run_upload_sample(&client, payload, &should_cancel, &on_upload_progress).await {
                Ok(sample) => {
                    log::info!("Cloudflare speed test: upload payload={} sample={}/3 success: {:.2} Mbps in {}ms", payload, i + 1, sample.mbps, sample.duration_ms);
                    on_upload_sample(sample.clone());
                    outcome.upload_samples.push(sample.clone());
                    samples_for_payload.push(sample);
                }
                Err(err) => {
                    log::error!("Cloudflare speed test: upload payload={} sample={}/3 failed: {}", payload, i + 1, err);
                    outcome.status = "partial".to_string();
                    outcome.error = Some(format!("Upload test failed: {err}"));
                    // Calculate upload summary with what we have
                    outcome.upload_summary = calculate_summary(&outcome.upload_samples);
                    return outcome;
                }
            }
        }

        if !samples_for_payload.is_empty() {
            let sum: f64 = samples_for_payload.iter().map(|s| s.mbps).sum();
            let avg = sum / samples_for_payload.len() as f64;
            log::info!("Cloudflare speed test: upload payload={} average speed={:.2} Mbps", payload, avg);
            last_payload_mbps = Some(avg);
        }
    }

    // Calculate upload summary
    outcome.upload_summary = calculate_summary(&outcome.upload_samples);
    if let Some(ref sum) = outcome.upload_summary {
        log::info!("Cloudflare speed test: upload phase completed. Median={:.2} Mbps, Best={:.2} Mbps", sum.median, sum.best);
    }
    outcome.status = "ok".to_string();
    log::info!("Cloudflare speed test: completed successfully!");
    outcome
}

async fn do_preflight<F>(client: &reqwest::Client, should_cancel: &F) -> Result<(String, String, String), String>
where
    F: Fn() -> bool,
{
    log::info!("Cloudflare speed test: attempting preflight check via speed.cloudflare.com/meta...");
    let request_future = client
        .get("https://speed.cloudflare.com/meta")
        .header(reqwest::header::ACCEPT, "application/json")
        .send();

    let response_result = poll_future_with_cancel(request_future, should_cancel).await;
    
    let mut use_fallback = false;
    let mut fallback_reason = String::new();
    
    let (ip, country, colo) = match response_result {
        PollWithCancel::Ready(Ok(response)) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(info) => {
                        let client_ip = info["clientIp"].as_str().unwrap_or("").to_string();
                        let country = info["country"].as_str().unwrap_or("").to_string();
                        let colo = info["colo"].as_str().unwrap_or("").to_string();
                        (client_ip, country, colo)
                    }
                    Err(err) => {
                        use_fallback = true;
                        fallback_reason = format!("parse json failed: {err}");
                        (String::new(), String::new(), String::new())
                    }
                }
            } else {
                use_fallback = true;
                fallback_reason = format!("HTTP status {}", response.status());
                (String::new(), String::new(), String::new())
            }
        }
        PollWithCancel::Ready(Err(err)) => {
            return Err(format!("request failed: {err}"));
        }
        PollWithCancel::Cancelled => return Err("cancelled".to_string()),
    };

    if !use_fallback {
        return Ok((ip, country, colo));
    }

    log::info!("Cloudflare speed test: preflight via /meta failed ({}), trying fallback /cdn-cgi/trace...", fallback_reason);

    let fallback_future = client
        .get("https://speed.cloudflare.com/cdn-cgi/trace")
        .send();

    let fallback_response = match poll_future_with_cancel(fallback_future, should_cancel).await {
        PollWithCancel::Ready(Ok(resp)) => resp,
        PollWithCancel::Ready(Err(err)) => return Err(format!("fallback request failed: {err}")),
        PollWithCancel::Cancelled => return Err("cancelled".to_string()),
    };

    if !fallback_response.status().is_success() {
        return Err(format!("fallback HTTP status {}", fallback_response.status()));
    }

    let text = fallback_response
        .text()
        .await
        .map_err(|err| format!("read fallback text failed: {err}"))?;

    let mut client_ip = String::new();
    let mut country = String::new();
    let mut colo = String::new();

    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "ip" => client_ip = value.to_string(),
                "loc" => country = value.to_string(),
                "colo" => colo = value.to_string(),
                _ => {}
            }
        }
    }

    if client_ip.is_empty() {
        return Err("fallback parse failed: ip not found".to_string());
    }

    Ok((client_ip, country, colo))
}

async fn run_download_sample<F, P>(
    client: &reqwest::Client,
    payload_bytes: u64,
    should_cancel: &F,
    on_progress: &P,
) -> Result<SpeedSample, String>
where
    F: Fn() -> bool,
    P: Fn(u64, f64),
{
    let url = format!("https://speed.cloudflare.com/__down?bytes={payload_bytes}");
    let started_at = Instant::now();

    let response = match poll_future_with_cancel(
        client.get(&url).header(reqwest::header::ACCEPT_ENCODING, "identity").send(),
        should_cancel,
    )
    .await
    {
        PollWithCancel::Ready(Ok(resp)) => resp,
        PollWithCancel::Ready(Err(err)) => return Err(format!("request failed: {err}")),
        PollWithCancel::Cancelled => return Err("cancelled".to_string()),
    };

    if !response.status().is_success() {
        return Err(format!("HTTP status {}", response.status()));
    }

    let mut stream = response.bytes_stream();
    let mut bytes_read = 0_u64;
    let mut last_update = Instant::now();
    let mut transfer_started_at = None;

    loop {
        let next_item = match poll_future_with_cancel(stream.next(), should_cancel).await {
            PollWithCancel::Ready(item) => item,
            PollWithCancel::Cancelled => return Err("cancelled".to_string()),
        };

        match next_item {
            Some(Ok(bytes)) => {
                if transfer_started_at.is_none() {
                    transfer_started_at = Some(Instant::now());
                }
                bytes_read += bytes.len() as u64;
                let active_start = transfer_started_at.unwrap();
                let duration_ms = active_start.elapsed().as_millis().max(1) as f64;
                let seconds = duration_ms / 1000.0;
                let mbps = (bytes_read as f64 * 8.0) / seconds / 1_000_000.0;

                if last_update.elapsed() >= Duration::from_millis(150) {
                    on_progress(bytes_read, mbps);
                    last_update = Instant::now();
                }
            }
            Some(Err(err)) => return Err(format!("stream read error: {err}")),
            None => break,
        }
    }

    let final_start = transfer_started_at.unwrap_or(started_at);
    let duration_ms = final_start.elapsed().as_millis().max(1) as u64;
    let seconds = duration_ms as f64 / 1000.0;
    let mbps = (bytes_read as f64 * 8.0) / seconds / 1_000_000.0;

    // Репортим финальное состояние
    on_progress(bytes_read, mbps);

    Ok(SpeedSample {
        payload_bytes,
        duration_ms,
        mbps,
    })
}

async fn run_upload_sample<F, P>(
    client: &reqwest::Client,
    payload_bytes: u64,
    should_cancel: &F,
    on_progress: &P,
) -> Result<SpeedSample, String>
where
    F: Fn() -> bool,
    P: Fn(u64, f64) + Send + Sync + Clone + 'static,
{
    let started_at = Instant::now();
    let upload_started_at = Arc::new(std::sync::Mutex::new(None));
    let upload_started_at_clone = upload_started_at.clone();

    let bytes_written = Arc::new(AtomicU64::new(0));
    let bytes_written_clone = bytes_written.clone();
    let chunk_size = 65536;

    let progress_cb = on_progress.clone();
    let last_update = Arc::new(std::sync::Mutex::new(Instant::now()));

    let stream = unfold(0u64, move |sent| {
        let bytes_written_clone = bytes_written_clone.clone();
        let progress_cb = progress_cb.clone();
        let last_update = last_update.clone();
        let upload_started_at_clone = upload_started_at_clone.clone();
        let started_at = started_at;
        async move {
            if sent >= payload_bytes {
                return None;
            }

            {
                let mut start_lock = upload_started_at_clone.lock().unwrap();
                if start_lock.is_none() {
                    *start_lock = Some(Instant::now());
                }
            }

            let len = (payload_bytes - sent).min(chunk_size as u64) as usize;
            let chunk = bytes::Bytes::from(vec![0u8; len]);
            let written = bytes_written_clone.fetch_add(len as u64, Ordering::SeqCst) + len as u64;

            let start_time = {
                let start_lock = upload_started_at_clone.lock().unwrap();
                start_lock.unwrap_or(started_at)
            };
            let duration_ms = start_time.elapsed().as_millis().max(1) as f64;
            let seconds = duration_ms / 1000.0;
            let mbps = (written as f64 * 8.0) / seconds / 1_000_000.0;

            let mut last_up = last_update.lock().unwrap();
            if last_up.elapsed() >= Duration::from_millis(150) || written >= payload_bytes {
                progress_cb(written, mbps);
                *last_up = Instant::now();
            }

            Some((Ok::<_, std::io::Error>(chunk), sent + len as u64))
        }
    });

    let body = reqwest::Body::wrap_stream(stream);

    let response_future = client
        .post("https://speed.cloudflare.com/__up")
        .body(body)
        .send();

    let send_result = poll_future_with_cancel(response_future, should_cancel).await;

    match send_result {
        PollWithCancel::Ready(Ok(response)) => {
            if !response.status().is_success() {
                return Err(format!("HTTP status {}", response.status()));
            }
        }
        PollWithCancel::Ready(Err(err)) => return Err(format!("request failed: {err}")),
        PollWithCancel::Cancelled => return Err("cancelled".to_string()),
    }

    let final_bytes = bytes_written.load(Ordering::SeqCst);
    let final_start = {
        let start_lock = upload_started_at.lock().unwrap();
        start_lock.unwrap_or(started_at)
    };
    let duration_ms = final_start.elapsed().as_millis().max(1) as u64;
    let seconds = duration_ms as f64 / 1000.0;
    let mbps = (final_bytes as f64 * 8.0) / seconds / 1_000_000.0;

    // Репортим финальное состояние
    on_progress(final_bytes, mbps);

    Ok(SpeedSample {
        payload_bytes: final_bytes,
        duration_ms,
        mbps,
    })
}

enum PollWithCancel<T> {
    Ready(T),
    Cancelled,
}

async fn poll_future_with_cancel<F, C>(future: F, should_cancel: &C) -> PollWithCancel<F::Output>
where
    F: Future,
    C: Fn() -> bool,
{
    let mut future = pin!(future);
    let mut interval = tokio::time::interval(CANCEL_POLL_INTERVAL);
    // Пропускаем первый немедленный тик интервала
    interval.tick().await;

    loop {
        if should_cancel() {
            return PollWithCancel::Cancelled;
        }
        tokio::select! {
            output = &mut future => {
                return PollWithCancel::Ready(output);
            }
            _ = interval.tick() => {
                if should_cancel() {
                    return PollWithCancel::Cancelled;
                }
            }
        }
    }
}

fn calculate_summary(samples: &[SpeedSample]) -> Option<SpeedMetricSummary> {
    if samples.is_empty() {
        return None;
    }
    
    let max_payload = samples.iter().map(|s| s.payload_bytes).max().unwrap_or(0);
    if max_payload == 0 {
        return None;
    }

    let target_samples: Vec<&SpeedSample> = samples
        .iter()
        .filter(|s| s.payload_bytes == max_payload)
        .collect();

    if target_samples.is_empty() {
        return None;
    }

    let mut values: Vec<f64> = target_samples.iter().map(|s| s.mbps).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let sum: f64 = values.iter().sum();
    let average = sum / values.len() as f64;

    let best = *values.iter().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap();

    let median = {
        let n = values.len();
        if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        }
    };

    let p90 = {
        let n = values.len();
        let idx = ((n - 1) as f64 * 0.9).round() as usize;
        values[idx.min(n - 1)]
    };

    Some(SpeedMetricSummary {
        median,
        average,
        p90,
        best,
    })
}

async fn measure_cloudflare_latency<F>(
    client: &reqwest::Client,
    should_cancel: &F,
) -> Result<(u64, u64), String>
where
    F: Fn() -> bool,
{
    log::info!("Cloudflare speed test: starting latency/jitter measurement...");
    let mut latencies = Vec::with_capacity(10);
    
    for _ in 0..10 {
        if should_cancel() {
            return Err("cancelled".to_string());
        }
        
        let start = Instant::now();
        let request_future = client.get("https://speed.cloudflare.com/meta").send();
        
        match poll_future_with_cancel(request_future, should_cancel).await {
            PollWithCancel::Ready(Ok(resp)) => {
                if resp.status().is_success() {
                    let elapsed = start.elapsed().as_millis() as u64;
                    latencies.push(elapsed);
                } else {
                    log::warn!("Cloudflare speed test: latency probe HTTP status {}", resp.status());
                }
            }
            PollWithCancel::Ready(Err(err)) => {
                log::warn!("Cloudflare speed test: latency probe request failed: {}", err);
            }
            PollWithCancel::Cancelled => return Err("cancelled".to_string()),
        }
        
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    if latencies.len() < 2 {
        return Err("insufficient successful latency samples".to_string());
    }

    let clean_latencies = &latencies[1..];
    if clean_latencies.is_empty() {
        return Err("no clean latency samples".to_string());
    }

    let mut sorted = clean_latencies.to_vec();
    sorted.sort_unstable();
    let median = {
        let n = sorted.len();
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2
        }
    };

    let mut sum_diff = 0.0;
    for i in 1..clean_latencies.len() {
        sum_diff += (clean_latencies[i] as i64 - clean_latencies[i - 1] as i64).abs() as f64;
    }
    let jitter = (sum_diff / (clean_latencies.len() - 1) as f64).round() as u64;

    log::info!(
        "Cloudflare speed test: latency measurement completed. Raw: {:?}, Median: {} ms, Jitter: {} ms",
        latencies,
        median,
        jitter
    );

    Ok((median, jitter))
}
