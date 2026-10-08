//! Batched deletion of request payload rows.
//!
//! Clearing request logs and retention pruning used to delete every payload
//! row inside the same transaction as the request log rows, followed by a
//! full `VACUUM`. With full payload storage that transaction could touch
//! gigabytes, blow up the WAL and hold the write lock long enough for gateway
//! writes to fail with `database is locked`.
//!
//! Now the clear/prune transaction only makes the old data unreachable and
//! records what has to go:
//! * clear: bumps the generation (in-flight jobs are rejected) and stores, per
//!   payload table, the largest rowid and the new generation in
//!   `request_log_payload_purges`. Every payload row stores the generation
//!   that was current when it was written, so rows written before the clear
//!   are told apart from later rows (even ones that reuse a rowid in the same
//!   second) by generation, never by time;
//! * prune: raises `retention_cutoff` (jobs older than it are rejected) and
//!   rebases surviving manifests whose parent is about to be deleted.
//!
//! [`Storage::drain_request_log_payload_purges`] then deletes the doomed rows
//! in short transactions with `secure_delete` enabled, and finally sweeps
//! blobs that no manifest references anymore. All state is persisted, so a
//! restart simply continues where the previous process stopped. Doomed
//! manifests are never chosen as parents of new manifests (see
//! `select_request_log_payload_parent`), so new data never depends on rows
//! the purge is deleting.

use rusqlite::{types::Value, OptionalExtension, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Storage;

/// Payload tables purged by rowid after a clear and by `created_at` for
/// retention. Blobs are handled by the reference-checked sweep instead.
const PURGED_PAYLOAD_TABLES: [&str; 4] = [
    "request_log_payloads",
    "request_log_payload_manifests",
    "request_log_upstream_attempts",
    "request_log_response_links",
];
const MANIFESTS_TABLE: &str = "request_log_payload_manifests";
const BLOB_GC_MARKER: &str = "request_log_payload_blobs";
const PURGES_TABLE: &str = "request_log_payload_purges";

/// Tables that carry the `generation` column (migration 143).
pub(super) const REQUEST_LOG_PAYLOAD_GENERATION_TABLES: [&str; 5] = [
    "request_log_payloads",
    "request_log_payload_manifests",
    "request_log_upstream_attempts",
    "request_log_response_links",
    PURGES_TABLE,
];

/// SQL value for the `generation` column of a payload write: the clear
/// generation current in the same statement, so the value and the write are
/// atomic with respect to a clear on another connection.
pub(super) const CURRENT_PAYLOAD_GENERATION_SQL: &str =
    "(SELECT generation FROM request_log_payload_state WHERE id = 1)";

/// Condition that the clear marker given by the `max_rowid`, `cleared_at` and
/// `generation` SQL expressions dooms the row aliased `row`. Markers written since migration 143 carry the
/// generation the clear bumped to and doom rows of any lower generation, plus
/// rows written before the migration (generation NULL). Older markers
/// (generation NULL) keep the time-based condition, restricted to rows
/// written before the migration, so they never match newer rows.
fn cleared_row_sql(row: &str, max_rowid: &str, cleared_at: &str, generation: &str) -> String {
    format!(
        "{row}.rowid <= {max_rowid}
         AND ({row}.generation IS NULL OR {row}.generation < {generation})
         AND ({generation} IS NOT NULL OR {row}.created_at <= {cleared_at})"
    )
}

const MIN_BATCH_ROWS: i64 = 50;
const MAX_BATCH_ROWS: i64 = 4000;
const INITIAL_BATCH_ROWS: i64 = 1000;
const MANIFEST_BATCH_DIVISOR: i64 = 4;
const BLOB_SCAN_ROWS: i64 = 2000;
const ID_CHUNK: usize = 400;
const KEY_CHUNK: usize = 200;
/// Batches aim to hold the write lock for about this long, well below the
/// 3 s busy timeout other connections use.
const TARGET_BATCH_TIME: Duration = Duration::from_millis(60);
/// Pause between batches so gateway writers can take the write lock.
const PAUSE_BETWEEN_BATCHES: Duration = Duration::from_millis(15);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestLogPayloadPurgeProgress {
    /// Payload rows (all tables, including blobs) deleted by this call.
    pub deleted_rows: usize,
    /// Work is left because the time budget ran out.
    pub pending: bool,
}

/// A pending clear of one payload table, as stored in
/// `request_log_payload_purges`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClearMarker {
    max_rowid: i64,
    cleared_at: i64,
    /// Generation the clear bumped to; `None` for markers written before
    /// migration 143.
    generation: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum Doomed {
    /// Rows written before a clear committed.
    Cleared(ClearMarker),
    /// Rows older than the retention cutoff.
    Retention { cutoff: i64 },
}

impl Doomed {
    fn predicate(self, table: &str) -> (String, Vec<Value>) {
        match self {
            Self::Cleared(marker) => (
                format!(
                    "{} ORDER BY rowid",
                    cleared_row_sql(table, "?1", "?2", "?3")
                ),
                vec![
                    Value::Integer(marker.max_rowid),
                    Value::Integer(marker.cleared_at),
                    marker.generation.map_or(Value::Null, Value::Integer),
                ],
            ),
            Self::Retention { cutoff } => {
                ("created_at < ?1".to_string(), vec![Value::Integer(cutoff)])
            }
        }
    }
}

/// SQL condition, for alias `alias` of payload table `table`, that holds for
/// rows a pending clear or the retention cutoff has NOT scheduled for
/// deletion. Readers and parent selection only see these rows, so payloads
/// that are still waiting for the batched purge are already invisible (for
/// example a request that was in flight while the logs were cleared).
pub(super) fn live_payload_row_sql(table: &str, alias: &str) -> String {
    format!(
        "{alias}.created_at >= COALESCE(
            (SELECT retention_cutoff FROM request_log_payload_state WHERE id = 1), 0)
         AND NOT EXISTS (
            SELECT 1 FROM request_log_payload_purges purge
            WHERE purge.table_name = '{table}'
              AND {cleared})",
        cleared = cleared_row_sql(
            alias,
            "purge.max_rowid",
            "purge.cleared_at",
            "purge.generation"
        )
    )
}

