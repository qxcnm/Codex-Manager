use crate::app_storage::apply_runtime_storage_env;
use crate::rpc_client::{normalize_addr, rpc_call};
use crate::service_runtime::{
    spawn_service_with_addr, stop_service, validate_initialize_response, wait_for_service_ready,
};
use std::fs;
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const SERVICE_READY_RETRIES: usize = 40;
const SERVICE_READY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
const BIND_PROBE_RETRIES: usize = 10;
const BIND_PROBE_DELAY: Duration = Duration::from_millis(120);
const MODEL_CACHE_FILE: &str = "models_cache.json";
const CODEX_CONFIG_FILE: &str = "config.toml";
const ENV_CODEX_HOME: &str = "CODEX_HOME";
const ENV_HOME: &str = "HOME";
const ENV_USERPROFILE: &str = "USERPROFILE";
const ENV_HOMEDRIVE: &str = "HOMEDRIVE";
const ENV_HOMEPATH: &str = "HOMEPATH";
const MANAGED_CONFIG_BEGIN: &str = "# BEGIN CODEXMANAGER CLI CONFIG";
const MANAGED_CONFIG_END: &str = "# END CODEXMANAGER CLI CONFIG";
const MANAGED_PROVIDER_BEGIN_PREFIX: &str = "# BEGIN CODEXMANAGER MODEL PROVIDER ";
const MANAGED_PROVIDER_END: &str = "# END CODEXMANAGER MODEL PROVIDER";

fn is_addr_in_use(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::AddrInUse
}

fn probe_bind_target_available(bind_addr: &str, connect_addr: &str) -> Result<(), String> {
    let trimmed = bind_addr.trim();
    if trimmed.len() > "localhost:".len()
        && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &trimmed["localhost:".len()..];
        let v4 = TcpListener::bind(format!("127.0.0.1:{port}"));
        let v6 = TcpListener::bind(format!("[::1]:{port}"));
        if v4.as_ref().is_err_and(is_addr_in_use) || v6.as_ref().is_err_and(is_addr_in_use) {
            return Err(format!("端口已被占用: {connect_addr}"));
        }
        v4.map_err(|err| format!("检查服务端口失败 ({connect_addr}): {err}"))?;
        if let Err(err) = v6 {
            log::debug!(
                "IPv6 loopback bind probe skipped for {}: {}",
                connect_addr,
                err
            );
        }
        return Ok(());
    }

    TcpListener::bind(trimmed).map(|_| ()).map_err(|err| {
        if is_addr_in_use(&err) {
            format!("端口已被占用: {connect_addr}")
        } else {
            format!("检查服务端口失败 ({connect_addr}): {err}")
        }
    })
}

pub(crate) fn ensure_bind_target_available(
    bind_addr: &str,
    connect_addr: &str,
) -> Result<(), String> {
    let mut last_err = None;
    for attempt in 0..=BIND_PROBE_RETRIES {
        match probe_bind_target_available(bind_addr, connect_addr) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                if attempt < BIND_PROBE_RETRIES {
                    thread::sleep(BIND_PROBE_DELAY);
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| format!("检查服务端口失败 ({connect_addr})")))
}

fn parse_codex_cli_version(user_agent: &str) -> Option<String> {
    let marker = "codex_cli_rs/";
    let start = user_agent.find(marker)? + marker.len();
    let version = user_agent[start..].split_whitespace().next()?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

fn normalize_codex_home_hint(codex_home: Option<&str>) -> Option<PathBuf> {
    let trimmed = codex_home?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    let is_dot_codex = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".codex"));
    is_dot_codex.then_some(path)
}

fn default_codex_home_dir() -> Result<PathBuf, String> {
    if let Ok(raw) = std::env::var(ENV_USERPROFILE) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join(".codex"));
        }
    }

    if let Ok(raw) = std::env::var(ENV_HOME) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join(".codex"));
        }
    }

    let home_drive = std::env::var(ENV_HOMEDRIVE).unwrap_or_default();
    let home_path = std::env::var(ENV_HOMEPATH).unwrap_or_default();
    let combined = format!("{home_drive}{home_path}");
    if !combined.trim().is_empty() {
        return Ok(PathBuf::from(combined).join(".codex"));
    }

    Err("无法解析 Codex CLI 的 Home 目录".to_string())
}

