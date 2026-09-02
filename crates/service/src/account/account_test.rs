use codexmanager_core::storage::Storage;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use rand::RngCore;
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::account_status::{
    load_account_status_context, mark_account_limited_for_test_rate_limit,
    mark_account_unavailable_for_test_auth_status, restore_account_active_after_test,
    AccountStatusContext,
};
use crate::account_warmup::{
    build_warmup_headers, resolve_warmup_authorization, summarize_warmup_error, WARMUP_UPSTREAM_URL,
};
use crate::storage_helpers::open_storage;

const DEFAULT_TEXT_TEST_PROMPT: &str = "hi";
const DEFAULT_IMAGE_TEST_PROMPT: &str =
    "Generate a cute orange cat astronaut sticker on a clean pastel background.";
const DEFAULT_TEXT_TEST_MODEL: &str = "gpt-5.3-codex";
const DEFAULT_IMAGE_TEST_MODEL: &str = "gpt-image-2";
const ACCOUNT_TEST_OVERALL_TIMEOUT: Duration = Duration::from_secs(120);

/// 测试类型：文字模型直连，或图片模型走 image_generation 工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestKind {
    Text,
    Image,
}

impl TestKind {
    fn parse(value: Option<&str>) -> TestKind {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("image") => TestKind::Image,
            _ => TestKind::Text,
        }
    }

    fn is_image(self) -> bool {
        self == TestKind::Image
    }
}

/// 账号测试事件，序列化为 PRD 约定的 SSE 事件结构。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTestEvent {
    pub test_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AccountTestEvent {
    fn new(test_id: &str, event_type: &str) -> Self {
        Self {
            test_id: test_id.to_string(),
            event_type: event_type.to_string(),
            text: None,
            model: None,
            status: None,
            image_url: None,
            mime_type: None,
            success: None,
            error: None,
        }
    }

    fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    fn with_image(mut self, image_url: impl Into<String>, mime_type: impl Into<String>) -> Self {
        self.image_url = Some(image_url.into());
        self.mime_type = Some(mime_type.into());
        self
    }

    fn with_success(mut self, success: bool) -> Self {
        self.success = Some(success);
        self
    }

    fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
}

/// 账号测试启动结果，由 `account/test` RPC 直接返回给前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTestStartResult {
    pub test_id: String,
    pub started: bool,
    pub model: String,
}

enum AccountTestOutcome {
    Success,
    AuthError(u16),
    RateLimited,
    Failed(String),
    Canceled,
}

type AccountTestEventHandler = Arc<dyn Fn(AccountTestEvent) + Send + Sync>;

#[derive(Clone)]
struct ActiveAccountTest {
    test_id: String,
    cancel_flag: Arc<AtomicBool>,
}

struct AccountTestSubscriber {
    id: u64,
    sender: Sender<AccountTestEvent>,
}

pub(crate) struct AccountTestEventSubscription {
    test_id: String,
    subscriber_id: u64,
    receiver: Receiver<AccountTestEvent>,
}