/// One drain per database file at a time: a clear running its synchronous
/// budget and the maintenance thread must not take turns on the write lock,
/// that would leave almost no room for gateway writes. In-memory databases
/// are private to their connection and are not coordinated.
fn active_drains() -> &'static Mutex<HashSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

struct DrainSlot(Option<PathBuf>);

impl DrainSlot {
    fn acquire(path: Option<&Path>) -> Option<Self> {
        let Some(path) = path else {
            return Some(Self(None));
        };
        let mut active = active_drains()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active
            .insert(path.to_path_buf())
            .then(|| Self(Some(path.to_path_buf())))
    }
}

impl Drop for DrainSlot {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            active_drains()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&path);
        }
    }
}

struct BatchSize {
    rows: i64,
    last_elapsed: Duration,
}

impl BatchSize {
    fn new() -> Self {
        Self {
            rows: INITIAL_BATCH_ROWS,
            last_elapsed: Duration::ZERO,
        }
    }

    fn rows(&self, table: &str) -> i64 {
        if table == MANIFESTS_TABLE {
            (self.rows / MANIFEST_BATCH_DIVISOR).max(MIN_BATCH_ROWS / 2)
        } else {
            self.rows
        }
    }

    fn adapt(&mut self, elapsed: Duration) {
        self.last_elapsed = elapsed;
        if elapsed > TARGET_BATCH_TIME.saturating_mul(2) {
            self.rows = (self.rows / 2).max(MIN_BATCH_ROWS);
        } else if elapsed < TARGET_BATCH_TIME / 2 {
            self.rows = (self.rows * 2).min(MAX_BATCH_ROWS);
        }
    }

    /// Pause at least as long as the last batch held the write lock, so the
    /// purge never occupies more than about half of the lock time.
    fn pause(&self) {
        std::thread::sleep(self.last_elapsed.max(PAUSE_BETWEEN_BATCHES));
    }
}

fn placeholders(count: usize, group: &str) -> String {
    vec![group; count].join(",")
}

impl Storage {
    fn has_request_log_payload_purges(&self) -> Result<bool> {
        self.has_table(PURGES_TABLE)
    }