fn resolve_codex_home_dir(codex_home: Option<&str>) -> Result<PathBuf, String> {
    if let Ok(raw) = std::env::var(ENV_CODEX_HOME) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    if let Some(path) = normalize_codex_home_hint(codex_home) {
        return Ok(path);
    }

    default_codex_home_dir()
}

fn ensure_models_cache_models(models: &[serde_json::Value]) -> Result<(), String> {
    if models.is_empty() {
        return Err("模型目录为空，拒绝覆写 Codex 模型缓存".to_string());
    }

    for model in models {
        let slug = model
            .get("slug")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or("");
        if slug.is_empty() {
            return Err("模型目录中存在缺少 slug 的条目，无法同步缓存".to_string());
        }
    }

    Ok(())
}

fn write_models_cache_file(
    cache_path: &Path,
    fetched_at: &str,
    client_version: &str,
    models: &[serde_json::Value],
    etag: Option<String>,
) -> Result<(), String> {
    let parent = cache_path
        .parent()
        .ok_or_else(|| format!("无法定位模型缓存目录: {}", cache_path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("创建 Codex 模型缓存目录失败 ({}): {err}", parent.display()))?;

    let payload = serde_json::json!({
        "fetched_at": fetched_at,
        "etag": etag,
        "client_version": client_version,
        "models": models,
    });
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|err| format!("序列化 Codex 模型缓存失败: {err}"))?;
    fs::write(cache_path, bytes)
        .map_err(|err| format!("写入 Codex 模型缓存失败 ({}): {err}", cache_path.display()))
}

fn normalize_required_config_text(field: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(trimmed.to_string())
}

fn normalize_provider_id(value: Option<&str>) -> Result<String, String> {
    let provider_id = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("codexmanager");
    if !provider_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err("providerId may only contain letters, numbers, '_' and '-'".to_string());
    }
    Ok(provider_id.to_string())
}

fn normalize_env_key(value: Option<&str>) -> Result<String, String> {
    let env_key = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("CODEXMANAGER_API_KEY");
    if !env_key
        .chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err("envKey may only contain uppercase letters, numbers, and '_'".to_string());
    }
    Ok(env_key.to_string())
}

fn normalize_cli_base_url(value: &str) -> Result<String, String> {
    let normalized = normalize_required_config_text("baseUrl", value)?
        .trim_end_matches('/')
        .to_string();
    let parsed =
        reqwest::Url::parse(normalized.as_str()).map_err(|_| "baseUrl is invalid".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("baseUrl must use http or https".to_string());
    }
    Ok(normalized)
}

fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn table_header_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || trimmed.starts_with("[[") {
        return None;
    }
    let end = trimmed.find(']')?;
    let name = trimmed[1..end].trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

fn is_provider_table_for_id(table: &str, provider_id: &str) -> bool {
    table == format!("model_providers.{provider_id}")
        || table == format!("model_providers.\"{provider_id}\"")
        || table.starts_with(format!("model_providers.{provider_id}.").as_str())
        || table.starts_with(format!("model_providers.\"{provider_id}\".").as_str())
}

