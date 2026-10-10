//! SQLite space accounting and reclamation.
//!
//! Clearing or pruning request logs frees pages inside the database file but
//! never shrinks the file: SQLite keeps the free pages and reuses them for new
//! rows. Any capacity based decision MUST therefore use
//! [`DatabaseSpaceUsage::used_bytes`] (live pages) instead of the file size.
//! Otherwise a freshly cleared database still looks full and data that should
//! be kept gets deleted.
//!
//! Free pages are returned to the operating system by
//! [`Storage::incremental_vacuum_step`] when the database uses
//! `auto_vacuum = INCREMENTAL` (all databases created by this version), or by
//! the explicit, user triggered [`Storage::convert_to_incremental_auto_vacuum`]
//! for older databases. Nothing on the request path rewrites the whole file.

use rusqlite::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::Storage;

pub const AUTO_VACUUM_NONE: i64 = 0;
pub const AUTO_VACUUM_FULL: i64 = 1;
pub const AUTO_VACUUM_INCREMENTAL: i64 = 2;
pub const PURGE_PENDING_REBUILD_ERROR: &str =
    "request payload purge still pending, rebuild after it finishes";

/// Set when a TRUNCATE checkpoint after a purge could not complete (readers
/// were still on an older snapshot). Maintenance retries it later, so the
/// WAL does not keep pre-purge page images around indefinitely.
static WAL_CHECKPOINT_PENDING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DatabaseSpaceUsage {
    pub page_size: i64,
    pub page_count: i64,
    pub freelist_count: i64,
    /// Bytes held by live pages: `(page_count - freelist_count) * page_size`.
    /// Use this, never the file size, for capacity limits.
    pub used_bytes: i64,
    /// Bytes held by free pages that a vacuum would return to the OS.
    pub reclaimable_bytes: i64,
    pub file_bytes: Option<u64>,
    pub wal_bytes: Option<u64>,
    /// 0 = none, 1 = full, 2 = incremental.
    pub auto_vacuum: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalCheckpointOutcome {
    /// The checkpoint could not finish because readers still use older data.
    pub busy: bool,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

/// Whether a post-purge WAL checkpoint still has to be retried.
pub fn wal_checkpoint_pending() -> bool {
    WAL_CHECKPOINT_PENDING.load(Ordering::Relaxed)
}

fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

fn wal_path(path: &Path) -> PathBuf {
    let mut raw = path.as_os_str().to_owned();
    raw.push("-wal");
    PathBuf::from(raw)
}

impl Storage {
    fn pragma_i64(&self, pragma: &str) -> Result<i64> {
        self.conn
            .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
    }

    pub fn database_space_usage(&self) -> Result<DatabaseSpaceUsage> {
        let page_size = self.pragma_i64("page_size")?;
        let page_count = self.pragma_i64("page_count")?;
        let freelist_count = self.pragma_i64("freelist_count")?;
        let auto_vacuum = self.pragma_i64("auto_vacuum")?;
        let (file_bytes, wal_bytes) = match self.conn.path() {
            Some(path) => (file_len(path), file_len(&wal_path(path))),
            None => (None, None),
        };
        Ok(DatabaseSpaceUsage {
            page_size,
            page_count,
            freelist_count,
            used_bytes: page_count
                .saturating_sub(freelist_count)
                .saturating_mul(page_size),
            reclaimable_bytes: freelist_count.saturating_mul(page_size),
            file_bytes,
            wal_bytes,
            auto_vacuum,
        })
    }

    /// New databases are created in `auto_vacuum = INCREMENTAL` mode, which
    /// can only be chosen before the first table exists. Existing databases
    /// are never converted here (that needs a full rebuild).
    pub(super) fn prefer_incremental_auto_vacuum_for_new_database(&self) -> Result<()> {
        let objects: i64 =
            self.conn
                .query_row("SELECT COUNT(1) FROM sqlite_master", [], |row| row.get(0))?;
        if objects > 0 || self.pragma_i64("auto_vacuum")? == AUTO_VACUUM_INCREMENTAL {
            return Ok(());
        }
        // Best effort: a failure only means the database keeps the default
        // mode and space is reclaimed through the explicit conversion.
        if self
            .conn
            .execute_batch("PRAGMA auto_vacuum = INCREMENTAL")
            .is_err()
        {
            return Ok(());
        }
        if self.pragma_i64("auto_vacuum")? != AUTO_VACUUM_INCREMENTAL {
            // Opening the file in WAL mode already wrote the header page, so
            // the new mode only applies after a rebuild. The database has no
            // tables yet, which makes this VACUUM instantaneous.
            let _ = self.conn.execute_batch("VACUUM");
        }
        Ok(())
    }

    /// Return up to `max_pages` free pages to the operating system. Only has
    /// an effect on `auto_vacuum = INCREMENTAL` databases. Holds the write
    /// lock while it runs, so callers keep `max_pages` small.
    pub fn incremental_vacuum_step(&self, max_pages: i64) -> Result<i64> {
        if self.pragma_i64("auto_vacuum")? != AUTO_VACUUM_INCREMENTAL {
            return Ok(0);
        }
        let before = self.pragma_i64("freelist_count")?;
        if before == 0 {
            return Ok(0);
        }
        self.conn
            .execute_batch(&format!("PRAGMA incremental_vacuum({})", max_pages.max(1)))?;
        let after = self.pragma_i64("freelist_count")?;
        Ok(before.saturating_sub(after))
    }

    /// One-time, user triggered conversion of an older database to
    /// `auto_vacuum = INCREMENTAL`. This rewrites the whole file and blocks
    /// other writers until it finishes. Returns `false` when the database
    /// already uses incremental mode.
    pub fn convert_to_incremental_auto_vacuum(&self) -> Result<bool> {
        if self.pragma_i64("auto_vacuum")? == AUTO_VACUUM_INCREMENTAL {
            return Ok(false);
        }
        // VACUUM may renumber the rowids of the payload tables (their primary
        // keys are not INTEGER PRIMARY KEY), which would invalidate the rowid
        // bounds of a pending clear purge.
        if self.request_log_payload_purge_pending()? {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some(PURGE_PENDING_REBUILD_ERROR.to_string()),
            ));
        }
        // VACUUM rebuilds the file through a transient copy. With
        // temp_store=MEMORY (set on every connection) SQLite would keep that
        // whole copy in RAM, so use a temporary file for the rebuild.
        self.conn
            .execute_batch("PRAGMA auto_vacuum = INCREMENTAL; PRAGMA temp_store = FILE")?;
        let rebuilt = self.conn.execute_batch("VACUUM");
        let restored = self.conn.execute_batch("PRAGMA temp_store = MEMORY");
        rebuilt?;
        restored?;
        self.checkpoint_after_purge();
        Ok(self.pragma_i64("auto_vacuum")? == AUTO_VACUUM_INCREMENTAL)
    }

    pub fn checkpoint_wal_truncate(&self) -> Result<WalCheckpointOutcome> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok(WalCheckpointOutcome {
                    busy: row.get::<_, i64>(0)? != 0,
                    log_frames: row.get(1)?,
                    checkpointed_frames: row.get(2)?,
                })
            })
    }

    /// Checkpoint and truncate the WAL after deleting data, so page images
    /// written before the purge do not stay in the WAL file. A busy or failed
    /// checkpoint is remembered and retried by
    /// [`Self::retry_pending_wal_checkpoint`].
    pub(super) fn checkpoint_after_purge(&self) {
        let completed = matches!(self.checkpoint_wal_truncate(), Ok(outcome) if !outcome.busy);
        WAL_CHECKPOINT_PENDING.store(!completed, Ordering::Relaxed);
    }

    /// Retry a checkpoint that could not complete after a purge. Returns
    /// `true` when nothing is pending anymore.
    pub fn retry_pending_wal_checkpoint(&self) -> Result<bool> {
        if !wal_checkpoint_pending() {
            return Ok(true);
        }
        let outcome = self.checkpoint_wal_truncate()?;
        WAL_CHECKPOINT_PENDING.store(outcome.busy, Ordering::Relaxed);
        Ok(!outcome.busy)
    }

    pub fn secure_delete_mode(&self) -> Result<i64> {
        self.pragma_i64("secure_delete")
    }

    /// Run `operation` with `secure_delete` enabled, so deleted rows are
    /// overwritten instead of lingering in free pages (they may contain
    /// unredacted request bodies). The previous mode is always restored,
    /// because pooled connections are reused afterwards.
    pub(super) fn with_secure_delete<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let previous = self.secure_delete_mode().unwrap_or(0);
        self.conn.execute_batch("PRAGMA secure_delete = ON")?;
        let result = operation();
        let restore = match previous {
            1 => "PRAGMA secure_delete = ON",
            2 => "PRAGMA secure_delete = FAST",
            _ => "PRAGMA secure_delete = OFF",
        };
        let restored = self.conn.execute_batch(restore);
        let value = result?;
        restored?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "storage_space_tests.rs"]
mod tests;
