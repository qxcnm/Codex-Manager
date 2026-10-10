//! Owned, database-independent persistence contracts. Each method preserves one
//! repository transaction boundary. Futures never expose connections or ORM rows.
use super::super::*;
use std::{future::Future, pin::Pin};
pub type StorageFuture<'a, T> =
    Pin<Box<dyn Future<Output = std::result::Result<T, String>> + Send + 'a>>;

pub trait AccountsStore: Send + Sync {
    fn accounts(&self) -> StorageFuture<'_, Vec<Account>>;
    fn account(&self, id: String) -> StorageFuture<'_, Option<Account>>;
    fn token(&self, id: String) -> StorageFuture<'_, Option<Token>>;
    fn import_account(
        &self,
        account: Account,
        token: Token,
        note: Option<String>,
        tags: Option<String>,
        identity: Option<AccountAgentIdentity>,
    ) -> StorageFuture<'_, ()>;
    fn rotate_token(&self, expected: Token, next: Token) -> StorageFuture<'_, bool>;
}

pub trait AccessStore: Send + Sync {
    fn users(&self) -> StorageFuture<'_, Vec<AppUser>>;
    fn user(&self, id: String) -> StorageFuture<'_, Option<AppUser>>;
    fn create_user(
        &self,
        user: AppUser,
        initial_balance_credit_micros: i64,
    ) -> StorageFuture<'_, ()>;
    fn update_user(&self, user: AppUser) -> StorageFuture<'_, ()>;
    fn delete_user(&self, id: String) -> StorageFuture<'_, ()>;
    fn groups(&self) -> StorageFuture<'_, Vec<ModelGroup>>;
    fn group(&self, id: String) -> StorageFuture<'_, Option<ModelGroup>>;
    fn update_user_profile(
        &self,
        id: String,
        display_name: Option<String>,
    ) -> StorageFuture<'_, ()>;
    fn update_user_password(
        &self,
        id: String,
        expected_password_hash: String,
        password_hash: String,
    ) -> StorageFuture<'_, ()>;
    fn save_group(&self, group: ModelGroup) -> StorageFuture<'_, ()>;
    fn remove_group(&self, id: String) -> StorageFuture<'_, ()>;
    fn group_models(&self) -> StorageFuture<'_, Vec<ModelGroupModel>>;
    fn set_group_models(&self, id: String, rows: Vec<ModelGroupModel>) -> StorageFuture<'_, ()>;
    fn group_users(&self) -> StorageFuture<'_, Vec<UserModelGroup>>;
    fn set_group_users(&self, id: String, rows: Vec<UserModelGroup>) -> StorageFuture<'_, ()>;
    fn allowed_models(&self, id: String, now: i64) -> StorageFuture<'_, Vec<String>>;
    fn revoke_session(&self, hash: String, now: i64) -> StorageFuture<'_, ()>;
}

pub trait ApiKeysStore: Send + Sync {
    fn api_key(&self, id: String) -> StorageFuture<'_, Option<ApiKey>>;
    fn api_key_by_hash(&self, hash: String) -> StorageFuture<'_, Option<ApiKey>>;
    fn api_key_secret(&self, id: String) -> StorageFuture<'_, Option<String>>;
    /// Create key, profile, secret, quota, optional user owner and wallet atomically.
    fn create_api_key(&self, input: ApiKeyCreate) -> StorageFuture<'_, ()>;
    /// Apply only supplied fields and recheck member ownership inside the transaction.
    fn update_api_key(&self, id: String, patch: ApiKeyConfigPatch) -> StorageFuture<'_, ()>;
    fn api_key_summaries(
        &self,
        user_id: Option<String>,
    ) -> StorageFuture<'_, Vec<ApiKeyListSummary>>;
    fn api_keys(&self) -> StorageFuture<'_, Vec<ApiKey>>;
    fn api_key_owner(&self, id: String) -> StorageFuture<'_, Option<ApiKeyOwner>>;
    fn save_api_key_owner(&self, owner: ApiKeyOwner) -> StorageFuture<'_, ()>;
    fn set_api_key_status(&self, id: String, status: String) -> StorageFuture<'_, ()>;
    fn delete_api_key(&self, id: String) -> StorageFuture<'_, ()>;
}

pub trait ObservabilityStore: Send + Sync {
    fn append_request(&self, log: RequestLog, stat: RequestTokenStat) -> StorageFuture<'_, i64>;
    fn clear_request_logs(&self) -> StorageFuture<'_, ()>;
    fn usage(&self, id: String) -> StorageFuture<'_, Option<UsageSnapshotRecord>>;
    fn save_usage(&self, snapshot: UsageSnapshotRecord, retain: usize) -> StorageFuture<'_, usize>;
}

pub trait CatalogBillingStore: Send + Sync {
    fn models(&self, include_hidden: bool) -> StorageFuture<'_, Vec<ManagedModelV2>>;
    fn model(&self, slug: String) -> StorageFuture<'_, Option<ManagedModelV2>>;
    fn wallet(&self, kind: String, id: String) -> StorageFuture<'_, Option<AppWallet>>;
    fn ensure_wallet(&self, kind: String, id: String) -> StorageFuture<'_, AppWallet>;
    fn adjust_wallet(&self, entry: AppWalletLedgerEntry)
        -> StorageFuture<'_, AppWalletLedgerEntry>;
}

pub trait SettingsPluginsStore: Send + Sync {
    fn settings(&self) -> StorageFuture<'_, Vec<(String, String)>>;
    fn setting(&self, key: String) -> StorageFuture<'_, Option<String>>;
    fn save_setting(&self, key: String, value: String, now: i64) -> StorageFuture<'_, ()>;
    fn remove_setting(&self, key: String) -> StorageFuture<'_, ()>;
    fn plugins(&self) -> StorageFuture<'_, Vec<PluginInstallListSummary>>;
    fn plugin_tasks(&self, id: Option<String>) -> StorageFuture<'_, Vec<PluginTaskListSummary>>;
    /// Update an installed plugin and repair enabled interval schedules in
    /// the same backend abstraction used by the HTTP service.
    fn update_plugin_status(
        &self,
        id: String,
        status: String,
        last_error: Option<String>,
    ) -> StorageFuture<'_, ()>;
    fn repair_plugin_schedules(&self, id: Option<String>, now: i64) -> StorageFuture<'_, usize>;
}

pub trait DomainStorage:
    AccountsStore
    + AccessStore
    + ApiKeysStore
    + ObservabilityStore
    + CatalogBillingStore
    + SettingsPluginsStore
{
    fn kind(&self) -> StorageBackendKind;
    fn check(&self) -> StorageFuture<'_, StorageHealth>;
}