fn strip_legacy_codexmanager_managed_blocks(content: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == MANAGED_CONFIG_BEGIN || trimmed.starts_with(MANAGED_PROVIDER_BEGIN_PREFIX) {
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed == MANAGED_CONFIG_END || trimmed == MANAGED_PROVIDER_END {
                skipping = false;
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

fn build_codex_provider_block(
    provider_id: &str,
    provider_name: &str,
    base_url: &str,
    env_key: &str,
) -> String {
    [
        format!("[model_providers.{provider_id}]"),
        format!("name = {}", toml_string(provider_name)),
        format!("base_url = {}", toml_string(base_url)),
        "wire_api = \"responses\"".to_string(),
        format!("env_key = {}", toml_string(env_key)),
    ]
    .join("\n")
}

fn build_codex_provider_config(
    existing: &str,
    provider_id: &str,
    provider_name: &str,
    base_url: &str,
    env_key: &str,
) -> String {
    let cleaned = strip_legacy_codexmanager_managed_blocks(existing);
    let provider_block = build_codex_provider_block(provider_id, provider_name, base_url, env_key);
    let mut out = Vec::new();
    let mut skipping_provider_table = false;
    let mut inserted = false;

    for line in cleaned.lines() {
        if let Some(table) = table_header_name(line) {
            if is_provider_table_for_id(table.as_str(), provider_id) {
                if !inserted {
                    out.push(provider_block.as_str());
                    inserted = true;
                }
                skipping_provider_table = true;
                continue;
            }
            skipping_provider_table = false;
        }

        if skipping_provider_table {
            continue;
        }
        out.push(line);
    }

    if !inserted {
        if !out.iter().all(|line| line.trim().is_empty()) {
            out.push("");
        }
        out.push(provider_block.as_str());
    }

    format!("{}\n", out.join("\n").trim())
}

fn write_codex_provider_config_file(
    config_path: &Path,
    provider_id: &str,
    provider_name: &str,
    base_url: &str,
    env_key: &str,
) -> Result<(), String> {
    let parent = config_path
        .parent()
        .ok_or_else(|| format!("无法定位 Codex 配置目录: {}", config_path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("创建 Codex 配置目录失败 ({}): {err}", parent.display()))?;
    let existing = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(format!(
                "读取 Codex 配置失败 ({}): {err}",
                config_path.display()
            ))
        }
    };
    let next =
        build_codex_provider_config(&existing, provider_id, provider_name, base_url, env_key);
    fs::write(config_path, next).map_err(|err| {
        format!(
            "写入 Codex provider 配置失败 ({}): {err}",
            config_path.display()
        )
    })
}

/// 函数 `service_initialize`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_initialize(
    app: tauri::AppHandle,
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let v = tauri::async_runtime::spawn_blocking(move || rpc_call("initialize", addr, None))
        .await
        .map_err(|err| format!("initialize task failed: {err}"))??;
    validate_initialize_response(&v)?;
    Ok(v)
}

/// 函数 `service_start`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_start(app: tauri::AppHandle, addr: String) -> Result<(), String> {
    let connect_addr = normalize_addr(&addr)?;
    apply_runtime_storage_env(&app);
    let bind_mode = codexmanager_service::current_service_bind_mode();
    let bind_addr = codexmanager_service::listener_bind_addr_for_mode(&connect_addr, &bind_mode);
    tauri::async_runtime::spawn_blocking(move || {
        log::info!(
            "service_start requested connect_addr={} bind_addr={}",
            connect_addr,
            bind_addr
        );
        stop_service();
        ensure_bind_target_available(&bind_addr, &connect_addr)?;
        spawn_service_with_addr(&app, &bind_addr, &connect_addr)?;
        wait_for_service_ready(
            &connect_addr,
            SERVICE_READY_RETRIES,
            SERVICE_READY_RETRY_DELAY,
        )
        .map_err(|err| {
            log::error!(
                "service health check failed at {} (bind {}): {}",
                connect_addr,
                bind_addr,
                err
            );
            stop_service();
            format!("service not ready at {connect_addr}: {err}")
        })
    })
    .await
    .map_err(|err| format!("service_start task failed: {err}"))?
}

/// 函数 `service_stop`
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
#[tauri::command]
pub async fn service_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        stop_service();
        Ok(())
    })
    .await
    .map_err(|err| format!("service_stop task failed: {err}"))?
}

/// 函数 `service_rpc_token`
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
#[tauri::command]
pub async fn service_rpc_token() -> Result<String, String> {
    Ok(codexmanager_service::rpc_auth_token().to_string())
}

