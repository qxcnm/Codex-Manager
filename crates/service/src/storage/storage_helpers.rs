use codexmanager_core::storage::Storage;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_STORAGE_MAX_CONNECTIONS: usize = 32;
const DEFAULT_STORAGE_MAX_IDLE_CONNECTIONS: usize = 16;
const DEFAULT_STORAGE_ACQUIRE_TIMEOUT_MS: u64 = 30_000;
const ENV_STORAGE_MAX_CONNECTIONS: &str = "CODEXMANAGER_STORAGE_MAX_CONNECTIONS";
const ENV_STORAGE_MAX_IDLE_CONNECTIONS: &str = "CODEXMANAGER_STORAGE_MAX_IDLE_CONNECTIONS";
const ENV_STORAGE_ACQUIRE_TIMEOUT_MS: &str = "CODEXMANAGER_STORAGE_ACQUIRE_TIMEOUT_MS";

#[derive(Default)]
struct StorageBucket {
    idle: Vec<Storage>,
    open_count: usize,
    opening_count: usize,
}

#[derive(Default)]
struct StoragePoolState {
    buckets: HashMap<String, StorageBucket>,
}

struct StoragePool {
    state: Mutex<StoragePoolState>,
    available: Condvar,
}

impl StoragePool {
    fn new() -> Self {
        Self {
            state: Mutex::new(StoragePoolState::default()),
            available: Condvar::new(),
        }
    }
}

pub(crate) struct StorageHandle {
    path: String,
    storage: Option<Storage>,
}

impl StorageHandle {
    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - path: 参数 path
    /// - storage: 参数 storage
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new(path: String, storage: Storage) -> Self {
        Self {
            path,
            storage: Some(storage),
        }
    }
}

impl Deref for StorageHandle {
    type Target = Storage;

    /// 函数 `deref`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn deref(&self) -> &Self::Target {
        self.storage.as_ref().expect("storage handle should exist")
    }
}

impl DerefMut for StorageHandle {
    /// 函数 `deref_mut`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.storage.as_mut().expect("storage handle should exist")
    }
}

impl Drop for StorageHandle {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        let Some(storage) = self.storage.take() else {
            return;
        };
        let path = self.path.clone();
        return_storage_to_pool(path, storage);
    }
}

/// 函数 `normalize_key_part`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_key_part(value: Option<&str>) -> Option<String> {
    // 规范化 key 片段，去除空白并避免分隔符冲突
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.replace("::", "_"))
}

