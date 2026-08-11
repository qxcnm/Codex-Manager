use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chrono::{SecondsFormat, Utc};
use codexmanager_core::storage::{CodexAgentIdentityRecord, CodexAgentIdentityUpsert, Storage};
use crypto_box::SecretKey as Curve25519SecretKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use rand::RngCore as _;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha512};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const AUTH_API_BASE: &str = "https://auth.openai.com/api/accounts";
const KEY_DERIVATION_CONTEXT: &[u8] = b"codex-agent-identity-ed25519-v1";
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(20);
const AUTO_BOOTSTRAP_QUEUE_CAPACITY: usize = 4096;
static AUTO_BOOTSTRAP_QUEUE: OnceLock<crossbeam_channel::Sender<String>> = OnceLock::new();
static AUTO_BOOTSTRAP_PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentIdentityOperationResult {
    pub account_id: String,
    pub auth_mode: String,
    pub status: String,
    pub agent_runtime_id: Option<String>,
    pub has_task: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportedAgentIdentity {
    pub agent_runtime_id: String,
    pub agent_private_key: String,
    pub task_id: Option<String>,
    pub account_id: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub is_fedramp: bool,
}

#[derive(Debug)]
struct GeneratedKeyMaterial {
    private_key_pkcs8_base64: String,
    public_key_ssh: String,
}

#[derive(Debug, Serialize)]
struct RegisterAgentRequest {
    abom: AgentBillOfMaterials,
    agent_public_key: String,
    capabilities: Vec<String>,
    ttl: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AgentBillOfMaterials {
    agent_version: String,
    agent_harness_id: String,
    running_location: String,
}

#[derive(Debug, Deserialize)]
struct RegisterAgentResponse {
    agent_runtime_id: String,
}

#[derive(Debug, Serialize)]
struct RegisterTaskRequest {
    timestamp: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct RegisterTaskResponse {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default, rename = "taskId")]
    task_id_camel: Option<String>,
    #[serde(default)]
    encrypted_task_id: Option<String>,
    #[serde(default, rename = "encryptedTaskId")]
    encrypted_task_id_camel: Option<String>,
}

#[derive(Debug, Serialize)]
struct AgentAssertionEnvelope {
    agent_runtime_id: String,
    task_id: String,
    timestamp: String,
    signature: String,
}

#[derive(Debug, Clone, Default)]
struct AccessTokenBinding {
    chatgpt_user_id: Option<String>,
    email: Option<String>,
    plan_type: Option<String>,
    is_fedramp: bool,
}

fn optional_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub(crate) fn parse_imported_agent_identity(
    item: &Value,
) -> Result<Option<ImportedAgentIdentity>, String> {
    let root = item
        .get("agent_identity")
        .or_else(|| item.get("agentIdentity"));
    let auth_mode = optional_string(item, &["auth_mode", "authMode"]);
    if root.is_none()
        && !auth_mode
            .as_deref()
            .is_some_and(|mode| mode.eq_ignore_ascii_case("agentIdentity"))
        && optional_string(item, &["agent_runtime_id", "agentRuntimeId"]).is_none()
    {
        return Ok(None);
    }
    let source = root.unwrap_or(item);
    let agent_runtime_id = optional_string(source, &["agent_runtime_id", "agentRuntimeId"])
        .ok_or_else(|| "missing field: agent_runtime_id/agentRuntimeId".to_string())?;
    let agent_private_key = optional_string(source, &["agent_private_key", "agentPrivateKey"])
        .ok_or_else(|| "missing field: agent_private_key/agentPrivateKey".to_string())?;
    signing_key_from_pkcs8_base64(&agent_private_key)?;
    Ok(Some(ImportedAgentIdentity {
        agent_runtime_id,
        agent_private_key,
        task_id: optional_string(source, &["task_id", "taskId"]),
        account_id: optional_string(source, &["account_id", "accountId"])
            .or_else(|| optional_string(item, &["account_id", "accountId"])),
        chatgpt_user_id: optional_string(
            source,
            &["chatgpt_user_id", "chatgptUserId", "user_id", "userId"],
        ),
        email: optional_string(source, &["email"]).or_else(|| optional_string(item, &["email"])),
        plan_type: optional_string(source, &["plan_type", "planType"]),
        is_fedramp: source
            .get("chatgpt_account_is_fedramp")
            .or_else(|| source.get("chatgptAccountIsFedramp"))
            .or_else(|| source.get("is_fedramp"))
            .or_else(|| source.get("isFedramp"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }))
}

pub(crate) fn save_imported_agent_identity(
    storage: &Storage,
    account_id: &str,
    identity: &ImportedAgentIdentity,
) -> Result<(), String> {
    storage
        .upsert_codex_agent_identity(&CodexAgentIdentityUpsert {
            account_id: account_id.to_string(),
            agent_runtime_id: identity.agent_runtime_id.clone(),
            agent_private_key: identity.agent_private_key.clone(),
            task_id: identity.task_id.clone(),
            chatgpt_user_id: identity.chatgpt_user_id.clone(),
            email: identity.email.clone(),
            plan_type: identity.plan_type.clone(),
            is_fedramp: identity.is_fedramp,
            status: if identity.task_id.is_some() {
                "ready".to_string()
            } else {
                "task_pending".to_string()
            },
            last_error: None,
        })
        .map_err(|error| error.to_string())
}

pub(crate) fn bootstrap_account_agent_identity(
    account_id: &str,
) -> Result<AgentIdentityOperationResult, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    bootstrap_account_agent_identity_with_storage(&storage, account_id)
}

pub(crate) fn enqueue_agent_identity_bootstrap(account_id: &str) -> bool {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return false;
    }
    let pending = AUTO_BOOTSTRAP_PENDING.get_or_init(|| Mutex::new(HashSet::new()));
    {
        let mut guard =
            crate::lock_utils::lock_recover(pending, "agent_identity_bootstrap_pending");
        if !guard.insert(account_id.to_string()) {
            return true;
        }
    }
    let sender = AUTO_BOOTSTRAP_QUEUE.get_or_init(|| {
        let (sender, receiver) = crossbeam_channel::bounded::<String>(AUTO_BOOTSTRAP_QUEUE_CAPACITY);
        for index in 0..3 {
            let receiver = receiver.clone();
            std::thread::Builder::new()
                .name(format!("agent-identity-bootstrap-{index}"))
                .spawn(move || {
                    while let Ok(account_id) = receiver.recv() {
                        match bootstrap_account_agent_identity(&account_id) {
                            Ok(result) => log::info!(
                                "event=agent_identity_bootstrap_completed account_id={} status={} has_task={}",
                                result.account_id,
                                result.status,
                                result.has_task
                            ),
                            Err(error) => log::warn!(
                                "event=agent_identity_bootstrap_failed account_id={} error={}",
                                account_id,
                                sanitized_error_body(error)
                            ),
                        }
                        if let Some(pending) = AUTO_BOOTSTRAP_PENDING.get() {
                            crate::lock_utils::lock_recover(
                                pending,
                                "agent_identity_bootstrap_pending",
                            )
                            .remove(&account_id);
                        }
                    }
                })
                .expect("spawn agent identity bootstrap worker");
        }
        sender
    });
    match sender.try_send(account_id.to_string()) {
        Ok(()) => true,
        Err(error) => {
            crate::lock_utils::lock_recover(pending, "agent_identity_bootstrap_pending")
                .remove(account_id);
            log::warn!("agent identity bootstrap queue rejected {account_id}: {error}");
            false
        }
    }
}

pub(crate) fn bootstrap_account_agent_identity_with_storage(
    storage: &Storage,
    account_id: &str,
) -> Result<AgentIdentityOperationResult, String> {
    if let Some(existing) = storage
        .find_codex_agent_identity(account_id)
        .map_err(|error| error.to_string())?
    {
        return ensure_task(storage, existing);
    }
    let token = storage
        .find_token_by_account_id(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "account token not found".to_string())?;
    let access_token = token.access_token.trim();
    if access_token.is_empty() {
        return Err("access_token is empty".to_string());
    }
    let binding = decode_access_token_binding(access_token)?;
    let key = generate_key_material()?;
    let client = registration_client(account_id)?;
    let runtime_id = register_agent_identity(
        &client,
        access_token,
        binding.is_fedramp,
        &key.public_key_ssh,
    )?;
    let record = CodexAgentIdentityUpsert {
        account_id: account_id.to_string(),
        agent_runtime_id: runtime_id.clone(),
        agent_private_key: key.private_key_pkcs8_base64.clone(),
        task_id: None,
        chatgpt_user_id: binding.chatgpt_user_id,
        email: binding.email,
        plan_type: binding.plan_type,
        is_fedramp: binding.is_fedramp,
        status: "task_pending".to_string(),
        last_error: None,
    };
    storage
        .upsert_codex_agent_identity(&record)
        .map_err(|error| error.to_string())?;
    ensure_task(
        storage,
        CodexAgentIdentityRecord {
            account_id: account_id.to_string(),
            agent_runtime_id: runtime_id,
            agent_private_key: key.private_key_pkcs8_base64,
            task_id: None,
            chatgpt_user_id: record.chatgpt_user_id,
            email: record.email,
            plan_type: record.plan_type,
            is_fedramp: record.is_fedramp,
            status: record.status,
            last_error: None,
            created_at: 0,
            updated_at: 0,
        },
    )
}

fn ensure_task(
    storage: &Storage,
    mut record: CodexAgentIdentityRecord,
) -> Result<AgentIdentityOperationResult, String> {
    if record
        .task_id
        .as_deref()
        .is_some_and(|task| !task.trim().is_empty())
    {
        return Ok(operation_result(&record, None));
    }
    let client = registration_client(&record.account_id)?;
    match register_agent_task(&client, &record) {
        Ok(task_id) => {
            storage
                .update_codex_agent_identity_task(&record.account_id, &task_id)
                .map_err(|error| error.to_string())?;
            record.task_id = Some(task_id);
            record.status = "ready".to_string();
            record.last_error = None;
            Ok(operation_result(&record, None))
        }
        Err(error) => {
            let _ = storage.update_codex_agent_identity_status(
                &record.account_id,
                "task_failed",
                Some(&error),
            );
            Err(error)
        }
    }
}

pub(crate) fn build_agent_identity_authorization(
    storage: &Storage,
    account_id: &str,
) -> Result<Option<String>, String> {
    let Some(mut record) = storage
        .find_codex_agent_identity(account_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if record
        .task_id
        .as_deref()
        .is_none_or(|task| task.trim().is_empty())
    {
        ensure_task(storage, record.clone())?;
        record = storage
            .find_codex_agent_identity(account_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "agent identity disappeared after task registration".to_string())?;
    }
    let task_id = record
        .task_id
        .as_deref()
        .ok_or_else(|| "agent identity task id is missing".to_string())?;
    Ok(Some(build_assertion(&record, task_id)?))
}

pub(crate) fn recover_agent_identity_authorization(
    storage: &Storage,
    account_id: &str,
) -> Result<Option<String>, String> {
    let changed = storage
        .clear_codex_agent_identity_task(account_id)
        .map_err(|error| error.to_string())?;
    if !changed {
        return Ok(None);
    }
    build_agent_identity_authorization(storage, account_id)
}

pub(crate) fn list_agent_identity_statuses() -> Result<Vec<AgentIdentityOperationResult>, String> {
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .list_codex_agent_identities()
        .map_err(|error| error.to_string())
        .map(|items| {
            items
                .iter()
                .map(|record| operation_result(record, record.last_error.clone()))
                .collect()
        })
}

fn operation_result(
    record: &CodexAgentIdentityRecord,
    message: Option<String>,
) -> AgentIdentityOperationResult {
    AgentIdentityOperationResult {
        account_id: record.account_id.clone(),
        auth_mode: "agentIdentity".to_string(),
        status: record.status.clone(),
        agent_runtime_id: Some(mask_runtime_id(&record.agent_runtime_id)),
        has_task: record
            .task_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        message,
    }
}

fn mask_runtime_id(value: &str) -> String {
    if value.len() <= 12 {
        return "***".to_string();
    }
    format!("{}…{}", &value[..6], &value[value.len() - 4..])
}

fn registration_client(account_id: &str) -> Result<Client, String> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(REGISTRATION_TIMEOUT)
        .user_agent(crate::gateway::current_codex_user_agent());
    if let Some(proxy_url) = crate::gateway::current_upstream_proxy_url_for_account(account_id) {
        builder =
            builder.proxy(reqwest::Proxy::all(&proxy_url).map_err(|error| error.to_string())?);
    }
    builder.build().map_err(|error| error.to_string())
}

fn register_agent_identity(
    client: &Client,
    access_token: &str,
    is_fedramp: bool,
    public_key_ssh: &str,
) -> Result<String, String> {
    let request = RegisterAgentRequest {
        abom: AgentBillOfMaterials {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            agent_harness_id: "codex-cli".to_string(),
            running_location: format!("exec-{}", std::env::consts::OS),
        },
        agent_public_key: public_key_ssh.to_string(),
        capabilities: vec!["responsesapi".to_string()],
        ttl: None,
    };
    let mut builder = client
        .post(format!("{AUTH_API_BASE}/v1/agent/register"))
        .bearer_auth(access_token)
        .json(&request);
    if is_fedramp {
        builder = builder.header("X-OpenAI-Fedramp", "true");
    }
    let response = builder
        .send()
        .map_err(|error| format!("agent identity registration request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = sanitized_error_body(response.text().unwrap_or_default());
        return Err(format!(
            "agent identity registration failed: status={} body={}",
            status.as_u16(),
            body
        ));
    }
    let value: RegisterAgentResponse = response
        .json()
        .map_err(|error| format!("invalid agent identity registration response: {error}"))?;
    let runtime_id = value.agent_runtime_id.trim();
    if runtime_id.is_empty() {
        return Err("agent identity registration omitted runtime id".to_string());
    }
    Ok(runtime_id.to_string())
}

fn register_agent_task(
    client: &Client,
    record: &CodexAgentIdentityRecord,
) -> Result<String, String> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signing_key = signing_key_from_pkcs8_base64(&record.agent_private_key)?;
    let payload = format!("{}:{timestamp}", record.agent_runtime_id);
    let signature = BASE64_STANDARD.encode(signing_key.sign(payload.as_bytes()).to_bytes());
    let response = client
        .post(format!(
            "{AUTH_API_BASE}/v1/agent/{}/task/register",
            record.agent_runtime_id
        ))
        .json(&RegisterTaskRequest {
            timestamp,
            signature,
        })
        .send()
        .map_err(|error| format!("agent task registration request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = sanitized_error_body(response.text().unwrap_or_default());
        return Err(format!(
            "agent task registration failed: status={} body={}",
            status.as_u16(),
            body
        ));
    }
    let response: RegisterTaskResponse = response
        .json()
        .map_err(|error| format!("invalid agent task registration response: {error}"))?;
    if let Some(task_id) = response.task_id.or(response.task_id_camel) {
        return Ok(task_id);
    }
    let encrypted = response
        .encrypted_task_id
        .or(response.encrypted_task_id_camel)
        .ok_or_else(|| "agent task registration omitted task id".to_string())?;
    decrypt_task_id(&record.agent_private_key, &encrypted)
}

fn build_assertion(record: &CodexAgentIdentityRecord, task_id: &str) -> Result<String, String> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signing_key = signing_key_from_pkcs8_base64(&record.agent_private_key)?;
    let payload = format!("{}:{task_id}:{timestamp}", record.agent_runtime_id);
    let envelope = AgentAssertionEnvelope {
        agent_runtime_id: record.agent_runtime_id.clone(),
        task_id: task_id.to_string(),
        timestamp,
        signature: BASE64_STANDARD.encode(signing_key.sign(payload.as_bytes()).to_bytes()),
    };
    let serialized = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
    Ok(format!(
        "AgentAssertion {}",
        URL_SAFE_NO_PAD.encode(serialized)
    ))
}

fn generate_key_material() -> Result<GeneratedKeyMaterial, String> {
    let mut seed_material = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut seed_material);
    let mut digest = Sha512::new();
    digest.update(KEY_DERIVATION_CONTEXT);
    digest.update(seed_material);
    let digest = digest.finalize();
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&digest[..32]);
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let private_key = signing_key
        .to_pkcs8_der()
        .map_err(|error| format!("encode agent identity private key failed: {error}"))?;
    Ok(GeneratedKeyMaterial {
        private_key_pkcs8_base64: BASE64_STANDARD.encode(private_key.as_bytes()),
        public_key_ssh: encode_ssh_public_key(&signing_key.verifying_key()),
    })
}