impl AccountTestEventSubscription {
    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<AccountTestEvent, crossbeam_channel::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for AccountTestEventSubscription {
    fn drop(&mut self) {
        if let Some(subscribers) = ACCOUNT_TEST_EVENT_SUBSCRIBERS.get() {
            let mut guard =
                crate::lock_utils::lock_recover(subscribers, "account_test_event_subscribers");
            let remove_test_id = if let Some(entries) = guard.get_mut(&self.test_id) {
                entries.retain(|entry| entry.id != self.subscriber_id);
                entries.is_empty()
            } else {
                false
            };
            if remove_test_id {
                guard.remove(&self.test_id);
            }
        }
    }
}

static ACCOUNT_TEST_EVENT_HANDLER: OnceLock<Mutex<Option<AccountTestEventHandler>>> =
    OnceLock::new();
static ACCOUNT_TEST_EVENT_SUBSCRIBERS: OnceLock<
    Mutex<HashMap<String, Vec<AccountTestSubscriber>>>,
> = OnceLock::new();
static ACTIVE_ACCOUNT_TESTS: OnceLock<Mutex<HashMap<String, ActiveAccountTest>>> = OnceLock::new();
static ACCOUNT_TEST_SUBSCRIBER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 函数 `set_account_test_event_handler`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - handler: 参数 handler
///
/// # 返回
/// 无
///
/// 桌面端通过该回调将测试事件转发到前端。
pub fn set_account_test_event_handler<F>(handler: F)
where
    F: Fn(AccountTestEvent) + Send + Sync + 'static,
{
    let slot = ACCOUNT_TEST_EVENT_HANDLER.get_or_init(|| Mutex::new(None));
    let mut guard = crate::lock_utils::lock_recover(slot, "account_test_event_handler");
    *guard = Some(Arc::new(handler));
}

/// 函数 `subscribe_account_test_events`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 返回
/// 返回账号测试事件订阅通道
pub(crate) fn subscribe_account_test_events(test_id: &str) -> AccountTestEventSubscription {
    let (sender, receiver) = bounded(64);
    let subscriber_id = ACCOUNT_TEST_SUBSCRIBER_COUNTER.fetch_add(1, Ordering::Relaxed);
    let subscribers = ACCOUNT_TEST_EVENT_SUBSCRIBERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = crate::lock_utils::lock_recover(subscribers, "account_test_event_subscribers");
    guard
        .entry(test_id.to_string())
        .or_default()
        .push(AccountTestSubscriber {
            id: subscriber_id,
            sender,
        });
    AccountTestEventSubscription {
        test_id: test_id.to_string(),
        subscriber_id,
        receiver,
    }
}

/// 函数 `notify_account_test_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - event: 参数 event
///
/// # 返回
/// 无
pub(crate) fn notify_account_test_event(event: AccountTestEvent) {
    let handler = ACCOUNT_TEST_EVENT_HANDLER.get().and_then(|slot| {
        let guard = crate::lock_utils::lock_recover(slot, "account_test_event_handler");
        guard.clone()
    });
    if let Some(handler) = handler {
        handler(event.clone());
    }
    if let Some(subscribers) = ACCOUNT_TEST_EVENT_SUBSCRIBERS.get() {
        let mut guard =
            crate::lock_utils::lock_recover(subscribers, "account_test_event_subscribers");
        let remove_test_id = if let Some(entries) = guard.get_mut(&event.test_id) {
            entries.retain(|entry| match entry.sender.try_send(event.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
            entries.is_empty()
        } else {
            false
        };
        if remove_test_id {
            guard.remove(&event.test_id);
        }
    }
}

pub(crate) fn normalize_account_test_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    Some(value.to_string())
}

fn generate_account_test_id() -> String {
    let mut bytes = [0_u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut test_id = String::with_capacity(5 + bytes.len() * 2);
    test_id.push_str("test-");
    for byte in bytes {
        write!(&mut test_id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    test_id
}

/// 函数 `start_account_test`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - account_id: 参数 account_id
/// - model: 参数 model
/// - prompt: 参数 prompt
///
/// # 返回
/// 返回测试启动结果
pub(crate) fn start_account_test(
    account_id: &str,
    model: Option<String>,
    prompt: Option<String>,
    kind: Option<String>,
    test_id: Option<String>,
) -> Result<AccountTestStartResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("缺少账号 ID".to_string());
    }

    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let account = storage
        .find_account_by_id(account_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "账号不存在".to_string())?;
    let token = storage
        .find_token_by_account_id(account_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "账号缺少访问令牌".to_string())?;
    let status_context = load_account_status_context(&storage, account_id);
    drop(account);
    drop(token);

    // 前端可自行生成 testId 并在订阅前持有它，从而在事件到达时就能按 testId 隔离，避免
    // 「订阅到拿到 testId」之间的竞态串流。未提供时回退到服务端自增 ID 保持向后兼容。
    let test_id = match test_id {
        Some(value) => {
            normalize_account_test_id(&value).ok_or_else(|| "无效测试 ID".to_string())?
        }
        None => generate_account_test_id(),
    };
    let cancel_flag = register_active_test(account_id, &test_id)?;

    let test_kind = resolve_test_kind(&storage, model.as_deref(), TestKind::parse(kind.as_deref()));
    let resolved_model = resolve_model_slug(&storage, model.as_deref(), test_kind);
    let resolved_prompt = resolve_prompt(prompt.as_deref(), test_kind);
    drop(storage);

    let thread_account_id = account_id.to_string();
    let thread_test_id = test_id.clone();
    let thread_model = resolved_model.clone();
    let thread_prompt = resolved_prompt.clone();
    let thread_kind = test_kind;
    std::thread::spawn(move || {
        run_account_test(
            &thread_account_id,
            &thread_test_id,
            &thread_model,
            &thread_prompt,
            thread_kind,
            cancel_flag,
            status_context,
        );
    });

    Ok(AccountTestStartResult {
        test_id,
        started: true,
        model: resolved_model,
    })
}

/// 函数 `cancel_account_test`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回是否取消了进行中的测试
pub(crate) fn cancel_account_test(account_id: &str, test_id: &str) -> Result<bool, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("缺少账号 ID".to_string());
    }
    let test_id = normalize_account_test_id(test_id).ok_or_else(|| "无效测试 ID".to_string())?;
    let registry = ACTIVE_ACCOUNT_TESTS.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = crate::lock_utils::lock_recover(registry, "account_test_active_tests");
    match guard.get(account_id) {
        Some(active) if active.test_id == test_id => {
            active.cancel_flag.store(true, Ordering::Relaxed);
            Ok(true)
        }
        Some(_) | None => Ok(false),
    }
}

fn register_active_test(account_id: &str, test_id: &str) -> Result<Arc<AtomicBool>, String> {
    let registry = ACTIVE_ACCOUNT_TESTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = crate::lock_utils::lock_recover(registry, "account_test_active_tests");
    if guard.contains_key(account_id) {
        return Err("该账号已有进行中的测试".to_string());
    }
    let flag = Arc::new(AtomicBool::new(false));
    guard.insert(
        account_id.to_string(),
        ActiveAccountTest {
            test_id: test_id.to_string(),
            cancel_flag: flag.clone(),
        },
    );
    Ok(flag)
}

fn remove_active_test(account_id: &str, test_id: &str) {
    if let Some(registry) = ACTIVE_ACCOUNT_TESTS.get() {
        let mut guard = crate::lock_utils::lock_recover(registry, "account_test_active_tests");
        if guard
            .get(account_id)
            .is_some_and(|active| active.test_id == test_id)
        {
            guard.remove(account_id);
        }
    }
}

/// 依据所选模型的真实能力修正测试类型，避免「文字直连 + 图片专用模型」这类组合把
/// `gpt-image-2` 当成顶层主模型直连、被上游判定为「ChatGPT 账号不支持该模型」。
/// 仅当模型在托管模型目录里能查到能力时才自动修正；未知模型沿用调用方传入的显式类型。
fn resolve_test_kind(storage: &Storage, requested: Option<&str>, explicit: TestKind) -> TestKind {
    let Some(slug) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return explicit;
    };
    let Ok(Some(model)) = storage.get_managed_model_v2(slug) else {
        return explicit;
    };
    let supports_image = crate::models_v2::supports_image_generation(&model);
    let supports_text = crate::models_v2::supports_text_generation(&model);
    match (supports_image, supports_text) {
        (true, false) => TestKind::Image,
        (false, true) => TestKind::Text,
        _ => explicit,
    }
}

