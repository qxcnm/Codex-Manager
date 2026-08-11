use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(600),
    Duration::from_secs(1_800),
    Duration::from_secs(3_600),
];
const RETRY_QUEUE_CAPACITY: usize = 4096;
const FAST_REPROBE_MAX_PENDING: usize = 256;
const FAST_REPROBE_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_secs(1),
    Duration::from_secs(3),
];

#[derive(Clone)]
struct RetryEntry {
    attempt: usize,
    due_at: Instant,
}

static RETRY_SENDER: OnceLock<Sender<String>> = OnceLock::new();
static FAST_REPROBES: OnceLock<Mutex<FastReprobeRegistry>> = OnceLock::new();

#[derive(Default)]
struct FastReprobeRegistry {
    next_generation: u64,
    pending: HashMap<String, u64>,
}

fn fast_reprobe_registry() -> &'static Mutex<FastReprobeRegistry> {
    FAST_REPROBES.get_or_init(|| Mutex::new(FastReprobeRegistry::default()))
}

fn register_fast_reprobe(account_id: &str) -> Option<u64> {
    let mut registry =
        crate::lock_utils::lock_recover(fast_reprobe_registry(), "codex_fast_reprobes");
    if registry.pending.contains_key(account_id) {
        return None;
    }
    if registry.pending.len() >= FAST_REPROBE_MAX_PENDING {
        log::warn!(
            "event=codex_fast_reprobe_capacity_reached account_id={} pending={}",
            account_id,
            registry.pending.len()
        );
        return None;
    }
    registry.next_generation = registry.next_generation.wrapping_add(1).max(1);
    let generation = registry.next_generation;
    registry.pending.insert(account_id.to_string(), generation);
    Some(generation)
}

fn is_fast_reprobe_active(account_id: &str, generation: u64) -> bool {
    let registry = crate::lock_utils::lock_recover(fast_reprobe_registry(), "codex_fast_reprobes");
    registry.pending.get(account_id).copied() == Some(generation)
}

fn remove_fast_reprobe_if_current(account_id: &str, generation: u64) {
    let mut registry =
        crate::lock_utils::lock_recover(fast_reprobe_registry(), "codex_fast_reprobes");
    if registry.pending.get(account_id).copied() == Some(generation) {
        registry.pending.remove(account_id);
    }
}

/// Run a state write while cancelling any older fast reprobe. Keeping the
/// registry lock through the write prevents an already-running stale probe
/// from overwriting a newer successful user request.
pub(super) fn record_after_cancelling_fast_reprobe<F>(account_id: &str, record: F)
where
    F: FnOnce(),
{
    let mut registry =
        crate::lock_utils::lock_recover(fast_reprobe_registry(), "codex_fast_reprobes");
    registry.pending.remove(account_id);
    record();
}

fn record_if_fast_reprobe_is_current<F>(account_id: &str, generation: u64, record: F) -> bool
where
    F: FnOnce(),
{
    let mut registry =
        crate::lock_utils::lock_recover(fast_reprobe_registry(), "codex_fast_reprobes");
    if registry.pending.get(account_id).copied() != Some(generation) {
        return false;
    }
    record();
    registry.pending.remove(account_id);
    true
}

pub(crate) fn schedule_codex_fast_reprobe(account_id: &str) -> bool {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return false;
    }
    let Some(generation) = register_fast_reprobe(account_id) else {
        return false;
    };

    #[cfg(test)]
    let _ = generation;

    #[cfg(not(test))]
    {
        let worker_account_id = account_id.to_string();
        let spawn_result = std::thread::Builder::new()
            .name("codex-fast-reprobe".to_string())
            .spawn(move || run_fast_reprobe(worker_account_id, generation));
        if let Err(error) = spawn_result {
            remove_fast_reprobe_if_current(account_id, generation);
            log::warn!(
                "event=codex_fast_reprobe_spawn_failed account_id={} error={}",
                account_id,
                error
            );
            return false;
        }
    }

    true
}

