//! Opt-in persistent local service + remote database acceptance with a deterministic provider.
//! This exercises the production server entrypoint, not a production deployment/provider.
//! Run in separate processes for mysql/postgres via CODEXMANAGER_TEST_GATEWAY_BACKEND;
//! database URLs use the existing CODEXMANAGER_TEST_MYSQL_URL/POSTGRES_URL variables.
#[path = "gateway_logs/support.rs"]
#[allow(dead_code, unused_imports)]
mod support;
use codexmanager_core::storage::{
    AppWallet, ModelPriceTierV2, ModelPriceV2, ModelRouteV2, StorageBackendKind,
};
use codexmanager_storage_seaorm::{
    AccountTokenRecord, AccountTokensRepository, AccountsRepository, ApiKeyDetailsRepository,
    ApiKeyRecord, ApiKeysRepository, AppSetting, BillingRepository, ManagedModelsRepository,
    ModelGroupModelRecord, ModelGroupRecord, ModelGroupsRepository, RequestLogFilter,
    RequestLogsRepository, SeaOrmStorage, SettingsRepository, UserModelGroupRecord,
    UsersRepository,
};
use reqwest::blocking::Client;
use serde_json::Value;
use support::*;
#[path = "remote_gateway/disconnect.rs"]
mod disconnect;

struct PersistentServer {
    addr: String,
    port: u16,
    finished: Receiver<std::io::Result<()>>,
    join: Option<thread::JoinHandle<()>>,
}

impl PersistentServer {
    fn start(client: &Client) -> Self {
        let probe = bind_test_listener("persistent remote gateway");
        let port = probe.local_addr().unwrap().port();
        let v6_probe = TcpListener::bind(("::1", port)).expect("reserve IPv6 listener");
        drop((probe, v6_probe));
        let addr = format!("localhost:{port}");
        let server_addr = addr.clone();
        let (tx, finished) = mpsc::channel();
        codexmanager_service::clear_shutdown_flag();
        let join = thread::spawn(move || {
            let result = codexmanager_service::start_server(&server_addr);
            let _ = tx.send(result);
        });
        let server = Self {
            addr,
            port,
            finished,
            join: Some(join),
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if client
                .get(format!("http://127.0.0.1:{port}/health"))
                .timeout(Duration::from_millis(250))
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                return server;
            }
            if let Ok(result) = server.finished.try_recv() {
                panic!("persistent server exited before readiness: {result:?}");
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("persistent service readiness timed out");
    }

    fn stop(mut self) {
        let started = Instant::now();
        codexmanager_service::request_shutdown(&self.addr);
        self.finished
            .recv_timeout(Duration::from_secs(10))
            .expect("graceful shutdown timeout")
            .expect("clean server result");
        self.join
            .take()
            .unwrap()
            .join()
            .expect("server thread did not panic");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "shutdown exceeded 10 seconds"
        );
        for address in [
            format!("127.0.0.1:{}", self.port),
            format!("[::1]:{}", self.port),
        ] {
            assert!(
                TcpStream::connect(&address).is_err(),
                "listener remains open: {address}"
            );
        }
    }
}

impl Drop for PersistentServer {
    fn drop(&mut self) {
        if self.join.is_some() {
            codexmanager_service::request_shutdown(&self.addr);
            if self.finished.recv_timeout(Duration::from_secs(10)).is_ok() {
                let _ = self.join.take().unwrap().join();
            }
        }
        codexmanager_service::clear_shutdown_flag();
    }
}

