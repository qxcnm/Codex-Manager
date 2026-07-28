use super::{
    http_queue_size, http_stream_queue_size, http_stream_worker_count, http_worker_count,
    send_with_timeout, should_bypass_queue, HTTP_QUEUE_MIN, HTTP_STREAM_QUEUE_MIN,
    HTTP_STREAM_WORKER_MIN, HTTP_WORKER_MIN,
};
use crossbeam_channel::bounded;
use std::time::{Duration, Instant};

/// 函数 `worker_count_has_minimum_guard`
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
fn worker_count_has_minimum_guard() {
    assert!(http_worker_count() >= HTTP_WORKER_MIN);
    assert!(http_stream_worker_count() >= HTTP_STREAM_WORKER_MIN);
}

/// 函数 `queue_size_has_minimum_guard`
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
fn queue_size_has_minimum_guard() {
    assert!(http_queue_size(0) >= HTTP_QUEUE_MIN);
    assert!(http_stream_queue_size(0) >= HTTP_STREAM_QUEUE_MIN);
}

/// 函数 `worker_count_has_default_upper_guard`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn worker_count_has_default_upper_guard() {
    assert!(http_worker_count() <= 16);
    assert!(http_stream_worker_count() <= 4);
}

/// 函数 `full_queue_times_out_quickly`
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
fn full_queue_times_out_quickly() {
    let (tx, rx) = bounded::<usize>(1);
    tx.send(1).expect("seed queue");

    let started = Instant::now();
    let result = send_with_timeout(&tx, 2, Duration::from_millis(10));

    assert_eq!(result, Err(2));
    assert!(started.elapsed() < Duration::from_millis(200));
    drop(rx);
}

/// 函数 `bypass_queue_covers_health_and_metrics`
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
fn bypass_queue_covers_health_and_metrics() {
    assert!(should_bypass_queue("/health"));
    assert!(should_bypass_queue("/metrics"));
    assert!(!should_bypass_queue("/rpc"));
}
