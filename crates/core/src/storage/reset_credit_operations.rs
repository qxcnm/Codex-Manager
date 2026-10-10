use rusqlite::{Error, OptionalExtension, Result, Row, Transaction};

use super::Storage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetCreditOperationStatus {
    Pending,
    Completed,
    Failed,
}

impl ResetCreditOperationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetCreditOperation {
    pub operation_id: String,
    pub account_id: String,
    pub redeem_request_id: String,
    pub status: ResetCreditOperationStatus,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetCreditOperationClaim {
    Created(ResetCreditOperation),
    Existing(ResetCreditOperation),
    /// Another operation for this account is still pending. Callers must not
    /// start a second upstream redemption until that outcome is reconciled.
    PendingAccount(ResetCreditOperation),
    AccountConflict(ResetCreditOperation),
}

impl ResetCreditOperationClaim {
    pub fn operation(&self) -> &ResetCreditOperation {
        match self {
            Self::Created(operation)
            | Self::Existing(operation)
            | Self::PendingAccount(operation)
            | Self::AccountConflict(operation) => operation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetCreditOperationUpdate {
    Updated(ResetCreditOperation),
    Existing(ResetCreditOperation),
    AccountConflict(ResetCreditOperation),
    NotFound,
}

impl ResetCreditOperationUpdate {
    pub fn operation(&self) -> Option<&ResetCreditOperation> {
        match self {
            Self::Updated(operation)
            | Self::Existing(operation)
            | Self::AccountConflict(operation) => Some(operation),
            Self::NotFound => None,
        }
    }
}

fn invalid_stored_status(value: String) -> Error {
    Error::FromSql(format!("invalid reset credit operation status: {value}"))
}

fn operation_from_row(row: &Row<'_>) -> Result<ResetCreditOperation> {
    let raw_status: String = row.get(3)?;
    let status = ResetCreditOperationStatus::from_storage(&raw_status)
        .ok_or_else(|| invalid_stored_status(raw_status))?;
    Ok(ResetCreditOperation {
        operation_id: row.get(0)?,
        account_id: row.get(1)?,
        redeem_request_id: row.get(2)?,
        status,
        result_json: row.get(4)?,
        error: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn validate_identifier(name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        return Err(Error::InvalidParameterName(format!(
            "{name} must be non-empty and trimmed"
        )));
    }
    Ok(())
}

const OPERATION_SELECT_SQL: &str =
    "SELECT operation_id, account_id, redeem_request_id, status, result_json, error,\n\
            created_at, updated_at\n\
     FROM reset_credit_operations WHERE operation_id = ?1";

fn operation_in_transaction(
    tx: &Transaction<'_>,
    operation_id: &str,
) -> Result<Option<ResetCreditOperation>> {
    tx.query_row(OPERATION_SELECT_SQL, [operation_id], operation_from_row)
        .optional()
}

/// Returns the pending operation protected by the account guard. Stale guards
/// are removed while the caller's write transaction is still holding the DB
/// lock, so a crashed/legacy record cannot permanently block the account.
fn pending_operation_for_guard(
    tx: &Transaction<'_>,
    account_id: &str,
) -> Result<Option<ResetCreditOperation>> {
    let guard_operation_id: Option<String> = tx
        .query_row(
            "SELECT operation_id FROM reset_credit_operation_accounts
             WHERE account_id = ?1",
            [account_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(guard_operation_id) = guard_operation_id else {
        return Ok(None);
    };
    let operation = operation_in_transaction(tx, &guard_operation_id)?;
    if let Some(operation) = operation.filter(|operation| {
        operation.account_id == account_id
            && operation.status == ResetCreditOperationStatus::Pending
    }) {
        return Ok(Some(operation));
    }
    tx.execute(
        "DELETE FROM reset_credit_operation_accounts
         WHERE account_id = ?1 AND operation_id = ?2",
        (account_id, guard_operation_id),
    )?;
    Ok(None)
}

impl Storage {
    pub fn get_reset_credit_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<ResetCreditOperation>> {
        validate_identifier("operationId", operation_id)?;
        let mut statement = self.conn.prepare(
            "SELECT operation_id, account_id, redeem_request_id, status, result_json, error,
                    created_at, updated_at
             FROM reset_credit_operations WHERE operation_id = ?1",
        )?;
        let mut rows = statement.query([operation_id])?;
        rows.next()?.map(operation_from_row).transpose()
    }

    pub fn find_pending_reset_credit_operation(
        &self,
        account_id: &str,
    ) -> Result<Option<ResetCreditOperation>> {
        validate_identifier("accountId", account_id)?;
        let mut statement = self.conn.prepare(
            "SELECT operation_id, account_id, redeem_request_id, status, result_json, error,
                    created_at, updated_at
             FROM reset_credit_operations
             WHERE account_id = ?1 AND status = 'pending'
             ORDER BY created_at ASC, operation_id ASC
             LIMIT 1",
        )?;
        let mut rows = statement.query([account_id])?;
        rows.next()?.map(operation_from_row).transpose()
    }

    /// Claims a caller-generated operation id exactly once. The persisted
    /// redeem request id is authoritative for every later replay.
    pub fn claim_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        redeem_request_id: &str,
        now: i64,
    ) -> Result<ResetCreditOperationClaim> {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        validate_identifier("redeemRequestId", redeem_request_id)?;

        // BEGIN IMMEDIATE makes the guard check and insert one atomic claim
        // across all processes sharing the SQLite file.
        let tx = self.conn.unchecked_transaction()?;
        if let Some(operation) = operation_in_transaction(&tx, operation_id)? {
            return if operation.account_id != account_id {
                Ok(ResetCreditOperationClaim::AccountConflict(operation))
            } else {
                // A pending operation created before migration 136 may not have
                // a guard yet. Repair it before allowing a replay.
                if operation.status == ResetCreditOperationStatus::Pending {
                    if let Some(guarded) = pending_operation_for_guard(&tx, account_id)? {
                        if guarded.operation_id != operation_id {
                            tx.commit()?;
                            return Ok(ResetCreditOperationClaim::PendingAccount(guarded));
                        }
                    } else {
                        tx.execute(
                            "INSERT OR IGNORE INTO reset_credit_operation_accounts
                             (account_id, operation_id, created_at) VALUES (?1, ?2, ?3)",
                            (account_id, operation_id, operation.created_at),
                        )?;
                        if let Some(guarded) = pending_operation_for_guard(&tx, account_id)? {
                            if guarded.operation_id != operation_id {
                                tx.commit()?;
                                return Ok(ResetCreditOperationClaim::PendingAccount(guarded));
                            }
                        }
                    }
                } else {
                    // Clean up a stale guard left by an interrupted migration.
                    let _ = pending_operation_for_guard(&tx, account_id)?;
                }
                tx.commit()?;
                Ok(ResetCreditOperationClaim::Existing(operation))
            };
        }
        if let Some(operation) = pending_operation_for_guard(&tx, account_id)? {
            tx.commit()?;
            return Ok(ResetCreditOperationClaim::PendingAccount(operation));
        }

        let guard_created = tx.execute(
            "INSERT OR IGNORE INTO reset_credit_operation_accounts
             (account_id, operation_id, created_at) VALUES (?1, ?2, ?3)",
            (account_id, operation_id, now),
        )? == 1;
        if !guard_created {
            let operation =
                pending_operation_for_guard(&tx, account_id)?.ok_or(Error::QueryReturnedNoRows)?;
            tx.commit()?;
            return Ok(ResetCreditOperationClaim::PendingAccount(operation));
        }

        tx.execute(
            "INSERT INTO reset_credit_operations
                (operation_id, account_id, redeem_request_id, status, result_json, error,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, 'pending', NULL, NULL, ?4, ?4)",
            (operation_id, account_id, redeem_request_id, now),
        )?;
        let operation =
            operation_in_transaction(&tx, operation_id)?.ok_or(Error::QueryReturnedNoRows)?;
        tx.commit()?;
        Ok(ResetCreditOperationClaim::Created(operation))
    }

    pub fn complete_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate> {
        self.finish_reset_credit_operation(
            operation_id,
            account_id,
            ResetCreditOperationStatus::Completed,
            Some(result_json),
            None,
            now,
        )
    }

    pub fn fail_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        error: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate> {
        self.finish_reset_credit_operation(
            operation_id,
            account_id,
            ResetCreditOperationStatus::Failed,
            None,
            Some(error),
            now,
        )
    }

    /// Enriches a successful operation after best-effort usage refreshes. This
    /// cannot move pending or failed operations into a successful state.
    pub fn update_completed_reset_credit_operation_result(
        &self,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate> {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE reset_credit_operations
             SET result_json = ?3, updated_at = ?4
             WHERE operation_id = ?1 AND account_id = ?2 AND status = 'completed'",
            (operation_id, account_id, result_json, now),
        )? == 1;
        let Some(operation) = operation_in_transaction(&tx, operation_id)? else {
            tx.commit()?;
            return Ok(ResetCreditOperationUpdate::NotFound);
        };
        if operation.account_id != account_id {
            tx.commit()?;
            Ok(ResetCreditOperationUpdate::AccountConflict(operation))
        } else {
            // Release the account guard only after the terminal state is
            // written, in the same transaction. This prevents a second
            // redemption from entering between the two operations.
            tx.execute(
                "DELETE FROM reset_credit_operation_accounts
                 WHERE account_id = ?1 AND operation_id = ?2",
                (account_id, operation_id),
            )?;
            tx.commit()?;
            if updated {
                Ok(ResetCreditOperationUpdate::Updated(operation))
            } else {
                Ok(ResetCreditOperationUpdate::Existing(operation))
            }
        }
    }

    fn finish_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        status: ResetCreditOperationStatus,
        result_json: Option<&str>,
        error: Option<&str>,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate> {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        // Keep the terminal state transition and account-guard release in one
        // write transaction. A second process must not claim the account in
        // the interval after the operation becomes terminal but before its
        // guard is removed.
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE reset_credit_operations
             SET status = ?3, result_json = ?4, error = ?5, updated_at = ?6
             WHERE operation_id = ?1 AND account_id = ?2 AND status = 'pending'",
            (
                operation_id,
                account_id,
                status.as_str(),
                result_json,
                error,
                now,
            ),
        )? == 1;
        let Some(operation) = operation_in_transaction(&tx, operation_id)? else {
            tx.commit()?;
            return Ok(ResetCreditOperationUpdate::NotFound);
        };
        if operation.account_id != account_id {
            tx.commit()?;
            Ok(ResetCreditOperationUpdate::AccountConflict(operation))
        } else {
            tx.execute(
                "DELETE FROM reset_credit_operation_accounts
                 WHERE account_id = ?1 AND operation_id = ?2",
                (account_id, operation_id),
            )?;
            tx.commit()?;
            if updated {
                Ok(ResetCreditOperationUpdate::Updated(operation))
            } else {
                Ok(ResetCreditOperationUpdate::Existing(operation))
            }
        }
    }
}

#[cfg(test)]
#[path = "reset_credit_operations_tests.rs"]
mod tests;
