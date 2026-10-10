//! Implementations of core persistence contracts. No process-global storage lookup.
use codexmanager_core::storage::*;
use std::sync::Arc;
use tokio::sync::Semaphore;
mod seaorm;
mod sqlite;
#[cfg(test)]
mod tests;

/// A concrete SQLite pool captured at application composition time.
pub struct SqliteDomainStorage {
    storage: Arc<Storage>,
    slots: Arc<Semaphore>,
}
impl SqliteDomainStorage {
    pub fn new(storage: Storage) -> Self {
        Self {
            storage: Arc::new(storage),
            slots: Arc::new(Semaphore::new(8)),
        }
    }
    fn execute<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Storage) -> rusqlite::Result<T> + Send + 'static,
    ) -> StorageFuture<'_, T> {
        let storage = self.storage.clone();
        let slots = self.slots.clone();
        Box::pin(async move {
            let permit = slots
                .acquire_owned()
                .await
                .map_err(|_| "storage is closing".to_owned())?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                work(&storage).map_err(|error| format!("sqlite operation failed: {error}"))
            })
            .await
            .map_err(|_| "sqlite worker interrupted".to_owned())?
        })
    }
}
impl DomainStorage for SqliteDomainStorage {
    fn kind(&self) -> StorageBackendKind {
        StorageBackendKind::Sqlite
    }
    fn check(&self) -> StorageFuture<'_, StorageHealth> {
        self.execute(|s| s.health())
    }
}

impl StorageBackend for SqliteDomainStorage {
    fn backend_kind(&self) -> StorageBackendKind {
        StorageBackendKind::Sqlite
    }

    fn health(&self) -> rusqlite::Result<StorageHealth> {
        self.storage.health()
    }

    fn domain(&self) -> Option<&dyn DomainStorage> {
        Some(self)
    }
}

impl DomainStorage for crate::SeaOrmStorage {
    fn kind(&self) -> StorageBackendKind {
        self.backend
    }
    fn check(&self) -> StorageFuture<'_, StorageHealth> {
        Box::pin(async move { self.health_check().await.map_err(|error| error.to_string()) })
    }
}

impl StorageBackend for crate::SeaOrmStorage {
    fn backend_kind(&self) -> StorageBackendKind {
        self.backend
    }

    fn health(&self) -> rusqlite::Result<StorageHealth> {
        // The async adapter is checked through `DomainStorage::check`; this
        // compatibility method only exposes the captured backend synchronously.
        Ok(StorageHealth {
            backend: self.backend,
            schema_ready: false,
        })
    }

    fn domain(&self) -> Option<&dyn DomainStorage> {
        Some(self)
    }
}
