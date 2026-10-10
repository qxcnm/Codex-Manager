//! Process-owned database pool for the synchronous domain migration boundary.
//!
//! Call `run` only from the existing bounded blocking domain workers. Native
//! async handlers can await `storage` directly; neither path builds a runtime,
//! connection pool, or schema for each request.

use codexmanager_storage_seaorm::{SeaOrmStorage, StorageConfig};
use std::future::Future;
use std::sync::LazyLock;
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, Semaphore};

static STORAGE: LazyLock<Mutex<Option<(StorageConfig, SeaOrmStorage)>>> =
    LazyLock::new(|| Mutex::new(None));
static OPERATIONS: Semaphore = Semaphore::const_new(8);

pub(crate) async fn storage() -> Result<SeaOrmStorage, String> {
    // SQLx connection drivers must outlive caller runtimes, including transient
    // embedded/test listeners. Only the process executor creates the pool.
    runtime()?
        .spawn(load_storage())
        .await
        .map_err(|_| "database initialization was interrupted".to_owned())?
}

async fn load_storage() -> Result<SeaOrmStorage, String> {
    let config = StorageConfig::from_env().map_err(|err| err.to_string())?;
    config.validate().map_err(|err| err.to_string())?;
    let mut cached = STORAGE.lock().await;
    if let Some((active_config, storage)) = cached.as_ref() {
        if *active_config != config {
            return Err(
                "database configuration changed; restart the service to switch databases".into(),
            );
        }
        return Ok(storage.clone());
    }
    let storage = SeaOrmStorage::connect_with_config(config.clone())
        .await
        .map_err(|err| err.to_string())?;
    storage.migrate().await.map_err(|err| err.to_string())?;
    storage
        .reconcile_builtin_model_catalog()
        .await
        .map_err(|err| err.to_string())?;
    storage
        .health_check()
        .await
        .map_err(|err| err.to_string())?;
    *cached = Some((config, storage.clone()));
    Ok(storage)
}

fn runtime() -> Result<&'static Runtime, String> {
    crate::runtime::service_runtime::process_runtime()
}

pub(crate) fn run<T, F, Fut>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SeaOrmStorage) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    let runtime = runtime()?;
    // The permit belongs to the future, including panic/cancellation paths.
    // The join result also propagates panics instead of leaving a waiting
    // receiver alive forever.
    let task = runtime.spawn(async move {
        let _permit = OPERATIONS
            .acquire()
            .await
            .map_err(|_| "database runtime is closing".to_owned())?;
        operation(storage().await?).await
    });
    wait_for_database(task).map_err(|_| "database operation was interrupted".to_owned())?
}

fn wait_for_database<F: Future>(future: F) -> F::Output {
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(|| futures_lite::future::block_on(future))
    } else {
        futures_lite::future::block_on(future)
    }
}

/// Continue or finish an already admitted transaction on its existing
/// connection. Re-acquiring an operation permit here can deadlock when other
/// admitted operations are waiting for the transaction's domain lock.
pub(crate) fn run_transaction<T, Fut>(future: Fut) -> Result<T, String>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
{
    wait_for_database(runtime()?.spawn(future))
        .map_err(|_| "database transaction was interrupted".to_owned())?
}

#[cfg(test)]
pub(crate) fn clear_for_tests() {
    if let Ok(runtime) = runtime() {
        futures_lite::future::block_on(runtime.spawn(async {
            if let Some((_, storage)) = STORAGE.lock().await.take() {
                let _ = storage.connection().clone().close().await;
            }
        }))
        .expect("clear database runtime");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_storage_seaorm::{AppSetting, ManagedModelsRepository, SettingsRepository};

    struct EnvGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            clear_for_tests();
            for (key, value) in self.0.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn pool_survives_operations_and_rejects_live_backend_changes() {
        let _guard = crate::test_env_guard();
        let _restore = EnvGuard(
            ["CODEXMANAGER_STORAGE_BACKEND", "CODEXMANAGER_DATABASE_URL"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        clear_for_tests();
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        std::env::set_var("CODEXMANAGER_DATABASE_URL", "sqlite::memory:");
        run(|storage| async move {
            SettingsRepository::set(
                storage.connection(),
                AppSetting {
                    key: "shared_pool".into(),
                    value: "kept".into(),
                    updated_at: 1,
                },
            )
            .await
            .map_err(|e| e.to_string())
        })
        .unwrap();
        let value = run(|storage| async move {
            SettingsRepository::get(storage.connection(), "shared_pool")
                .await
                .map_err(|e| e.to_string())
        })
        .unwrap()
        .unwrap();
        assert_eq!(value.value, "kept");
        let catalog_slugs = run(|storage| async move {
            ManagedModelsRepository::list(storage.connection(), true)
                .await
                .map(|models| {
                    models
                        .into_iter()
                        .map(|model| model.slug)
                        .collect::<Vec<_>>()
                })
                .map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(catalog_slugs.len(), 12);
        for slug in [
            "gpt-6.1-sol",
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
        ] {
            assert!(catalog_slugs.iter().any(|candidate| candidate == slug));
        }
        std::env::set_var(
            "CODEXMANAGER_DATABASE_URL",
            "sqlite://would-switch.sqlite?mode=rwc",
        );
        assert!(run(|_| async { Ok(()) }).unwrap_err().contains("restart"));
    }
}