    /// Part of the clear transaction: reject in-flight jobs and record which
    /// rows the purge has to delete. The caller owns the transaction.
    pub(super) fn begin_request_log_payload_clear(&self, cleared_at: i64) -> Result<()> {
        if self.has_table("request_log_payload_state")? {
            self.conn.execute(
                "UPDATE request_log_payload_state SET generation = generation + 1 WHERE id = 1",
                [],
            )?;
        }
        if !self.has_request_log_payload_purges()? {
            return self.delete_request_log_payloads_unbatched();
        }
        // Rows written before this clear carry a lower generation; rows
        // written after it carry this one or higher.
        let generation = self.request_log_payload_generation()?;
        for table in PURGED_PAYLOAD_TABLES {
            if !self.has_table(table)? {
                continue;
            }
            let max_rowid: Option<i64> =
                self.conn
                    .query_row(&format!("SELECT MAX(rowid) FROM {table}"), [], |row| {
                        row.get(0)
                    })?;
            let Some(max_rowid) = max_rowid else {
                continue;
            };
            // Merging into an older marker: every row it covers predates this
            // clear, so the higher generation and rowid bound cover it too.
            self.conn.execute(
                "INSERT INTO request_log_payload_purges
                    (table_name, max_rowid, cleared_at, cursor_rowid, generation)
                 VALUES (?1, ?2, ?3, 0, ?4)
                 ON CONFLICT(table_name) DO UPDATE SET
                    max_rowid = MAX(max_rowid, excluded.max_rowid),
                    cleared_at = MAX(cleared_at, excluded.cleared_at),
                    generation = MAX(COALESCE(generation, excluded.generation),
                                     excluded.generation)",
                (table, max_rowid, cleared_at, generation),
            )?;
        }
        self.mark_request_log_payload_blob_gc()
    }

    /// Fallback for databases that predate the purge table.
    fn delete_request_log_payloads_unbatched(&self) -> Result<()> {
        for table in [
            "request_log_response_links",
            "request_log_upstream_attempts",
            "request_log_payload_manifest_items",
            "request_log_payload_manifest_fields",
            "request_log_payload_manifests",
            "request_log_payload_blobs",
            "request_log_payloads",
        ] {
            if self.has_table(table)? {
                self.conn.execute(&format!("DELETE FROM {table}"), [])?;
            }
        }
        Ok(())
    }

    /// Ask the blob sweep to (re)check every blob that exists now.
    pub(super) fn mark_request_log_payload_blob_gc(&self) -> Result<()> {
        if !self.has_request_log_payload_purges()?
            || !self.has_table("request_log_payload_blobs")?
        {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO request_log_payload_purges (table_name, max_rowid, cleared_at, cursor_rowid)
             SELECT ?1, COALESCE(MAX(id), 0), 0, 0 FROM request_log_payload_blobs
             WHERE 1
             ON CONFLICT(table_name) DO UPDATE SET
                max_rowid = MAX(max_rowid, excluded.max_rowid),
                cursor_rowid = 0",
            [BLOB_GC_MARKER],
        )?;
        Ok(())
    }

    /// Part of the prune transaction: surviving manifests whose parent is
    /// older than `cutoff` take ownership of all their items, so the parent
    /// can be deleted later without breaking them.
    pub(super) fn rebase_request_log_payload_survivors(&self, cutoff: i64) -> Result<()> {
        if !self.has_table(MANIFESTS_TABLE)? {
            return Ok(());
        }
        let mut stmt = self.conn.prepare(
            "SELECT m.trace_id, m.stage, m.item_count FROM request_log_payload_manifests m
             WHERE m.created_at >= ?1 AND EXISTS (
                 SELECT 1 FROM request_log_payload_manifests parent
                 WHERE parent.trace_id = m.parent_trace_id
                   AND parent.stage = m.stage
                   AND parent.created_at < ?1)",
        )?;
        let survivors = stmt
            .query_map([cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (trace_id, stage, item_count) in survivors {
            self.materialize_request_log_payload_manifest(&trace_id, &stage, item_count)?;
        }
        Ok(())
    }

    /// Whether a clear purge or blob sweep is still pending.
    pub fn request_log_payload_purge_pending(&self) -> Result<bool> {
        if !self.has_request_log_payload_purges()? {
            return Ok(false);
        }
        self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM request_log_payload_purges)",
            [],
            |row| row.get(0),
        )
    }