#[cfg(not(test))]
fn run_fast_reprobe(account_id: String, generation: u64) {
    let mut last_error = None;
    let mut attempts = 0usize;
    let mut all_attempts_not_found = true;
    let mut terminal_definitive_failure = false;
    for (index, delay) in FAST_REPROBE_DELAYS.iter().enumerate() {
        std::thread::sleep(*delay);
        if !is_fast_reprobe_active(account_id.as_str(), generation) {
            return;
        }

        crate::account_warmup::reload_warmup_client();
        let result =
            crate::account_warmup::probe_account_responses_for_batch(account_id.as_str(), None);
        if !is_fast_reprobe_active(account_id.as_str(), generation) {
            return;
        }

        match result {
            Ok(()) => {
                let Some(storage) = crate::storage_helpers::open_storage() else {
                    remove_fast_reprobe_if_current(account_id.as_str(), generation);
                    return;
                };
                if record_if_fast_reprobe_is_current(account_id.as_str(), generation, || {
                    super::record_codex_probe_available(
                        &storage,
                        account_id.as_str(),
                        super::CODEX_RESPONSES_VERIFIED,
                    );
                }) {
                    let _ = super::refresh_usage_for_account(account_id.as_str());
                    log::info!(
                        "event=codex_fast_reprobe_recovered account_id={} attempt={}",
                        account_id,
                        index + 1
                    );
                }
                return;
            }
            Err(error) => {
                attempts += 1;
                all_attempts_not_found &= is_fast_reprobe_not_found(error.as_str());
                let retry = fast_reprobe_should_retry(error.as_str())
                    && index + 1 < FAST_REPROBE_DELAYS.len();
                terminal_definitive_failure = !fast_reprobe_should_retry(error.as_str());
                log::warn!(
                    "event=codex_fast_reprobe_attempt_failed account_id={} attempt={} retry={} error={}",
                    account_id,
                    index + 1,
                    retry,
                    error
                );
                last_error = Some(error);
                if !retry {
                    break;
                }
            }
        }
    }

    let Some(error) = last_error else {
        remove_fast_reprobe_if_current(account_id.as_str(), generation);
        return;
    };
    let confirmed_not_found = fast_reprobe_confirms_not_found(attempts, all_attempts_not_found);
    if terminal_definitive_failure || confirmed_not_found {
        let Some(storage) = crate::storage_helpers::open_storage() else {
            remove_fast_reprobe_if_current(account_id.as_str(), generation);
            return;
        };
        let recorded = record_if_fast_reprobe_is_current(account_id.as_str(), generation, || {
            super::record_codex_admission_probe_failure(
                &storage,
                account_id.as_str(),
                error.as_str(),
            );
        });
        if recorded {
            log::warn!(
                "event=codex_fast_reprobe_confirmed_failure account_id={} attempts={} error={}",
                account_id,
                attempts,
                error
            );
        }
    } else {
        // Network/challenge noise is inconclusive even after the quick series.
        // Preserve the last verified healthy state and let the slower network
        // retry loop keep checking without removing the account from service.
        remove_fast_reprobe_if_current(account_id.as_str(), generation);
        schedule_codex_network_reprobe(account_id.as_str());
        log::warn!(
            "event=codex_fast_reprobe_inconclusive account_id={} attempts={} error={}",
            account_id,
            attempts,
            error
        );
    }
}

fn is_fast_reprobe_not_found(error: &str) -> bool {
    let normalized = error.trim().to_ascii_lowercase();
    normalized.contains("status=404")
        || normalized.contains("status: 404")
        || normalized.contains("404 not found")
}

fn fast_reprobe_confirms_not_found(attempts: usize, all_attempts_not_found: bool) -> bool {
    attempts == FAST_REPROBE_DELAYS.len() && all_attempts_not_found
}

fn fast_reprobe_should_retry(error: &str) -> bool {
    is_fast_reprobe_not_found(error) || is_transient_reprobe_error(error)
}

pub(super) fn is_transient_reprobe_error(error: &str) -> bool {
    let normalized = error.trim().to_ascii_lowercase();
    if normalized.contains("cloudflare")
        || normalized.contains("challenge")
        || normalized.contains("timeout")
        || normalized.contains("timed out")
        || normalized.contains("connection")
        || normalized.contains("connect error")
        || normalized.contains("dns")
        || normalized.contains("error sending request")
        || normalized.contains("status=502")
        || normalized.contains("status 502")
        || normalized.contains("status=503")
        || normalized.contains("status 503")
        || normalized.contains("status=504")
        || normalized.contains("status 504")
    {
        return true;
    }
    false
}

pub(super) fn schedule_codex_network_reprobe(account_id: &str) {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return;
    }
    let sender = RETRY_SENDER.get_or_init(|| {
        let (sender, receiver) = bounded::<String>(RETRY_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("codex-network-reprobe".to_string())
            .spawn(move || run_retry_loop(receiver))
            .expect("spawn Codex network reprobe worker");
        sender
    });
    if let Err(error) = sender.try_send(account_id.to_string()) {
        log::warn!(
            "event=codex_network_reprobe_queue_rejected account_id={} error={}",
            account_id,
            error
        );
    }
}

