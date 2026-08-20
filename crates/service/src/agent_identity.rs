use std::collections::{BTreeMap, HashMap};
use std::io::Read as _;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use chrono::{SecondsFormat, Utc};
use codexmanager_core::auth::{extract_chatgpt_account_id, extract_chatgpt_user_id};
use codexmanager_core::storage::{now_ts, Account, AccountAgentIdentity, Storage, Token};
use crypto_box::SecretKey as Curve25519SecretKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha512};

const AGENT_ASSERTION_SCHEME: &str = "AgentAssertion";
const AGENT_IDENTITY_AUTHAPI_BASE_URL: &str = "https://auth.openai.com/api/accounts";
const AGENT_IDENTITY_CAPABILITY_RESPONSES_API: &str = "responsesapi";
const AGENT_IDENTITY_KEY_DERIVATION_CONTEXT: &[u8] = b"codex-agent-identity-ed25519-v1";
const AGENT_IDENTITY_KEY_SEED_BYTES: usize = 64;
const AGENT_IDENTITY_BOOTSTRAP_FAILURE_TTL_SECS: u64 = 5 * 60;
const AGENT_IDENTITY_REGISTRATION_OPERATION: &str = "identity";
const AGENT_TASK_REGISTRATION_OPERATION: &str = "task";
const AGENT_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(15);
const AGENT_TASK_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_TASK_REGISTRATION_RESPONSE_LIMIT: u64 = 64 * 1024;

static ACCOUNT_AGENT_TASK_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static ACCOUNT_AGENT_BOOTSTRAP_FAILURES: OnceLock<Mutex<HashMap<String, BootstrapFailure>>> =
    OnceLock::new();

#[derive(Clone)]
struct BootstrapFailure {
    failed_at: Instant,
    material_digest: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAgentIdentityAuthorization {
    pub(crate) value: String,
    pub(crate) task_id: String,
    pub(crate) is_fedramp: bool,
    pub(crate) account_scope_id: Option<String>,
}

#[derive(Clone)]
struct AgentIdentityBinding {
    chatgpt_user_id: Option<String>,
    account_scope_id: Option<String>,
    access_token: String,
}

#[derive(Serialize)]
struct RegisterAgentTaskRequest {
    timestamp: String,
    signature: String,
}

#[derive(Deserialize)]
struct RegisterAgentTaskResponse {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default, rename = "taskId")]
    task_id_camel: Option<String>,
    #[serde(default)]
    encrypted_task_id: Option<String>,
    #[serde(default, rename = "encryptedTaskId")]
    encrypted_task_id_camel: Option<String>,
}

#[derive(Serialize)]
struct AgentBillOfMaterials {
    agent_version: String,
    agent_harness_id: String,
    running_location: String,
}

#[derive(Serialize)]
struct RegisterAgentIdentityRequest {
    abom: AgentBillOfMaterials,
    agent_public_key: String,
    capabilities: Vec<String>,
    ttl: Option<u64>,
}

#[derive(Deserialize)]
struct RegisterAgentIdentityResponse {
    #[serde(default)]
    agent_runtime_id: Option<String>,
    #[serde(default, rename = "agentRuntimeId")]
    agent_runtime_id_camel: Option<String>,
}

struct GeneratedAgentKeyMaterial {
    private_key_pkcs8_base64: String,
    public_key_ssh: String,
}

pub(crate) fn authorization_header_for_agent_identity(
    identity: &AccountAgentIdentity,
) -> Result<String, String> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    authorization_header_for_agent_identity_at(identity, &timestamp)
}

pub(crate) fn validate_agent_identity(identity: &AccountAgentIdentity) -> Result<(), String> {
    required_value(&identity.agent_runtime_id, "agent_runtime_id")?;
    signing_key_from_pkcs8_base64(&identity.agent_private_key)?;
    Ok(())
}

/// Reuses a matching Agent Identity or creates the missing durable identity
/// material for a ChatGPT bearer account. Existing identities are never reused
/// across a different ChatGPT user or selected account scope.
pub(crate) fn resolve_or_bootstrap_account_agent_identity_authorization(
    storage: &Storage,
    client: &reqwest::blocking::Client,
    account: &Account,
    token: &Token,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
        storage,
        client,
        account,
        token,
        AGENT_IDENTITY_AUTHAPI_BASE_URL,
        None,
    )
}