fn resolve_model_slug(storage: &Storage, requested: Option<&str>, kind: TestKind) -> String {
    if let Some(slug) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        return slug.to_string();
    }
    let predicate = if kind.is_image() {
        crate::models_v2::supports_image_generation
    } else {
        crate::models_v2::supports_text_generation
    };
    storage
        .list_api_models_v2()
        .ok()
        .and_then(|models| models.into_iter().find(predicate).map(|model| model.slug))
        .filter(|slug| !slug.trim().is_empty())
        .unwrap_or_else(|| {
            if kind.is_image() {
                DEFAULT_IMAGE_TEST_MODEL.to_string()
            } else {
                DEFAULT_TEXT_TEST_MODEL.to_string()
            }
        })
}

fn resolve_prompt(requested: Option<&str>, kind: TestKind) -> String {
    if let Some(prompt) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        return prompt.to_string();
    }
    if kind.is_image() {
        DEFAULT_IMAGE_TEST_PROMPT.to_string()
    } else {
        DEFAULT_TEXT_TEST_PROMPT.to_string()
    }
}

fn run_account_test(
    account_id: &str,
    test_id: &str,
    model: &str,
    prompt: &str,
    kind: TestKind,
    cancel_flag: Arc<AtomicBool>,
    status_context: AccountStatusContext,
) {
    let outcome = execute_account_test(account_id, test_id, model, prompt, kind, &cancel_flag);

    if let Some(storage) = open_storage() {
        match &outcome {
            AccountTestOutcome::Success => {
                let _ = restore_account_active_after_test(&storage, account_id, &status_context);
            }
            AccountTestOutcome::AuthError(status_code) => {
                let _ = mark_account_unavailable_for_test_auth_status(
                    &storage,
                    account_id,
                    *status_code,
                    &status_context,
                );
            }
            AccountTestOutcome::RateLimited => {
                let _ =
                    mark_account_limited_for_test_rate_limit(&storage, account_id, &status_context);
            }
            AccountTestOutcome::Failed(_) | AccountTestOutcome::Canceled => {}
        }
    }

    remove_active_test(account_id, test_id);
}

