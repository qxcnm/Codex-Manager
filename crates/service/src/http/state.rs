//! Dependencies captured once for a listener; requests never select a database.
use codexmanager_core::storage::{DomainStorage, Storage};
use codexmanager_storage_seaorm::{SqliteDomainStorage, StorageConfig};
use std::sync::Arc;
use tokio::sync::{broadcast, watch, OnceCell, Semaphore};

enum StorageSource {
    Sqlite(std::path::PathBuf),
    SeaOrm(Result<StorageConfig, String>),
    Injected,
}

pub struct AppState {
    source: StorageSource,
    storage: OnceCell<Arc<dyn DomainStorage>>,
    pub(crate) request_limits: super::middleware::RequestLimits,
    pub(crate) rpc_slots: Arc<Semaphore>,
    task_channel: broadcast::Sender<&'static str>,
    pub(crate) shutdown: watch::Sender<bool>,
    // Shared listener client is reserved for handlers that opt into connection reuse.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) http_client: reqwest::Client,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        let source = if crate::storage_helpers::seaorm_enabled() {
            StorageSource::SeaOrm(StorageConfig::from_env().map_err(|error| error.to_string()))
        } else {
            StorageSource::Sqlite(
                std::env::var_os("CODEXMANAGER_DB_PATH")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| "codexmanager.db".into()),
            )
        };
        Arc::new(Self::with_source(source))
    }

    fn with_source(source: StorageSource) -> Self {
        let (task_channel, _) = broadcast::channel(128);
        Self {
            source,
            storage: OnceCell::new(),
            request_limits: super::middleware::RequestLimits::new(256, 64),
            rpc_slots: Arc::new(Semaphore::new(64)),
            task_channel,
            shutdown: watch::channel(false).0,
            http_client: reqwest::Client::new(),
        }
    }

    pub fn with_storage(storage: Arc<dyn DomainStorage>) -> Arc<Self> {
        let state = Self::with_source(StorageSource::Injected);
        let _ = state.storage.set(storage);
        Arc::new(state)
    }

    pub async fn storage(&self) -> Result<Arc<dyn DomainStorage>, String> {
        self.storage
            .get_or_try_init(|| async {
                match &self.source {
                    StorageSource::Injected => Err("injected storage missing".to_owned()),
                    StorageSource::Sqlite(path) => {
                        let path = path.clone();
                        let storage = tokio::task::spawn_blocking(move || {
                            let storage = Storage::open(path).map_err(|error| error.to_string())?;
                            storage.init().map_err(|error| error.to_string())?;
                            Ok::<_, String>(storage)
                        })
                        .await
                        .map_err(|_| "storage initialization interrupted".to_owned())??;
                        Ok(Arc::new(SqliteDomainStorage::new(storage)) as Arc<dyn DomainStorage>)
                    }
                    StorageSource::SeaOrm(config) => {
                        // The already initialized process pool is shared, never reconnected.
                        // Validate the captured configuration before accepting that pool.
                        let config = config.as_ref().map_err(Clone::clone)?;
                        config.validate().map_err(|error| error.to_string())?;
                        if StorageConfig::from_env().map_err(|error| error.to_string())? != *config
                        {
                            return Err(
                                "database configuration changed; restart the service".into()
                            );
                        }
                        let storage = crate::storage::seaorm_runtime::storage().await?;
                        Ok(Arc::new(storage) as Arc<dyn DomainStorage>)
                    }
                }
            })
            .await
            .cloned()
    }

    /// The shared client is composed with the listener so handlers and
    /// adapters can reuse one connection pool instead of constructing a
    /// client per request.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_shutting_down(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// Subscribe to listener-owned task signals without coupling handlers to
    /// a concrete executor or persistence implementation.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn task_receiver(&self) -> broadcast::Receiver<&'static str> {
        self.task_channel.subscribe()
    }

    pub(crate) fn signal_task(&self, name: &'static str) {
        let _ = self.task_channel.send(name);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::Storage;
    use codexmanager_storage_seaorm::SqliteDomainStorage;

    #[tokio::test]
    async fn injected_domain_storage_is_captured_once_and_checked() {
        let sqlite = Storage::open_in_memory().expect("sqlite");
        sqlite.init().expect("schema");
        let state = AppState::with_storage(Arc::new(SqliteDomainStorage::new(sqlite)));
        let first = state.storage().await.expect("storage");
        let second = state.storage().await.expect("same storage");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first.kind(),
            codexmanager_core::storage::StorageBackendKind::Sqlite
        );
        assert!(first.check().await.expect("health").schema_ready);
        assert!(!state.is_shutting_down());
        let _ = state.http_client();
        let mut task_receiver = state.task_receiver();
        state.signal_task("test");
        assert_eq!(task_receiver.recv().await.expect("task signal"), "test");
    }
}