fn resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
    storage: &Storage,
    client: &reqwest::blocking::Client,
    account: &Account,
    token: &Token,
    authapi_base_url: &str,
    failed_task_id: Option<&str>,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    let binding = resolve_agent_identity_binding(account, token);
    let existing = load_account_agent_identity(storage, &account.id)?;
    if let Some(identity) = existing.as_ref() {
        if agent_identity_matches_binding(identity, &binding)
            && validate_agent_identity(identity).is_ok()
        {
            return apply_binding_scope(
                resolve_account_agent_identity_authorization_with_validation(
                    storage,
                    &account.id,
                    failed_task_id,
                    |candidate| agent_identity_matches_binding(candidate, &binding),
                    |candidate| register_agent_identity_task(client, candidate, authapi_base_url),
                ),
                &binding,
            );
        }
    }

    if binding.access_token.is_empty() {
        if existing.is_some() {
            return Err("stored agent identity does not match the account binding".to_string());
        }
        return Ok(None);
    }
    let Some(chatgpt_user_id) = binding.chatgpt_user_id.as_ref() else {
        return Ok(None);
    };
    let Some(account_scope_id) = binding.account_scope_id.as_ref() else {
        return Ok(None);
    };
    let registration_digest = access_token_digest(&binding.access_token);

    let task_lock = account_agent_task_lock(&account.id);
    {
        let _guard = crate::lock_utils::lock_recover(
            task_lock.as_ref(),
            "account_agent_identity_bootstrap_lock",
        );
        let current = load_account_agent_identity(storage, &account.id)?;
        if current.as_ref().is_some_and(|identity| {
            agent_identity_matches_binding(identity, &binding)
                && validate_agent_identity(identity).is_ok()
        }) {
            // A concurrent request completed identity registration while this
            // request was waiting. Task resolution below will reuse its work.
        } else {
            if bootstrap_failure_is_active(
                &account.id,
                AGENT_IDENTITY_REGISTRATION_OPERATION,
                registration_digest,
            ) {
                return Err(
                    "agent identity registration is cooling down after a recent failure"
                        .to_string(),
                );
            }

            let registration_result = (|| {
                let key_material = generate_agent_key_material()?;
                let is_fedramp = token_chatgpt_account_is_fedramp(&binding.access_token)
                    || token_chatgpt_account_is_fedramp(&token.id_token);
                let agent_runtime_id = register_agent_identity(
                    client,
                    authapi_base_url,
                    &binding.access_token,
                    is_fedramp,
                    &key_material,
                )?;
                let now = now_ts();
                let identity = AccountAgentIdentity {
                    account_id: account.id.clone(),
                    agent_runtime_id,
                    agent_private_key: key_material.private_key_pkcs8_base64,
                    task_id: None,
                    chatgpt_user_id: chatgpt_user_id.clone(),
                    chatgpt_account_is_fedramp: is_fedramp,
                    auth_mode: "agentIdentity".to_string(),
                    workspace_id: Some(account_scope_id.clone()),
                    created_at: now,
                    updated_at: now,
                };
                validate_agent_identity(&identity)?;
                storage
                    .upsert_account_agent_identity(&identity)
                    .map_err(|err| format!("persist bootstrapped agent identity failed: {err}"))
            })();
            if let Err(err) = registration_result {
                remember_bootstrap_failure(
                    &account.id,
                    AGENT_IDENTITY_REGISTRATION_OPERATION,
                    registration_digest,
                );
                return Err(err);
            }
            clear_bootstrap_failure(
                &account.id,
                AGENT_IDENTITY_REGISTRATION_OPERATION,
                registration_digest,
            );
        }
    }

    apply_binding_scope(
        resolve_account_agent_identity_authorization_with_validation(
            storage,
            &account.id,
            failed_task_id,
            |identity| agent_identity_matches_binding(identity, &binding),
            |identity| register_agent_identity_task(client, identity, authapi_base_url),
        ),
        &binding,
    )
}

pub(crate) fn recover_account_agent_identity_authorization(
    storage: &Storage,
    client: &reqwest::blocking::Client,
    account: &Account,
    token: &Token,
    failed_task_id: &str,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    let failed_task_id = required_value(failed_task_id, "failed task_id")?;
    resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
        storage,
        client,
        account,
        token,
        AGENT_IDENTITY_AUTHAPI_BASE_URL,
        Some(failed_task_id),
    )
}

fn resolve_agent_identity_binding(account: &Account, token: &Token) -> AgentIdentityBinding {
    let access_token = token.access_token.trim().to_string();
    let chatgpt_user_id =
        extract_chatgpt_user_id(&access_token).or_else(|| extract_chatgpt_user_id(&token.id_token));
    let account_scope_id = extract_chatgpt_account_id(&access_token)
        .or_else(|| extract_chatgpt_account_id(&token.id_token))
        .or_else(|| {
            account
                .workspace_id
                .as_deref()
                .and_then(non_empty_borrowed)
                .map(str::to_string)
        })
        .or_else(|| {
            account
                .chatgpt_account_id
                .as_deref()
                .and_then(non_empty_borrowed)
                .map(str::to_string)
        });
    AgentIdentityBinding {
        chatgpt_user_id,
        account_scope_id,
        access_token,
    }
}

fn agent_identity_matches_binding(
    identity: &AccountAgentIdentity,
    binding: &AgentIdentityBinding,
) -> bool {
    if let Some(expected_user_id) = binding.chatgpt_user_id.as_deref() {
        if non_empty_borrowed(&identity.chatgpt_user_id) != Some(expected_user_id) {
            return false;
        }
    }
    if let (Some(expected_scope), Some(identity_scope)) = (
        binding.account_scope_id.as_deref(),
        identity
            .workspace_id
            .as_deref()
            .and_then(non_empty_borrowed),
    ) {
        if identity_scope != expected_scope {
            return false;
        }
    }
    true
}

fn apply_binding_scope(
    result: Result<Option<ResolvedAgentIdentityAuthorization>, String>,
    binding: &AgentIdentityBinding,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    result.map(|resolved| {
        resolved.map(|mut resolved| {
            if resolved.account_scope_id.is_none() {
                resolved.account_scope_id = binding.account_scope_id.clone();
            }
            resolved
        })
    })
}