fn execute_account_test(
    account_id: &str,
    test_id: &str,
    model: &str,
    prompt: &str,
    kind: TestKind,
    cancel_flag: &Arc<AtomicBool>,
) -> AccountTestOutcome {
    notify_account_test_event(AccountTestEvent::new(test_id, "test_start").with_model(model));
    notify_account_test_event(
        AccountTestEvent::new(test_id, "status").with_status("正在连接上游…"),
    );

    let client = match build_test_client(account_id) {
        Ok(client) => client,
        Err(err) => {
            emit_redacted_error(test_id, &err, &[]);
            return AccountTestOutcome::Failed(err);
        }
    };

    let storage = match open_storage() {
        Some(storage) => storage,
        None => {
            let message = "storage unavailable".to_string();
            emit_redacted_error(test_id, &message, &[]);
            return AccountTestOutcome::Failed(message);
        }
    };

    let account = match storage.find_account_by_id(account_id) {
        Ok(Some(account)) => account,
        Ok(None) => {
            let message = "账号不存在".to_string();
            emit_redacted_error(test_id, &message, &[]);
            return AccountTestOutcome::Failed(message);
        }
        Err(err) => {
            let message = err.to_string();
            emit_redacted_error(test_id, &message, &[]);
            return AccountTestOutcome::Failed(message);
        }
    };
    let token = match storage.find_token_by_account_id(account_id) {
        Ok(Some(token)) => token,
        Ok(None) => {
            let message = "账号缺少访问令牌".to_string();
            emit_redacted_error(test_id, &message, &[]);
            return AccountTestOutcome::Failed(message);
        }
        Err(err) => {
            let message = err.to_string();
            emit_redacted_error(test_id, &message, &[]);
            return AccountTestOutcome::Failed(message);
        }
    };

    let secrets = vec![
        token.access_token.clone(),
        token.refresh_token.clone(),
        token.id_token.clone(),
    ];
    let authorization = match resolve_warmup_authorization(&storage, &client, &account, &token) {
        Ok(authorization) => authorization,
        Err(err) => {
            emit_redacted_error(test_id, &err, &secrets);
            return AccountTestOutcome::Failed(redact(&err, &secrets));
        }
    };
    let headers = match build_warmup_headers(&account, &authorization) {
        Ok(headers) => headers,
        Err(err) => {
            emit_redacted_error(test_id, &err, &secrets);
            return AccountTestOutcome::Failed(redact(&err, &secrets));
        }
    };
    // 诊断：账号测试与「裸 Bearer curl」不一致时，靠这行定位差异来源。
    // 只记录布尔/非敏感字段，绝不落 token 或 chatgpt-account-id 的值。
    log::info!(
        "event=account_test_request_shape account_id={} uses_agent_identity={} has_chatgpt_account_id_header={} user_agent={}",
        account_id,
        authorization.uses_agent_identity,
        headers.contains_key("chatgpt-account-id"),
        headers
            .get(reqwest::header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default(),
    );
    // 测试不再需要数据库连接，尽早归还到连接池，避免长时间占用。
    drop(storage);

    if kind.is_image() {
        execute_image_test(
            &client,
            &headers,
            test_id,
            model,
            prompt,
            cancel_flag,
            &secrets,
        )
    } else {
        execute_text_test(
            &client,
            &headers,
            test_id,
            model,
            prompt,
            cancel_flag,
            &secrets,
        )
    }
}

fn build_test_client(account_id: &str) -> Result<Client, String> {
    let proxy_url = crate::gateway::account_test_proxy_url_for_account(account_id)?;
    // 只记录是否套代理，不落代理地址（地址可能含账号密码）。
    log::info!(
        "event=account_test_proxy account_id={} has_proxy={}",
        account_id,
        proxy_url.is_some()
    );
    crate::gateway::build_account_test_client_with_timeouts(
        proxy_url.as_deref(),
        ACCOUNT_TEST_OVERALL_TIMEOUT,
    )
}

fn execute_text_test(
    client: &Client,
    headers: &HeaderMap,
    test_id: &str,
    model: &str,
    prompt: &str,
    cancel_flag: &Arc<AtomicBool>,
    secrets: &[String],
) -> AccountTestOutcome {
    let body = json!({
        "model": model,
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": prompt
            }]
        }],
        "stream": true,
        "store": false
    });

    let response = match client
        .post(WARMUP_UPSTREAM_URL)
        .headers(headers.clone())
        .json(&body)
        .send()
    {
        Ok(response) => response,
        Err(err) => {
            let message = redact(&format!("测试请求发送失败: {err}"), secrets);
            emit_redacted_error(test_id, &message, secrets);
            return AccountTestOutcome::Failed(message);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().unwrap_or_default();
        let message = redact(
            &summarize_warmup_error(status.as_u16(), headers, &body_text),
            secrets,
        );
        emit_redacted_error(test_id, &message, secrets);
        return classify_http_outcome(status.as_u16(), &message);
    }

    notify_account_test_event(AccountTestEvent::new(test_id, "status").with_status("已连接上游"));

    let mut reader = BufReader::new(response);
    let mut line = String::new();
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            notify_account_test_event(
                AccountTestEvent::new(test_id, "status").with_status("已取消测试"),
            );
            notify_account_test_event(
                AccountTestEvent::new(test_id, "test_complete").with_success(false),
            );
            return AccountTestOutcome::Canceled;
        }

        line.clear();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(err) => {
                let message = redact(&format!("读取测试流失败: {err}"), secrets);
                emit_redacted_error(test_id, &message, secrets);
                return AccountTestOutcome::Failed(message);
            }
        };
        if bytes == 0 {
            let message = "连接中断".to_string();
            emit_redacted_error(test_id, &message, secrets);
            return AccountTestOutcome::Failed(message);
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if let Some(outcome) =
                process_text_sse_event(test_id, event_name.as_deref(), &data_lines)
            {
                return finish_text_test(test_id, outcome, secrets);
            }
            event_name = None;
            data_lines.clear();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }
}

