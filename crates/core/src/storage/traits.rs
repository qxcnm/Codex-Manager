//! Database independent storage boundary used by service-mode adapters.
//!
//! The legacy synchronous health boundary remains for compatibility. Async
//! domain contracts below serve injected SQLite and SeaORM adapters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackendKind {
    Sqlite,
    Mysql,
    Postgres,
}

impl StorageBackendKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageHealth {
    pub backend: StorageBackendKind,
    pub schema_ready: bool,
}

pub trait StorageBackend {
    fn backend_kind(&self) -> StorageBackendKind;
    fn health(&self) -> rusqlite::Result<StorageHealth>;

    /// Return the database-independent domain contract when this backend is
    /// composed into an async service.  The legacy `Storage` handle keeps the
    /// default `None` implementation so desktop callers retain its synchronous
    /// API; service composition uses the concrete adapters below and never
    /// reaches through this boundary to a connection or ORM entity.
    fn domain(&self) -> Option<&dyn DomainStorage> {
        None
    }
}

impl StorageBackend for super::Storage {
    fn backend_kind(&self) -> StorageBackendKind {
        StorageBackendKind::Sqlite
    }

    fn health(&self) -> rusqlite::Result<StorageHealth> {
        // A cheap, read-only query proves the connection is usable while
        // preserving all existing migration and transaction behaviour.
        self.conn.query_row("SELECT 1", [], |_row| Ok(()))?;
        Ok(StorageHealth {
            backend: StorageBackendKind::Sqlite,
            schema_ready: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_strings_are_stable() {
        assert_eq!(StorageBackendKind::Sqlite.as_str(), "sqlite");
        assert_eq!(StorageBackendKind::Mysql.as_str(), "mysql");
        assert_eq!(StorageBackendKind::Postgres.as_str(), "postgres");
    }

    #[test]
    fn sqlite_storage_implements_health_boundary() {
        let storage = super::super::Storage::open_in_memory().expect("storage");
        let health = storage.health().expect("health");
        assert_eq!(health.backend, StorageBackendKind::Sqlite);
        assert!(health.schema_ready);
        assert!(StorageBackend::domain(&storage).is_none());
    }
}

mod domain;
pub use domain::*;
mod api_keys;
pub use api_keys::*;