    /// Delete doomed payload rows in short transactions. `budget = None`
    /// runs until everything is done. Every batch re-reads the persisted
    /// markers, so this continues correctly after a restart. When another
    /// drain already works on the same database file this returns at once
    /// with `pending` set instead of competing for the write lock.
    pub fn drain_request_log_payload_purges(
        &self,
        budget: Option<Duration>,
    ) -> Result<RequestLogPayloadPurgeProgress> {
        let mut progress = RequestLogPayloadPurgeProgress::default();
        if !self.has_request_log_payload_purges()? {
            return Ok(progress);
        }
        let Some(_slot) = DrainSlot::acquire(self.conn.path()) else {
            progress.pending = true;
            return Ok(progress);
        };
        let started = Instant::now();
        let out_of_time = || budget.is_some_and(|budget| started.elapsed() >= budget);
        let mut batch = BatchSize::new();
        self.with_secure_delete(|| {
            for table in PURGED_PAYLOAD_TABLES {
                if !self.has_table(table)? {
                    continue;
                }
                loop {
                    let Some(marker) = self.request_log_payload_purge_marker(table)? else {
                        break;
                    };
                    if out_of_time() {
                        progress.pending = true;
                        return Ok(progress);
                    }
                    let (deleted, finished) = self.purge_request_log_payload_batch(
                        table,
                        Doomed::Cleared(marker),
                        &mut batch,
                    )?;
                    progress.deleted_rows += deleted;
                    if finished {
                        break;
                    }
                    batch.pause();
                }
            }

            let cutoff = self.request_log_payload_retention_cutoff()?;
            if cutoff > 0 {
                for table in PURGED_PAYLOAD_TABLES {
                    if !self.has_table(table)? {
                        continue;
                    }
                    loop {
                        if out_of_time() {
                            progress.pending = true;
                            return Ok(progress);
                        }
                        let (deleted, finished) = self.purge_request_log_payload_batch(
                            table,
                            Doomed::Retention { cutoff },
                            &mut batch,
                        )?;
                        progress.deleted_rows += deleted;
                        if finished {
                            break;
                        }
                        batch.pause();
                    }
                }
            }

            loop {
                if out_of_time() {
                    progress.pending = self.request_log_payload_purge_pending()?;
                    return Ok(progress);
                }
                let sweep_started = Instant::now();
                let Some(deleted) = self.sweep_request_log_payload_blob_batch()? else {
                    break;
                };
                batch.last_elapsed = sweep_started.elapsed();
                progress.deleted_rows += deleted;
                batch.pause();
            }
            Ok(progress)
        })
    }

