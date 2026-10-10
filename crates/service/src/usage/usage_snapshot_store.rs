use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_status::{
    load_account_status_context, set_account_status_with_context, AccountStatusContext,
};
use codexmanager_core::auth::parse_id_token_claims;
use codexmanager_core::storage::{now_ts, Storage, UsageSnapshotRecord};
use codexmanager_core::usage::{
    merge_missing_extra_rate_limits, parse_usage_snapshot, usage_payload_declares_extra_rate_limits,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT: usize = 1;
const USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV: &str =
    "CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT";
const EXTRA_RATE_LIMITS_PROVENANCE_KEY: &str = "_codexmanager_extra_rate_limits_provenance";
const EXTRA_RATE_LIMITS_RECOVERY_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ExtraRateLimitsProvenance {
    source_captured_at: i64,
    identity_sha256: String,
    #[serde(default)]
    stable_local_identity: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload_identity_sha256: Option<String>,
    recovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UsageIdentity {
    local_sha256: String,
    stable_local_identity: bool,
    payload_sha256: Option<String>,
}

fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn payload_identity(value: &Value) -> (Option<&str>, Option<&str>) {
    let account_id = non_empty_string(
        value
            .get("account_id")
            .or_else(|| value.get("accountId"))
            .or_else(|| value.pointer("/account/id")),
    );
    let user_id = non_empty_string(
        value
            .get("user_id")
            .or_else(|| value.get("userId"))
            .or_else(|| value.pointer("/user/id")),
    );
    (account_id, user_id)
}

fn sha256_json(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

fn usage_identity_sha256(
    storage: &Storage,
    local_account_id: &str,
    value: &Value,
) -> rusqlite::Result<UsageIdentity> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let (payload_account_id, payload_user_id) = payload_identity(value);
    let payload_sha256 = (payload_account_id.is_some() || payload_user_id.is_some()).then(|| {
        sha256_json(&serde_json::json!({
            "scope": "payload",
            "account_id": payload_account_id,
            "user_id": payload_user_id,
        }))
    });
    let local = storage.find_account_with_token_by_id(local_account_id)?;
    let (chatgpt_account_id, workspace_id, token_sub) = local
        .as_ref()
        .map(|(account, token)| {
            let token_sub = parse_id_token_claims(&token.id_token)
                .or_else(|_| parse_id_token_claims(&token.access_token))
                .ok()
                .map(|claims| claims.sub)
                .filter(|value| !value.trim().is_empty());
            (
                account.chatgpt_account_id.as_deref(),
                account.workspace_id.as_deref(),
                token_sub,
            )
        })
        .unwrap_or((None, None, None));
    let stable_local_identity = chatgpt_account_id.is_some_and(|value| !value.trim().is_empty())
        || workspace_id.is_some_and(|value| !value.trim().is_empty())
        || token_sub.is_some();
    let local_sha256 = sha256_json(&serde_json::json!({
        "scope": "local",
        "account_id": local_account_id,
        "chatgpt_account_id": chatgpt_account_id,
        "workspace_id": workspace_id,
        "token_sub": token_sub,
    }));
    Ok(UsageIdentity {
        local_sha256,
        stable_local_identity,
        payload_sha256,
    })
}

fn recovery_identity_matches(
    provenance: &ExtraRateLimitsProvenance,
    current: &UsageIdentity,
) -> bool {
    if provenance.identity_sha256 != current.local_sha256 {
        return false;
    }
    match (
        provenance.payload_identity_sha256.as_deref(),
        current.payload_sha256.as_deref(),
    ) {
        (Some(previous), Some(current)) => previous == current,
        _ if provenance.stable_local_identity && current.stable_local_identity => true,
        _ => false,
    }
}

fn normalized_identifier(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn entry_is_reserve(entry: &Value) -> bool {
    ["source_key", "limit_id", "limit_name", "metered_feature"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .map(normalized_identifier)
        .any(|identifier| matches!(identifier.as_str(), "gptreserve" | "lunareserve"))
}

fn window_reset_at(window: &serde_json::Map<String, Value>) -> Option<i64> {
    let reset_at = window.get("reset_at").or_else(|| window.get("resetAt"))?;
    let mut timestamp = reset_at
        .as_i64()
        .or_else(|| {
            reset_at
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
        })
        .or_else(|| {
            reset_at
                .as_str()
                .and_then(|value| value.trim().parse().ok())
        })?;
    if timestamp > 1_000_000_000_000 {
        timestamp /= 1000;
    }
    Some(timestamp)
}

fn window_is_usable_at(value: Option<&Value>, now: i64) -> bool {
    let Some(window) = value.and_then(Value::as_object) else {
        return false;
    };
    let remaining = window
        .get("remaining_percent")
        .or_else(|| window.get("remainingPercent"))
        .and_then(Value::as_f64);
    let used = window
        .get("used_percent")
        .or_else(|| window.get("usedPercent"))
        .and_then(Value::as_f64);
    let has_capacity = remaining
        .map(|value| value > 0.0)
        .unwrap_or_else(|| used.map(|value| value < 100.0).unwrap_or(false));
    let reset_is_current = if window.contains_key("reset_at") || window.contains_key("resetAt") {
        window_reset_at(window).is_some_and(|at| at > now)
    } else {
        true
    };
    has_capacity && reset_is_current
}

fn has_current_usable_reserve(credits_json: Option<&str>, now: i64) -> bool {
    let Some(raw) = credits_json.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    value
        .get("_codexmanager_extra_rate_limits")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                let Some(object) = entry.as_object() else {
                    return false;
                };
                if !entry_is_reserve(entry)
                    || object
                        .get("allowed")
                        .and_then(Value::as_bool)
                        .is_some_and(|allowed| !allowed)
                    || object
                        .get("limit_reached")
                        .and_then(Value::as_bool)
                        .is_some_and(|reached| reached)
                {
                    return false;
                }
                window_is_usable_at(object.get("primary_window"), now)
                    || window_is_usable_at(object.get("secondary_window"), now)
            })
        })
}