fn process_text_sse_event(
    test_id: &str,
    event_name: Option<&str>,
    data_lines: &[String],
) -> Option<AccountTestOutcome> {
    let name = event_name.map(str::trim).filter(|value| !value.is_empty());

    if data_lines.is_empty() {
        if let Some(name) = name {
            if is_terminal_event(name) {
                return Some(AccountTestOutcome::Success);
            }
            if is_error_event(name) {
                return Some(AccountTestOutcome::Failed(format!("测试失败: {name}")));
            }
        }
        return None;
    }

    let data = data_lines.join("\n");
    let trimmed = data.trim();
    if trimmed == "[DONE]" {
        return Some(AccountTestOutcome::Success);
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return None;
    };
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .or(name);

    match event_type {
        Some("response.output_text.delta") => {
            if let Some(delta) = value.get("delta").and_then(serde_json::Value::as_str) {
                notify_account_test_event(
                    AccountTestEvent::new(test_id, "content").with_text(delta),
                );
            }
            None
        }
        Some("response.completed") | Some("response.done") => Some(AccountTestOutcome::Success),
        Some("error") => {
            let message = extract_stream_error_message(&value);
            Some(AccountTestOutcome::Failed(message))
        }
        Some("response.failed") | Some("response.incomplete") => {
            let message = extract_stream_error_message(&value);
            Some(AccountTestOutcome::Failed(message))
        }
        _ => None,
    }
}

fn finish_text_test(
    test_id: &str,
    outcome: AccountTestOutcome,
    secrets: &[String],
) -> AccountTestOutcome {
    match &outcome {
        AccountTestOutcome::Success => {
            notify_account_test_event(
                AccountTestEvent::new(test_id, "test_complete").with_success(true),
            );
        }
        AccountTestOutcome::Failed(message) => {
            emit_redacted_error(test_id, message, secrets);
        }
        _ => {}
    }
    outcome
}