    fn request_log_payload_purge_marker(&self, table: &str) -> Result<Option<ClearMarker>> {
        self.conn
            .query_row(
                "SELECT max_rowid, cleared_at, generation
                 FROM request_log_payload_purges WHERE table_name = ?1",
                [table],
                |row| {
                    Ok(ClearMarker {
                        max_rowid: row.get(0)?,
                        cleared_at: row.get(1)?,
                        generation: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    fn request_log_payload_retention_cutoff(&self) -> Result<i64> {
        if !self.has_table("request_log_payload_state")? {
            return Ok(0);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT retention_cutoff FROM request_log_payload_state WHERE id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    /// Delete one batch of doomed rows. Returns the number of deleted rows
    /// and whether the table has no doomed rows left (a finished clear purge
    /// also drops its marker in the same transaction).
    fn purge_request_log_payload_batch(
        &self,
        table: &str,
        doomed: Doomed,
        batch: &mut BatchSize,
    ) -> Result<(usize, bool)> {
        let limit = batch.rows(table);
        let (predicate, params) = doomed.predicate(table);
        let started = Instant::now();
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT rowid FROM {table} WHERE {predicate} LIMIT {limit}"
        ))?;
        let rowids = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>>>()?;
        drop(stmt);
        let mut deleted = 0;
        if table == MANIFESTS_TABLE && !rowids.is_empty() {
            deleted += self.delete_request_log_payload_manifest_children(&rowids)?;
            self.mark_request_log_payload_blob_gc()?;
        }
        for chunk in rowids.chunks(ID_CHUNK) {
            deleted += self.conn.execute(
                &format!(
                    "DELETE FROM {table} WHERE rowid IN ({})",
                    placeholders(chunk.len(), "?")
                ),
                rusqlite::params_from_iter(chunk.iter().copied()),
            )?;
        }
        let finished = (rowids.len() as i64) < limit;
        if let (true, Doomed::Cleared(marker)) = (finished, doomed) {
            // Only the marker this batch worked on: a clear that committed
            // since then has merged a higher bound into it.
            self.conn.execute(
                "DELETE FROM request_log_payload_purges
                 WHERE table_name = ?1 AND max_rowid = ?2 AND cleared_at = ?3
                   AND generation IS ?4",
                (
                    table,
                    marker.max_rowid,
                    marker.cleared_at,
                    marker.generation,
                ),
            )?;
        }
        tx.commit()?;
        batch.adapt(started.elapsed());
        Ok((deleted, finished))
    }

    /// Items and fields of the given manifests (by rowid).
    fn delete_request_log_payload_manifest_children(&self, rowids: &[i64]) -> Result<usize> {
        let mut keys = Vec::with_capacity(rowids.len());
        for chunk in rowids.chunks(ID_CHUNK) {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT trace_id, stage FROM request_log_payload_manifests WHERE rowid IN ({})",
                placeholders(chunk.len(), "?")
            ))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
            for row in rows {
                keys.push(row?);
            }
        }
        let mut deleted = 0;
        for chunk in keys.chunks(KEY_CHUNK) {
            let params = chunk
                .iter()
                .flat_map(|(trace_id, stage)| {
                    [Value::Text(trace_id.clone()), Value::Text(stage.clone())]
                })
                .collect::<Vec<_>>();
            for child in [
                "request_log_payload_manifest_items",
                "request_log_payload_manifest_fields",
            ] {
                deleted += self.conn.execute(
                    &format!(
                        "DELETE FROM {child} WHERE (trace_id, stage) IN (VALUES {})",
                        placeholders(chunk.len(), "(?, ?)")
                    ),
                    rusqlite::params_from_iter(params.iter().cloned()),
                )?;
            }
        }
        Ok(deleted)
    }

    /// Check the next range of blobs and delete the ones no manifest item or
    /// field references. Returns `None` when the sweep is complete. The
    /// reference check runs inside the delete transaction, so a blob reused
    /// by a concurrent writer is never removed.
    fn sweep_request_log_payload_blob_batch(&self) -> Result<Option<usize>> {
        let tx = self.conn.unchecked_transaction()?;
        let marker: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT max_rowid, cursor_rowid FROM request_log_payload_purges WHERE table_name = ?1",
                [BLOB_GC_MARKER],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((max_rowid, cursor)) = marker else {
            tx.commit()?;
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id FROM request_log_payload_blobs
             WHERE id > ?1 AND id <= ?2 ORDER BY id LIMIT {BLOB_SCAN_ROWS}"
        ))?;
        let ids = stmt
            .query_map((cursor, max_rowid), |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>>>()?;
        drop(stmt);
        let Some(&last) = ids.last() else {
            self.conn.execute(
                "DELETE FROM request_log_payload_purges
                 WHERE table_name = ?1 AND max_rowid = ?2 AND cursor_rowid = ?3",
                (BLOB_GC_MARKER, max_rowid, cursor),
            )?;
            tx.commit()?;
            return Ok(None);
        };
        let mut deleted = 0;
        for chunk in ids.chunks(ID_CHUNK) {
            deleted += self.conn.execute(
                &format!(
                    "DELETE FROM request_log_payload_blobs
                     WHERE id IN ({})
                       AND NOT EXISTS (SELECT 1 FROM request_log_payload_manifest_items i
                                       WHERE i.blob_id = request_log_payload_blobs.id)
                       AND NOT EXISTS (SELECT 1 FROM request_log_payload_manifest_fields f
                                       WHERE f.blob_id = request_log_payload_blobs.id)",
                    placeholders(chunk.len(), "?")
                ),
                rusqlite::params_from_iter(chunk.iter().copied()),
            )?;
        }
        self.conn.execute(
            "UPDATE request_log_payload_purges SET cursor_rowid = ?1
             WHERE table_name = ?2 AND cursor_rowid = ?3",
            (last, BLOB_GC_MARKER, cursor),
        )?;
        tx.commit()?;
        Ok(Some(deleted))
    }
}

#[cfg(test)]
#[path = "request_log_payload_purge_tests.rs"]
mod tests;