fn verify_listener(client: &Client, server: &PersistentServer, member: &str) {
    for address in [
        format!("127.0.0.1:{}", server.port),
        format!("[::1]:{}", server.port),
    ] {
        let health = client
            .get(format!("http://{address}/health"))
            .send()
            .unwrap();
        assert_eq!(health.status().as_u16(), 200);
        assert!(health.headers().contains_key("x-request-id"));
        assert_eq!(health.text().unwrap(), "ok");
        let metrics = client
            .get(format!("http://{address}/metrics"))
            .send()
            .unwrap();
        assert_eq!(metrics.status().as_u16(), 200);
        assert!(metrics.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/plain"));
        assert!(metrics.text().unwrap().contains("codexmanager_"));
    }
    let rpc_url = format!("http://{}/rpc", server.addr);
    let request = serde_json::json!({"jsonrpc":"2.0","id":41,"method":"accountManager/status"});
    for token in [None, Some("fixture-invalid-rpc-token")] {
        let mut builder = client.post(&rpc_url).json(&request);
        if let Some(token) = token {
            builder = builder.header("x-codexmanager-rpc-token", token);
        }
        assert_eq!(builder.send().unwrap().status().as_u16(), 401);
    }
    let rpc_token = codexmanager_service::rpc_auth_token();
    let status = client
        .post(&rpc_url)
        .header("x-codexmanager-rpc-token", rpc_token)
        .header("x-codexmanager-rpc-actor-role", "admin")
        .json(&request)
        .send()
        .unwrap();
    assert_eq!(status.status().as_u16(), 200);
    let status: Value = status.json().unwrap();
    assert_eq!(status["id"], 41);
    assert_eq!(status["result"]["mode"], "accounts");
    assert_eq!(status["result"]["distributionEnabled"], true);
    let denied: Value = client
        .post(&rpc_url)
        .header("x-codexmanager-rpc-token", rpc_token)
        .header("x-codexmanager-rpc-actor-role", "member")
        .header("x-codexmanager-rpc-actor-user-id", member)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":42,"method":"accountManager/users/list"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(denied["result"]["errorCode"], "permission_denied");
}

#[test]
#[ignore = "requires an explicit isolated gateway backend: sqlite, mysql or postgres"]
fn remote_database_drives_real_gateway_and_durable_usage() {
    let _lock = test_env_guard();
    let _polling = EnvGuard::set("CODEXMANAGER_DISABLE_POLLING", "1");
    let _keepalive = EnvGuard::set("CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED", "false");
    let _refresh = EnvGuard::set("CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED", "false");
    let _warmup = EnvGuard::set("CODEXMANAGER_WARMUP_CRON_ENABLED", "false");
    let _managed_proxy = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _proxy_pool = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
    let _system_proxy_bypass = EnvGuard::set("NO_PROXY", "*");
    let backend =
        std::env::var("CODEXMANAGER_TEST_GATEWAY_BACKEND").expect("explicit isolated backend");
    let (kind, url_env) = match backend.as_str() {
        "sqlite" => (StorageBackendKind::Sqlite, "CODEXMANAGER_TEST_SQLITE_URL"),
        "mysql" => (StorageBackendKind::Mysql, "CODEXMANAGER_TEST_MYSQL_URL"),
        "postgres" => (
            StorageBackendKind::Postgres,
            "CODEXMANAGER_TEST_POSTGRES_URL",
        ),
        _ => panic!("unsupported test backend"),
    };
    let dir = new_test_dir("codexmanager-remote-gateway");
    let url = if kind == StorageBackendKind::Sqlite {
        format!(
            "sqlite://{}?mode=rwc",
            dir.join("seaorm-authority.db")
                .to_string_lossy()
                .replace('\\', "/")
        )
    } else {
        std::env::var(url_env).expect("isolated database URL")
    };
    let db_path = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let local = Storage::open(&db_path).unwrap();
    local.init().unwrap();
    let now = now_ts();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let key_id = format!("remote-http-key-{stamp}");
    let account_id = format!("remote-http-account-{stamp}");
    let user_id = format!("remote-http-user-{stamp}");
    let wallet_id = format!("remote-http-wallet-{stamp}");
    let group_id = format!("remote-http-group-{stamp}");
    let model_slug = format!("remote-http-model-{stamp}");
    let platform_key = format!("fixture-remote-{stamp}");
    let local_only_key = format!("fixture-local-{stamp}");
    let key = ApiKey {
        id: key_id.clone(),
        name: Some("remote gateway fixture".into()),
        model_slug: None,
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".into(),
        aggregate_api_id: None,
        aggregate_api_url: None,
        account_plan_filter: None,
        client_type: "codex".into(),
        protocol_type: "openai_compat".into(),
        auth_scheme: "authorization_bearer".into(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: hash_platform_key_for_test(&platform_key),
        status: "active".into(),
        created_at: now,
        last_used_at: None,
    };
    let mut local_key = key.clone();
    local_key.id = format!("local-only-{stamp}");
    local_key.key_hash = hash_platform_key_for_test(&local_only_key);
    local.insert_api_key(&local_key).unwrap();
    let mut model = local.get_managed_model_v2("gpt-5.4-mini").unwrap().unwrap();
    model.id.clear();
    model.slug = model_slug.clone();
    model.display_name = model_slug.clone();
    model.origin = "custom".into();
    model.builtin_revision = None;
    model.enabled = true;
    model.supported_in_api = true;
    model.visibility = "list".into();
    model.permission_group_ids.clear();
    model.price = ModelPriceV2 {
        price_status: "custom".into(),
        price_source: Some("fixture".into()),
        input_microusd_per_1m: Some(2_000_000),
        cached_input_microusd_per_1m: Some(200_000),
        cache_write_microusd_per_1m: None,
        output_microusd_per_1m: Some(4_000_000),
    };
    model.price_tiers = vec![ModelPriceTierV2 {
        min_input_tokens: 0,
        input_microusd_per_1m: 2_000_000,
        cached_input_microusd_per_1m: 200_000,
        cache_write_microusd_per_1m: None,
        output_microusd_per_1m: 4_000_000,
    }];
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "account_pool".into(),
        source_id: "default".into(),
        upstream_model: model_slug.clone(),
        enabled: true,
        priority: 0,
        weight: 1,
    }];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let remote = runtime
        .block_on(SeaOrmStorage::connect(kind, &url))
        .unwrap();
    runtime.block_on(async {
        remote.migrate().await.unwrap();
        // These are isolated acceptance databases, never a production URL.
        SettingsRepository::set(
            remote.connection(),
            AppSetting {
                key: "distribution.enabled".into(),
                value: "true".into(),
                updated_at: now,
            },
        )
        .await
        .unwrap();
        SettingsRepository::set(
            remote.connection(),
            AppSetting {
                key: "web.auth.mode".into(),
                value: "accounts".into(),
                updated_at: now,
            },
        )
        .await
        .unwrap();
        ManagedModelsRepository::upsert_many(
            remote.connection(),
            &[ManagedModelV2Upsert {
                previous_slug: None,
                model,
            }],
        )
        .await
        .unwrap();
        AccountsRepository::insert_account(
            remote.connection(),
            &Account {
                id: account_id.clone(),
                label: "remote provider fixture".into(),
                issuer: "https://auth.openai.com".into(),
                chatgpt_account_id: None,
                workspace_id: Some(format!("workspace-{stamp}")),
                group_name: Some(account_id.clone()),
                sort: 0,
                status: "force_enabled".into(),
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
        AccountTokensRepository::upsert(
            remote.connection(),
            AccountTokenRecord {
                account_id: account_id.clone(),
                id_token: String::new(),
                access_token: "fixture-upstream-access".into(),
                refresh_token: String::new(),
                api_key_access_token: Some("fixture-upstream-api".into()),
                last_refresh: now,
                access_token_exp: None,
                next_refresh_at: None,
                last_refresh_attempt_at: None,
            },
        )
        .await
        .unwrap();
        let mut record = ApiKeyRecord::from(key);
        record.account_group_filter = Some(account_id.clone());
        ApiKeysRepository::upsert(remote.connection(), record)
            .await
            .unwrap();
        UsersRepository::put(
            remote.connection(),
            AppUser {
                id: user_id.clone(),
                username: user_id.clone(),
                display_name: None,
                password_hash: "fixture-not-used-for-login".into(),
                role: "member".into(),
                status: "active".into(),
                created_at: now,
                updated_at: now,
                last_login_at: None,
            },
        )
        .await
        .unwrap();
        UsersRepository::put_owner(
            remote.connection(),
            ApiKeyOwner {
                key_id: key_id.clone(),
                owner_kind: "user".into(),
                owner_user_id: Some(user_id.clone()),
                project_id: None,
                updated_at: now,
            },
        )
        .await
        .unwrap();
        BillingRepository::create_wallet(
            remote.connection(),
            AppWallet {
                id: wallet_id.clone(),
                owner_kind: "user".into(),
                owner_id: user_id.clone(),
                balance_credit_micros: 1_000_000,
                frozen_credit_micros: 0,
                status: "active".into(),
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
        ModelGroupsRepository::upsert(
            remote.connection(),
            ModelGroupRecord {
                id: group_id.clone(),
                name: group_id.clone(),
                description: None,
                status: "active".into(),
                sort: 0,
                is_default: false,
                rate_multiplier_millis: 1500,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
        ModelGroupsRepository::replace_models_v2(
            remote.connection(),
            &group_id,
            &[ModelGroupModelRecord {
                group_id: group_id.clone(),
                platform_model_slug: model_slug.clone(),
                enabled: true,
                rate_multiplier_millis: None,
                billing_model_slug: None,
                note: None,
                created_at: now,
                updated_at: now,
            }],
        )
        .await
        .unwrap();
        ModelGroupsRepository::replace_user_assignments(
            remote.connection(),
            &group_id,
            &[UserModelGroupRecord {
                user_id: user_id.clone(),
                group_id: group_id.clone(),
                status: "active".into(),
                expires_at: None,
                created_at: now,
                updated_at: now,
            }],
        )
        .await
        .unwrap();
    });
    let _backend = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", &backend);
    let _url = EnvGuard::set("CODEXMANAGER_DATABASE_URL", &url);
    let sse=format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"remote provider hello\"}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"fixture-response\",\"status\":\"completed\",\"model\":\"{model_slug}\",\"output\":[{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"remote provider hello\"}}]}}],\"usage\":{{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}}}}\n\ndata: [DONE]\n\n");
    let (upstream, requests, upstream_thread) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, sse, "text/event-stream".into()); 4],
            Duration::from_secs(30),
        );
    let _upstream = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{upstream}/backend-api/codex"),
    );
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(0)
        .build()
        .unwrap();
    let server = PersistentServer::start(&client);
    verify_listener(&client, &server, &user_id);
    let (status, _) = post_http_raw_with_read_timeout(
        &server.addr,
        "/v1/responses",
        &serde_json::json!({"model":model_slug,"input":"denied"}).to_string(),
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {local_only_key}")),
        ],
        Duration::from_secs(20),
    );
    assert_eq!(
        status, 403,
        "remote mode must reject a key found only in SQLite"
    );
    for (path, stream) in [
        ("/v1/responses", false),
        ("/v1/responses", true),
        ("/v1/chat/completions", false),
        ("/v1/chat/completions", true),
    ] {
        let payload = if path.ends_with("chat/completions") {
            serde_json::json!({"model":model_slug,"messages":[{"role":"user","content":"hello"}],"stream":stream})
        } else {
            serde_json::json!({"model":model_slug,"input":"hello","stream":stream})
        };
        let response = client
            .post(format!("http://{}{path}", server.addr))
            .bearer_auth(&platform_key)
            .json(&payload)
            .send()
            .unwrap();
        let status = response.status().as_u16();
        let content_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(response.headers().contains_key("x-request-id"));
        let body = response.text().unwrap();
        assert_eq!(status, 200, "{path} stream={stream}: {body}");
        assert!(body.contains("remote provider hello"), "{body}");
        if stream {
            assert!(content_type.contains("text/event-stream"), "{content_type}");
            let events: Vec<Value> = body
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim)
                .filter(|line| *line != "[DONE]")
                .map(|line| serde_json::from_str(line).expect("valid SSE JSON event"))
                .collect();
            assert!(!events.is_empty());
            if path.ends_with("chat/completions") {
                assert!(events
                    .iter()
                    .any(|event| event["choices"][0]["finish_reason"] == "stop"));
            } else {
                assert!(events
                    .iter()
                    .any(|event| event["type"] == "response.completed"));
            }
        } else {
            assert!(content_type.contains("application/json"), "{content_type}");
            let json: Value = serde_json::from_str(&body).expect("valid non-streaming JSON");
            assert!(json.get("usage").is_some());
        }
        let captured = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(captured.path.ends_with("/responses"));
    }
    // Shutdown immediately after the final SSE reaches EOS: do not poll storage
    // or wait for the provider before testing the service's finalization drain.
    server.stop();
    upstream_thread.join().unwrap();
    assert!(requests.try_recv().is_err());
    // The server has drained active requests; verify committed data through a
    // fresh remote connection, independently of the running service's pool.
    let remote = runtime
        .block_on(SeaOrmStorage::connect(kind, &url))
        .unwrap();
    runtime.block_on(async {
        let scope = RequestLogFilter {
            key_ids: Some(vec![key_id.clone()]),
            ..Default::default()
        };
        let summary = RequestLogsRepository::summarize_filtered(remote.connection(), &scope)
            .await
            .unwrap();
        assert_eq!(
            (summary.count, summary.success_count, summary.total_tokens),
            (4, 4, 12)
        );
        assert_eq!(
            ApiKeyDetailsRepository::token_usage(remote.connection(), &key_id)
                .await
                .unwrap(),
            12
        );
        assert!(ApiKeysRepository::get(remote.connection(), &key_id)
            .await
            .unwrap()
            .unwrap()
            .last_used_at
            .is_some());
        let entries = BillingRepository::ledger(remote.connection(), &wallet_id, 10)
            .await
            .unwrap();
        assert_eq!(entries.len(), 4, "one ledger debit per completed request");
        for entry in &entries {
            assert_eq!(entry.entry_kind, "request_charge");
            assert_eq!(entry.api_key_id.as_deref(), Some(key_id.as_str()));
            assert_eq!(entry.amount_credit_micros, -12);
            let snapshot =
                BillingRepository::snapshot(remote.connection(), entry.request_log_id.unwrap())
                    .await
                    .unwrap()
                    .expect("immutable pricing snapshot");
            assert_eq!((snapshot.input_tokens, snapshot.output_tokens), (2, 1));
            assert_eq!(snapshot.base_cost_microusd, 8);
            assert_eq!(snapshot.rate_multiplier_millis, 1500);
            assert_eq!(snapshot.charged_cost_microusd, 12);
            assert_eq!(snapshot.usage_source, "actual");
        }
        let wallet = BillingRepository::wallet(remote.connection(), &wallet_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(wallet.balance_credit_micros, 1_000_000 - 48);
        assert_eq!(wallet.frozen_credit_micros, 0);
        let logs = RequestLogsRepository::list_filtered(remote.connection(), &scope, 0, 10)
            .await
            .unwrap();
        assert_eq!(logs.len(), 4);
        for entry in &entries {
            let log =
                RequestLogsRepository::get(remote.connection(), entry.request_log_id.unwrap())
                    .await
                    .unwrap()
                    .expect("ledger must reference a durable request log");
            assert_eq!(log.log.key_id.as_deref(), Some(key_id.as_str()));
            assert_eq!(log.log.status_code, Some(200));
            assert_eq!(log.log.model.as_deref(), Some(model_slug.as_str()));
        }
    });
    assert_eq!(
        local.count_request_logs(None, None, None, None).unwrap(),
        0,
        "remote gateway logs must never be written to the local SQLite file"
    );
    disconnect::exercise(
        &client,
        &runtime,
        kind,
        &url,
        &platform_key,
        &key_id,
        &model_slug,
        &wallet_id,
        stamp,
    );
    assert_eq!(local.count_request_logs(None, None, None, None).unwrap(), 0);
}
