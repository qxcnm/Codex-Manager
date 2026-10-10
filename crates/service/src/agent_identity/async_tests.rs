use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn cancelled_agent_registration_releases_lock_without_failure_cooldown() {
    let _env = crate::test_env_guard();
    let _ = crate::gateway::current_wire_originator();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    for bootstrap_identity in [true, false] {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let now = now_ts();
        let account = Account {
            id: format!("async-cancel-registration-{bootstrap_identity}"),
            label: "async registration cancellation".to_owned(),
            issuer: "https://auth.openai.com".to_owned(),
            chatgpt_account_id: Some("workspace-async".to_owned()),
            workspace_id: Some("workspace-async".to_owned()),
            group_name: None,
            sort: 0,
            status: "active".to_owned(),
            created_at: now,
            updated_at: now,
        };
        storage.insert_account(&account).unwrap();
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::json!({"https://api.openai.com/auth": {
                "chatgpt_user_id": "user-async",
                "chatgpt_account_id": "workspace-async"
            }})
            .to_string(),
        );
        let token = Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: format!("e30.{payload}.signature"),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now,
        };
        let operation = if bootstrap_identity {
            AGENT_IDENTITY_REGISTRATION_OPERATION
        } else {
            AGENT_TASK_REGISTRATION_OPERATION
        };
        let digest = if bootstrap_identity {
            access_token_digest(&token.access_token)
        } else {
            let key = generate_agent_key_material().unwrap();
            let identity = AccountAgentIdentity {
                account_id: account.id.clone(),
                agent_runtime_id: "runtime-async".to_owned(),
                agent_private_key: key.private_key_pkcs8_base64,
                task_id: None,
                chatgpt_user_id: "user-async".to_owned(),
                chatgpt_account_is_fedramp: false,
                auth_mode: "agentIdentity".to_owned(),
                workspace_id: Some("workspace-async".to_owned()),
                created_at: now,
                updated_at: now,
            };
            storage.upsert_account_agent_identity(&identity).unwrap();
            agent_identity_material_digest(&identity)
        };

        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let started = Arc::new(tokio::sync::Notify::new());
            let requests = Arc::new(AtomicUsize::new(0));
            let app = axum::Router::new().fallback({
                let started = started.clone();
                let requests = requests.clone();
                move |uri: axum::http::Uri| {
                    let started = started.clone();
                    let requests = requests.clone();
                    async move {
                        if requests.fetch_add(1, Ordering::SeqCst) == 0 {
                            started.notify_one();
                            return std::future::pending::<&'static str>().await;
                        }
                        if uri.path() == "/v1/agent/register" {
                            r#"{"agent_runtime_id":"runtime-async"}"#
                        } else {
                            r#"{"task_id":"task-async"}"#
                        }
                    }
                }
            });
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let (cancel, cancelled) = tokio::sync::watch::channel(false);
            let mut registration = Box::pin(crate::http::gateway_request::scope_response_cancellation(
                cancelled,
                async_completion::resolve_with_base_url(&storage, &client, &account, &token, &base, None),
            ));
            tokio::select! {
                _ = started.notified() => {},
                result = &mut registration => panic!("registration completed before cancellation: {result:?}"),
            }
            cancel.send_replace(true);
            let error = tokio::time::timeout(Duration::from_secs(1), registration)
                .await
                .expect("single-worker runtime can cancel pending provider I/O")
                .unwrap_err();
            assert_eq!(error, AGENT_REGISTRATION_CANCELLED);
            assert!(account_agent_task_lock(&account.id).try_lock().is_ok());
            assert!(!bootstrap_failure_is_active(&account.id, operation, digest));

            let authorization = async_completion::resolve_with_base_url(
                &storage, &client, &account, &token, &base, None,
            )
            .await
            .expect("a cancelled attempt must allow immediate retry")
            .unwrap();
            assert_eq!(authorization.task_id, "task-async");
            let stored = storage.find_account_agent_identity(&account.id).unwrap().unwrap();
            assert_eq!(stored.task_id.as_deref(), Some("task-async"));
            assert_eq!(stored.workspace_id.as_deref(), Some("workspace-async"));
            assert_eq!(requests.load(Ordering::SeqCst), if bootstrap_identity { 3 } else { 2 });
            server.abort();
            let _ = server.await;
        });
    }
}