fn run_retry_loop(receiver: Receiver<String>) {
    let mut pending = HashMap::<String, RetryEntry>::new();
    loop {
        let wait = pending
            .values()
            .map(|entry| entry.due_at.saturating_duration_since(Instant::now()))
            .min();
        let received = match wait {
            Some(wait) => match receiver.recv_timeout(wait) {
                Ok(account_id) => Some(account_id),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match receiver.recv() {
                Ok(account_id) => Some(account_id),
                Err(_) => break,
            },
        };
        if let Some(account_id) = received {
            pending.entry(account_id).or_insert_with(|| RetryEntry {
                attempt: 0,
                due_at: Instant::now() + RETRY_DELAYS[0],
            });
        }

        while let Some(account_id) = next_due_account(&pending) {
            let Some(entry) = pending.remove(account_id.as_str()) else {
                continue;
            };
            crate::account_warmup::reload_warmup_client();
            crate::usage_http::reload_usage_http_client_from_env();
            match super::probe_codex_account_batch_admission(account_id.as_str(), None) {
                Ok(()) => {
                    let _ = super::refresh_usage_for_account(account_id.as_str());
                    log::info!(
                        "event=codex_network_reprobe_recovered account_id={} attempt={}",
                        account_id,
                        entry.attempt + 1
                    );
                }
                Err(error)
                    if is_transient_reprobe_error(error.as_str())
                        && entry.attempt + 1 < RETRY_DELAYS.len() =>
                {
                    let next_attempt = entry.attempt + 1;
                    pending.insert(
                        account_id.clone(),
                        RetryEntry {
                            attempt: next_attempt,
                            due_at: Instant::now() + RETRY_DELAYS[next_attempt],
                        },
                    );
                    log::warn!(
                        "event=codex_network_reprobe_rescheduled account_id={} attempt={} error={}",
                        account_id,
                        entry.attempt + 1,
                        error
                    );
                }
                Err(error) => log::warn!(
                    "event=codex_network_reprobe_stopped account_id={} attempt={} error={}",
                    account_id,
                    entry.attempt + 1,
                    error
                ),
            }
        }
    }
}

fn next_due_account(pending: &HashMap<String, RetryEntry>) -> Option<String> {
    let now = Instant::now();
    pending
        .iter()
        .filter(|(_, entry)| entry.due_at <= now)
        .min_by_key(|(_, entry)| entry.due_at)
        .map(|(account_id, _)| account_id.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        fast_reprobe_confirms_not_found, fast_reprobe_should_retry, is_fast_reprobe_not_found,
        is_transient_reprobe_error, record_after_cancelling_fast_reprobe, register_fast_reprobe,
        remove_fast_reprobe_if_current,
    };

    #[test]
    fn retries_network_and_cloudflare_failures_only() {
        assert!(is_transient_reprobe_error(
            "status=403 body=Cloudflare 安全验证页"
        ));
        assert!(is_transient_reprobe_error("request timed out"));
        assert!(is_transient_reprobe_error("status=502 origin bad gateway"));
        assert!(!is_transient_reprobe_error(
            "status=401 code=token_invalidated"
        ));
        assert!(!is_transient_reprobe_error("quota_exhausted"));
    }

    #[test]
    fn fast_reprobe_retries_not_found_and_network_but_not_auth() {
        assert!(fast_reprobe_should_retry("status=404 body=Not Found"));
        assert!(fast_reprobe_should_retry("request timed out"));
        assert!(fast_reprobe_should_retry("status=503 upstream unavailable"));
        assert!(!fast_reprobe_should_retry("status=401 token_invalidated"));
        assert!(!fast_reprobe_should_retry("quota_exhausted"));
        assert!(is_fast_reprobe_not_found("status=404 body=Not Found"));
        assert!(!is_fast_reprobe_not_found(
            "status=503 upstream unavailable"
        ));
        assert!(fast_reprobe_confirms_not_found(3, true));
        assert!(!fast_reprobe_confirms_not_found(2, true));
        assert!(!fast_reprobe_confirms_not_found(3, false));
    }

    #[test]
    fn fast_reprobe_deduplicates_and_success_cancels_stale_generation() {
        let account_id = "test-fast-reprobe-dedup";
        record_after_cancelling_fast_reprobe(account_id, || {});
        let generation = register_fast_reprobe(account_id).expect("first registration");
        assert!(register_fast_reprobe(account_id).is_none());

        record_after_cancelling_fast_reprobe(account_id, || {});
        let next_generation = register_fast_reprobe(account_id).expect("registration after cancel");
        assert_ne!(generation, next_generation);
        remove_fast_reprobe_if_current(account_id, generation);
        assert!(register_fast_reprobe(account_id).is_none());
        remove_fast_reprobe_if_current(account_id, next_generation);
    }
}