fn read_provenance(credits_json: &str) -> Option<ExtraRateLimitsProvenance> {
    serde_json::from_str::<Value>(credits_json)
        .ok()?
        .get(EXTRA_RATE_LIMITS_PROVENANCE_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn with_provenance(
    credits_json: Option<String>,
    provenance: &ExtraRateLimitsProvenance,
) -> Option<String> {
    let raw = credits_json?;
    let Ok(mut value) = serde_json::from_str::<Value>(raw.trim()) else {
        return Some(raw);
    };
    let Some(object) = value.as_object_mut() else {
        return Some(raw);
    };
    let Ok(serialized_provenance) = serde_json::to_value(provenance) else {
        return Some(raw);
    };
    object.insert(
        EXTRA_RATE_LIMITS_PROVENANCE_KEY.to_string(),
        serialized_provenance,
    );
    Some(value.to_string())
}

fn usage_status_updates_blocked(context: &AccountStatusContext) -> bool {
    let normalized = context.status.trim();
    normalized.eq_ignore_ascii_case("disabled") || normalized.eq_ignore_ascii_case("force_enabled")
}

/// 函数 `usage_snapshots_retain_per_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_snapshots_retain_per_account() -> usize {
    std::env::var(USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT)
}

/// 函数 `apply_status_from_snapshot`
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
pub(crate) fn apply_status_from_snapshot(
    storage: &Storage,
    record: &UsageSnapshotRecord,
) -> Availability {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    apply_status_from_snapshot_with_change(storage, record).0
}

fn apply_status_from_snapshot_with_change(
    storage: &Storage,
    record: &UsageSnapshotRecord,
) -> (Availability, bool) {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let availability = evaluate_snapshot(record);
    let context = load_account_status_context(storage, &record.account_id);

    if usage_status_updates_blocked(&context) {
        return (availability, false);
    }

    let changed = match availability {
        Availability::Available => set_account_status_with_context(
            storage,
            &record.account_id,
            "active",
            "usage_ok",
            Some(&context),
        ),
        Availability::Unavailable("usage_exhausted_primary" | "usage_exhausted_secondary") => {
            set_account_status_with_context(
                storage,
                &record.account_id,
                "limited",
                "usage_limit_exhausted",
                Some(&context),
            )
        }
        Availability::Unavailable(_) => false,
    };
    (availability, changed)
}

/// 函数 `store_usage_snapshot`
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
pub(crate) fn store_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
) -> Result<UsageSnapshotRecord, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    store_usage_snapshot_at(
        storage,
        account_id,
        value,
        now_ts(),
        usage_snapshots_retain_per_account(),
    )
}

