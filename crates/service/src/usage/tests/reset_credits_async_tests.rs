use super::*;
use axum::{http::StatusCode, response::IntoResponse, routing::any, Json, Router};
use codexmanager_core::storage::{now_ts, Account};
use std::ffi::OsString;
use std::time::Duration;

struct Restore(Vec<(&'static str, Option<OsString>)>);
impl Restore {
    fn set(&mut self, key: &'static str, value: Option<&str>) {
        self.0.push((key, std::env::var_os(key)));
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        crate::usage_http::reload_usage_http_client_from_env();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_redemption_cannot_start_another_charge() {
    let _env = crate::test_env_guard();
    let dir = std::env::temp_dir().join(format!("reset-async-{}", random_uuid_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut restore = Restore(Vec::new());
    restore.set("CODEXMANAGER_DATABASE_URL", None);
    restore.set("CODEXMANAGER_STORAGE_BACKEND", Some("sqlite"));
    restore.set(
        "CODEXMANAGER_DB_PATH",
        Some(dir.join("fixture.db").to_str().unwrap()),
    );
    restore.set("CODEXMANAGER_UPSTREAM_PROXY_URL", Some(""));
    restore.set("NO_PROXY", Some("*"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    restore.set(
        "CODEXMANAGER_USAGE_BASE_URL",
        Some(&format!("http://{}", listener.local_addr().unwrap())),
    );
    crate::usage_http::reload_usage_http_client_from_env();
    crate::storage_helpers::initialize_storage().unwrap();
    let storage = open_storage().unwrap();
    let id = format!("reset-cancel-{}", random_uuid_v4());
    storage
        .insert_account(&Account {
            id: id.clone(),
            label: "cancel fixture".into(),
            issuer: "fixture".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .unwrap();
    storage
        .insert_token(&Token {
            account_id: id.clone(),
            id_token: String::new(),
            access_token: "fixture-access".into(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .unwrap();
    drop(storage);
    let posted = Arc::new(tokio::sync::Notify::new());
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new().fallback(any({
        let posted = posted.clone();
        let count = count.clone();
        move |method: axum::http::Method| {
            let posted = posted.clone();
            let count = count.clone();
            async move {
                if method == axum::http::Method::POST {
                    count.fetch_add(1, Ordering::SeqCst);
                    posted.notify_one();
                    std::future::pending::<()>().await;
                }
                Json(serde_json::json!({"available_count":1}))
            }
        }
    }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let operation_id = random_uuid_v4();
    let work = tokio::spawn({
        let id = id.clone();
        let operation_id = operation_id.clone();
        async move { consume_reset_credit_async(&id, &operation_id).await }
    });
    tokio::time::timeout(Duration::from_secs(5), posted.notified())
        .await
        .expect("provider sees redemption");
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let error = consume_reset_credit_async(&id, &random_uuid_v4())
        .await
        .unwrap_err();
    assert!(
        error.contains(RESET_CREDIT_RESULT_UNKNOWN_MESSAGE),
        "unexpected cancellation outcome: {error}"
    );
    assert!(
        error.contains(&format!(
            "{RESET_CREDIT_PENDING_OPERATION_PREFIX}{operation_id}"
        )),
        "pending operation id must be recoverable: {error}"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_operation_replay_returns_saved_result_without_another_charge() {
    let _env = crate::test_env_guard();
    let dir = std::env::temp_dir().join(format!("reset-replay-{}", random_uuid_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut restore = Restore(Vec::new());
    restore.set("CODEXMANAGER_DATABASE_URL", None);
    restore.set("CODEXMANAGER_STORAGE_BACKEND", Some("sqlite"));
    restore.set(
        "CODEXMANAGER_DB_PATH",
        Some(dir.join("fixture.db").to_str().unwrap()),
    );
    restore.set("CODEXMANAGER_UPSTREAM_PROXY_URL", Some(""));
    restore.set("NO_PROXY", Some("*"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    restore.set(
        "CODEXMANAGER_USAGE_BASE_URL",
        Some(&format!("http://{}", listener.local_addr().unwrap())),
    );
    crate::usage_http::reload_usage_http_client_from_env();
    crate::storage_helpers::initialize_storage().unwrap();
    let storage = open_storage().unwrap();
    let id = format!("reset-replay-{}", random_uuid_v4());
    storage
        .insert_account(&Account {
            id: id.clone(),
            label: "replay fixture".into(),
            issuer: "fixture".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .unwrap();
    storage
        .insert_token(&Token {
            account_id: id.clone(),
            id_token: String::new(),
            access_token: "fixture-access".into(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .unwrap();
    drop(storage);

    let post_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new().fallback(any({
        let post_count = post_count.clone();
        move |method: axum::http::Method| {
            let post_count = post_count.clone();
            async move {
                if method == axum::http::Method::POST {
                    post_count.fetch_add(1, Ordering::SeqCst);
                }
                Json(serde_json::json!({"available_count":1}))
            }
        }
    }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let operation_id = random_uuid_v4();
    let first = consume_reset_credit_async(&id, &operation_id)
        .await
        .expect("first redemption");
    let replay = consume_reset_credit_async(&id, &operation_id)
        .await
        .expect("replay saved result");

    assert!(first.consumed);
    assert_eq!(replay, first);
    assert_eq!(post_count.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_operation_replay_reuses_redeem_id_after_ambiguous_provider_error() {
    let _env = crate::test_env_guard();
    let dir = std::env::temp_dir().join(format!("reset-pending-replay-{}", random_uuid_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut restore = Restore(Vec::new());
    restore.set("CODEXMANAGER_DATABASE_URL", None);
    restore.set("CODEXMANAGER_STORAGE_BACKEND", Some("sqlite"));
    restore.set(
        "CODEXMANAGER_DB_PATH",
        Some(dir.join("fixture.db").to_str().unwrap()),
    );
    restore.set("CODEXMANAGER_UPSTREAM_PROXY_URL", Some(""));
    restore.set("NO_PROXY", Some("*"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    restore.set(
        "CODEXMANAGER_USAGE_BASE_URL",
        Some(&format!("http://{}", listener.local_addr().unwrap())),
    );
    crate::usage_http::reload_usage_http_client_from_env();
    crate::storage_helpers::initialize_storage().unwrap();
    let storage = open_storage().unwrap();
    let id = format!("reset-pending-replay-{}", random_uuid_v4());
    storage
        .insert_account(&Account {
            id: id.clone(),
            label: "pending replay fixture".into(),
            issuer: "fixture".into(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".into(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .unwrap();
    storage
        .insert_token(&Token {
            account_id: id.clone(),
            id_token: String::new(),
            access_token: "fixture-access".into(),
            refresh_token: String::new(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .unwrap();
    drop(storage);

    let post_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new().fallback(any({
        let post_count = post_count.clone();
        move |method: axum::http::Method| {
            let post_count = post_count.clone();
            async move {
                if method == axum::http::Method::POST {
                    let count = post_count.fetch_add(1, Ordering::SeqCst) + 1;
                    if count == 1 {
                        return (
                            StatusCode::BAD_GATEWAY,
                            Json(serde_json::json!({"error":"fixture upstream reset"})),
                        )
                            .into_response();
                    }
                }
                Json(serde_json::json!({"available_count":1})).into_response()
            }
        }
    }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let operation_id = random_uuid_v4();
    let first_error = consume_reset_credit_async(&id, &operation_id)
        .await
        .unwrap_err();
    assert!(first_error.contains(RESET_CREDIT_RESULT_UNKNOWN_MESSAGE));
    let storage = open_storage().unwrap();
    let operation = storage
        .get_reset_credit_operation(&operation_id)
        .unwrap()
        .expect("pending operation");
    assert_eq!(operation.status, ResetCreditOperationStatus::Pending);
    let redeem_request_id = operation.redeem_request_id.clone();
    drop(storage);

    let replay = consume_reset_credit_async(&id, &operation_id)
        .await
        .expect("same operation can be reconciled");
    assert!(replay.consumed);
    let storage = open_storage().unwrap();
    let completed = storage
        .get_reset_credit_operation(&operation_id)
        .unwrap()
        .expect("completed operation");
    assert_eq!(completed.status, ResetCreditOperationStatus::Completed);
    assert_eq!(completed.redeem_request_id, redeem_request_id);
    assert_eq!(post_count.load(Ordering::SeqCst), 2);
    server.abort();
    let _ = server.await;
}
