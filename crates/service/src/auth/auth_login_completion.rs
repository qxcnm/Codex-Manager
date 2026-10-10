//! OAuth completion awaits network I/O directly. The legacy Storage facade is
//! isolated to short, bounded persistence phases; it is never held across HTTP.
use super::*;
use codexmanager_core::storage::LoginSession;

static AUTH_STORAGE_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
static CLEANUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CLEANUPS_CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct CleanupGuard;
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        CLEANUPS.fetch_sub(1, Ordering::AcqRel);
        CLEANUPS_CHANGED.notify_waiters();
    }
}

pub(crate) async fn drain_auth_completions() {
    // A cancelled claim may still be executing on a blocking worker. Its
    // returned guard schedules cleanup even if the receiver has disappeared.
    let workers = AUTH_STORAGE_WORKERS.acquire_many(8).await;
    drop(workers);
    loop {
        let changed = CLEANUPS_CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if CLEANUPS.load(Ordering::Acquire) == 0 {
            return;
        }
        changed.await;
    }
}

pub(crate) async fn run_auth_storage<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let permit = AUTH_STORAGE_WORKERS
        .acquire()
        .await
        .map_err(|_| "auth storage unavailable".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    })
    .await
    .map_err(|_| "auth storage operation interrupted".to_owned())?
}

struct CompletionGuard(Option<LoginSession>);
impl CompletionGuard {
    fn disarm(&mut self) {
        self.0 = None;
    }
}
impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() {
            CLEANUPS.fetch_add(1, Ordering::AcqRel);
            let cleanup = CleanupGuard;
            auth_runtime().spawn(async move {
                let _cleanup = cleanup;
                if let Err(error) = run_auth_storage(move || {
                    finish_failed(&session, "login completion interrupted")
                })
                .await
                {
                    log::warn!("event=oauth_cancel_cleanup_failed error={error}");
                }
            });
        }
    }
}

fn finish_failed(session: &LoginSession, error: &str) -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .finish_claimed_login_session(session, "failed", Some(error))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) async fn complete_login_async(state: &str, code: &str) -> Result<(), String> {
    complete_login_with_redirect_async(state, code, None).await
}

pub(crate) async fn complete_login_with_redirect_async(
    state: &str,
    code: &str,
    redirect_uri: Option<&str>,
) -> Result<(), String> {
    let state = state.to_owned();
    let claim_state = state.clone();
    let requested_redirect = redirect_uri.map(str::to_owned);
    let (session, redirect_uri, guard) = run_auth_storage(move || {
        let storage = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        let storage = crate::account::remote_storage::AccountStorage::new(&storage);
        let session = storage
            .get_login_session(&claim_state)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "unknown login session".to_owned())?;
        if session.status != "pending" {
            return Err(format!(
                "login session is no longer pending (status: {})",
                session.status
            ));
        }
        let redirect = requested_redirect
            .or_else(resolve_redirect_uri)
            .unwrap_or_else(|| "http://localhost:1455/auth/callback".to_owned());
        if !storage
            .claim_login_session_for_completion(&claim_state)
            .map_err(|e| e.to_string())?
        {
            return Err("login session is no longer pending".to_owned());
        }
        Ok((session.clone(), redirect, CompletionGuard(Some(session))))
    })
    .await?;
    let issuer = std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_owned());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_owned());
    let tokens = match exchange_code_for_tokens_async(
        &issuer,
        &client_id,
        &redirect_uri,
        &session.code_verifier,
        code,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            let expected_session = session.clone();
            let message = error.clone();
            let mut guard = guard;
            run_auth_storage(move || {
                finish_failed(&expected_session, &message)?;
                guard.disarm();
                Ok(())
            })
            .await?;
            return Err(error);
        }
    };
    let api_key_access_token = obtain_api_key_async(&issuer, &client_id, &tokens.id_token)
        .await
        .ok();
    run_auth_storage(move || {
        let mut guard = guard;
        let expected_session = session.clone();
        let result = persist_login(
            &state,
            session,
            issuer,
            redirect_uri,
            tokens,
            api_key_access_token,
        );
        if let Err(error) = &result {
            if let Err(status_error) = finish_failed(&expected_session, error) {
                log::warn!("failed to mark login session failed: {status_error}");
            }
        }
        guard.disarm();
        result
    })
    .await
}