fn store_usage_snapshot_at(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
    captured_at: i64,
    retain: usize,
) -> Result<UsageSnapshotRecord, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    store_usage_snapshot_at_with_previous_observer(
        storage,
        account_id,
        value,
        captured_at,
        retain,
        || {},
    )
}

fn store_usage_snapshot_at_with_previous_observer<F>(
    storage: &Storage,
    account_id: &str,
    value: serde_json::Value,
    captured_at: i64,
    retain: usize,
    previous_observed: F,
) -> Result<UsageSnapshotRecord, String>
where
    F: FnOnce(),
{
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    // 解析并写入用量快照
    let parsed = parse_usage_snapshot(&value);
    let declares_extra_rate_limits = usage_payload_declares_extra_rate_limits(&value);
    let record = UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: parsed.used_percent,
        window_minutes: parsed.window_minutes,
        resets_at: parsed.resets_at,
        secondary_used_percent: parsed.secondary_used_percent,
        secondary_window_minutes: parsed.secondary_window_minutes,
        secondary_resets_at: parsed.secondary_resets_at,
        credits_json: parsed.credits_json,
        captured_at,
    };
    let status_changed = std::cell::Cell::new(false);
    let status_changed_in_transaction = &status_changed;
    let mut status_record = record.clone();
    let (record, _) = storage
        .insert_usage_snapshot_and_prune_with_previous(
            &record,
            retain,
            move |previous, credits_json| {
                let identity = usage_identity_sha256(storage, account_id, &value)?;
                previous_observed();
                let current_credits_json = credits_json.take();
                *credits_json = if declares_extra_rate_limits {
                    if has_current_usable_reserve(current_credits_json.as_deref(), captured_at) {
                        with_provenance(
                            current_credits_json,
                            &ExtraRateLimitsProvenance {
                                source_captured_at: captured_at,
                                identity_sha256: identity.local_sha256,
                                stable_local_identity: identity.stable_local_identity,
                                payload_identity_sha256: identity.payload_sha256,
                                recovered: false,
                            },
                        )
                    } else {
                        current_credits_json
                    }
                } else {
                    let recovery = previous.and_then(|snapshot| {
                        let credits_json = snapshot.credits_json.as_deref()?;
                        let provenance = read_provenance(credits_json)?;
                        let source_age = captured_at.checked_sub(provenance.source_captured_at)?;
                        (!provenance.recovered
                            && recovery_identity_matches(&provenance, &identity)
                            && (0..=EXTRA_RATE_LIMITS_RECOVERY_TTL_SECONDS).contains(&source_age)
                            && has_current_usable_reserve(Some(credits_json), captured_at))
                        .then_some((credits_json, provenance))
                    });
                    if let Some((recovery_credits_json, mut provenance)) = recovery {
                        let merged = merge_missing_extra_rate_limits(
                            current_credits_json.as_deref(),
                            Some(recovery_credits_json),
                        )
                        .or(current_credits_json);
                        provenance.recovered = true;
                        with_provenance(merged, &provenance)
                    } else {
                        current_credits_json
                    }
                };
                status_record.credits_json = credits_json.clone();
                if let Some(previous) = previous {
                    status_record.captured_at = status_record.captured_at.max(previous.captured_at);
                }
                status_changed_in_transaction
                    .set(apply_status_from_snapshot_with_change(storage, &status_record).1);
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
    if status_changed.get() {
        // The first invalidation happens before the SQLite transaction commits.
        // Invalidate once more afterwards so no reader can retain candidates
        // rebuilt from the pre-commit account state.
        crate::gateway::invalidate_candidate_cache();
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use codexmanager_core::storage::{Account, Storage, Token};
    use codexmanager_core::usage::has_usable_luna_reserve;
    use std::sync::{mpsc, Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    const ACCOUNT_ID: &str = "usage-account";
    const USER_ID: &str = "usage-user";

    struct TempUsageDb {
        path: std::path::PathBuf,
    }

    impl TempUsageDb {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "codexmanager-usage-snapshot-{label}-{}-{}.sqlite",
                std::process::id(),
                rand::random::<u64>()
            ));
            let storage = Storage::open(&path).expect("open temporary storage");
            storage.init().expect("init temporary storage");
            drop(storage);
            Self { path }
        }
    }

    impl Drop for TempUsageDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
            let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
        }
    }

    fn reserve_payload(now: i64) -> Value {
        serde_json::json!({
            "account_id": ACCOUNT_ID,
            "user_id": USER_ID,
            "rate_limit": {
                "primary_window": {
                    "used_percent": 100.0,
                    "limit_window_seconds": 18000
                }
            },
            "additionalRateLimits": [{
                "limitName": "Luna Reserve",
                "meteredFeature": "base_model_inference",
                "rateLimit": {
                    "primaryWindow": {
                        "remainingPercent": 75.0,
                        "resetAt": now + 3600,
                        "limitWindowSeconds": 604800
                    }
                }
            }]
        })
    }

    fn without_payload_identity(mut value: Value) -> Value {
        let object = value.as_object_mut().expect("usage payload object");
        object.remove("account_id");
        object.remove("user_id");
        value
    }

    fn jwt_with_sub(sub: &str) -> String {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({ "sub": sub }).to_string());
        format!("header.{payload}.signature")
    }

    fn upsert_local_identity(
        storage: &Storage,
        chatgpt_account_id: &str,
        workspace_id: &str,
        token_sub: &str,
    ) {
        storage
            .insert_account(&Account {
                id: "local-account".to_string(),
                label: "local-account".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(chatgpt_account_id.to_string()),
                workspace_id: Some(workspace_id.to_string()),
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: 1,
                updated_at: 1,
            })
            .expect("upsert local account");
        storage
            .insert_token(&Token {
                account_id: "local-account".to_string(),
                id_token: jwt_with_sub(token_sub),
                access_token: String::new(),
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: 1,
            })
            .expect("upsert local token");
    }

    fn upsert_opaque_local_token(storage: &Storage, access_token: &str) {
        storage
            .insert_account(&Account {
                id: "local-account".to_string(),
                label: "local-account".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: 1,
                updated_at: 1,
            })
            .expect("upsert opaque local account");
        storage
            .insert_token(&Token {
                account_id: "local-account".to_string(),
                id_token: String::new(),
                access_token: access_token.to_string(),
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: 1,
            })
            .expect("upsert opaque local token");
    }

    fn ambiguous_payload(identity: (&str, &str), declaration: Option<Value>) -> Value {
        let mut payload = serde_json::json!({
            "account_id": identity.0,
            "user_id": identity.1,
            "rate_limit": {
                "primary_window": {
                    "used_percent": 100.0,
                    "limit_window_seconds": 18000
                }
            },
            "credits": { "balance": 2.0 }
        });
        if let Some(declaration) = declaration {
            payload
                .as_object_mut()
                .expect("payload object")
                .insert("additional_rate_limits".to_string(), declaration);
        }
        payload
    }

    fn open_storage() -> Storage {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
    }

    fn store_at(
        storage: &Storage,
        value: Value,
        captured_at: i64,
        retain: usize,
    ) -> UsageSnapshotRecord {
        store_usage_snapshot_at(storage, "local-account", value, captured_at, retain)
            .expect("store usage")
    }

    #[test]
    fn ambiguous_null_recovers_only_once_from_immediately_previous_authoritative_snapshot() {
        let storage = open_storage();
        let now = 2_000_000_000;
        let authoritative = store_at(&storage, reserve_payload(now), now, 1);
        let provenance = read_provenance(authoritative.credits_json.as_deref().expect("credits"))
            .expect("authoritative provenance");
        assert_eq!(provenance.source_captured_at, now);
        assert!(!provenance.recovered);

        let recovered = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + 1,
            1,
        );
        assert!(has_usable_luna_reserve(recovered.credits_json.as_deref()));
        let provenance = read_provenance(recovered.credits_json.as_deref().expect("credits"))
            .expect("recovery provenance");
        assert_eq!(provenance.source_captured_at, now);
        assert!(provenance.recovered);

        let cleared = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + 2,
            1,
        );
        assert!(!has_usable_luna_reserve(cleared.credits_json.as_deref()));
        assert_eq!(
            storage
                .usage_snapshot_count_for_account("local-account")
                .expect("count snapshots"),
            1
        );
    }

    #[test]
    fn reserve_identity_requires_an_explicit_reserve_name() {
        assert!(entry_is_reserve(&serde_json::json!({
            "limit_name": "Luna Reserve",
            "metered_feature": "base_model_inference"
        })));
        assert!(entry_is_reserve(&serde_json::json!({
            "source_key": "gpt-reserve"
        })));
        assert!(!entry_is_reserve(&serde_json::json!({
            "metered_feature": "base_model_inference"
        })));
    }

    #[test]
    fn missing_field_recovers_once_then_clears() {
        let storage = open_storage();
        let now = 2_000_000_000;
        store_at(&storage, reserve_payload(now), now, 1);

        let recovered = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), None),
            now + 1,
            1,
        );
        assert!(has_usable_luna_reserve(recovered.credits_json.as_deref()));
        let cleared = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), None),
            now + 2,
            1,
        );
        assert!(!has_usable_luna_reserve(cleared.credits_json.as_deref()));
    }

    #[test]
    fn explicit_empty_array_or_object_blocks_historical_recovery_after_real_prune() {
        for explicit_empty in [serde_json::json!([]), serde_json::json!({})] {
            let storage = open_storage();
            let now = 2_000_000_000;
            store_at(&storage, reserve_payload(now), now, 1);
            let cleared = store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(explicit_empty)),
                now + 1,
                1,
            );
            assert!(!has_usable_luna_reserve(cleared.credits_json.as_deref()));
            assert_eq!(
                storage
                    .usage_snapshot_count_for_account("local-account")
                    .expect("count after prune"),
                1
            );
            let still_cleared = store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
                now + 2,
                1,
            );
            assert!(!has_usable_luna_reserve(
                still_cleared.credits_json.as_deref()
            ));
            assert_eq!(
                storage
                    .usage_snapshot_count_for_account("local-account")
                    .expect("count after second prune"),
                1
            );
        }
    }

    #[test]
    fn recovery_rejects_expired_ttl_and_expired_reserve_window() {
        let now = 2_000_000_000;
        let storage = open_storage();
        store_at(&storage, reserve_payload(now), now, 1);
        let expired_ttl = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + EXTRA_RATE_LIMITS_RECOVERY_TTL_SECONDS + 1,
            1,
        );
        assert!(!has_usable_luna_reserve(
            expired_ttl.credits_json.as_deref()
        ));

        let storage = open_storage();
        let mut expiring = reserve_payload(now);
        expiring
            .pointer_mut("/additionalRateLimits/0/rateLimit/primaryWindow/resetAt")
            .map(|value| *value = serde_json::json!(now + 1))
            .expect("reset_at exists");
        store_at(&storage, expiring, now, 1);
        let expired_window = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + 2,
            1,
        );
        assert!(!has_usable_luna_reserve(
            expired_window.credits_json.as_deref()
        ));

        let storage = open_storage();
        store_at(&storage, reserve_payload(now + 10), now + 10, 0);
        let future_source = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now,
            0,
        );
        assert!(!has_usable_luna_reserve(
            future_source.credits_json.as_deref()
        ));
        let latest = storage
            .latest_usage_snapshot_for_account("local-account")
            .expect("read latest after future source")
            .expect("latest snapshot");
        assert_eq!(latest.captured_at, now + 10);
        assert!(!has_usable_luna_reserve(latest.credits_json.as_deref()));
    }

    #[test]
    fn recovery_rejects_an_unparseable_explicit_reset_timestamp() {
        let storage = open_storage();
        let now = 2_000_000_000;
        let mut payload = reserve_payload(now);
        payload
            .pointer_mut("/additionalRateLimits/0/rateLimit/primaryWindow/resetAt")
            .map(|value| *value = serde_json::json!("not-a-timestamp"))
            .expect("reset_at exists");
        store_at(&storage, payload, now, 1);

        let current = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + 1,
            1,
        );
        assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
    }

    #[test]
    fn recovery_rejects_identity_change() {
        let now = 2_000_000_000;
        for identity in [
            ("different-account", USER_ID),
            (ACCOUNT_ID, "different-user"),
        ] {
            let storage = open_storage();
            store_at(&storage, reserve_payload(now), now, 1);
            let current = store_at(
                &storage,
                ambiguous_payload(identity, Some(Value::Null)),
                now + 1,
                1,
            );
            assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
        }
    }

    #[test]
    fn explicit_withdrawal_or_exhaustion_is_authoritative() {
        let now = 2_000_000_000;
        for override_fields in [
            serde_json::json!({"allowed": false}),
            serde_json::json!({"limit_reached": true}),
            serde_json::json!({"remaining_percent": 0.0}),
        ] {
            let storage = open_storage();
            store_at(&storage, reserve_payload(now), now, 0);
            let mut explicit = reserve_payload(now + 1);
            let entry = explicit
                .pointer_mut("/additionalRateLimits/0")
                .and_then(Value::as_object_mut)
                .expect("reserve entry");
            if let Some(allowed) = override_fields.get("allowed") {
                entry.insert("allowed".to_string(), allowed.clone());
            }
            if let Some(limit_reached) = override_fields.get("limit_reached") {
                entry.insert("limit_reached".to_string(), limit_reached.clone());
            }
            if let Some(remaining) = override_fields.get("remaining_percent") {
                explicit
                    .pointer_mut("/additionalRateLimits/0/rateLimit/primaryWindow/remainingPercent")
                    .map(|value| *value = remaining.clone())
                    .expect("remaining percent");
            }
            let authoritative = store_at(&storage, explicit, now + 1, 0);
            assert!(!has_current_usable_reserve(
                authoritative.credits_json.as_deref(),
                now + 1
            ));
            let ambiguous = store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
                now + 2,
                0,
            );
            assert!(!has_usable_luna_reserve(ambiguous.credits_json.as_deref()));
        }
    }

    #[test]
    fn retained_history_is_never_used_when_immediately_previous_snapshot_is_empty() {
        let now = 2_000_000_000;
        for retain in [2, 0] {
            let storage = open_storage();
            store_at(&storage, reserve_payload(now), now, retain);
            store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(serde_json::json!([]))),
                now + 1,
                retain,
            );
            let current = store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
                now + 2,
                retain,
            );
            assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
            assert_eq!(
                storage
                    .usage_snapshot_count_for_account("local-account")
                    .expect("count retained snapshots"),
                if retain == 0 { 3 } else { retain as i64 }
            );
        }
    }

    #[test]
    fn legacy_snapshot_without_provenance_is_not_a_recovery_source() {
        let storage = open_storage();
        let now = 2_000_000_000;
        let parsed = parse_usage_snapshot(&reserve_payload(now));
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "local-account".to_string(),
                used_percent: parsed.used_percent,
                window_minutes: parsed.window_minutes,
                resets_at: parsed.resets_at,
                secondary_used_percent: parsed.secondary_used_percent,
                secondary_window_minutes: parsed.secondary_window_minutes,
                secondary_resets_at: parsed.secondary_resets_at,
                credits_json: parsed.credits_json,
                captured_at: now,
            })
            .expect("insert legacy snapshot");

        let current = store_at(
            &storage,
            ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
            now + 1,
            0,
        );
        assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
    }

    #[test]
    fn payload_identity_can_disappear_when_stable_local_identity_still_matches() {
        let storage = open_storage();
        let now = 2_000_000_000;
        upsert_local_identity(&storage, "chatgpt-account", "workspace", "subject-a");
        let authoritative = store_at(&storage, reserve_payload(now), now, 1);
        assert!(
            read_provenance(authoritative.credits_json.as_deref().expect("credits"))
                .is_some_and(|provenance| provenance.payload_identity_sha256.is_some())
        );

        let recovered = store_at(
            &storage,
            without_payload_identity(ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null))),
            now + 1,
            1,
        );
        assert!(has_usable_luna_reserve(recovered.credits_json.as_deref()));
    }

    #[test]
    fn local_account_and_token_subject_bound_fallback_recovery() {
        let now = 2_000_000_000;

        let storage = open_storage();
        upsert_local_identity(&storage, "chatgpt-account", "workspace", "subject-a");
        store_at(
            &storage,
            without_payload_identity(reserve_payload(now)),
            now,
            1,
        );
        let recovered = store_at(
            &storage,
            without_payload_identity(ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null))),
            now + 1,
            1,
        );
        assert!(has_usable_luna_reserve(recovered.credits_json.as_deref()));

        for (chatgpt_account_id, workspace_id, token_sub) in [
            ("different-account", "workspace", "subject-a"),
            ("chatgpt-account", "different-workspace", "subject-a"),
            ("chatgpt-account", "workspace", "subject-b"),
        ] {
            let storage = open_storage();
            upsert_local_identity(&storage, "chatgpt-account", "workspace", "subject-a");
            store_at(
                &storage,
                without_payload_identity(reserve_payload(now)),
                now,
                1,
            );
            upsert_local_identity(&storage, chatgpt_account_id, workspace_id, token_sub);
            let current = store_at(
                &storage,
                without_payload_identity(ambiguous_payload(
                    (ACCOUNT_ID, USER_ID),
                    Some(Value::Null),
                )),
                now + 1,
                1,
            );
            assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
        }
    }

    #[test]
    fn opaque_token_without_stable_identity_cannot_restore_an_unbound_snapshot() {
        let storage = open_storage();
        let now = 2_000_000_000;
        upsert_opaque_local_token(&storage, "opaque-token-a");
        store_at(
            &storage,
            without_payload_identity(reserve_payload(now)),
            now,
            1,
        );

        upsert_opaque_local_token(&storage, "opaque-token-b");
        let current = store_at(
            &storage,
            without_payload_identity(ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null))),
            now + 1,
            1,
        );
        assert!(!has_usable_luna_reserve(current.credits_json.as_deref()));
    }

    #[test]
    fn concurrent_ambiguous_snapshots_recover_one_source_at_most_once() {
        let db = TempUsageDb::new("concurrent-null");
        let now = 2_000_000_000;
        let storage = Storage::open(&db.path).expect("open seed storage");
        upsert_local_identity(&storage, "chatgpt-account", "workspace", "subject-a");
        store_at(&storage, reserve_payload(now), now, 1);
        drop(storage);

        let start = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let path = db.path.clone();
            let start = Arc::clone(&start);
            handles.push(thread::spawn(move || {
                let storage = Storage::open(path).expect("open concurrent storage");
                start.wait();
                store_at(
                    &storage,
                    ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
                    now + 1,
                    1,
                )
            }));
        }
        start.wait();
        let records = handles
            .into_iter()
            .map(|handle| handle.join().expect("join concurrent null writer"))
            .collect::<Vec<_>>();
        assert_eq!(
            records
                .iter()
                .filter(|record| has_usable_luna_reserve(record.credits_json.as_deref()))
                .count(),
            1
        );

        let storage = Storage::open(&db.path).expect("open verification storage");
        let latest = storage
            .latest_usage_snapshot_for_account("local-account")
            .expect("read latest concurrent snapshot")
            .expect("latest concurrent snapshot");
        assert!(!has_usable_luna_reserve(latest.credits_json.as_deref()));
        assert_eq!(
            storage
                .usage_snapshot_count_for_account("local-account")
                .expect("count concurrent snapshots"),
            1
        );
        assert_eq!(
            storage
                .find_account_status_by_id("local-account")
                .expect("read final concurrent account status")
                .as_deref(),
            Some("limited")
        );
    }

    #[test]
    fn explicit_empty_cannot_be_overtaken_by_a_paused_ambiguous_write() {
        let db = TempUsageDb::new("explicit-empty-interleaving");
        let now = 2_000_000_000;
        let storage = Storage::open(&db.path).expect("open seed storage");
        upsert_local_identity(&storage, "chatgpt-account", "workspace", "subject-a");
        store_at(&storage, reserve_payload(now), now, 1);
        drop(storage);

        let (observed_tx, observed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let null_path = db.path.clone();
        let null_writer = thread::spawn(move || {
            let storage = Storage::open(null_path).expect("open null writer storage");
            store_usage_snapshot_at_with_previous_observer(
                &storage,
                "local-account",
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(Value::Null)),
                now + 1,
                1,
                move || {
                    observed_tx.send(()).expect("signal previous read");
                    release_rx.recv().expect("release paused null writer");
                },
            )
            .expect("store paused null snapshot")
        });
        observed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("null writer observed authoritative snapshot");

        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let explicit_path = db.path.clone();
        let explicit_writer = thread::spawn(move || {
            let storage = Storage::open(explicit_path).expect("open explicit writer storage");
            started_tx.send(()).expect("signal explicit writer start");
            let record = store_at(
                &storage,
                ambiguous_payload((ACCOUNT_ID, USER_ID), Some(serde_json::json!([]))),
                now + 2,
                1,
            );
            done_tx.send(()).expect("signal explicit writer completion");
            record
        });
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("explicit writer started");
        let completed_while_null_was_paused =
            done_rx.recv_timeout(Duration::from_millis(500)).is_ok();
        release_tx.send(()).expect("release null writer");
        let null_record = null_writer.join().expect("join null writer");
        let explicit_record = explicit_writer.join().expect("join explicit writer");

        assert!(!completed_while_null_was_paused);
        assert!(has_usable_luna_reserve(null_record.credits_json.as_deref()));
        assert!(!has_usable_luna_reserve(
            explicit_record.credits_json.as_deref()
        ));
        let storage = Storage::open(&db.path).expect("open verification storage");
        let latest = storage
            .latest_usage_snapshot_for_account("local-account")
            .expect("read latest interleaved snapshot")
            .expect("latest interleaved snapshot");
        assert!(!has_usable_luna_reserve(latest.credits_json.as_deref()));
        assert_eq!(
            storage
                .find_account_status_by_id("local-account")
                .expect("read final interleaved account status")
                .as_deref(),
            Some("limited")
        );
    }

    #[test]
    fn provenance_serialization_failure_preserves_original_credits() {
        let raw = "not-json".to_string();
        let provenance = ExtraRateLimitsProvenance {
            source_captured_at: 1,
            identity_sha256: "identity".to_string(),
            stable_local_identity: true,
            payload_identity_sha256: None,
            recovered: false,
        };
        assert_eq!(with_provenance(Some(raw.clone()), &provenance), Some(raw));
    }
}