#[cfg(test)]
fn resolve_account_agent_identity_authorization_with<F>(
    storage: &Storage,
    account_id: &str,
    failed_task_id: Option<&str>,
    register_task: F,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String>
where
    F: FnOnce(&AccountAgentIdentity) -> Result<String, String>,
{
    resolve_account_agent_identity_authorization_with_validation(
        storage,
        account_id,
        failed_task_id,
        |_| true,
        register_task,
    )
}

fn resolve_account_agent_identity_authorization_with_validation<F, V>(
    storage: &Storage,
    account_id: &str,
    failed_task_id: Option<&str>,
    identity_is_valid_for_account: V,
    register_task: F,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String>
where
    F: FnOnce(&AccountAgentIdentity) -> Result<String, String>,
    V: Fn(&AccountAgentIdentity) -> bool,
{
    let account_id = required_value(account_id, "account_id")?;
    let identity = load_account_agent_identity(storage, account_id)?;
    let Some(identity) = identity else {
        return Ok(None);
    };
    if !identity_is_valid_for_account(&identity) {
        return Ok(None);
    }
    if task_can_be_reused(&identity, failed_task_id) {
        return resolved_authorization(&identity).map(Some);
    }

    let task_lock = account_agent_task_lock(account_id);
    let _guard = crate::lock_utils::lock_recover(&task_lock, "account_agent_task_lock");

    // Re-read under the per-account lock. Request paths use separate SQLite
    // handles, so the caller's snapshot cannot prove that registration is
    // still needed after waiting for another request.
    let mut identity = load_account_agent_identity(storage, account_id)?
        .ok_or_else(|| "agent identity disappeared during task registration".to_string())?;
    if !identity_is_valid_for_account(&identity) {
        return Ok(None);
    }
    if task_can_be_reused(&identity, failed_task_id) {
        return resolved_authorization(&identity).map(Some);
    }

    let material_digest = agent_identity_material_digest(&identity);
    if bootstrap_failure_is_active(
        account_id,
        AGENT_TASK_REGISTRATION_OPERATION,
        material_digest,
    ) {
        return Err("agent task registration is cooling down after a recent failure".to_string());
    }

    let result = (|| {
        let task_id = register_task(&identity)?;
        let task_id = required_value(&task_id, "registered task_id")?.to_string();
        let updated = storage
            .update_account_agent_identity_task_id(
                account_id,
                &identity.agent_runtime_id,
                &identity.agent_private_key,
                Some(&task_id),
            )
            .map_err(|err| format!("persist agent identity task failed: {err}"))?;
        if !updated {
            return Err("agent identity disappeared before task persistence".to_string());
        }
        identity.task_id = Some(task_id);
        resolved_authorization(&identity).map(Some)
    })();
    match &result {
        Ok(_) => clear_bootstrap_failure(
            account_id,
            AGENT_TASK_REGISTRATION_OPERATION,
            material_digest,
        ),
        Err(_) => remember_bootstrap_failure(
            account_id,
            AGENT_TASK_REGISTRATION_OPERATION,
            material_digest,
        ),
    }
    result
}

fn account_agent_task_lock(account_id: &str) -> Arc<Mutex<()>> {
    let locks = ACCOUNT_AGENT_TASK_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    crate::lock_utils::lock_recover(locks, "account_agent_task_locks")
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn load_account_agent_identity(
    storage: &Storage,
    account_id: &str,
) -> Result<Option<AccountAgentIdentity>, String> {
    storage
        .find_account_agent_identity(account_id)
        .map_err(|err| format!("load agent identity failed: {err}"))
}

fn task_can_be_reused(identity: &AccountAgentIdentity, failed_task_id: Option<&str>) -> bool {
    let Some(task_id) = identity
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
    else {
        return false;
    };
    failed_task_id.is_none_or(|failed_task_id| task_id != failed_task_id)
}

fn resolved_authorization(
    identity: &AccountAgentIdentity,
) -> Result<ResolvedAgentIdentityAuthorization, String> {
    let task_id = identity
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
        .ok_or_else(|| "agent identity task_id is empty".to_string())?;
    Ok(ResolvedAgentIdentityAuthorization {
        value: authorization_header_for_agent_identity(identity)?,
        task_id: task_id.to_string(),
        is_fedramp: identity.chatgpt_account_is_fedramp,
        account_scope_id: identity
            .workspace_id
            .as_deref()
            .and_then(non_empty_borrowed)
            .map(str::to_string),
    })
}

fn register_agent_identity(
    client: &reqwest::blocking::Client,
    authapi_base_url: &str,
    access_token: &str,
    is_fedramp: bool,
    key_material: &GeneratedAgentKeyMaterial,
) -> Result<String, String> {
    let request = RegisterAgentIdentityRequest {
        abom: AgentBillOfMaterials {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            agent_harness_id: "codex-cli".to_string(),
            running_location: format!("cli-{}", std::env::consts::OS),
        },
        agent_public_key: key_material.public_key_ssh.clone(),
        capabilities: vec![AGENT_IDENTITY_CAPABILITY_RESPONSES_API.to_string()],
        ttl: None,
    };
    let url = format!(
        "{}/v1/agent/register",
        authapi_base_url.trim_end_matches('/')
    );
    let mut request_builder = client
        .post(&url)
        .timeout(AGENT_REGISTRATION_TIMEOUT)
        // Keep the auth request on the same first-party identity path as the
        // official Codex client.  The account bearer token alone is valid for
        // normal Responses calls, but agent registration also relies on the
        // Codex originator and User-Agent headers for request classification.
        .header("originator", crate::gateway::current_wire_originator())
        .header("User-Agent", crate::gateway::current_codex_user_agent())
        .bearer_auth(access_token)
        .json(&request);
    if is_fedramp {
        request_builder = request_builder.header("x-openai-fedramp", "true");
    }
    let response = request_builder
        .send()
        .map_err(|err| format!("agent identity registration request failed: {err}"))?;
    let status = response.status();
    let mut body = Vec::new();
    response
        .take(AGENT_TASK_REGISTRATION_RESPONSE_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|err| format!("read agent identity registration response failed: {err}"))?;
    if body.len() as u64 > AGENT_TASK_REGISTRATION_RESPONSE_LIMIT {
        return Err("agent identity registration response exceeded 64 KiB".to_string());
    }
    if !status.is_success() {
        return Err(format!(
            "agent identity registration returned status {}",
            status.as_u16()
        ));
    }
    let response: RegisterAgentIdentityResponse = serde_json::from_slice(&body)
        .map_err(|err| format!("agent identity registration response is invalid: {err}"))?;
    response
        .agent_runtime_id
        .or(response.agent_runtime_id_camel)
        .and_then(non_empty_owned)
        .ok_or_else(|| "agent identity registration response omitted agent_runtime_id".to_string())
}

fn generate_agent_key_material() -> Result<GeneratedAgentKeyMaterial, String> {
    let mut seed_material = [0_u8; AGENT_IDENTITY_KEY_SEED_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut seed_material);
    let mut digest = Sha512::new();
    digest.update(AGENT_IDENTITY_KEY_DERIVATION_CONTEXT);
    digest.update(seed_material);
    let digest = digest.finalize();
    let mut signing_key_bytes = [0_u8; 32];
    signing_key_bytes.copy_from_slice(&digest[..32]);
    let signing_key = SigningKey::from_bytes(&signing_key_bytes);
    let private_key = signing_key
        .to_pkcs8_der()
        .map_err(|err| format!("failed to encode agent identity private key: {err}"))?;
    Ok(GeneratedAgentKeyMaterial {
        private_key_pkcs8_base64: BASE64_STANDARD.encode(private_key.as_bytes()),
        public_key_ssh: encode_ssh_ed25519_public_key(&signing_key.verifying_key()),
    })
}

fn encode_ssh_ed25519_public_key(verifying_key: &VerifyingKey) -> String {
    let mut blob = Vec::with_capacity(4 + 11 + 4 + 32);
    append_ssh_string(&mut blob, b"ssh-ed25519");
    append_ssh_string(&mut blob, verifying_key.as_bytes());
    format!("ssh-ed25519 {}", BASE64_STANDARD.encode(blob))
}

fn append_ssh_string(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u32).to_be_bytes());
    buffer.extend_from_slice(value);
}

fn token_chatgpt_account_is_fedramp(token: &str) -> bool {
    let payload = token.split('.').nth(1).and_then(|payload| {
        URL_SAFE_NO_PAD
            .decode(payload)
            .ok()
            .and_then(|decoded| serde_json::from_slice::<serde_json::Value>(&decoded).ok())
    });
    let Some(payload) = payload else {
        return false;
    };
    payload
        .get("chatgpt_account_is_fedramp")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            payload
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_is_fedramp"))
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false)
}

