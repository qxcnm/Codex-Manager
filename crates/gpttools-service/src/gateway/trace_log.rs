use gpttools_core::storage::now_ts;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static TRACE_FILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static TRACE_SEQ: AtomicU64 = AtomicU64::new(1);

fn trace_file_path() -> PathBuf {
    if let Ok(db_path) = std::env::var("GPTTOOLS_DB_PATH") {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            return parent.join("gateway-trace.log");
        }
    }
    PathBuf::from("gateway-trace.log")
}

fn sanitize_text(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn short_fingerprint(value: &str) -> String {
    let mut hash: u64 = 14695981039346656037;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

fn append_trace_line(line: &str) {
    let lock = TRACE_FILE_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };
    let file_path = trace_file_path();
    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
    {
        Ok(file) => file,
        Err(err) => {
            log::warn!(
                "gateway trace open failed: path={}, err={}",
                file_path.display(),
                err
            );
            return;
        }
    };
    if let Err(err) = writeln!(file, "{line}") {
        log::warn!(
            "gateway trace write failed: path={}, err={}",
            file_path.display(),
            err
        );
    }
}

pub(crate) fn next_trace_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);
    let seq = TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("trc_{millis}_{seq:x}")
}

pub(crate) fn log_request_start(
    trace_id: &str,
    key_id: &str,
    method: &str,
    path: &str,
    model: Option<&str>,
    reasoning: Option<&str>,
    is_stream: bool,
    protocol_type: &str,
) {
    let ts = now_ts();
    let model = model.unwrap_or("-");
    let reasoning = reasoning.unwrap_or("-");
    let line = format!(
        "ts={ts} event=REQUEST_START trace_id={} key_id={} method={} path={} model={} reasoning={} stream={} protocol={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(method),
        sanitize_text(path),
        sanitize_text(model),
        sanitize_text(reasoning),
        is_stream,
        sanitize_text(protocol_type),
    );
    append_trace_line(&line);
}

pub(crate) fn log_request_body_preview(trace_id: &str, body: &[u8]) {
    let ts = now_ts();
    let raw = String::from_utf8_lossy(body).to_string();
    let compact = raw
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n' && *ch != '\t')
        .collect::<String>();
    let preview = if compact.len() > 900 {
        format!("{}...", &compact[..900])
    } else {
        compact
    };
    let line = format!(
        "ts={ts} event=REQUEST_BODY trace_id={} len={} preview={}",
        sanitize_text(trace_id),
        body.len(),
        sanitize_text(preview.as_str()),
    );
    append_trace_line(&line);
}

pub(crate) fn log_request_gate_wait(trace_id: &str, key_id: &str, path: &str, model: Option<&str>) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_WAIT trace_id={} key_id={} path={} model={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(path),
        sanitize_text(model.unwrap_or("-")),
    );
    append_trace_line(&line);
}

pub(crate) fn log_request_gate_acquired(
    trace_id: &str,
    key_id: &str,
    path: &str,
    model: Option<&str>,
    wait_ms: u128,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_ACQUIRED trace_id={} key_id={} path={} model={} wait_ms={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(path),
        sanitize_text(model.unwrap_or("-")),
        wait_ms,
    );
    append_trace_line(&line);
}

pub(crate) fn log_request_gate_skip(trace_id: &str, reason: &str) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_SKIP trace_id={} reason={}",
        sanitize_text(trace_id),
        sanitize_text(reason),
    );
    append_trace_line(&line);
}

pub(crate) fn log_candidate_start(
    trace_id: &str,
    idx: usize,
    total: usize,
    account_id: &str,
    strip_session_affinity: bool,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=CANDIDATE_START trace_id={} candidate={}/{} account_id={} strip_session_affinity={}",
        sanitize_text(trace_id),
        idx + 1,
        total,
        sanitize_text(account_id),
        strip_session_affinity,
    );
    append_trace_line(&line);
}

pub(crate) fn log_candidate_skip(
    trace_id: &str,
    idx: usize,
    total: usize,
    account_id: &str,
    reason: &str,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=CANDIDATE_SKIP trace_id={} candidate={}/{} account_id={} reason={}",
        sanitize_text(trace_id),
        idx + 1,
        total,
        sanitize_text(account_id),
        sanitize_text(reason),
    );
    append_trace_line(&line);
}

pub(crate) fn log_attempt_result(
    trace_id: &str,
    account_id: &str,
    upstream_url: Option<&str>,
    status_code: u16,
    error: Option<&str>,
) {
    let ts = now_ts();
    let url = upstream_url.unwrap_or("-");
    let error = error.unwrap_or("-");
    let line = format!(
        "ts={ts} event=ATTEMPT_RESULT trace_id={} account_id={} status={} upstream_url={} error={}",
        sanitize_text(trace_id),
        sanitize_text(account_id),
        status_code,
        sanitize_text(url),
        sanitize_text(error),
    );
    append_trace_line(&line);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log_attempt_profile(
    trace_id: &str,
    account_id: &str,
    candidate_index: usize,
    total: usize,
    strip_session_affinity: bool,
    has_incoming_session: bool,
    has_incoming_turn_state: bool,
    has_incoming_conversation: bool,
    prompt_cache_key: Option<&str>,
    request_shape: Option<&str>,
    body_len: usize,
    body_model: Option<&str>,
) {
    let ts = now_ts();
    let prompt_cache_key_fp = prompt_cache_key
        .map(short_fingerprint)
        .unwrap_or_else(|| "-".to_string());
    let session_source = if strip_session_affinity {
        "failover_regen"
    } else if has_incoming_session {
        "incoming_header"
    } else if prompt_cache_key.is_some() {
        "prompt_cache_key"
    } else {
        "generated"
    };
    let request_shape = request_shape.unwrap_or("-");
    let line = format!(
        "ts={ts} event=ATTEMPT_PROFILE trace_id={} account_id={} candidate={}/{} strip_session_affinity={} session_source={} has_turn_state={} has_conversation={} prompt_cache_key_fp={} request_shape={} body_len={} body_model={}",
        sanitize_text(trace_id),
        sanitize_text(account_id),
        candidate_index + 1,
        total,
        strip_session_affinity,
        session_source,
        has_incoming_turn_state,
        has_incoming_conversation,
        prompt_cache_key_fp,
        sanitize_text(request_shape),
        body_len,
        sanitize_text(body_model.unwrap_or("-")),
    );
    append_trace_line(&line);
}

pub(crate) fn log_request_final(
    trace_id: &str,
    status_code: u16,
    final_account_id: Option<&str>,
    upstream_url: Option<&str>,
    error: Option<&str>,
    elapsed_ms: u128,
) {
    let ts = now_ts();
    let account_id = final_account_id.unwrap_or("-");
    let upstream_url = upstream_url.unwrap_or("-");
    let error = error.unwrap_or("-");
    let line = format!(
        "ts={ts} event=REQUEST_FINAL trace_id={} status={} account_id={} upstream_url={} elapsed_ms={} error={}",
        sanitize_text(trace_id),
        status_code,
        sanitize_text(account_id),
        sanitize_text(upstream_url),
        elapsed_ms,
        sanitize_text(error),
    );
    append_trace_line(&line);
}
