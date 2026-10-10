use super::*;
use crate::*;

impl AccountsStore for SeaOrmStorage {
    fn accounts(&self) -> StorageFuture<'_, Vec<Account>> {
        Box::pin(async move {
            let db = self.connection();
            AccountsRepository::list_accounts(db)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn account(&self, id: String) -> StorageFuture<'_, Option<Account>> {
        Box::pin(async move {
            let db = self.connection();
            AccountsRepository::find_account_by_id(db, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn token(&self, id: String) -> StorageFuture<'_, Option<Token>> {
        Box::pin(async move {
            let db = self.connection();
            AccountsRepository::find_token_by_account_id(db, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn import_account(
        &self,
        account: Account,
        token: Token,
        note: Option<String>,
        tags: Option<String>,
        identity: Option<AccountAgentIdentity>,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            AccountsRepository::upsert_imported_account_bundle(
                db,
                &account,
                note.as_deref(),
                tags.as_deref(),
                &token,
                identity.as_ref(),
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn rotate_token(&self, expected: Token, next: Token) -> StorageFuture<'_, bool> {
        Box::pin(async move {
            let db = self.connection();
            AccountTokensRepository::compare_and_swap_token(db, &expected, &next)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl AccessStore for SeaOrmStorage {
    fn users(&self) -> StorageFuture<'_, Vec<AppUser>> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::list(db)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn user(&self, id: String) -> StorageFuture<'_, Option<AppUser>> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::get(db, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn create_user(
        &self,
        user: AppUser,
        initial_balance_credit_micros: i64,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::create_user(db, user, initial_balance_credit_micros, false)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn update_user(&self, user: AppUser) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::update_user(db, user)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn delete_user(&self, id: String) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::delete_user(db, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn update_user_profile(
        &self,
        id: String,
        display_name: Option<String>,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::update_profile(db, &id, display_name, now_ts())
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn update_user_password(
        &self,
        id: String,
        expected_password_hash: String,
        password_hash: String,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::update_password(
                db,
                &id,
                &expected_password_hash,
                password_hash,
                now_ts(),
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn groups(&self) -> StorageFuture<'_, Vec<ModelGroup>> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::list_all(db)
                .await
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn group(&self, id: String) -> StorageFuture<'_, Option<ModelGroup>> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::get(db, &id)
                .await
                .map(|row| row.map(Into::into))
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn save_group(&self, group: ModelGroup) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::upsert(db, group.into())
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn remove_group(&self, id: String) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::delete(db, &id)
                .await
                .map(|_| ())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn group_models(&self) -> StorageFuture<'_, Vec<ModelGroupModel>> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::list_models_v2(db)
                .await
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn set_group_models(&self, id: String, rows: Vec<ModelGroupModel>) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::replace_models_v2(
                db,
                &id,
                &rows.into_iter().map(Into::into).collect::<Vec<_>>(),
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn group_users(&self) -> StorageFuture<'_, Vec<UserModelGroup>> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::list_all_user_assignments(db)
                .await
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn set_group_users(&self, id: String, rows: Vec<UserModelGroup>) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::replace_user_assignments(
                db,
                &id,
                &rows.into_iter().map(Into::into).collect::<Vec<_>>(),
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn allowed_models(&self, id: String, now: i64) -> StorageFuture<'_, Vec<String>> {
        Box::pin(async move {
            let db = self.connection();
            ModelGroupsRepository::allowed_slugs_v2(db, &id, now)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn revoke_session(&self, hash: String, now: i64) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::revoke_session(db, &hash, now)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl ApiKeysStore for SeaOrmStorage {
    fn api_key(&self, id: String) -> StorageFuture<'_, Option<ApiKey>> {
        Box::pin(async move {
            ApiKeysRepository::get(self.connection(), &id)
                .await
                .map(|row| row.map(Into::into))
                .map_err(|error| error.to_string())
        })
    }
    fn api_key_by_hash(&self, hash: String) -> StorageFuture<'_, Option<ApiKey>> {
        Box::pin(async move {
            ApiKeysRepository::find_by_hash(self.connection(), &hash)
                .await
                .map(|row| row.map(Into::into))
                .map_err(|error| error.to_string())
        })
    }
    fn api_key_secret(&self, id: String) -> StorageFuture<'_, Option<String>> {
        Box::pin(async move {
            ApiKeyDetailsRepository::secret(self.connection(), &id)
                .await
                .map_err(|error| error.to_string())
        })
    }
    fn create_api_key(&self, input: ApiKeyCreate) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            ApiKeyDetailsRepository::create_with_owner(
                db,
                input.key.into(),
                input.secret,
                input.quota_limit_tokens,
                input.account_group_filter,
                input.owner_user_id,
            )
            .await
            .map_err(|error| error.to_string())
        })
    }
    fn update_api_key(&self, id: String, patch: ApiKeyConfigPatch) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let owner_user_id = patch.owner_user_id.clone();
            ApiKeyDetailsRepository::update_with_owner(
                self.connection(),
                &id,
                patch.quota_limit_tokens,
                owner_user_id.as_deref(),
                move |key| {
                    let mut group = key.account_group_filter.clone();
                    let mut mapped: ApiKey = key.clone().into();
                    patch.apply(&mut mapped, &mut group);
                    key.name = mapped.name;
                    key.model_slug = mapped.model_slug;
                    key.reasoning_effort = mapped.reasoning_effort;
                    key.service_tier = mapped.service_tier;
                    key.rotation_strategy = mapped.rotation_strategy;
                    key.aggregate_api_id = mapped.aggregate_api_id;
                    key.account_plan_filter = mapped.account_plan_filter;
                    key.account_group_filter = group;
                    key.client_type = mapped.client_type;
                    key.protocol_type = mapped.protocol_type;
                    key.auth_scheme = mapped.auth_scheme;
                    key.upstream_base_url = mapped.upstream_base_url;
                    key.static_headers_json = mapped.static_headers_json;
                    Ok(())
                },
            )
            .await
            .map_err(|error| error.to_string())
        })
    }
    fn api_key_summaries(
        &self,
        user_id: Option<String>,
    ) -> StorageFuture<'_, Vec<ApiKeyListSummary>> {
        Box::pin(async move {
            let db = self.connection();
            let rows = ApiKeysRepository::list(db)
                .await
                .map_err(|e| e.to_string())?;
            let mut result = Vec::new();
            for row in rows {
                if let Some(id) = &user_id {
                    let owner = UsersRepository::owner(db, &row.id)
                        .await
                        .map_err(|e| e.to_string())?;
                    if !owner.is_some_and(|o| {
                        o.owner_kind == "user" && o.owner_user_id.as_ref() == Some(id)
                    }) {
                        continue;
                    }
                }
                let quota_limit_tokens = ApiKeyDetailsRepository::quota(db, &row.id)
                    .await
                    .map_err(|e| e.to_string())?;
                result.push(ApiKeyListSummary {
                    id: row.id,
                    name: row.name,
                    model_slug: row.model_slug,
                    reasoning_effort: row.reasoning_effort,
                    service_tier: row.service_tier,
                    rotation_strategy: row.rotation_strategy,
                    aggregate_api_id: row.aggregate_api_id,
                    account_plan_filter: row.account_plan_filter,
                    account_group_filter: row.account_group_filter,
                    client_type: row.client_type,
                    protocol_type: row.protocol_type,
                    auth_scheme: row.auth_scheme,
                    upstream_base_url: row.upstream_base_url,
                    static_headers_json: row.static_headers_json,
                    status: row.status,
                    created_at: row.created_at,
                    last_used_at: row.last_used_at,
                    quota_limit_tokens,
                    aggregate_api_url: None,
                });
            }
            Ok(result)
        })
    }

    fn api_keys(&self) -> StorageFuture<'_, Vec<ApiKey>> {
        Box::pin(async move {
            let db = self.connection();
            ApiKeysRepository::list(db)
                .await
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn api_key_owner(&self, id: String) -> StorageFuture<'_, Option<ApiKeyOwner>> {
        Box::pin(async move {
            let db = self.connection();
            UsersRepository::owner(db, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn save_api_key_owner(&self, owner: ApiKeyOwner) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            UsersRepository::put_owner(self.connection(), owner)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn set_api_key_status(&self, id: String, status: String) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            ApiKeyDetailsRepository::update(self.connection(), &id, None, |key| {
                key.status = status;
                Ok(())
            })
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn delete_api_key(&self, id: String) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            ApiKeyDetailsRepository::delete(self.connection(), &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl ObservabilityStore for SeaOrmStorage {
    fn append_request(&self, log: RequestLog, stat: RequestTokenStat) -> StorageFuture<'_, i64> {
        Box::pin(async move {
            let db = self.connection();
            RequestLogsRepository::append_with_usage(db, log, stat)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn clear_request_logs(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            RequestLogsRepository::clear(self.connection(), now_ts())
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn usage(&self, id: String) -> StorageFuture<'_, Option<UsageSnapshotRecord>> {
        Box::pin(async move {
            let db = self.connection();
            UsageSnapshotsRepository::latest_for_account(db, &id)
                .await
                .map(|row| row.map(|row| row.snapshot))
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn save_usage(&self, snapshot: UsageSnapshotRecord, retain: usize) -> StorageFuture<'_, usize> {
        Box::pin(async move {
            let db = self.connection();
            UsageSnapshotsRepository::insert_and_prune(db, &snapshot, retain)
                .await
                .map(|(_, n)| n)
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl CatalogBillingStore for SeaOrmStorage {
    fn models(&self, include_hidden: bool) -> StorageFuture<'_, Vec<ManagedModelV2>> {
        Box::pin(async move {
            let db = self.connection();
            ManagedModelsRepository::list(db, include_hidden)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn model(&self, slug: String) -> StorageFuture<'_, Option<ManagedModelV2>> {
        Box::pin(async move {
            let db = self.connection();
            ManagedModelsRepository::get(db, &slug)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn wallet(&self, kind: String, id: String) -> StorageFuture<'_, Option<AppWallet>> {
        Box::pin(async move {
            let db = self.connection();
            BillingRepository::wallet_by_owner(db, &kind, &id)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn ensure_wallet(&self, kind: String, id: String) -> StorageFuture<'_, AppWallet> {
        Box::pin(async move {
            BillingRepository::ensure_wallet(
                self.connection(),
                &format!("wlt_domain_{kind}_{id}"),
                &kind,
                &id,
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn adjust_wallet(
        &self,
        entry: AppWalletLedgerEntry,
    ) -> StorageFuture<'_, AppWalletLedgerEntry> {
        Box::pin(async move {
            let db = self.connection();
            BillingRepository::adjust_balance(db, entry)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl SettingsPluginsStore for SeaOrmStorage {
    fn settings(&self) -> StorageFuture<'_, Vec<(String, String)>> {
        Box::pin(async move {
            let db = self.connection();
            SettingsRepository::list(db)
                .await
                .map(|rows| rows.into_iter().map(|row| (row.key, row.value)).collect())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn setting(&self, key: String) -> StorageFuture<'_, Option<String>> {
        Box::pin(async move {
            let db = self.connection();
            SettingsRepository::get(db, &key)
                .await
                .map(|row| row.map(|row| row.value))
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn save_setting(&self, key: String, value: String, now: i64) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            SettingsRepository::set(
                db,
                AppSetting {
                    key,
                    value,
                    updated_at: now,
                },
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn remove_setting(&self, key: String) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let db = self.connection();
            SettingsRepository::delete(db, &key)
                .await
                .map(|_| ())
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn plugins(&self) -> StorageFuture<'_, Vec<PluginInstallListSummary>> {
        Box::pin(async move {
            let db = self.connection();
            PluginsRepository::list_plugin_install_summaries(db)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn plugin_tasks(&self, id: Option<String>) -> StorageFuture<'_, Vec<PluginTaskListSummary>> {
        Box::pin(async move {
            let db = self.connection();
            PluginsRepository::list_plugin_task_summaries(db, id.as_deref())
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn update_plugin_status(
        &self,
        id: String,
        status: String,
        last_error: Option<String>,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            PluginsRepository::update_plugin_install_status(
                self.connection(),
                &id,
                &status,
                last_error.as_deref(),
            )
            .await
            .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
    fn repair_plugin_schedules(&self, id: Option<String>, now: i64) -> StorageFuture<'_, usize> {
        Box::pin(async move {
            PluginsRepository::repair_plugin_task_schedules(self.connection(), id.as_deref(), now)
                .await
                .map_err(|error| format!("storage operation failed: {error}"))
        })
    }
}

impl From<ModelGroup> for ModelGroupRecord {
    fn from(row: ModelGroup) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            status: row.status,
            sort: row.sort,
            is_default: row.is_default,
            rate_multiplier_millis: row.rate_multiplier_millis,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<ModelGroupRecord> for ModelGroup {
    fn from(row: ModelGroupRecord) -> Self {
        Self {
            id: row.id,
            name: row.name,
            description: row.description,
            status: row.status,
            sort: row.sort,
            is_default: row.is_default,
            rate_multiplier_millis: row.rate_multiplier_millis,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<ModelGroupModel> for ModelGroupModelRecord {
    fn from(row: ModelGroupModel) -> Self {
        Self {
            group_id: row.group_id,
            platform_model_slug: row.platform_model_slug,
            enabled: row.enabled,
            rate_multiplier_millis: row.rate_multiplier_millis,
            billing_model_slug: row.billing_model_slug,
            note: row.note,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<ModelGroupModelRecord> for ModelGroupModel {
    fn from(row: ModelGroupModelRecord) -> Self {
        Self {
            group_id: row.group_id,
            platform_model_slug: row.platform_model_slug,
            enabled: row.enabled,
            rate_multiplier_millis: row.rate_multiplier_millis,
            billing_model_slug: row.billing_model_slug,
            note: row.note,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<UserModelGroup> for UserModelGroupRecord {
    fn from(row: UserModelGroup) -> Self {
        Self {
            user_id: row.user_id,
            group_id: row.group_id,
            status: row.status,
            expires_at: row.expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<UserModelGroupRecord> for UserModelGroup {
    fn from(row: UserModelGroupRecord) -> Self {
        Self {
            user_id: row.user_id,
            group_id: row.group_id,
            status: row.status,
            expires_at: row.expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