fn access_token_digest(access_token: &str) -> [u8; 64] {
    let mut digest = Sha512::new();
    digest.update(b"agent-identity-registration\0");
    digest.update(access_token.as_bytes());
    digest.finalize().into()
}

fn agent_identity_material_digest(identity: &AccountAgentIdentity) -> [u8; 64] {
    let mut digest = Sha512::new();
    digest.update(b"agent-task-registration\0");
    digest.update(identity.agent_runtime_id.as_bytes());
    digest.update(b"\0");
    digest.update(identity.agent_private_key.as_bytes());
    digest.finalize().into()
}

fn bootstrap_failure_key(account_id: &str, operation: &str) -> String {
    format!("{account_id}\0{operation}")
}

fn bootstrap_failure_is_active(
    account_id: &str,
    operation: &str,
    material_digest: [u8; 64],
) -> bool {
    let failures = ACCOUNT_AGENT_BOOTSTRAP_FAILURES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut failures = crate::lock_utils::lock_recover(failures, "agent_bootstrap_failures");
    let ttl = Duration::from_secs(AGENT_IDENTITY_BOOTSTRAP_FAILURE_TTL_SECS);
    failures.retain(|_, failure| failure.failed_at.elapsed() < ttl);
    failures
        .get(&bootstrap_failure_key(account_id, operation))
        .is_some_and(|failure| failure.material_digest == material_digest)
}

fn remember_bootstrap_failure(account_id: &str, operation: &str, material_digest: [u8; 64]) {
    let failures = ACCOUNT_AGENT_BOOTSTRAP_FAILURES.get_or_init(|| Mutex::new(HashMap::new()));
    crate::lock_utils::lock_recover(failures, "agent_bootstrap_failures").insert(
        bootstrap_failure_key(account_id, operation),
        BootstrapFailure {
            failed_at: Instant::now(),
            material_digest,
        },
    );
}

fn clear_bootstrap_failure(account_id: &str, operation: &str, material_digest: [u8; 64]) {
    let failures = ACCOUNT_AGENT_BOOTSTRAP_FAILURES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut failures = crate::lock_utils::lock_recover(failures, "agent_bootstrap_failures");
    let key = bootstrap_failure_key(account_id, operation);
    if failures
        .get(&key)
        .is_some_and(|failure| failure.material_digest == material_digest)
    {
        failures.remove(&key);
    }
}

fn register_agent_identity_task(
    client: &reqwest::blocking::Client,
    identity: &AccountAgentIdentity,
    authapi_base_url: &str,
) -> Result<String, String> {
    validate_agent_identity(identity)?;
    let runtime_id = required_value(&identity.agent_runtime_id, "agent_runtime_id")?;
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signing_key = signing_key_from_pkcs8_base64(&identity.agent_private_key)?;
    let payload = format!("{runtime_id}:{timestamp}");
    let request = RegisterAgentTaskRequest {
        timestamp,
        signature: BASE64_STANDARD.encode(signing_key.sign(payload.as_bytes()).to_bytes()),
    };
    let url = format!(
        "{}/v1/agent/{}/task/register",
        authapi_base_url.trim_end_matches('/'),
        runtime_id
    );
    let response = client
        .post(&url)
        .timeout(AGENT_TASK_REGISTRATION_TIMEOUT)
        // The official Codex auth client applies the same identity headers to
        // task registration as to runtime registration.
        .header("originator", crate::gateway::current_wire_originator())
        .header("User-Agent", crate::gateway::current_codex_user_agent())
        .json(&request)
        .send()
        .map_err(|err| format!("agent task registration request failed: {err}"))?;
    let status = response.status();
    let mut body = Vec::new();
    response
        .take(AGENT_TASK_REGISTRATION_RESPONSE_LIMIT + 1)
        .read_to_end(&mut body)
        .map_err(|err| format!("read agent task registration response failed: {err}"))?;
    if body.len() as u64 > AGENT_TASK_REGISTRATION_RESPONSE_LIMIT {
        return Err("agent task registration response exceeded 64 KiB".to_string());
    }
    if !status.is_success() {
        return Err(format!(
            "agent task registration returned status {}",
            status.as_u16()
        ));
    }
    let response: RegisterAgentTaskResponse = serde_json::from_slice(&body)
        .map_err(|err| format!("agent task registration response is invalid: {err}"))?;
    task_id_from_registration_response(identity, response)
}