/// 函数 `service_sync_codex_models_cache`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - user_agent: 参数 user_agent
/// - models: 参数 models
/// - codex_home: 参数 codex_home
/// - etag: 参数 etag
/// - fetched_at: 参数 fetched_at
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_sync_codex_models_cache(
    user_agent: String,
    models: Vec<serde_json::Value>,
    codex_home: Option<String>,
    etag: Option<String>,
    fetched_at: String,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_models_cache_models(&models)?;
        let client_version = parse_codex_cli_version(&user_agent)
            .ok_or_else(|| format!("无法从 userAgent 解析 Codex CLI 版本: {user_agent}"))?;
        let codex_home_dir = resolve_codex_home_dir(codex_home.as_deref())?;
        let cache_path = codex_home_dir.join(MODEL_CACHE_FILE);
        write_models_cache_file(&cache_path, &fetched_at, &client_version, &models, etag)?;
        Ok(serde_json::json!({
            "cachePath": cache_path.to_string_lossy().to_string(),
            "clientVersion": client_version,
            "modelsCount": models.len(),
        }))
    })
    .await
    .map_err(|err| format!("service_sync_codex_models_cache task failed: {err}"))?
}

#[tauri::command]
pub async fn service_sync_codex_provider_config(
    base_url: String,
    provider_id: Option<String>,
    provider_name: Option<String>,
    env_key: Option<String>,
    codex_home: Option<String>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider_id = normalize_provider_id(provider_id.as_deref())?;
        let provider_name = provider_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("CodexManager Local")
            .to_string();
        let base_url = normalize_cli_base_url(base_url.as_str())?;
        let env_key = normalize_env_key(env_key.as_deref())?;
        let codex_home_dir = resolve_codex_home_dir(codex_home.as_deref())?;
        let config_path = codex_home_dir.join(CODEX_CONFIG_FILE);

        write_codex_provider_config_file(
            &config_path,
            provider_id.as_str(),
            provider_name.as_str(),
            base_url.as_str(),
            env_key.as_str(),
        )?;

        Ok(serde_json::json!({
            "configPath": config_path.to_string_lossy().to_string(),
            "providerId": provider_id,
            "providerName": provider_name,
            "baseUrl": base_url,
            "envKey": env_key,
        }))
    })
    .await
    .map_err(|err| format!("service_sync_codex_provider_config task failed: {err}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var_os(key);
            match value {
                Some(current) => std::env::set_var(key, current),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.as_ref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn parse_codex_cli_version_extracts_semver() {
        assert_eq!(
            parse_codex_cli_version("codex_cli_rs/0.120.0"),
            Some("0.120.0".to_string())
        );
        assert_eq!(
            parse_codex_cli_version("prefix codex_cli_rs/0.121.1 extra"),
            Some("0.121.1".to_string())
        );
        assert_eq!(parse_codex_cli_version("codex_cli_rs/"), None);
    }

    #[test]
    fn resolve_codex_home_dir_prefers_env_over_hint() {
        let _guard = ENV_TEST_LOCK.lock().expect("env test lock");
        let _codex_home = EnvGuard::set(ENV_CODEX_HOME, Some("D:/custom-codex-home"));
        let _userprofile = EnvGuard::set(ENV_USERPROFILE, Some("C:/Users/test"));

        let resolved = resolve_codex_home_dir(Some("C:/Users/test/.codex")).expect("resolve");

        assert_eq!(resolved, PathBuf::from("D:/custom-codex-home"));
    }

    #[test]
    fn resolve_codex_home_dir_falls_back_to_userprofile() {
        let _guard = ENV_TEST_LOCK.lock().expect("env test lock");
        let _codex_home = EnvGuard::set(ENV_CODEX_HOME, None);
        let _userprofile = EnvGuard::set(ENV_USERPROFILE, Some("C:/Users/test"));
        let _home = EnvGuard::set(ENV_HOME, None);
        let _homedrive = EnvGuard::set(ENV_HOMEDRIVE, None);
        let _homepath = EnvGuard::set(ENV_HOMEPATH, None);

        let resolved = resolve_codex_home_dir(None).expect("resolve default home");

        assert_eq!(resolved, PathBuf::from("C:/Users/test/.codex"));
    }

    #[test]
    fn write_models_cache_file_persists_models_and_client_version() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codexmanager-model-cache-{unique}"));
        let cache_path = root.join(MODEL_CACHE_FILE);
        let models = vec![serde_json::json!({
            "slug": "gpt-5.4-mini",
            "display_name": "gpt-5.4-mini",
            "supported_in_api": true,
            "visibility": "list"
        })];

        write_models_cache_file(
            &cache_path,
            "2026-04-12T10:00:00.000Z",
            "0.120.0",
            &models,
            None,
        )
        .expect("write cache");

        let payload: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).expect("read cache file"))
                .expect("parse cache file");

        assert_eq!(
            payload
                .get("client_version")
                .and_then(|value| value.as_str()),
            Some("0.120.0")
        );
        assert_eq!(
            payload
                .get("models")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|item| item.get("slug"))
                .and_then(|value| value.as_str()),
            Some("gpt-5.4-mini")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_codex_provider_config_only_replaces_managed_provider() {
        let existing = r#"model = "gpt-5.5"
model_provider = "openai"
review_model = "gpt-5.5"

# BEGIN CODEXMANAGER CLI CONFIG
model = "deepseek-v4-pro"
model_provider = "cm"
review_model = "deepseek-v4-pro"
# END CODEXMANAGER CLI CONFIG

[model_providers.codexmanager]
name = "Old"
base_url = "http://old.example/v1"
wire_api = "responses"

[model_providers.codexmanager.auth]
command = "old-token-command"

[model_providers.other]
name = "Other"
base_url = "http://other.example/v1"

[projects.'C:\Users\test\repo']
trust_level = "trusted"
"#;

        let merged = build_codex_provider_config(
            existing,
            "codexmanager",
            "CodexManager Local",
            "http://localhost:48760/v1",
            "CODEXMANAGER_API_KEY",
        );

        assert!(merged.contains("model = \"gpt-5.5\""));
        assert!(merged.contains("model_provider = \"openai\""));
        assert!(merged.contains("review_model = \"gpt-5.5\""));
        assert!(merged.contains("[model_providers.codexmanager]"));
        assert!(merged.contains("name = \"CodexManager Local\""));
        assert!(merged.contains("base_url = \"http://localhost:48760/v1\""));
        assert!(merged.contains("wire_api = \"responses\""));
        assert!(merged.contains("env_key = \"CODEXMANAGER_API_KEY\""));
        assert!(merged.contains("[model_providers.other]"));
        assert!(merged.contains("[projects.'C:\\Users\\test\\repo']"));
        assert!(!merged.contains("BEGIN CODEXMANAGER CLI CONFIG"));
        assert!(!merged.contains("deepseek-v4-pro"));
        assert!(!merged.contains("http://old.example/v1"));
        assert!(!merged.contains("old-token-command"));
        assert_eq!(merged.matches("[model_providers.codexmanager]").count(), 1);
    }

    #[test]
    fn write_codex_provider_config_file_writes_only_config_toml() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codexmanager-provider-{unique}"));
        let config_path = root.join(CODEX_CONFIG_FILE);
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(
            &config_path,
            r#"model = "gpt-5.5"
model_provider = "openai"
"#,
        )
        .expect("write config");

        write_codex_provider_config_file(
            &config_path,
            "codexmanager",
            "CodexManager Local",
            "http://localhost:48760/v1",
            "CODEXMANAGER_API_KEY",
        )
        .expect("write provider config");

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("model = \"gpt-5.5\""));
        assert!(content.contains("model_provider = \"openai\""));
        assert!(content.contains("[model_providers.codexmanager]"));
        assert!(!root.join("auth.json").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