fn signing_key_from_pkcs8_base64(value: &str) -> Result<SigningKey, String> {
    let bytes = BASE64_STANDARD
        .decode(value.trim())
        .map_err(|_| "agent identity private key is not valid base64".to_string())?;
    SigningKey::from_pkcs8_der(&bytes)
        .map_err(|_| "agent identity private key is not valid Ed25519 PKCS#8".to_string())
}

fn encode_ssh_public_key(key: &VerifyingKey) -> String {
    let mut blob = Vec::with_capacity(51);
    append_ssh_string(&mut blob, b"ssh-ed25519");
    append_ssh_string(&mut blob, key.as_bytes());
    format!("ssh-ed25519 {}", BASE64_STANDARD.encode(blob))
}

fn append_ssh_string(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
}

fn decrypt_task_id(private_key: &str, encrypted: &str) -> Result<String, String> {
    let signing_key = signing_key_from_pkcs8_base64(private_key)?;
    let ciphertext = BASE64_STANDARD
        .decode(encrypted.trim())
        .map_err(|_| "encrypted agent task id is not valid base64".to_string())?;
    let digest = Sha512::digest(signing_key.to_bytes());
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&digest[..32]);
    secret[0] &= 248;
    secret[31] &= 127;
    secret[31] |= 64;
    let plaintext = Curve25519SecretKey::from(secret)
        .unseal(&ciphertext)
        .map_err(|_| "decrypt agent task id failed".to_string())?;
    String::from_utf8(plaintext).map_err(|_| "decrypted agent task id is invalid UTF-8".to_string())
}