/// 函数 `compact_key_part`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn compact_key_part(value: &str) -> String {
    // 对过长/复杂后缀做短哈希，避免账号ID过长且保留稳定唯一性。
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let should_hash = trimmed.len() > 16
        || trimmed.contains('|')
        || trimmed.contains('-')
        || trimmed.contains(' ');
    if !should_hash {
        return trimmed.to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(12);
    for b in digest.iter().take(6) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `account_key`
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
pub(crate) fn account_key(account_id: &str, tags: Option<&str>) -> String {
    // 组合账号与标签，生成稳定的账户唯一标识
    let mut parts = Vec::new();
    parts.push(account_id.to_string());
    if let Some(value) = normalize_key_part(tags) {
        let compact = compact_key_part(&value);
        if !compact.is_empty() {
            parts.push(compact);
        }
    }
    parts.join("::")
}

/// 函数 `hash_platform_key`
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
pub(crate) fn hash_platform_key(key: &str) -> String {
    // 对平台 Key 做不可逆哈希，避免明文存储
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_platform_key`
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
pub(crate) fn generate_platform_key() -> String {
    // 生成随机平台 Key（十六进制）
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::with_capacity(buf.len() * 2);
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_key_id`
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
pub(crate) fn generate_key_id() -> String {
    // 生成短 ID 作为平台 Key 的展示标识
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("gk_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// 函数 `generate_aggregate_api_id`
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
pub(crate) fn generate_aggregate_api_id() -> String {
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("ag_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub(crate) fn generate_proxy_profile_id() -> String {
    let mut buf = [0u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let mut out = String::from("pp_");
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
static STORAGE_OPEN_COUNTS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, usize>>> =
    std::sync::OnceLock::new();

static STORAGE_POOL: OnceLock<StoragePool> = OnceLock::new();

fn storage_pool() -> &'static StoragePool {
    STORAGE_POOL.get_or_init(StoragePool::new)
}

fn lock_storage_pool_state(pool: &StoragePool) -> MutexGuard<'_, StoragePoolState> {
    pool.state.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage pool lock poisoned; recovering");
        poisoned.into_inner()
    })
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_u64_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

pub(crate) fn storage_max_connections() -> usize {
    env_usize_or(ENV_STORAGE_MAX_CONNECTIONS, DEFAULT_STORAGE_MAX_CONNECTIONS).max(1)
}

fn storage_max_idle_connections() -> usize {
    env_usize_or(
        ENV_STORAGE_MAX_IDLE_CONNECTIONS,
        DEFAULT_STORAGE_MAX_IDLE_CONNECTIONS,
    )
    .min(storage_max_connections())
}

fn storage_acquire_timeout() -> Duration {
    Duration::from_millis(env_u64_or(
        ENV_STORAGE_ACQUIRE_TIMEOUT_MS,
        DEFAULT_STORAGE_ACQUIRE_TIMEOUT_MS,
    ))
}

/// 函数 `open_storage`
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
pub(crate) fn open_storage() -> Option<StorageHandle> {
    // 读取数据库路径并打开存储
    let path = match std::env::var("CODEXMANAGER_DB_PATH") {
        Ok(path) => path,
        Err(_) => {
            log::warn!("CODEXMANAGER_DB_PATH not set");
            return None;
        }
    };
    open_storage_at_path(&path)
}

/// 函数 `open_storage_at_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn open_storage_at_path(path: &str) -> Option<StorageHandle> {
    acquire_storage_from_pool(path).map(|storage| StorageHandle::new(path.to_string(), storage))
}

fn open_fresh_storage(path: &str) -> Option<Storage> {
    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    let storage = match Storage::open(&path) {
        Ok(storage) => storage,
        Err(err) => {
            log::error!("open storage failed: {} ({})", path, err);
            return None;
        }
    };
    #[cfg(test)]
    record_storage_open_for_tests(path);
    Some(storage)
}

fn acquire_storage_from_pool(path: &str) -> Option<Storage> {
    let pool = storage_pool();
    let max_connections = storage_max_connections();
    let timeout = storage_acquire_timeout();
    let started_at = Instant::now();
    let mut state = lock_storage_pool_state(pool);

    loop {
        let bucket = state.buckets.entry(path.to_string()).or_default();
        if let Some(storage) = bucket.idle.pop() {
            return Some(storage);
        }

        if bucket.open_count < max_connections && bucket.opening_count == 0 {
            bucket.open_count += 1;
            bucket.opening_count += 1;
            drop(state);
            let storage = open_fresh_storage(path);
            finish_storage_open(path, storage.is_some());
            return storage;
        }

        let elapsed = started_at.elapsed();
        if elapsed >= timeout {
            log::error!(
                "storage pool acquire timed out: path={} max_connections={} timeout_ms={}",
                path,
                max_connections,
                timeout.as_millis()
            );
            return None;
        }

        let remaining = timeout.saturating_sub(elapsed);
        match pool.available.wait_timeout(state, remaining) {
            Ok((next_state, wait_result)) => {
                state = next_state;
                if wait_result.timed_out() {
                    log::error!(
                        "storage pool acquire timed out: path={} max_connections={} timeout_ms={}",
                        path,
                        max_connections,
                        timeout.as_millis()
                    );
                    return None;
                }
            }
            Err(poisoned) => {
                log::warn!("storage pool condvar lock poisoned; recovering");
                let (next_state, _) = poisoned.into_inner();
                state = next_state;
            }
        }
    }
}

fn finish_storage_open(path: &str, success: bool) {
    let pool = storage_pool();
    let mut state = lock_storage_pool_state(pool);
    if let Some(bucket) = state.buckets.get_mut(path) {
        bucket.opening_count = bucket.opening_count.saturating_sub(1);
        if !success {
            bucket.open_count = bucket.open_count.saturating_sub(1);
        }
    }
    pool.available.notify_all();
}

fn return_storage_to_pool(path: String, storage: Storage) {
    let pool = storage_pool();
    let max_idle = storage_max_idle_connections();
    let mut storage = Some(storage);
    {
        let mut state = lock_storage_pool_state(pool);
        let bucket = state.buckets.entry(path).or_default();
        if bucket.idle.len() < max_idle {
            if let Some(storage) = storage.take() {
                bucket.idle.push(storage);
            }
        } else {
            bucket.open_count = bucket.open_count.saturating_sub(1);
        }
    }
    pool.available.notify_one();
}

/// 函数 `initialize_storage`
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
pub(crate) fn initialize_storage() -> Result<(), String> {
    let path = std::env::var("CODEXMANAGER_DB_PATH")
        .map_err(|_| "CODEXMANAGER_DB_PATH not set".to_string())?;
    if !Path::new(&path).exists() {
        log::warn!("storage path missing: {}", path);
    }
    let storage =
        Storage::open(&path).map_err(|err| format!("open storage failed: {} ({})", path, err))?;
    storage
        .init()
        .map_err(|err| format!("storage init failed: {} ({})", path, err))?;
    Ok(())
}

/// 函数 `clear_storage_cache_for_tests`
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
#[cfg(test)]
fn clear_storage_cache_for_tests() {
    let pool = storage_pool();
    let mut state = lock_storage_pool_state(pool);
    state.buckets.clear();
    pool.available.notify_all();
}

/// 函数 `record_storage_open_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 无
#[cfg(test)]
fn record_storage_open_for_tests(path: &str) {
    let mutex = STORAGE_OPEN_COUNTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    let entry = counts.entry(path.to_string()).or_insert(0);
    *entry += 1;
}

/// 函数 `storage_open_count_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn storage_open_count_for_tests(path: &str) -> usize {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return 0;
    };
    let counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.get(path).copied().unwrap_or(0)
}

/// 函数 `clear_storage_open_count_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 无
#[cfg(test)]
fn clear_storage_open_count_for_tests(path: &str) {
    let Some(mutex) = STORAGE_OPEN_COUNTS.get() else {
        return;
    };
    let mut counts = mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("storage open count lock poisoned; recovering for tests");
        poisoned.into_inner()
    });
    counts.remove(path);
}

#[cfg(test)]
#[path = "tests/storage_helpers_tests.rs"]
mod tests;
