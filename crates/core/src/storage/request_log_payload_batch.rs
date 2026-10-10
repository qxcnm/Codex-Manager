//! Group commit of request payload writes.
//!
//! The service writer persists many captured jobs in one SQLite write
//! transaction instead of one transaction per job. Every job runs inside its
//! own savepoint so a failing job is rolled back without losing the rest of
//! the batch. All `*_if_current` guards keep their single-job semantics:
//! the generation / retention check runs inside the same transaction as the
//! write, so a concurrent clear or prune can never interleave.

use std::cell::Cell;

use rusqlite::Result;

use super::{
    RequestLogPayload, RequestLogPayloadManifest, RequestLogPayloadManifestInput,
    RequestLogPayloadManifestWrite, RequestLogPayloadParentHint, RequestLogUpstreamAttempt,
    Storage,
};

/// Handle passed to the closure of [`Storage::write_request_log_payload_batch`].
pub struct RequestLogPayloadBatch<'s> {
    storage: &'s Storage,
    next_savepoint: Cell<u64>,
}

impl Storage {
    /// Run `write` inside one `BEGIN IMMEDIATE` transaction and commit it.
    /// The closure's value is returned only when the commit succeeded; on a
    /// commit error nothing of the batch is persisted.
    pub fn write_request_log_payload_batch<T>(
        &self,
        write: impl FnOnce(&RequestLogPayloadBatch<'_>) -> T,
    ) -> Result<T> {
        let tx = self.conn.unchecked_transaction()?;
        let batch = RequestLogPayloadBatch {
            storage: self,
            next_savepoint: Cell::new(0),
        };
        let value = write(&batch);
        tx.commit()?;
        Ok(value)
    }
}

/// SQLite reported `SQLITE_BUSY` / `SQLITE_LOCKED` (any extended code):
/// another connection holds the write lock (a clear's batched deletes, a
/// maintenance task, ...). The request payload writer retries these
/// instead of dropping the batch.
pub fn is_sqlite_busy_error(err: &rusqlite::Error) -> bool {
    let rusqlite::Error::SqliteFailure(_, Some(message)) = err else {
        return false;
    };
    if let Some(rest) = message.split("(code: ").nth(1) {
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if let Ok(code) = digits.parse::<u32>() {
            return matches!(code & 0xff, 5 | 6);
        }
    }
    let message = message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("sqlite_busy")
}

impl RequestLogPayloadBatch<'_> {
    /// Run one job inside a savepoint. An error rolls back only this job.
    pub fn entry<T>(&self, job: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let index = self.next_savepoint.get();
        self.next_savepoint.set(index + 1);
        let name = format!("cm_request_log_payload_{index}");
        self.storage
            .conn
            .execute_batch(&format!("SAVEPOINT {name}"))?;
        match job(self) {
            Ok(value) => {
                self.storage
                    .conn
                    .execute_batch(&format!("RELEASE {name}"))?;
                Ok(value)
            }
            Err(err) => {
                let _ = self
                    .storage
                    .conn
                    .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                Err(err)
            }
        }
    }

    /// Clear generation as seen by this transaction. Stable until commit
    /// because the transaction holds the SQLite write lock.
    pub fn current_generation(&self) -> Result<i64> {
        self.storage.request_log_payload_generation()
    }

    /// Whether a job captured at `generation` / `created_at` would be
    /// accepted now (same clear-generation and retention check as the
    /// `*_if_current` writes, in this transaction).
    pub fn job_is_current(&self, generation: i64, created_at: i64) -> Result<bool> {
        self.storage
            .request_log_payload_job_is_current(generation, created_at)
    }

    /// Batched form of [`Storage::insert_request_log_payload_if_current`].
    pub fn insert_request_log_payload_if_current(
        &self,
        payload: &RequestLogPayload,
        generation: i64,
    ) -> Result<bool> {
        self.storage
            .insert_request_log_payload_if_current_in_tx(payload, generation)
    }

    /// Batched form of
    /// [`Storage::insert_request_log_payload_manifest_if_current`].
    pub fn insert_request_log_payload_manifest_if_current(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: i64,
    ) -> Result<RequestLogPayloadManifestWrite> {
        if !self.storage.has_table("request_log_payload_manifests")? {
            return Ok(RequestLogPayloadManifestWrite::default());
        }
        if let Some(identical) = self
            .storage
            .request_log_payload_manifest_identical_to_client(input)?
        {
            return Ok(identical);
        }
        self.storage
            .insert_request_log_payload_manifest_in_tx(input, parent_hint, Some(generation))
    }

    /// Batched form of
    /// [`Storage::record_request_log_upstream_attempt_if_current`].
    pub fn record_request_log_upstream_attempt_if_current(
        &self,
        attempt: &RequestLogUpstreamAttempt,
        generation: i64,
    ) -> Result<bool> {
        self.storage
            .record_request_log_upstream_attempt_if_current_in_tx(attempt, generation)
    }

    /// Stored preview body hash (sees earlier writes of this batch).
    pub fn find_request_log_payload_body_hash(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<String>> {
        self.storage
            .find_request_log_payload_body_hash(trace_id, stage)
    }

    /// Stored manifest (sees earlier writes of this batch).
    pub fn find_request_log_payload_manifest(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<RequestLogPayloadManifest>> {
        self.storage
            .find_request_log_payload_manifest(trace_id, stage)
    }
}

#[cfg(test)]
#[path = "request_log_payload_batch_tests.rs"]
mod tests;