fn execute_image_test(
    client: &Client,
    headers: &HeaderMap,
    test_id: &str,
    model: &str,
    prompt: &str,
    cancel_flag: &Arc<AtomicBool>,
    secrets: &[String],
) -> AccountTestOutcome {
    if cancel_flag.load(Ordering::Relaxed) {
        notify_account_test_event(
            AccountTestEvent::new(test_id, "status").with_status("已取消测试"),
        );
        notify_account_test_event(
            AccountTestEvent::new(test_id, "test_complete").with_success(false),
        );
        return AccountTestOutcome::Canceled;
    }

    let image_model = model.trim();
    // 图片测试与网关转发走同一条上游（chatgpt.com/backend-api/codex/responses），工具字段与
    // 网关 local_validation/request.rs 的 build_images_tool_from_request 保持一致（不带
    // `action`、带 `output_format:"png"`）。注意：这条直连上游不认 `metadata` 字段，带上会直接
    // 400「Unsupported parameter: metadata」，所以这里不能像网关内部那样塞 metadata。
    // 图片经 SSE 的 `response.output_item.done` / `response.completed` 事件回传（result 为 base64）。
    let image_headers = headers.clone();
    let body = json!({
        "model": crate::gateway::current_codex_image_main_model(),
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": prompt
            }]
        }],
        "tools": [{
            "type": "image_generation",
            "model": image_model,
            "output_format": "png"
        }],
        "tool_choice": {
            "type": "image_generation"
        },
        "stream": true,
        "store": false,
        "reasoning": {
            "effort": "medium",
            "summary": "auto"
        },
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"]
    });

    let response = match client
        .post(WARMUP_UPSTREAM_URL)
        .headers(image_headers.clone())
        .json(&body)
        .send()
    {
        Ok(response) => response,
        Err(err) => {
            let message = redact(&format!("测试请求发送失败: {err}"), secrets);
            emit_redacted_error(test_id, &message, secrets);
            return AccountTestOutcome::Failed(message);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().unwrap_or_default();
        let message = redact(
            &summarize_warmup_error(status.as_u16(), &image_headers, &body_text),
            secrets,
        );
        emit_redacted_error(test_id, &message, secrets);
        return classify_http_outcome(status.as_u16(), &message);
    }

    notify_account_test_event(AccountTestEvent::new(test_id, "status").with_status("已连接上游"));

    let mut reader = BufReader::new(response);
    let mut line = String::new();
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    let mut seen_images = HashSet::new();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            notify_account_test_event(
                AccountTestEvent::new(test_id, "status").with_status("已取消测试"),
            );
            notify_account_test_event(
                AccountTestEvent::new(test_id, "test_complete").with_success(false),
            );
            return AccountTestOutcome::Canceled;
        }

        line.clear();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(err) => {
                let message = redact(&format!("读取图片流失败: {err}"), secrets);
                emit_redacted_error(test_id, &message, secrets);
                return AccountTestOutcome::Failed(message);
            }
        };
        if bytes == 0 {
            let message = "连接中断".to_string();
            emit_redacted_error(test_id, &message, secrets);
            return AccountTestOutcome::Failed(message);
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if let Some(outcome) = process_image_sse_event(
                test_id,
                event_name.as_deref(),
                &data_lines,
                &mut seen_images,
            ) {
                return finish_image_test(test_id, outcome, &seen_images, secrets);
            }
            event_name = None;
            data_lines.clear();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }
}

fn process_image_sse_event(
    test_id: &str,
    event_name: Option<&str>,
    data_lines: &[String],
    seen_images: &mut HashSet<String>,
) -> Option<AccountTestOutcome> {
    let name = event_name.map(str::trim).filter(|value| !value.is_empty());

    if data_lines.is_empty() {
        if let Some(name) = name {
            if is_terminal_event(name) {
                return Some(AccountTestOutcome::Success);
            }
            if is_error_event(name) {
                return Some(AccountTestOutcome::Failed(format!("测试失败: {name}")));
            }
        }
        return None;
    }

    let data = data_lines.join("\n");
    let trimmed = data.trim();
    if trimmed == "[DONE]" {
        return Some(AccountTestOutcome::Success);
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return None;
    };
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .or(name);

    match event_type {
        Some("response.output_item.done") => {
            if let Some(item) = value.get("item") {
                emit_image_item(test_id, item, seen_images);
            }
            None
        }
        Some("response.completed") | Some("response.done") => {
            // 兜底：`response.completed` 可能携带完整的 `response.output[]`（未走增量事件）。
            if let Some(output) = value
                .get("response")
                .and_then(|response| response.get("output"))
                .and_then(serde_json::Value::as_array)
            {
                for item in output {
                    emit_image_item(test_id, item, seen_images);
                }
            }
            Some(AccountTestOutcome::Success)
        }
        Some("error") => {
            let message = extract_stream_error_message(&value);
            Some(AccountTestOutcome::Failed(message))
        }
        Some("response.failed") | Some("response.incomplete") => {
            let message = extract_stream_error_message(&value);
            Some(AccountTestOutcome::Failed(message))
        }
        _ => None,
    }
}