fn task_id_from_registration_response(
    identity: &AccountAgentIdentity,
    response: RegisterAgentTaskResponse,
) -> Result<String, String> {
    if let Some(task_id) = response
        .task_id
        .or(response.task_id_camel)
        .and_then(non_empty_owned)
    {
        return Ok(task_id);
    }
    let encrypted_task_id = response
        .encrypted_task_id
        .or(response.encrypted_task_id_camel)
        .and_then(non_empty_owned)
        .ok_or_else(|| "agent task registration response omitted task id".to_string())?;
    decrypt_agent_task_id(identity, &encrypted_task_id)
}

fn decrypt_agent_task_id(
    identity: &AccountAgentIdentity,
    encrypted_task_id: &str,
) -> Result<String, String> {
    let signing_key = signing_key_from_pkcs8_base64(&identity.agent_private_key)?;
    let ciphertext = BASE64_STANDARD
        .decode(encrypted_task_id.trim())
        .map_err(|err| format!("encrypted agent task id is not valid base64: {err}"))?;
    let plaintext = curve25519_secret_key_from_signing_key(&signing_key)
        .unseal(&ciphertext)
        .map_err(|_| "failed to decrypt encrypted agent task id".to_string())?;
    let task_id = String::from_utf8(plaintext)
        .map_err(|err| format!("decrypted agent task id is not valid UTF-8: {err}"))?;
    non_empty_owned(task_id).ok_or_else(|| "decrypted agent task id is empty".to_string())
}

fn curve25519_secret_key_from_signing_key(signing_key: &SigningKey) -> Curve25519SecretKey {
    let digest = Sha512::digest(signing_key.to_bytes());
    let mut secret_key = [0_u8; 32];
    secret_key.copy_from_slice(&digest[..32]);
    secret_key[0] &= 248;
    secret_key[31] &= 127;
    secret_key[31] |= 64;
    Curve25519SecretKey::from(secret_key)
}

fn non_empty_owned(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn non_empty_borrowed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(crate) fn is_agent_identity_task_invalid_response(status: u16, body: &[u8]) -> bool {
    if status != 401 {
        return false;
    }
    let lower = String::from_utf8_lossy(body).to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    [
        r#""code":"invalid_task_id""#,
        r#""code":"task_not_found""#,
        r#""code":"task_expired""#,
        r#""error":"invalid_task_id""#,
    ]
    .iter()
    .any(|marker| compact.contains(marker))
        || [
            "invalid_task_id",
            "task_not_found",
            "task_expired",
            "invalid task_id",
            "invalid task id",
            "task_id is invalid",
            "task id is invalid",
            "task not found",
            "task expired",
            "unknown task_id",
            "unknown task id",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
}

pub(crate) fn is_agent_identity_task_invalid_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let unauthorized = lower.contains("status=401")
        || lower.contains("status 401")
        || lower.contains("401 unauthorized");
    unauthorized && is_agent_identity_task_invalid_response(401, message.as_bytes())
}

fn authorization_header_for_agent_identity_at(
    identity: &AccountAgentIdentity,
    timestamp: &str,
) -> Result<String, String> {
    let runtime_id = required_value(&identity.agent_runtime_id, "agent_runtime_id")?;
    let task_id = identity
        .task_id
        .as_deref()
        .ok_or_else(|| "agent identity task_id is empty".to_string())?;
    let task_id = required_value(task_id, "task_id")?;
    let signing_key = signing_key_from_pkcs8_base64(&identity.agent_private_key)?;
    let signed_payload = format!("{runtime_id}:{task_id}:{timestamp}");
    let signature = BASE64_STANDARD.encode(signing_key.sign(signed_payload.as_bytes()).to_bytes());
    let envelope = BTreeMap::from([
        ("agent_runtime_id", runtime_id),
        ("signature", signature.as_str()),
        ("task_id", task_id),
        ("timestamp", timestamp),
    ]);
    let serialized = serde_json::to_vec(&envelope)
        .map_err(|err| format!("failed to serialize agent assertion: {err}"))?;
    Ok(format!(
        "{AGENT_ASSERTION_SCHEME} {}",
        URL_SAFE_NO_PAD.encode(serialized)
    ))
}

pub(crate) fn format_upstream_authorization(auth_token: &str) -> String {
    let trimmed = auth_token.trim();
    if trimmed.starts_with(&format!("{AGENT_ASSERTION_SCHEME} ")) {
        trimmed.to_string()
    } else {
        format!("Bearer {trimmed}")
    }
}

fn required_value<'a>(value: &'a str, field: &str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("agent identity {field} is empty"))
    } else {
        Ok(trimmed)
    }
}

