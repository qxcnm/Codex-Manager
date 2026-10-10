use super::*;

pub(crate) async fn resolve_or_bootstrap_account_agent_identity_authorization_async(
    storage: &Storage,
    client: &reqwest::Client,
    account: &Account,
    token: &Token,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    resolve_with_base_url(
        storage,
        client,
        account,
        token,
        AGENT_IDENTITY_AUTHAPI_BASE_URL,
        None,
    )
    .await
}

pub(crate) async fn recover_account_agent_identity_authorization_async(
    storage: &Storage,
    client: &reqwest::Client,
    account: &Account,
    token: &Token,
    failed_task_id: &str,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    let failed = required_value(failed_task_id, "failed task_id")?;
    resolve_with_base_url(
        storage,
        client,
        account,
        token,
        AGENT_IDENTITY_AUTHAPI_BASE_URL,
        Some(failed),
    )
    .await
}

pub(super) async fn resolve_with_base_url(
    storage: &Storage,
    client: &reqwest::Client,
    account: &Account,
    token: &Token,
    base: &str,
    failed_task_id: Option<&str>,
) -> Result<Option<ResolvedAgentIdentityAuthorization>, String> {
    let binding = resolve_agent_identity_binding(account, token);
    let lock = account_agent_task_lock(&account.id);
    let _guard = crate::http::gateway_request::with_response_cancellation(lock.lock())
        .await
        .map_err(|_| "agent identity authorization cancelled".to_owned())?;
    // Re-read after acquiring the shared async/sync lock. The database bridge
    // yields the executor for each short persistence operation.
    let mut identity = load_account_agent_identity(storage, &account.id)?;
    if !identity.as_ref().is_some_and(|identity| {
        agent_identity_matches_binding(identity, &binding)
            && validate_agent_identity(identity).is_ok()
    }) {
        if binding.access_token.is_empty() {
            return if identity.is_some() {
                Err("stored agent identity does not match the account binding".to_owned())
            } else {
                Ok(None)
            };
        }
        let (Some(user), Some(scope)) = (&binding.chatgpt_user_id, &binding.account_scope_id)
        else {
            return Ok(None);
        };
        let digest = access_token_digest(&binding.access_token);
        if bootstrap_failure_is_active(&account.id, AGENT_IDENTITY_REGISTRATION_OPERATION, digest) {
            return Err(
                "agent identity registration is cooling down after a recent failure".to_owned(),
            );
        }
        let result = async {
            let key = generate_agent_key_material()?;
            let fedramp = token_chatgpt_account_is_fedramp(&binding.access_token)
                || token_chatgpt_account_is_fedramp(&token.id_token);
            let runtime =
                register_agent_identity(client, base, &binding.access_token, fedramp, &key).await?;
            let now = now_ts();
            let identity = AccountAgentIdentity {
                account_id: account.id.clone(),
                agent_runtime_id: runtime,
                agent_private_key: key.private_key_pkcs8_base64,
                task_id: None,
                chatgpt_user_id: user.clone(),
                chatgpt_account_is_fedramp: fedramp,
                auth_mode: "agentIdentity".to_owned(),
                workspace_id: Some(scope.clone()),
                created_at: now,
                updated_at: now,
            };
            validate_agent_identity(&identity)?;
            crate::account::remote_storage::AccountStorage::new(storage)
                .upsert_account_agent_identity(&identity)
                .map_err(|error| format!("persist bootstrapped agent identity failed: {error}"))?;
            Ok::<_, String>(identity)
        }
        .await;
        match result {
            Ok(created) => {
                clear_bootstrap_failure(&account.id, AGENT_IDENTITY_REGISTRATION_OPERATION, digest);
                identity = Some(created);
            }
            Err(error) => {
                if error != AGENT_REGISTRATION_CANCELLED {
                    remember_bootstrap_failure(
                        &account.id,
                        AGENT_IDENTITY_REGISTRATION_OPERATION,
                        digest,
                    );
                }
                return Err(error);
            }
        }
    }
    let mut identity =
        identity.ok_or_else(|| "agent identity disappeared during registration".to_owned())?;
    if task_can_be_reused(&identity, failed_task_id) {
        return apply_binding_scope(resolved_authorization(&identity).map(Some), &binding);
    }
    let digest = agent_identity_material_digest(&identity);
    if bootstrap_failure_is_active(&account.id, AGENT_TASK_REGISTRATION_OPERATION, digest) {
        return Err("agent task registration is cooling down after a recent failure".to_owned());
    }
    let result = async {
        let task = register_agent_identity_task_async(client, &identity, base).await?;
        let task = required_value(&task, "registered task_id")?.to_owned();
        let updated = crate::account::remote_storage::AccountStorage::new(storage)
            .update_account_agent_identity_task_id(
                &account.id,
                &identity.agent_runtime_id,
                &identity.agent_private_key,
                Some(&task),
            )
            .map_err(|error| format!("persist agent identity task failed: {error}"))?;
        if !updated {
            return Err("agent identity disappeared before task persistence".to_owned());
        }
        identity.task_id = Some(task);
        apply_binding_scope(resolved_authorization(&identity).map(Some), &binding)
    }
    .await;
    if result.is_ok() {
        clear_bootstrap_failure(&account.id, AGENT_TASK_REGISTRATION_OPERATION, digest);
    } else if result
        .as_ref()
        .err()
        .is_some_and(|error| error != AGENT_REGISTRATION_CANCELLED)
    {
        remember_bootstrap_failure(&account.id, AGENT_TASK_REGISTRATION_OPERATION, digest);
    }
    result
}
