//! Service account persistence selects one authoritative database.
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::{AccountsRepository, ResetCreditOperationsRepository};
use std::{collections::HashMap, ops::Deref};
pub(crate) struct AccountStorage<'a>(&'a Storage);
impl<'a> AccountStorage<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        Self(storage)
    }
}
impl Deref for AccountStorage<'_> {
    type Target = Storage;
    fn deref(&self) -> &Storage {
        self.0
    }
}
fn storage_error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn get_reset_credit_operation(
        &self,
        operation_id: &str,
    ) -> rusqlite::Result<Option<ResetCreditOperation>> {
        if !seaorm_enabled() {
            return self.0.get_reset_credit_operation(operation_id);
        }
        let operation_id = operation_id.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::get(remote.connection(), &operation_id)
                .await
                .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn find_pending_reset_credit_operation(
        &self,
        account_id: &str,
    ) -> rusqlite::Result<Option<ResetCreditOperation>> {
        if !seaorm_enabled() {
            return self.0.find_pending_reset_credit_operation(account_id);
        }
        let account_id = account_id.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::find_pending(remote.connection(), &account_id)
                .await
                .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn claim_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        redeem_request_id: &str,
        now: i64,
    ) -> rusqlite::Result<ResetCreditOperationClaim> {
        if !seaorm_enabled() {
            return self.0.claim_reset_credit_operation(
                operation_id,
                account_id,
                redeem_request_id,
                now,
            );
        }
        let operation_id = operation_id.to_owned();
        let account_id = account_id.to_owned();
        let redeem_request_id = redeem_request_id.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::claim(
                remote.connection(),
                &operation_id,
                &account_id,
                &redeem_request_id,
                now,
            )
            .await
            .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn complete_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> rusqlite::Result<ResetCreditOperationUpdate> {
        if !seaorm_enabled() {
            return self.0.complete_reset_credit_operation(
                operation_id,
                account_id,
                result_json,
                now,
            );
        }
        let operation_id = operation_id.to_owned();
        let account_id = account_id.to_owned();
        let result_json = result_json.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::complete(
                remote.connection(),
                &operation_id,
                &account_id,
                &result_json,
                now,
            )
            .await
            .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn fail_reset_credit_operation(
        &self,
        operation_id: &str,
        account_id: &str,
        error: &str,
        now: i64,
    ) -> rusqlite::Result<ResetCreditOperationUpdate> {
        if !seaorm_enabled() {
            return self
                .0
                .fail_reset_credit_operation(operation_id, account_id, error, now);
        }
        let operation_id = operation_id.to_owned();
        let account_id = account_id.to_owned();
        let error = error.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::fail(
                remote.connection(),
                &operation_id,
                &account_id,
                &error,
                now,
            )
            .await
            .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn update_completed_reset_credit_operation_result(
        &self,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> rusqlite::Result<ResetCreditOperationUpdate> {
        if !seaorm_enabled() {
            return self.0.update_completed_reset_credit_operation_result(
                operation_id,
                account_id,
                result_json,
                now,
            );
        }
        let operation_id = operation_id.to_owned();
        let account_id = account_id.to_owned();
        let result_json = result_json.to_owned();
        seaorm_block_on(move |remote| async move {
            ResetCreditOperationsRepository::update_completed_result(
                remote.connection(),
                &operation_id,
                &account_id,
                &result_json,
                now,
            )
            .await
            .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }

    pub(crate) fn list_accounts(&self) -> rusqlite::Result<Vec<Account>> {
        if !seaorm_enabled() {
            return self.0.list_accounts();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_accounts(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_by_id(&self, id: &str) -> rusqlite::Result<Option<Account>> {
        if !seaorm_enabled() {
            return self.0.find_account_by_id(id);
        }
        let id = id.to_owned();
        if let Some(tx) = current_usage_transaction() {
            return crate::storage_helpers::seaorm_transaction_block_on(async move {
                AccountsRepository::find_account_by_id(tx.as_ref(), &id)
                    .await
                    .map_err(|e| e.to_string())
            })
            .map_err(storage_error);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_token_by_account_id(&self, id: &str) -> rusqlite::Result<Option<Token>> {
        if !seaorm_enabled() {
            return self.0.find_token_by_account_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_token_by_account_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_with_token_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<(Account, Token)>> {
        if !seaorm_enabled() {
            return self.0.find_account_with_token_by_id(id);
        }
        let id = id.to_owned();
        if let Some(tx) = current_usage_transaction() {
            return crate::storage_helpers::seaorm_transaction_block_on(async move {
                AccountsRepository::find_account_with_token_by_id(tx.as_ref(), &id)
                    .await
                    .map_err(|e| e.to_string())
            })
            .map_err(storage_error);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_with_token_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_accounts_for_ids(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<Account>> {
        if !seaorm_enabled() {
            return self.0.list_accounts_for_ids(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_accounts_for_ids(remote.connection(), &account_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_tokens_for_accounts(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<Token>> {
        if !seaorm_enabled() {
            return self.0.list_tokens_for_accounts(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_tokens_for_accounts(remote.connection(), &account_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_token_plans_for_accounts(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<AccountTokenPlan>> {
        if !seaorm_enabled() {
            return self.0.list_account_token_plans_for_accounts(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_token_plans_for_accounts(
                remote.connection(),
                &account_ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_import_token_subjects(
        &self,
    ) -> rusqlite::Result<Vec<AccountImportTokenSubject>> {
        if !seaorm_enabled() {
            return self.0.list_account_import_token_subjects();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_import_token_subjects(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_import_snapshots(
        &self,
    ) -> rusqlite::Result<Vec<AccountImportSnapshot>> {
        if !seaorm_enabled() {
            return self.0.list_account_import_snapshots();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_import_snapshots(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_summary_rows(&self) -> rusqlite::Result<Vec<AccountListSummaryRow>> {
        if !seaorm_enabled() {
            return self.0.list_account_summary_rows();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_summary_rows(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_ids_for_ids(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.0.list_account_ids_for_ids(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_ids_for_ids(remote.connection(), &account_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn account_count(&self) -> rusqlite::Result<i64> {
        if !seaorm_enabled() {
            return self.0.account_count();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::account_count(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn account_exists(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.account_exists(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::account_exists(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn max_account_sort(&self) -> rusqlite::Result<Option<i64>> {
        if !seaorm_enabled() {
            return self.0.max_account_sort();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::max_account_sort(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_status_by_id(&self, id: &str) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.0.find_account_status_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_status_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_upsert_state_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountUpsertState>> {
        if !seaorm_enabled() {
            return self.0.find_account_upsert_state_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_upsert_state_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_workspace_identity_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountWorkspaceIdentity>> {
        if !seaorm_enabled() {
            return self.0.find_account_workspace_identity_by_id(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_workspace_identity_by_id(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn insert_account(&self, a: &Account) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.insert_account(a);
        }
        let a = a.clone();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::insert_account(remote.connection(), &a)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn insert_token(&self, t: &Token) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.insert_token(t);
        }
        let t = t.clone();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::insert_token(remote.connection(), &t)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn compare_and_swap_token(
        &self,
        expected: &Token,
        next: &Token,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.compare_and_swap_token(expected, next);
        }
        let expected = expected.clone();
        let next = next.clone();
        seaorm_block_on(move |remote| async move {
            codexmanager_storage_seaorm::AccountTokensRepository::compare_and_swap_token(
                remote.connection(),
                &expected,
                &next,
            )
            .await
            .map_err(|error| error.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_cleanup_candidates_by_statuses(
        &self,
        statuses: &[String],
    ) -> rusqlite::Result<Vec<AccountCleanupCandidate>> {
        if !seaorm_enabled() {
            return self.0.list_account_cleanup_candidates_by_statuses(statuses);
        }
        let statuses = statuses.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_cleanup_candidates_by_statuses(
                remote.connection(),
                &statuses,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_token_refresh_issuers_for_ids(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<AccountTokenRefreshIssuer>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_token_refresh_issuers_for_ids(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_token_refresh_issuers_for_ids(
                remote.connection(),
                &account_ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_auth_refresh_targets(
        &self,
    ) -> rusqlite::Result<Vec<AccountAuthRefreshTarget>> {
        if !seaorm_enabled() {
            return self.0.list_account_auth_refresh_targets();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_auth_refresh_targets(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_workspace_identities_for_subject(
        &self,
        subject: &str,
    ) -> rusqlite::Result<Vec<AccountWorkspaceIdentity>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_workspace_identities_for_subject(subject);
        }
        let subject = subject.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_workspace_identities_for_subject(
                remote.connection(),
                &subject,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_with_token_by_identity(
        &self,
        id: Option<&str>,
        chatgpt: Option<&str>,
        workspace: Option<&str>,
    ) -> rusqlite::Result<Option<(Account, Token)>> {
        if !seaorm_enabled() {
            return self
                .0
                .find_account_with_token_by_identity(id, chatgpt, workspace);
        }
        let id = id.map(str::to_owned);
        let chatgpt = chatgpt.map(str::to_owned);
        let workspace = workspace.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_with_token_by_identity(
                remote.connection(),
                id.as_deref(),
                chatgpt.as_deref(),
                workspace.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn touch_account_updated_at(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.touch_account_updated_at(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::touch_account_updated_at(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_subject_identity(
        &self,
        id: &str,
        subject: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_account_subject_identity(id, subject);
        }
        let id = id.to_owned();
        let subject = subject.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_subject_identity(remote.connection(), &id, &subject)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_workspace_identity(
        &self,
        id: &str,
        chatgpt: Option<&str>,
        workspace: Option<&str>,
        updated_at: i64,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .0
                .update_account_workspace_identity(id, chatgpt, workspace, updated_at);
        }
        let id = id.to_owned();
        let chatgpt = chatgpt.map(str::to_owned);
        let workspace = workspace.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_workspace_identity(
                remote.connection(),
                &id,
                chatgpt.as_deref(),
                workspace.as_deref(),
                updated_at,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_status_if_context_matches(
        &self,
        id: &str,
        expected: &str,
        at: i64,
        next: &str,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .0
                .update_account_status_if_context_matches(id, expected, at, next);
        }
        let id = id.to_owned();
        let expected = expected.to_owned();
        let next = next.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_status_if_context_matches(
                remote.connection(),
                &id,
                &expected,
                at,
                &next,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_status_if_changed_with_existence(
        &self,
        id: &str,
        status: &str,
    ) -> rusqlite::Result<(bool, bool)> {
        if !seaorm_enabled() {
            return self
                .0
                .update_account_status_if_changed_with_existence(id, status);
        }
        let id = id.to_owned();
        let status = status.to_owned();
        if let Some(tx) = current_usage_transaction() {
            return crate::storage_helpers::seaorm_transaction_block_on(async move {
                AccountsRepository::update_account_status_if_changed_with_existence(
                    tx.as_ref(),
                    &id,
                    &status,
                )
                .await
                .map_err(|e| e.to_string())
            })
            .map_err(storage_error);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_status_if_changed_with_existence(
                remote.connection(),
                &id,
                &status,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_sorts(
        &self,
        values: &[(String, i64)],
        updated_at: i64,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.0.update_account_sorts(values, updated_at);
        }
        let values = values.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_sorts(remote.connection(), &values, updated_at)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn preferred_account_id(&self) -> rusqlite::Result<Option<String>> {
        if !seaorm_enabled() {
            return self.0.preferred_account_id();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::preferred_account_id(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn set_preferred_account(&self, id: Option<&str>) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.set_preferred_account(id);
        }
        let id = id.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::set_preferred_account(remote.connection(), id.as_deref())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn clear_preferred_account_if(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.clear_preferred_account_if(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::clear_preferred_account_if(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_metadata(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountMetadata>> {
        if !seaorm_enabled() {
            return self.0.find_account_metadata(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_metadata(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_account_metadata(
        &self,
        id: &str,
        note: Option<&str>,
        tags: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.upsert_account_metadata(id, note, tags);
        }
        let id = id.to_owned();
        let note = note.map(str::to_owned);
        let tags = tags.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::upsert_account_metadata(
                remote.connection(),
                &id,
                note.as_deref(),
                tags.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_subscription(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountSubscription>> {
        if !seaorm_enabled() {
            return self.0.find_account_subscription(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_subscription(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_account_subscription(
        &self,
        id: &str,
        has: bool,
        account_plan: Option<&str>,
        plan: Option<&str>,
        expires: Option<i64>,
        renews: Option<i64>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.upsert_account_subscription(
                id,
                has,
                account_plan,
                plan,
                expires,
                renews,
            );
        }
        let id = id.to_owned();
        let account_plan = account_plan.map(str::to_owned);
        let plan = plan.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::upsert_account_subscription(
                remote.connection(),
                &id,
                has,
                account_plan.as_deref(),
                plan.as_deref(),
                expires,
                renews,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_agent_identity(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountAgentIdentity>> {
        if !seaorm_enabled() {
            return self.0.find_account_agent_identity(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_agent_identity(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_account_agent_identity(
        &self,
        identity: &AccountAgentIdentity,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.upsert_account_agent_identity(identity);
        }
        let identity = identity.clone();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::upsert_account_agent_identity(remote.connection(), &identity)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_imported_account_bundle(
        &self,
        account: &Account,
        note: Option<&str>,
        tags: Option<&str>,
        token: &Token,
        identity: Option<&AccountAgentIdentity>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .0
                .upsert_imported_account_bundle(account, note, tags, token, identity);
        }
        let account = account.clone();
        let note = note.map(str::to_owned);
        let tags = tags.map(str::to_owned);
        let token = token.clone();
        let identity = identity.cloned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::upsert_imported_account_bundle(
                remote.connection(),
                &account,
                note.as_deref(),
                tags.as_deref(),
                &token,
                identity.as_ref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn insert_event(&self, event: &Event) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.insert_event(event);
        }
        let event = event.clone();
        if let Some(tx) = current_usage_transaction() {
            return crate::storage_helpers::seaorm_transaction_block_on(async move {
                AccountsRepository::insert_event(tx.as_ref(), &event)
                    .await
                    .map_err(|e| e.to_string())
            })
            .map_err(storage_error);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::insert_event(remote.connection(), &event)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn latest_account_status_reasons(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        if !seaorm_enabled() {
            return self.0.latest_account_status_reasons(account_ids);
        }
        let account_ids = account_ids.to_vec();
        if let Some(tx) = current_usage_transaction() {
            return crate::storage_helpers::seaorm_transaction_block_on(async move {
                AccountsRepository::latest_account_status_reasons(tx.as_ref(), &account_ids)
                    .await
                    .map_err(|e| e.to_string())
            })
            .map_err(storage_error);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::latest_account_status_reasons(remote.connection(), &account_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn upsert_account_quota_capacity_override(
        &self,
        id: &str,
        primary: Option<i64>,
        secondary: Option<i64>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .0
                .upsert_account_quota_capacity_override(id, primary, secondary);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::upsert_account_quota_capacity_override(
                remote.connection(),
                &id,
                primary,
                secondary,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn load_account_summary_storage_snapshot_with_options(
        &self,
        account_ids: &[String],
        options: AccountSummaryStorageSnapshotOptions,
    ) -> rusqlite::Result<AccountSummaryStorageSnapshot> {
        if !seaorm_enabled() {
            return self
                .0
                .load_account_summary_storage_snapshot_with_options(account_ids, options);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::load_account_summary_storage_snapshot_with_options(
                remote.connection(),
                &account_ids,
                options,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_accounts(&self, account_ids: &[String]) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.0.delete_accounts(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::delete_accounts(remote.connection(), &account_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_account(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.delete_account(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::delete_account(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_account_proxy_settings(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountProxySettings>> {
        if !seaorm_enabled() {
            return self.0.find_account_proxy_settings(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_account_proxy_settings(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_proxy_settings(
        &self,
    ) -> rusqlite::Result<Vec<AccountProxySettings>> {
        if !seaorm_enabled() {
            return self.0.list_account_proxy_settings();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_proxy_settings(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn clear_account_proxy_settings(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.clear_account_proxy_settings(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::clear_account_proxy_settings(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn find_proxy_profile(&self, id: &str) -> rusqlite::Result<Option<ProxyProfile>> {
        if !seaorm_enabled() {
            return self.0.find_proxy_profile(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::find_proxy_profile(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_proxy_profiles(&self) -> rusqlite::Result<Vec<ProxyProfile>> {
        if !seaorm_enabled() {
            return self.0.list_proxy_profiles();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_proxy_profiles(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn insert_login_session(&self, s: &LoginSession) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.insert_login_session(s);
        }
        let s = s.clone();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::insert_login_session(remote.connection(), &s)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn get_login_session(&self, id: &str) -> rusqlite::Result<Option<LoginSession>> {
        if !seaorm_enabled() {
            return self.0.get_login_session(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::get_login_session(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn claim_login_session_for_completion(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.claim_login_session_for_completion(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::claim_login_session_for_completion(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn finish_login_session(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.finish_login_session(id, status, error);
        }
        let id = id.to_owned();
        let status = status.to_owned();
        let error = error.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::finish_login_session(
                remote.connection(),
                &id,
                &status,
                error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn fail_pending_login_session(
        &self,
        id: &str,
        error: Option<&str>,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.fail_pending_login_session(id, error);
        }
        let id = id.to_owned();
        let error = error.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::fail_pending_login_session(
                remote.connection(),
                &id,
                error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn finish_claimed_login_session(
        &self,
        expected: &LoginSession,
        status: &str,
        error: Option<&str>,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.finish_claimed_login_session(expected, status, error);
        }
        let expected = expected.clone();
        let status = status.to_owned();
        let error = error.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::finish_claimed_login_session(
                remote.connection(),
                &expected,
                &status,
                error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn cancel_login_session(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.cancel_login_session(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::cancel_login_session(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_login_session_code_verifier_if_pending(
        &self,
        id: &str,
        verifier: &str,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .0
                .update_login_session_code_verifier_if_pending(id, verifier);
        }
        let id = id.to_owned();
        let verifier = verifier.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_login_session_code_verifier_if_pending(
                remote.connection(),
                &id,
                &verifier,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn latest_usage_cleanup_rows_for_accounts(
        &self,
        account_ids: &[String],
    ) -> rusqlite::Result<Vec<UsageSnapshotCleanupRow>> {
        if !seaorm_enabled() {
            return self.0.latest_usage_cleanup_rows_for_accounts(account_ids);
        }
        let account_ids = account_ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::latest_usage_cleanup_rows_for_accounts(
                remote.connection(),
                &account_ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_usage_refresh_targets_by_statuses(
        &self,
        statuses: &[String],
    ) -> rusqlite::Result<Vec<AccountUsageRefreshTarget>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_usage_refresh_targets_by_statuses(statuses);
        }
        let statuses = statuses.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_usage_refresh_targets_by_statuses(
                remote.connection(),
                &statuses,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_usage_refresh_token_targets_by_statuses(
        &self,
        statuses: &[String],
    ) -> rusqlite::Result<Vec<AccountUsageRefreshTokenTarget>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_usage_refresh_token_targets_by_statuses(statuses);
        }
        let statuses = statuses.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_usage_refresh_token_targets_by_statuses(
                remote.connection(),
                &statuses,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_gateway_candidates(&self) -> rusqlite::Result<Vec<(Account, Token)>> {
        if !seaorm_enabled() {
            return self.0.list_gateway_candidates();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_gateway_candidates(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_gateway_candidates_unfiltered(
        &self,
    ) -> rusqlite::Result<Vec<(Account, Token)>> {
        if !seaorm_enabled() {
            return self.0.list_gateway_candidates_unfiltered();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_gateway_candidates_unfiltered(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_gateway_candidates_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<(Account, Token)>> {
        if !seaorm_enabled() {
            return self.0.list_gateway_candidates_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_gateway_candidates_for_accounts(remote.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_gateway_candidates_unfiltered_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<(Account, Token)>> {
        if !seaorm_enabled() {
            return self.0.list_gateway_candidates_unfiltered_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_gateway_candidates_unfiltered_for_accounts(
                remote.connection(),
                &ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_tokens(&self) -> rusqlite::Result<Vec<Token>> {
        if !seaorm_enabled() {
            return self.0.list_tokens();
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_tokens(remote.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_tokens_due_for_refresh(
        &self,
        due: i64,
        expiry: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<Token>> {
        if !seaorm_enabled() {
            return self.0.list_tokens_due_for_refresh(due, expiry, limit);
        }
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_tokens_due_for_refresh(remote.connection(), due, expiry, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_token_refresh_schedule(
        &self,
        id: &str,
        exp: Option<i64>,
        next: Option<i64>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_token_refresh_schedule(id, exp, next);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_token_refresh_schedule(remote.connection(), &id, exp, next)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn touch_token_refresh_attempt(&self, id: &str, at: i64) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.touch_token_refresh_attempt(id, at);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::touch_token_refresh_attempt(remote.connection(), &id, at)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses(
        &self,
        statuses: &[String],
    ) -> rusqlite::Result<Vec<AccountUsageRefreshTarget>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_usage_refresh_targets_with_usable_tokens_by_statuses(statuses);
        }
        let statuses = statuses.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_usage_refresh_targets_with_usable_tokens_by_statuses(
                remote.connection(),
                &statuses,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn delete_account_agent_identity(&self, id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.delete_account_agent_identity(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::delete_account_agent_identity(remote.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_label(&self, id: &str, value: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_account_label(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_label(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_group_name(
        &self,
        id: &str,
        value: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_account_group_name(id, value);
        }
        let id = id.to_owned();
        let value = value.map(str::to_owned);
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_group_name(
                remote.connection(),
                &id,
                value.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_sort(&self, id: &str, value: i64) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_account_sort(id, value);
        }
        let id = id.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_sort(remote.connection(), &id, value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn update_account_status(&self, id: &str, value: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.0.update_account_status(id, value);
        }
        let id = id.to_owned();
        let value = value.to_owned();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::update_account_status(remote.connection(), &id, &value)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_metadata_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountMetadata>> {
        if !seaorm_enabled() {
            return self.0.list_account_metadata_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_metadata_for_accounts(remote.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_subscriptions_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountSubscription>> {
        if !seaorm_enabled() {
            return self.0.list_account_subscriptions_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_subscriptions_for_accounts(remote.connection(), &ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn list_account_quota_capacity_overrides_for_accounts(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountQuotaCapacityOverride>> {
        if !seaorm_enabled() {
            return self
                .0
                .list_account_quota_capacity_overrides_for_accounts(ids);
        }
        let ids = ids.to_vec();
        seaorm_block_on(move |remote| async move {
            AccountsRepository::list_account_quota_capacity_overrides_for_accounts(
                remote.connection(),
                &ids,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
}
impl AccountStorage<'_> {
    pub(crate) fn account_quota_overview_stats(
        &self,
    ) -> rusqlite::Result<AccountQuotaOverviewStats> {
        if !seaorm_enabled() {
            return self.0.account_quota_overview_stats();
        }
        let accounts = self.list_accounts()?;
        let snapshots = self
            .latest_usage_snapshots_by_account()?
            .into_iter()
            .map(|s| (s.account_id.clone(), s))
            .collect::<HashMap<_, _>>();
        let mut out = AccountQuotaOverviewStats {
            account_count: accounts.len() as i64,
            ..Default::default()
        };
        let mut primary = Vec::new();
        let mut secondary = Vec::new();
        for a in accounts {
            if !["active", "available", "force_enabled"]
                .contains(&a.status.trim().to_ascii_lowercase().as_str())
            {
                continue;
            }
            out.available_count += 1;
            if let Some(s) = snapshots.get(&a.id) {
                let p = s.used_percent.map(|v| 100. - v.clamp(0., 100.));
                let q = s.secondary_used_percent.map(|v| 100. - v.clamp(0., 100.));
                if p.is_some_and(|v| v > 0. && v <= 20.) || q.is_some_and(|v| v > 0. && v <= 20.) {
                    out.low_quota_count += 1
                }
                primary.extend(p);
                secondary.extend(q);
                out.last_refreshed_at =
                    Some(out.last_refreshed_at.unwrap_or(i64::MIN).max(s.captured_at));
            }
        }
        out.primary_remain_percent_avg =
            (!primary.is_empty()).then(|| primary.iter().sum::<f64>() / primary.len() as f64);
        out.secondary_remain_percent_avg =
            (!secondary.is_empty()).then(|| secondary.iter().sum::<f64>() / secondary.len() as f64);
        Ok(out)
    }
    pub(crate) fn find_account_direct_auth_profile_by_id(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<AccountDirectAuthProfile>> {
        Ok(self
            .find_account_by_id(id)?
            .map(|a| AccountDirectAuthProfile {
                id: a.id,
                issuer: a.issuer,
                chatgpt_account_id: a.chatgpt_account_id,
                status: a.status,
            }))
    }
    pub(crate) fn list_active_account_codex_profile_candidates_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountCodexProfileCandidate>> {
        Ok(self
            .list_accounts_for_ids(ids)?
            .into_iter()
            .filter(|a| {
                ["active", "force_enabled"].contains(&a.status.trim().to_ascii_lowercase().as_str())
            })
            .map(|a| AccountCodexProfileCandidate {
                id: a.id,
                label: a.label,
                issuer: a.issuer,
                chatgpt_account_id: a.chatgpt_account_id,
                workspace_id: a.workspace_id,
                group_name: a.group_name,
                status: a.status,
            })
            .collect())
    }
    pub(crate) fn list_usable_account_token_candidates(
        &self,
    ) -> rusqlite::Result<Vec<AccountTokenCandidate>> {
        if !seaorm_enabled() {
            return self.0.list_usable_account_token_candidates();
        }
        Ok(self
            .list_tokens()?
            .into_iter()
            .filter(|t| !t.access_token.trim().is_empty() || !t.refresh_token.trim().is_empty())
            .map(|t| AccountTokenCandidate {
                account_id: t.account_id,
                has_access_token: !t.access_token.trim().is_empty(),
                has_refresh_token: !t.refresh_token.trim().is_empty(),
                last_refresh: t.last_refresh,
            })
            .collect())
    }
    pub(crate) fn list_api_key_ids_for_user(&self, id: &str) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.0.list_api_key_ids_for_user(id);
        }
        crate::auth::app_manager::list_api_key_ids_for_user(id).map_err(storage_error)
    }
    pub(crate) fn list_account_quota_source_summaries(
        &self,
    ) -> rusqlite::Result<Vec<AccountQuotaSourceSummary>> {
        Ok(self
            .list_accounts()?
            .into_iter()
            .map(|a| AccountQuotaSourceSummary {
                id: a.id,
                label: a.label,
                status: a.status,
            })
            .collect())
    }
    pub(crate) fn list_available_account_quota_pool_sources(
        &self,
    ) -> rusqlite::Result<Vec<AccountQuotaPoolSource>> {
        if !seaorm_enabled() {
            return self.0.list_available_account_quota_pool_sources();
        }
        Ok(self
            .list_account_quota_source_summaries()?
            .into_iter()
            .filter(|a| {
                ["active", "available", "force_enabled"]
                    .contains(&a.status.trim().to_ascii_lowercase().as_str())
            })
            .map(|a| AccountQuotaPoolSource {
                id: a.id,
                label: a.label,
            })
            .collect())
    }
}

impl AccountStorage<'_> {
    pub(crate) fn list_account_dashboard_source_metadata_for_ids(
        &self,
        ids: &[String],
    ) -> rusqlite::Result<Vec<AccountDashboardSourceMetadata>> {
        Ok(self
            .list_accounts_for_ids(ids)?
            .into_iter()
            .map(|a| AccountDashboardSourceMetadata {
                id: a.id,
                label: a.label,
                status: a.status,
            })
            .collect())
    }
}

impl AccountStorage<'_> {
    pub(crate) fn api_key_exists(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.api_key_exists(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            codexmanager_storage_seaorm::ApiKeysRepository::get(s.connection(), &id)
                .await
                .map(|k| k.is_some())
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn app_user_exists(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.0.app_user_exists(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            codexmanager_storage_seaorm::UsersRepository::get(s.connection(), &id)
                .await
                .map(|k| k.is_some())
                .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn api_key_quota_overview_stats(
        &self,
    ) -> rusqlite::Result<ApiKeyQuotaOverviewStats> {
        if !seaorm_enabled() {
            return self.0.api_key_quota_overview_stats();
        }
        seaorm_block_on(move |s| async move {
            use codexmanager_storage_seaorm::{ApiKeyDetailsRepository, ApiKeysRepository};
            let db = s.connection();
            let keys = ApiKeysRepository::list(db)
                .await
                .map_err(|e| e.to_string())?;
            let usage = ApiKeyDetailsRepository::usage_by_key(db, None, None)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|u| (u.key_id.clone(), u))
                .collect::<HashMap<_, _>>();
            let mut out = ApiKeyQuotaOverviewStats {
                key_count: keys.len() as i64,
                ..Default::default()
            };
            for key in keys {
                let used = ApiKeyDetailsRepository::token_usage(db, &key.id)
                    .await
                    .map_err(|e| e.to_string())?;
                out.total_used_tokens = out.total_used_tokens.saturating_add(used);
                if let Some(u) = usage.get(&key.id) {
                    out.estimated_cost_usd += u.estimated_cost_usd.max(0.);
                }
                if let Some(limit) = ApiKeyDetailsRepository::quota(db, &key.id)
                    .await
                    .map_err(|e| e.to_string())?
                    .filter(|n| *n > 0)
                {
                    out.limited_key_count += 1;
                    out.total_limit_tokens = out.total_limit_tokens.saturating_add(limit);
                    out.total_remaining_tokens = out
                        .total_remaining_tokens
                        .saturating_add(limit.saturating_sub(used).max(0));
                }
            }
            Ok(out)
        })
        .map_err(storage_error)
    }
    pub(crate) fn api_key_remaining_quota_tokens(&self) -> rusqlite::Result<i64> {
        Ok(self.api_key_quota_overview_stats()?.total_remaining_tokens)
    }
    pub(crate) fn list_account_quota_capacity_overrides(
        &self,
    ) -> rusqlite::Result<Vec<AccountQuotaCapacityOverride>> {
        if !seaorm_enabled() {
            return self.0.list_account_quota_capacity_overrides();
        }
        let ids = self
            .list_accounts()?
            .into_iter()
            .map(|a| a.id)
            .collect::<Vec<_>>();
        self.list_account_quota_capacity_overrides_for_accounts(&ids)
    }
    pub(crate) fn account_status_counts(&self) -> rusqlite::Result<Vec<AccountStatusCount>> {
        if !seaorm_enabled() {
            return self.0.account_status_counts();
        }
        let mut map = HashMap::<String, i64>::new();
        for a in self.list_accounts()? {
            *map.entry(a.status.trim().to_ascii_lowercase()).or_default() += 1;
        }
        let mut rows = map
            .into_iter()
            .map(|(status, count)| AccountStatusCount { status, count })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.status.cmp(&b.status)));
        Ok(rows)
    }
    pub(crate) fn token_account_count(&self) -> rusqlite::Result<i64> {
        if !seaorm_enabled() {
            return self.0.token_account_count();
        }
        Ok(self.list_tokens()?.len() as i64)
    }
    pub(crate) fn usage_snapshot_count(&self) -> rusqlite::Result<i64> {
        if !seaorm_enabled() {
            return self.0.usage_snapshot_count();
        }
        let mut count = 0;
        for a in self.latest_usage_snapshots_by_account()? {
            count += self.usage_snapshot_count_for_account(&a.account_id)?;
        }
        Ok(count)
    }
    pub(crate) fn low_quota_account_ids_for_accounts(
        &self,
        ids: &[String],
        primary: f64,
        secondary: f64,
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self
                .0
                .low_quota_account_ids_for_accounts(ids, primary, secondary);
        }
        let threshold = |v: f64| if v.is_finite() { v.clamp(0., 100.) } else { 0. };
        let primary = threshold(primary);
        let secondary = threshold(secondary);
        let mut out = self
            .latest_usage_snapshots_for_accounts(ids)?
            .into_iter()
            .filter(|s| {
                let p = if s.window_minutes.is_some_and(|w| w > 1443) {
                    secondary
                } else {
                    primary
                };
                let q = if s.secondary_window_minutes.is_some_and(|w| w <= 1443) {
                    primary
                } else {
                    secondary
                };
                (p > 0. && s.used_percent.is_some_and(|v| 100. - v <= p))
                    || (q > 0. && s.secondary_used_percent.is_some_and(|v| 100. - v <= q))
            })
            .map(|s| s.account_id)
            .collect::<Vec<_>>();
        out.sort();
        out.dedup();
        Ok(out)
    }
}

impl AccountStorage<'_> {
    pub(crate) fn list_account_ids(&self) -> rusqlite::Result<Vec<String>> {
        Ok(self.list_accounts()?.into_iter().map(|a| a.id).collect())
    }
    pub(crate) fn update_account_agent_identity_task_id(
        &self,
        id: &str,
        runtime: &str,
        key: &str,
        task: Option<&str>,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .0
                .update_account_agent_identity_task_id(id, runtime, key, task);
        }
        let id = id.to_owned();
        let runtime = runtime.to_owned();
        let key = key.to_owned();
        let task = task.map(str::to_owned);
        seaorm_block_on(move |s| async move {
            AccountsRepository::update_account_agent_identity_task_id(
                s.connection(),
                &id,
                &runtime,
                &key,
                task.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
    pub(crate) fn allowed_model_slugs_for_user_v2(
        &self,
        id: &str,
        at: i64,
    ) -> rusqlite::Result<Vec<String>> {
        if !seaorm_enabled() {
            return self.0.allowed_model_slugs_for_user_v2(id, at);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            codexmanager_storage_seaorm::ModelGroupsRepository::allowed_slugs_v2(
                s.connection(),
                &id,
                at,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(storage_error)
    }
}

thread_local! { static USAGE_TRANSACTION: std::cell::RefCell<Option<std::sync::Arc<codexmanager_storage_seaorm::AccountUsageTransaction>>> = const {std::cell::RefCell::new(None)}; }
pub(super) fn current_usage_transaction(
) -> Option<std::sync::Arc<codexmanager_storage_seaorm::AccountUsageTransaction>> {
    USAGE_TRANSACTION.with(|cell| cell.borrow().clone())
}
pub(super) fn with_usage_transaction<R>(
    tx: std::sync::Arc<codexmanager_storage_seaorm::AccountUsageTransaction>,
    operation: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<std::sync::Arc<codexmanager_storage_seaorm::AccountUsageTransaction>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            USAGE_TRANSACTION.with(|cell| *cell.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(USAGE_TRANSACTION.with(|cell| cell.replace(Some(tx))));
    operation()
}