fn persist_login(
    state: &str,
    session: LoginSession,
    issuer: String,
    redirect_uri: String,
    tokens: TokenResponse,
    api_key_access_token: Option<String>,
) -> Result<(), String> {
    let storage_handle = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage_handle);
    // A cancelled/expired login must not install account credentials.
    if storage
        .get_login_session(state)
        .map_err(|e| e.to_string())?
        .is_none_or(|current| {
            current.status != "completing"
                || current.code_verifier != session.code_verifier
                || current.state != session.state
                || current.created_at != session.created_at
        })
    {
        return Err("login session terminal state changed before completion".to_owned());
    }
    let claims = parse_id_token_claims(&tokens.id_token)?;
    ensure_workspace_allowed(
        session.workspace_id.as_deref(),
        &claims,
        &tokens.id_token,
        &tokens.access_token,
    )?;

    let subject_account_id = claims.sub.clone();
    let label = claims
        .email
        .clone()
        .unwrap_or_else(|| subject_account_id.clone());
    let claim_chatgpt_account_id = claims
        .auth
        .as_ref()
        .and_then(|auth| normalize_chatgpt_account_id(auth.chatgpt_account_id.as_deref()));
    let claim_workspace_id = normalize_workspace_id(claims.workspace_id.as_deref());
    let chatgpt_account_id = clean_value(
        claim_chatgpt_account_id
            .or_else(|| extract_chatgpt_account_id(&tokens.id_token))
            .or_else(|| extract_chatgpt_account_id(&tokens.access_token)),
    );
    let workspace_id = clean_value(
        claim_workspace_id
            .or_else(|| extract_workspace_id(&tokens.id_token))
            .or_else(|| extract_workspace_id(&tokens.access_token))
            .or_else(|| chatgpt_account_id.clone()),
    );
    let fallback_subject_key =
        build_fallback_subject_key(Some(&subject_account_id), session.tags.as_deref());
    let account_storage_id = build_account_storage_id(
        &subject_account_id,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        session.tags.as_deref(),
    );
    let account_key = resolve_existing_account_for_login(
        &storage,
        &subject_account_id,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        fallback_subject_key.as_deref(),
    )?
    .unwrap_or(account_storage_id);
    let now = now_ts();
    let existing_state = storage
        .find_account_upsert_state_by_id(&account_key)
        .map_err(|err| err.to_string())?;
    let sort = existing_state
        .as_ref()
        .map(|state| state.sort)
        .unwrap_or_else(|| next_account_sort(&storage));
    let created_at = existing_state
        .as_ref()
        .map(|state| state.created_at)
        .unwrap_or(now);
    let workspace_id_for_log = workspace_id.clone();
    let chatgpt_account_id_for_log = chatgpt_account_id.clone();
    let account = Account {
        id: account_key.clone(),
        label,
        issuer: issuer.clone(),
        chatgpt_account_id,
        workspace_id,
        group_name: session.group_name.clone(),
        sort,
        status: "active".to_string(),
        created_at,
        updated_at: now,
    };
    storage
        .insert_account(&account)
        .map_err(|err| err.to_string())?;
    storage
        .update_account_subject_identity(&account_key, &subject_account_id)
        .map_err(|err| err.to_string())?;
    storage
        .upsert_account_metadata(
            &account_key,
            session.note.as_deref(),
            session.tags.as_deref(),
        )
        .map_err(|err| err.to_string())?;

    let token = Token {
        account_id: account_key.clone(),
        id_token: tokens.id_token,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        api_key_access_token,
        last_refresh: now,
    };
    storage
        .insert_token(&token)
        .map_err(|err| err.to_string())?;

    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    log::info!(
        "oauth login persisted account: db_path={} login_id={} account_id={} workspace_id={} chatgpt_account_id={} redirect_uri={}",
        db_path,
        state,
        account_key,
        workspace_id_for_log.as_deref().unwrap_or("-"),
        chatgpt_account_id_for_log.as_deref().unwrap_or("-"),
        redirect_uri
    );

    if !storage
        .finish_claimed_login_session(&session, "success", None)
        .map_err(|e| e.to_string())?
    {
        return Err("login session terminal state changed before completion".to_owned());
    }
    crate::auth_account::set_current_auth_account_id(Some(&account_key))?;
    crate::auth_account::set_current_auth_mode(Some("chatgpt"))?;
    let _ = crate::usage_refresh::enqueue_usage_refresh_after_account_add(&account_key);
    Ok(())
}

#[cfg(test)]
#[path = "tests/auth_login_completion_tests.rs"]
mod tests;
