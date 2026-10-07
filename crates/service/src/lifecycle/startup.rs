use std::io;
use std::thread;

pub(crate) async fn reconcile_active_gateway_profile_after_startup_async() {
    let Some(storage) = crate::storage_helpers::open_storage() else {
        log::warn!("event=startup_gateway_profile_reconciliation_failed error=storage unavailable");
        return;
    };
    if let Err(err) =
        crate::codex_profile::sync_active_gateway_profile_from_storage_async(&storage).await
    {
        log::warn!("event=startup_gateway_profile_reconciliation_failed error={err}");
    }
}

pub(crate) fn reconcile_active_gateway_profile_after_startup() {
    match crate::process_runtime() {
        Ok(runtime) => {
            runtime.spawn(reconcile_active_gateway_profile_after_startup_async());
        }
        Err(err) => log::warn!(
            "event=startup_gateway_profile_reconciliation_failed error=runtime unavailable: {err}"
        ),
    }
}

pub struct ServerHandle {
    pub addr: String,
    join: thread::JoinHandle<()>,
}

impl ServerHandle {
    /// 函数 `join`
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
    pub fn join(self) {
        let _ = self.join.join();
    }
}

/// 函数 `start_one_shot_server`
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
pub fn start_one_shot_server() -> std::io::Result<ServerHandle> {
    crate::portable::bootstrap_current_process();
    crate::gateway::reload_runtime_config_from_env();
    crate::storage_helpers::initialize_storage()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    // Before the gateway accepts traffic: payload clear-generation mirror,
    // queue budget, spill directory and replay of leftover spill segments.
    crate::gateway::initialize_request_log_payload_pipeline();
    crate::sync_runtime_settings_from_storage();
    reconcile_active_gateway_profile_after_startup();
    // Integration tests use exactly the production Axum router, including its
    // authentication, body limits, streaming adapters and cancellation paths.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?.to_string();
    let runtime = crate::http::proxy_runtime::front_proxy_runtime()?;
    let join = thread::spawn(move || {
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener runtime");
            let (shutdown, stopped) = tokio::sync::oneshot::channel();
            let shutdown = std::sync::Arc::new(std::sync::Mutex::new(Some(shutdown)));
            let app = crate::http::router::build_router(crate::http::router::AppState::new())
                .layer(axum::middleware::from_fn(
                    move |request, next: axum::middleware::Next| {
                        let shutdown = shutdown.clone();
                        async move {
                            let response = next.run(request).await;
                            if let Some(shutdown) = shutdown
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .take()
                            {
                                let _ = shutdown.send(());
                            }
                            response
                        }
                    },
                ));
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
            // Deferred delivery owns accounting after the HTTP body finishes,
            // including a disconnected client. Wait before this listener exits.
            crate::gateway::drain_deferred_responses().await;
        });
    });
    Ok(ServerHandle { addr, join })
}

/// 函数 `start_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
pub fn start_server(addr: &str) -> std::io::Result<()> {
    crate::portable::bootstrap_current_process();
    crate::gateway::reload_runtime_config_from_env();
    crate::storage_helpers::initialize_storage()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    // Before the gateway accepts traffic: payload clear-generation mirror,
    // queue budget, spill directory and replay of leftover spill segments.
    crate::gateway::initialize_request_log_payload_pipeline();
    crate::sync_runtime_settings_from_storage();
    crate::storage_maintenance::ensure_storage_maintenance();
    reconcile_active_gateway_profile_after_startup();
    crate::app_settings::ensure_codex_latest_version_sync();
    crate::usage_refresh::ensure_usage_polling();
    crate::usage_refresh::ensure_gateway_keepalive();
    crate::usage_refresh::ensure_token_refresh_polling();
    crate::usage_refresh::ensure_warmup_cron();
    crate::usage_refresh::ensure_reset_warmup();
    crate::plugin::ensure_plugin_scheduler();
    crate::http::server::start_http(addr)
}