fn decode_access_token_binding(access_token: &str) -> Result<AccessTokenBinding, String> {
    let mut parts = access_token.split('.');
    let _header = parts.next();
    let payload = parts
        .next()
        .ok_or_else(|| "access_token is not a JWT".to_string())?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "access_token payload is not valid base64url".to_string())?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "access_token payload is not valid JSON".to_string())?;
    let exp = value.get("exp").and_then(Value::as_i64).unwrap_or(0);
    if exp > 0 && exp <= Utc::now().timestamp() {
        return Err("access_token is expired".to_string());
    }
    let auth = value
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object);
    let from_auth = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            auth.and_then(|map| map.get(*key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    };
    let chatgpt_user_id = from_auth(&["chatgpt_user_id", "chatgpt_account_user_id", "user_id"])
        .or_else(|| value.get("sub").and_then(Value::as_str).map(str::to_string));
    let email = value
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_string);
    let plan_type = from_auth(&["chatgpt_plan_type", "plan_type"]);
    let is_fedramp = auth
        .and_then(|map| map.get("chatgpt_account_is_fedramp"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(AccessTokenBinding {
        chatgpt_user_id,
        email,
        plan_type,
        is_fedramp,
    })
}

fn sanitized_error_body(body: String) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_round_trips_and_signs() {
        let key = generate_key_material().expect("generate key");
        let parsed = signing_key_from_pkcs8_base64(&key.private_key_pkcs8_base64)
            .expect("parse private key");
        assert_eq!(
            encode_ssh_public_key(&parsed.verifying_key()),
            key.public_key_ssh
        );
    }

    #[test]
    fn imports_snake_and_camel_case_identity_json() {
        let key = generate_key_material().expect("generate key");
        let snake = serde_json::json!({
            "auth_mode": "agentIdentity",
            "agent_runtime_id": "runtime-1",
            "agent_private_key": key.private_key_pkcs8_base64,
            "task_id": "task-1"
        });
        let parsed = parse_imported_agent_identity(&snake)
            .expect("parse")
            .expect("identity");
        assert_eq!(parsed.agent_runtime_id, "runtime-1");
        assert_eq!(parsed.task_id.as_deref(), Some("task-1"));
    }

    #[test]
    #[ignore = "requires a real account in CODEXMANAGER_DB_PATH"]
    fn live_bootstrap_from_environment_account() {
        let account_id = std::env::var("CODEX_AGENT_IDENTITY_TEST_ACCOUNT_ID")
            .expect("CODEX_AGENT_IDENTITY_TEST_ACCOUNT_ID");
        let storage = crate::storage_helpers::open_storage().expect("storage");
        storage.init().expect("migrations");
        drop(storage);
        let result = bootstrap_account_agent_identity(&account_id).expect("bootstrap identity");
        assert!(result.has_task, "task registration must complete");

        let storage = crate::storage_helpers::open_storage().expect("storage");
        let account = storage
            .find_account_by_id(&account_id)
            .expect("account query")
            .expect("account");
        let assertion = build_agent_identity_authorization(&storage, &account_id)
            .expect("build assertion")
            .expect("agent identity");
        let client = registration_client(&account_id).expect("client");
        let mut request = client
            .get("https://chatgpt.com/backend-api/codex/models?client_version=0.101.0")
            .header("Authorization", assertion)
            .header("originator", "codex-cli");
        if let Some(chatgpt_account_id) = account
            .chatgpt_account_id
            .as_deref()
            .or(account.workspace_id.as_deref())
        {
            request = request.header("ChatGPT-Account-ID", chatgpt_account_id);
        }
        let response = request.send().expect("model request");
        assert!(
            response.status().is_success(),
            "Agent Identity model request failed: status={}",
            response.status()
        );

        let assertion = build_agent_identity_authorization(&storage, &account_id)
            .expect("build usage assertion")
            .expect("agent identity");
        let workspace_id = account
            .chatgpt_account_id
            .as_deref()
            .or(account.workspace_id.as_deref());
        let usage = crate::usage_http::fetch_usage_snapshot(
            "https://chatgpt.com",
            &assertion,
            workspace_id,
        )
        .expect("Agent Identity usage request");
        assert!(usage.is_object(), "usage response must be an object");

        let assertion = build_agent_identity_authorization(&storage, &account_id)
            .expect("build responses assertion")
            .expect("agent identity");
        let mut request = client
            .post("https://chatgpt.com/backend-api/codex/responses")
            .header("Authorization", assertion)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .header("originator", "codex-cli")
            .json(&serde_json::json!({
                "model": "gpt-5.4-mini",
                "instructions": "Reply with exactly: OK",
                "input": [{
                    "role": "user",
                    "content": [{"type": "input_text", "text": "Return OK."}]
                }],
                "stream": true,
                "store": false
            }));
        if let Some(chatgpt_account_id) = workspace_id {
            request = request.header("ChatGPT-Account-ID", chatgpt_account_id);
        }
        let response = request.send().expect("responses request");
        let status = response.status();
        let body = response.text().expect("responses body");
        assert!(
            status.is_success(),
            "Agent Identity responses request failed: status={} body={}",
            status,
            sanitized_error_body(body)
        );
        assert!(
            body.contains("response.completed"),
            "Agent Identity responses stream did not complete"
        );
    }
}