fn signing_key_from_pkcs8_base64(private_key: &str) -> Result<SigningKey, String> {
    let private_key = BASE64_STANDARD
        .decode(private_key.trim())
        .map_err(|err| format!("agent identity private key is not valid base64: {err}"))?;
    SigningKey::from_pkcs8_der(&private_key)
        .map_err(|err| format!("agent identity private key is not valid PKCS#8: {err}"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
    use base64::Engine as _;
    use codexmanager_core::storage::{now_ts, Account};
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use ed25519_dalek::{Signature, SigningKey, Verifier as _};
    use tiny_http::{Response, Server, StatusCode};

    use super::*;

    fn identity() -> (AccountAgentIdentity, SigningKey) {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let private_key = signing_key.to_pkcs8_der().expect("encode private key");
        let now = now_ts();
        (
            AccountAgentIdentity {
                account_id: "account-1".to_string(),
                agent_runtime_id: "agent-runtime-1".to_string(),
                agent_private_key: BASE64_STANDARD.encode(private_key.as_bytes()),
                task_id: Some("task-1".to_string()),
                chatgpt_user_id: "user-1".to_string(),
                chatgpt_account_is_fedramp: false,
                auth_mode: "agentIdentity".to_string(),
                workspace_id: Some("workspace-1".to_string()),
                created_at: now,
                updated_at: now,
            },
            signing_key,
        )
    }

    fn insert_identity(storage: &Storage, identity: &AccountAgentIdentity) {
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: identity.account_id.clone(),
                label: identity.account_id.clone(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some("workspace-1".to_string()),
                workspace_id: Some("workspace-1".to_string()),
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .upsert_account_agent_identity(identity)
            .expect("insert identity");
    }

    fn jwt_with_chatgpt_identity(user_id: &str, account_id: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::json!({
                "sub": user_id,
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": user_id,
                    "chatgpt_account_id": account_id
                }
            })
            .to_string(),
        );
        format!("{header}.{payload}.signature")
    }

    fn request_header(request: &tiny_http::Request, name: &str) -> Option<String> {
        request.headers().iter().find_map(|header| {
            header
                .field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case(name)
                .then(|| header.value.as_str().to_string())
        })
    }

    #[test]
    fn agent_assertion_matches_codex_agent_identity_wire_format() {
        let (identity, signing_key) = identity();
        let timestamp = "2026-07-21T12:00:00Z";
        let header = authorization_header_for_agent_identity_at(&identity, timestamp)
            .expect("build agent assertion");
        let encoded = header
            .strip_prefix("AgentAssertion ")
            .expect("agent assertion scheme");
        let envelope: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).expect("decode assertion"))
                .expect("parse assertion");

        assert_eq!(envelope["agent_runtime_id"], "agent-runtime-1");
        assert_eq!(envelope["task_id"], "task-1");
        assert_eq!(envelope["timestamp"], timestamp);
        let signature_bytes = BASE64_STANDARD
            .decode(envelope["signature"].as_str().expect("signature"))
            .expect("decode signature");
        let signature = Signature::from_slice(&signature_bytes).expect("parse signature");
        signing_key
            .verifying_key()
            .verify(
                format!("agent-runtime-1:task-1:{timestamp}").as_bytes(),
                &signature,
            )
            .expect("verify signature");
    }

    #[test]
    fn upstream_authorization_preserves_agent_assertion_and_wraps_bearer() {
        assert_eq!(
            format_upstream_authorization("AgentAssertion encoded"),
            "AgentAssertion encoded"
        );
        assert_eq!(
            format_upstream_authorization("access-token"),
            "Bearer access-token"
        );
    }

    #[test]
    fn bearer_account_bootstrap_registers_and_persists_agent_identity() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let account = Account {
            id: "account-bootstrap".to_string(),
            label: "bootstrap@example.com".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("workspace-bootstrap".to_string()),
            workspace_id: Some("workspace-bootstrap".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        storage.insert_account(&account).expect("insert account");
        let access_token = jwt_with_chatgpt_identity("user-bootstrap", "workspace-bootstrap");
        let token = Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: access_token.clone(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now,
        };

        let server = Server::http("127.0.0.1:0").expect("start registration server");
        let base_url = format!("http://{}", server.server_addr());
        let (request_tx, request_rx) = mpsc::channel();
        let server_handle = thread::spawn(move || {
            for response_body in [
                r#"{"agent_runtime_id":"runtime-bootstrap"}"#,
                r#"{"task_id":"task-bootstrap"}"#,
            ] {
                let mut request = server
                    .recv_timeout(Duration::from_secs(5))
                    .expect("registration server timeout")
                    .expect("registration request");
                let path = request.url().to_string();
                let authorization = request_header(&request, "authorization");
                let originator = request_header(&request, "originator");
                let user_agent = request_header(&request, "user-agent");
                let mut body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut body)
                    .expect("read registration request");
                request_tx
                    .send((path, authorization, originator, user_agent, body))
                    .expect("record request");
                request
                    .respond(
                        Response::from_string(response_body)
                            .with_status_code(StatusCode(200))
                            .with_header(
                                tiny_http::Header::from_bytes("Content-Type", "application/json")
                                    .expect("content type"),
                            ),
                    )
                    .expect("respond registration request");
            }
        });

        let authorization =
            resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
                &storage,
                &reqwest::blocking::Client::new(),
                &account,
                &token,
                &base_url,
                None,
            )
            .expect("bootstrap identity")
            .expect("identity authorization");
        assert_eq!(authorization.task_id, "task-bootstrap");
        assert_eq!(
            authorization.account_scope_id.as_deref(),
            Some("workspace-bootstrap")
        );
        assert!(authorization.value.starts_with("AgentAssertion "));

        let (
            registration_path,
            registration_auth,
            registration_originator,
            registration_user_agent,
            registration_body,
        ) = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive identity registration");
        let (task_path, task_auth, _task_originator, _task_user_agent, _task_body) = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive task registration");
        server_handle.join().expect("join registration server");
        assert_eq!(registration_path, "/v1/agent/register");
        assert_eq!(
            registration_auth.as_deref(),
            Some(format!("Bearer {access_token}").as_str())
        );
        assert_eq!(
            registration_originator.as_deref(),
            Some(crate::gateway::current_wire_originator().as_str())
        );
        assert_eq!(
            registration_user_agent.as_deref(),
            Some(crate::gateway::current_codex_user_agent().as_str())
        );
        let registration_body: serde_json::Value =
            serde_json::from_str(&registration_body).expect("parse registration body");
        assert_eq!(
            registration_body["capabilities"],
            serde_json::json!(["responsesapi"])
        );
        assert_eq!(registration_body["abom"]["agent_harness_id"], "codex-cli");
        assert!(registration_body["agent_public_key"]
            .as_str()
            .is_some_and(|value| value.starts_with("ssh-ed25519 ")));
        assert_eq!(task_path, "/v1/agent/runtime-bootstrap/task/register");
        assert!(task_auth.is_none());

        let stored = storage
            .find_account_agent_identity(&account.id)
            .expect("load identity")
            .expect("stored identity");
        assert_eq!(stored.agent_runtime_id, "runtime-bootstrap");
        assert_eq!(stored.task_id.as_deref(), Some("task-bootstrap"));
        assert_eq!(stored.chatgpt_user_id, "user-bootstrap");
        validate_agent_identity(&stored).expect("valid stored identity");
    }

    #[test]
    fn bearer_account_bootstrap_skips_tokens_without_chatgpt_binding() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        let account = Account {
            id: "account-no-binding".to_string(),
            label: "no-binding".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        storage.insert_account(&account).expect("insert account");
        let token = Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: "opaque-token".to_string(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now,
        };

        assert!(
            resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
                &storage,
                &reqwest::blocking::Client::new(),
                &account,
                &token,
                "http://127.0.0.1:9",
                None,
            )
            .expect("skip unsupported token")
            .is_none()
        );
    }

    #[test]
    fn stored_identity_must_match_current_token_user_and_scope() {
        let (identity, _) = identity();
        let now = now_ts();
        let account = Account {
            id: identity.account_id.clone(),
            label: "binding@example.com".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("workspace-1".to_string()),
            workspace_id: Some("workspace-1".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };
        let token = |user_id: &str, scope_id: &str| Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: jwt_with_chatgpt_identity(user_id, scope_id),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now,
        };

        assert!(agent_identity_matches_binding(
            &identity,
            &resolve_agent_identity_binding(&account, &token("user-1", "workspace-1"))
        ));
        assert!(!agent_identity_matches_binding(
            &identity,
            &resolve_agent_identity_binding(&account, &token("user-2", "workspace-1"))
        ));
        assert!(!agent_identity_matches_binding(
            &identity,
            &resolve_agent_identity_binding(&account, &token("user-1", "workspace-2"))
        ));
    }

    #[test]
    fn failed_missing_task_registration_is_cooled_down() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let (mut identity, _) = identity();
        identity.account_id = "account-task-cooldown".to_string();
        identity.task_id = None;
        insert_identity(&storage, &identity);
        let account = storage
            .find_account_by_id(&identity.account_id)
            .expect("find account")
            .expect("account");
        let token = Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: jwt_with_chatgpt_identity("user-1", "workspace-1"),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        };

        let server = Server::http("127.0.0.1:0").expect("start task registration server");
        let base_url = format!("http://{}", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let server_handle = thread::spawn(move || {
            while let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(300)) {
                server_requests.fetch_add(1, Ordering::SeqCst);
                request
                    .respond(Response::empty(StatusCode(503)))
                    .expect("respond task registration");
            }
        });

        let first = resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
            &storage,
            &reqwest::blocking::Client::new(),
            &account,
            &token,
            &base_url,
            None,
        )
        .expect_err("first task registration must fail");
        assert!(first.contains("status 503"));
        let second = resolve_or_bootstrap_account_agent_identity_authorization_with_base_url(
            &storage,
            &reqwest::blocking::Client::new(),
            &account,
            &token,
            &base_url,
            None,
        )
        .expect_err("second task registration must be cooled down");
        assert!(second.contains("cooling down"));

        server_handle.join().expect("join registration server");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(storage
            .find_account_agent_identity(&account.id)
            .expect("find identity")
            .expect("identity")
            .task_id
            .is_none());
    }

    #[test]
    fn missing_task_is_registered_and_persisted_before_assertion() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let (mut identity, _) = identity();
        identity.account_id = "account-missing-task".to_string();
        identity.task_id = None;
        insert_identity(&storage, &identity);

        let authorization = resolve_account_agent_identity_authorization_with(
            &storage,
            &identity.account_id,
            None,
            |_| Ok("task-registered".to_string()),
        )
        .expect("resolve identity")
        .expect("identity authorization");

        assert_eq!(authorization.task_id, "task-registered");
        assert!(authorization.value.starts_with("AgentAssertion "));
        assert_eq!(
            storage
                .find_account_agent_identity(&identity.account_id)
                .expect("load identity")
                .expect("identity")
                .task_id
                .as_deref(),
            Some("task-registered")
        );
    }

    #[test]
    fn expired_task_is_replaced_once_and_stale_recovery_reuses_replacement() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let (mut identity, _) = identity();
        identity.account_id = "account-expired-task".to_string();
        insert_identity(&storage, &identity);
        let calls = AtomicUsize::new(0);

        let recovered = resolve_account_agent_identity_authorization_with(
            &storage,
            &identity.account_id,
            Some("task-1"),
            |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("task-2".to_string())
            },
        )
        .expect("recover task")
        .expect("identity authorization");
        assert_eq!(recovered.task_id, "task-2");

        let reused = resolve_account_agent_identity_authorization_with(
            &storage,
            &identity.account_id,
            Some("task-1"),
            |_| panic!("stale recovery must reuse the persisted replacement"),
        )
        .expect("reuse replacement")
        .expect("identity authorization");
        assert_eq!(reused.task_id, "task-2");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_missing_task_registration_runs_only_once() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "codexmanager-agent-task-{}-{suffix}.sqlite",
            std::process::id()
        ));
        let account_id = format!("account-concurrent-task-{suffix}");
        {
            let storage = Storage::open(&db_path).expect("open storage");
            storage.init().expect("init storage");
            let (mut identity, _) = identity();
            identity.account_id = account_id.clone();
            identity.task_id = None;
            insert_identity(&storage, &identity);
        }

        let worker_count = 6;
        let barrier = Arc::new(Barrier::new(worker_count));
        let registrations = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..worker_count {
            let db_path = db_path.clone();
            let account_id = account_id.clone();
            let barrier = Arc::clone(&barrier);
            let registrations = Arc::clone(&registrations);
            workers.push(thread::spawn(move || {
                let storage = Storage::open(db_path).expect("open worker storage");
                barrier.wait();
                resolve_account_agent_identity_authorization_with(
                    &storage,
                    &account_id,
                    None,
                    |_| {
                        registrations.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(40));
                        Ok("task-concurrent".to_string())
                    },
                )
                .expect("resolve concurrent identity")
                .expect("identity authorization")
                .task_id
            }));
        }
        for worker in workers {
            assert_eq!(worker.join().expect("join worker"), "task-concurrent");
        }
        assert_eq!(registrations.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
    }

    #[test]
    fn task_registration_uses_expected_path_and_signed_payload() {
        let server = Server::http("127.0.0.1:0").expect("start task registration server");
        let base_url = format!("http://{}", server.server_addr());
        let (request_tx, request_rx) = mpsc::channel();
        let server_handle = thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("registration server timeout")
                .expect("registration request");
            let path = request.url().to_string();
            let originator = request_header(&request, "originator");
            let user_agent = request_header(&request, "user-agent");
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .expect("read registration request");
            request_tx
                .send((path, originator, user_agent, body))
                .expect("record request");
            request
                .respond(
                    Response::from_string(r#"{"taskId":"task-from-server"}"#)
                        .with_status_code(StatusCode(200))
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", "application/json")
                                .expect("content type"),
                        ),
                )
                .expect("respond registration request");
        });
        let (identity, signing_key) = identity();
        let task_id =
            register_agent_identity_task(&reqwest::blocking::Client::new(), &identity, &base_url)
                .expect("register task");
        assert_eq!(task_id, "task-from-server");
        let (path, originator, user_agent, body) = request_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive request");
        server_handle.join().expect("join server");
        assert_eq!(path, "/v1/agent/agent-runtime-1/task/register");
        assert_eq!(
            originator.as_deref(),
            Some(crate::gateway::current_wire_originator().as_str())
        );
        assert_eq!(
            user_agent.as_deref(),
            Some(crate::gateway::current_codex_user_agent().as_str())
        );
        let body: serde_json::Value = serde_json::from_str(&body).expect("parse request body");
        let timestamp = body["timestamp"].as_str().expect("timestamp");
        let signature = BASE64_STANDARD
            .decode(body["signature"].as_str().expect("signature"))
            .expect("decode signature");
        signing_key
            .verifying_key()
            .verify(
                format!("agent-runtime-1:{timestamp}").as_bytes(),
                &Signature::from_slice(&signature).expect("parse signature"),
            )
            .expect("verify registration signature");
    }

    #[test]
    fn task_registration_http_error_does_not_echo_response_body() {
        let server = Server::http("127.0.0.1:0").expect("start task registration server");
        let base_url = format!("http://{}", server.server_addr());
        let server_handle = thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("registration server timeout")
                .expect("registration request");
            request
                .respond(
                    Response::from_string(r#"{"signature":"must-not-leak"}"#)
                        .with_status_code(StatusCode(401)),
                )
                .expect("respond registration request");
        });
        let (identity, _) = identity();
        let error =
            register_agent_identity_task(&reqwest::blocking::Client::new(), &identity, &base_url)
                .expect_err("registration should fail");
        server_handle.join().expect("join server");

        assert_eq!(error, "agent task registration returned status 401");
        assert!(!error.contains("must-not-leak"));
    }

    #[test]
    fn encrypted_task_registration_response_is_decrypted() {
        let (identity, signing_key) = identity();
        let secret = curve25519_secret_key_from_signing_key(&signing_key);
        let encrypted = secret
            .public_key()
            .seal(&mut rand::rngs::OsRng, b"task-encrypted")
            .expect("seal task id");
        let task_id = task_id_from_registration_response(
            &identity,
            RegisterAgentTaskResponse {
                task_id: None,
                task_id_camel: None,
                encrypted_task_id: Some(BASE64_STANDARD.encode(encrypted)),
                encrypted_task_id_camel: None,
            },
        )
        .expect("decrypt task id");
        assert_eq!(task_id, "task-encrypted");
    }

    #[test]
    fn invalid_task_detection_requires_unauthorized_task_failure() {
        assert!(is_agent_identity_task_invalid_response(
            401,
            br#"{"error":{"code":"task_expired"}}"#,
        ));
        assert!(is_agent_identity_task_invalid_error(
            "usage endpoint failed: status=401 body=task not found"
        ));
        assert!(!is_agent_identity_task_invalid_response(
            403,
            br#"{"code":"invalid_task_id"}"#,
        ));
        assert!(!is_agent_identity_task_invalid_error(
            "usage endpoint failed: status=500 body=task expired"
        ));
    }
}