fn finish_image_test(
    test_id: &str,
    outcome: AccountTestOutcome,
    seen_images: &HashSet<String>,
    secrets: &[String],
) -> AccountTestOutcome {
    match outcome {
        AccountTestOutcome::Success => {
            if seen_images.is_empty() {
                let message = "未收到图片结果".to_string();
                emit_redacted_error(test_id, &message, secrets);
                AccountTestOutcome::Failed(message)
            } else {
                notify_account_test_event(
                    AccountTestEvent::new(test_id, "test_complete").with_success(true),
                );
                AccountTestOutcome::Success
            }
        }
        AccountTestOutcome::Failed(message) => {
            emit_redacted_error(test_id, &message, secrets);
            AccountTestOutcome::Failed(message)
        }
        _ => outcome,
    }
}

fn image_item_to_data_uri(item: &serde_json::Value) -> Option<(String, String)> {
    if item.get("type").and_then(serde_json::Value::as_str) != Some("image_generation_call") {
        return None;
    }
    let base64_data = item
        .get("result")
        .and_then(serde_json::Value::as_str)?
        .trim();
    if base64_data.is_empty() {
        return None;
    }
    let format = item
        .get("output_format")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("png");
    let mime_type = image_mime_type(format);
    let image_url = format!("data:{mime_type};base64,{base64_data}");
    Some((image_url, mime_type.to_string()))
}

fn emit_image_item(
    test_id: &str,
    item: &serde_json::Value,
    seen_images: &mut HashSet<String>,
) -> bool {
    let Some((image_url, mime_type)) = image_item_to_data_uri(item) else {
        return false;
    };
    // 以 item.id（缺失时退化为 data URI）去重，避免 `output_item.done` 与 `response.completed` 重复。
    let dedup_key = item
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| image_url.clone());
    if !seen_images.insert(dedup_key) {
        return false;
    }
    notify_account_test_event(
        AccountTestEvent::new(test_id, "image").with_image(image_url, mime_type),
    );
    true
}

fn image_mime_type(format: &str) -> &'static str {
    match format.trim().to_ascii_lowercase().as_str() {
        "webp" => "image/webp",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        _ => "image/png",
    }
}

fn classify_http_outcome(status: u16, message: &str) -> AccountTestOutcome {
    match status {
        401 | 403 => AccountTestOutcome::AuthError(status),
        429 => AccountTestOutcome::RateLimited,
        _ => AccountTestOutcome::Failed(message.to_string()),
    }
}

fn extract_stream_error_message(value: &serde_json::Value) -> String {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| error.as_str())
        })
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| {
                    error
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| error.as_str())
                })
        })
        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("unknown stream error")
        .to_string()
}

fn is_terminal_event(value: &str) -> bool {
    matches!(value.trim(), "response.completed" | "response.done")
}

fn is_error_event(value: &str) -> bool {
    matches!(
        value.trim(),
        "error" | "response.failed" | "response.incomplete"
    )
}

fn redact(message: &str, secrets: &[String]) -> String {
    let mut out = message.to_string();
    for secret in secrets {
        let secret = secret.trim();
        if secret.len() < 4 {
            continue;
        }
        out = out.replace(secret, "***");
    }
    out
}

fn emit_redacted_error(test_id: &str, message: &str, secrets: &[String]) {
    notify_account_test_event(
        AccountTestEvent::new(test_id, "error").with_error(redact(message, secrets)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kind_parse() {
        assert_eq!(TestKind::parse(Some("image")), TestKind::Image);
        assert_eq!(TestKind::parse(Some("IMAGE")), TestKind::Image);
        assert_eq!(TestKind::parse(Some("text")), TestKind::Text);
        assert_eq!(TestKind::parse(Some("")), TestKind::Text);
        assert_eq!(TestKind::parse(None), TestKind::Text);
    }

    #[test]
    fn resolve_test_kind_auto_switches_image_only_model() {
        use codexmanager_core::storage::{ManagedModelV2, ManagedModelV2Upsert, ModelPriceV2};
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model: ManagedModelV2 {
                    slug: "custom-image-model".to_string(),
                    display_name: "Custom Image Model".to_string(),
                    origin: "custom".to_string(),
                    enabled: true,
                    supported_in_api: true,
                    visibility: "list".to_string(),
                    instructions_mode: "passthrough".to_string(),
                    capabilities: serde_json::json!({
                        "supports_image_generation": true,
                        "supports_text_generation": false
                    }),
                    price: ModelPriceV2 {
                        price_status: "missing".to_string(),
                        ..Default::default()
                    },
                    ..ManagedModelV2::default()
                },
                ..ManagedModelV2Upsert::default()
            })
            .expect("save image model");

        // 图片专用模型：即使前端传了默认的 text 类型，也要自动改成图片测试。
        assert_eq!(
            resolve_test_kind(&storage, Some("custom-image-model"), TestKind::Text),
            TestKind::Image
        );
        assert_eq!(
            resolve_test_kind(&storage, Some("custom-image-model"), TestKind::Image),
            TestKind::Image
        );
        // 未知模型 / 未指定模型：沿用显式类型，避免误判。
        assert_eq!(
            resolve_test_kind(&storage, Some("external-model"), TestKind::Text),
            TestKind::Text
        );
        assert_eq!(
            resolve_test_kind(&storage, None, TestKind::Text),
            TestKind::Text
        );
    }

    #[test]
    fn image_mime_type_mapping() {
        assert_eq!(image_mime_type("png"), "image/png");
        assert_eq!(image_mime_type("webp"), "image/webp");
        assert_eq!(image_mime_type("jpeg"), "image/jpeg");
        assert_eq!(image_mime_type("gif"), "image/gif");
        assert_eq!(image_mime_type("unknown"), "image/png");
    }

    #[test]
    fn redaction_masks_secrets() {
        let message = "auth error token=secret-token-value here";
        let redacted = redact(message, &["secret-token-value".to_string()]);
        assert!(!redacted.contains("secret-token-value"));
        assert!(redacted.contains("***"));
    }

    #[test]
    fn image_item_data_uri_extraction() {
        let image = serde_json::json!({
            "type": "image_generation_call",
            "id": "ig_1",
            "result": "aGVsbG8=",
            "output_format": "png"
        });
        let (url, mime) = image_item_to_data_uri(&image).expect("extract image");
        assert_eq!(url, "data:image/png;base64,aGVsbG8=");
        assert_eq!(mime, "image/png");

        let text = serde_json::json!({"type": "message", "role": "assistant"});
        assert!(image_item_to_data_uri(&text).is_none());

        let empty = serde_json::json!({"type": "image_generation_call", "result": ""});
        assert!(image_item_to_data_uri(&empty).is_none());

        let webp = serde_json::json!({
            "type": "image_generation_call",
            "result": "aGVsbG8=",
            "output_format": "webp"
        });
        assert_eq!(image_item_to_data_uri(&webp).unwrap().1, "image/webp");
    }

    #[test]
    fn account_test_id_validation_accepts_opaque_ascii_ids_only() {
        assert_eq!(
            normalize_account_test_id(" 550e8400-e29b-41d4-a716-446655440000 ").as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(normalize_account_test_id("").is_none());
        assert!(normalize_account_test_id("bad/id").is_none());
        assert!(normalize_account_test_id(&"a".repeat(129)).is_none());
    }

    #[test]
    fn server_generated_account_test_ids_are_opaque_and_unique() {
        let first = generate_account_test_id();
        let second = generate_account_test_id();
        assert_eq!(first.len(), 53);
        assert!(first.starts_with("test-"));
        assert!(normalize_account_test_id(&first).is_some());
        assert_ne!(first, second);
    }

    #[test]
    fn account_test_subscriptions_are_filtered_by_exact_test_id() {
        let first_id = "subscription-filter-first";
        let second_id = "subscription-filter-second";
        let first = subscribe_account_test_events(first_id);
        let second = subscribe_account_test_events(second_id);

        notify_account_test_event(AccountTestEvent::new(first_id, "status"));

        assert_eq!(
            first
                .receiver
                .try_recv()
                .expect("matching subscriber receives event")
                .test_id,
            first_id
        );
        assert!(second.receiver.try_recv().is_err());
    }

    #[test]
    fn cancel_account_test_requires_matching_test_id() {
        let account_id = "cancel-owner-account";
        let test_id = "cancel-owner-test";
        let cancel_flag = register_active_test(account_id, test_id).expect("register active test");

        assert!(!cancel_account_test(account_id, "stale-test").expect("stale cancel result"));
        assert!(!cancel_flag.load(Ordering::Relaxed));
        assert!(cancel_account_test(account_id, test_id).expect("matching cancel result"));
        assert!(cancel_flag.load(Ordering::Relaxed));

        remove_active_test(account_id, test_id);
    }
}
