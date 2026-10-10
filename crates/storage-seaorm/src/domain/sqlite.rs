use super::*;

impl AccountsStore for SqliteDomainStorage {
    fn accounts(&self) -> StorageFuture<'_, Vec<Account>> {
        self.execute(move |s| s.list_accounts())
    }
    fn account(&self, id: String) -> StorageFuture<'_, Option<Account>> {
        self.execute(move |s| s.find_account_by_id(&id))
    }
    fn token(&self, id: String) -> StorageFuture<'_, Option<Token>> {
        self.execute(move |s| s.find_token_by_account_id(&id))
    }
    fn import_account(
        &self,
        account: Account,
        token: Token,
        note: Option<String>,
        tags: Option<String>,
        identity: Option<AccountAgentIdentity>,
    ) -> StorageFuture<'_, ()> {
        self.execute(move |s| {
            s.upsert_imported_account_bundle(
                &account,
                note.as_deref(),
                tags.as_deref(),
                &token,
                identity.as_ref(),
            )
        })
    }
    fn rotate_token(&self, expected: Token, next: Token) -> StorageFuture<'_, bool> {
        self.execute(move |s| s.compare_and_swap_token(&expected, &next))
    }
}

impl AccessStore for SqliteDomainStorage {
    fn users(&self) -> StorageFuture<'_, Vec<AppUser>> {
        self.execute(move |s| s.list_app_users())
    }
    fn user(&self, id: String) -> StorageFuture<'_, Option<AppUser>> {
        self.execute(move |s| s.find_app_user_by_id(&id))
    }
    fn create_user(
        &self,
        user: AppUser,
        initial_balance_credit_micros: i64,
    ) -> StorageFuture<'_, ()> {
        self.execute(move |s| {
            if user.role == "admin" && initial_balance_credit_micros > 0 {
                return Err(rusqlite::Error::InvalidParameterName(
                    "管理员账号不参与额度分发".to_owned(),
                ));
            }
            s.insert_app_user(&user)?;
            if user.role == "member" {
                s.assign_default_model_group_to_user(&user.id)?;
                let wallet_id = format!("wlt_domain_user_{}", user.id);
                let wallet = s.ensure_wallet_for_owner(&wallet_id, "user", &user.id)?;
                if initial_balance_credit_micros > 0 {
                    let ledger = AppWalletLedgerEntry {
                        id: format!("wl_domain_initial_{}", user.id),
                        wallet_id: wallet.id,
                        entry_kind: "initial_grant".to_owned(),
                        amount_credit_micros: initial_balance_credit_micros,
                        balance_after_credit_micros: 0,
                        request_log_id: None,
                        api_key_id: None,
                        pricing_rule_id: None,
                        raw_usage_json: None,
                        note: Some("initial balance".to_owned()),
                        created_by_user_id: None,
                        created_at: user.created_at,
                    };
                    s.adjust_wallet_balance(&ledger)?;
                }
            }
            Ok(())
        })
    }
    fn update_user(&self, user: AppUser) -> StorageFuture<'_, ()> {
        self.execute(move |s| {
            let current = s
                .find_app_user_by_id(&user.id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            if current.role == "admin"
                && current.status == "active"
                && (user.role != "admin" || user.status != "active")
                && s.active_admin_count()? <= 1
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "至少需要保留一个启用的管理员账号".to_owned(),
                ));
            }
            s.update_app_user_display_name(&user.id, user.display_name.clone())?;
            if current.role != user.role {
                s.update_app_user_role(&user.id, &user.role)?;
            }
            if current.status != user.status {
                s.update_app_user_status(&user.id, &user.status)?;
            }
            if current.password_hash != user.password_hash {
                s.update_app_user_password_hash(&user.id, &user.password_hash)?;
            }
            if user.role == "member" {
                let wallet_id = format!("wlt_domain_user_{}", user.id);
                s.ensure_wallet_for_owner(&wallet_id, "user", &user.id)?;
            }
            Ok(())
        })
    }
    fn delete_user(&self, id: String) -> StorageFuture<'_, ()> {
        self.execute(move |s| {
            let current = s
                .find_app_user_by_id(&id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            if current.role == "admin" && current.status == "active" && s.active_admin_count()? <= 1
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "至少需要保留一个启用的管理员账号".to_owned(),
                ));
            }
            let deleted = s.delete_app_user(&id)?;
            if deleted == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })
    }
    fn update_user_profile(
        &self,
        id: String,
        display_name: Option<String>,
    ) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.update_app_user_display_name(&id, display_name))
    }
    fn update_user_password(
        &self,
        id: String,
        expected_password_hash: String,
        password_hash: String,
    ) -> StorageFuture<'_, ()> {
        self.execute(move |s| {
            s.update_app_user_password_hash_if_current(&id, &expected_password_hash, &password_hash)
                .and_then(|updated| {
                    if updated {
                        Ok(())
                    } else {
                        Err(rusqlite::Error::QueryReturnedNoRows)
                    }
                })
        })
    }
    fn groups(&self) -> StorageFuture<'_, Vec<ModelGroup>> {
        self.execute(move |s| s.list_model_groups())
    }
    fn group(&self, id: String) -> StorageFuture<'_, Option<ModelGroup>> {
        self.execute(move |s| s.find_model_group(&id))
    }
    fn save_group(&self, group: ModelGroup) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.upsert_model_group(&group))
    }
    fn remove_group(&self, id: String) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.delete_model_group(&id).map(|_| ()))
    }
    fn group_models(&self) -> StorageFuture<'_, Vec<ModelGroupModel>> {
        self.execute(move |s| s.list_model_group_models_v2())
    }
    fn set_group_models(&self, id: String, rows: Vec<ModelGroupModel>) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.replace_model_group_models_v2(&id, &rows))
    }
    fn group_users(&self) -> StorageFuture<'_, Vec<UserModelGroup>> {
        self.execute(move |s| s.list_user_model_groups())
    }
    fn set_group_users(&self, id: String, rows: Vec<UserModelGroup>) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.replace_user_model_groups_for_group(&id, &rows))
    }
    fn allowed_models(&self, id: String, now: i64) -> StorageFuture<'_, Vec<String>> {
        self.execute(move |s| s.allowed_model_slugs_for_user_v2(&id, now))
    }
    fn revoke_session(&self, hash: String, now: i64) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.revoke_app_user_session_by_token_hash(&hash, now))
    }
}

impl ApiKeysStore for SqliteDomainStorage {
    fn api_key(&self, id: String) -> StorageFuture<'_, Option<ApiKey>> {
        self.execute(move |s| s.find_api_key_by_id(&id))
    }
    fn api_key_by_hash(&self, hash: String) -> StorageFuture<'_, Option<ApiKey>> {
        self.execute(move |s| s.find_api_key_by_hash(&hash))
    }
    fn api_key_secret(&self, id: String) -> StorageFuture<'_, Option<String>> {
        self.execute(move |s| s.find_api_key_secret_by_id(&id))
    }
    fn create_api_key(&self, input: ApiKeyCreate) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.create_api_key_atomic(input))
    }
    fn update_api_key(&self, id: String, patch: ApiKeyConfigPatch) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.update_api_key_atomic(&id, patch))
    }
    fn api_key_summaries(
        &self,
        user_id: Option<String>,
    ) -> StorageFuture<'_, Vec<ApiKeyListSummary>> {
        self.execute(move |s| match user_id {
            Some(id) => s.list_api_key_summaries_for_user(&id),
            None => s.list_api_key_summaries(),
        })
    }

    fn api_keys(&self) -> StorageFuture<'_, Vec<ApiKey>> {
        self.execute(move |s| s.list_api_keys())
    }
    fn api_key_owner(&self, id: String) -> StorageFuture<'_, Option<ApiKeyOwner>> {
        self.execute(move |s| s.find_api_key_owner(&id))
    }
    fn save_api_key_owner(&self, owner: ApiKeyOwner) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.upsert_api_key_owner(&owner))
    }
    fn set_api_key_status(&self, id: String, status: String) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.update_api_key_status(&id, &status))
    }
    fn delete_api_key(&self, id: String) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.delete_api_key(&id))
    }
}

impl ObservabilityStore for SqliteDomainStorage {
    fn append_request(&self, log: RequestLog, stat: RequestTokenStat) -> StorageFuture<'_, i64> {
        self.execute(move |s| {
            s.insert_request_log_with_token_stat(&log, &stat)
                .map(|(id, _)| id)
        })
    }
    fn clear_request_logs(&self) -> StorageFuture<'_, ()> {
        self.execute(|s| s.clear_request_logs())
    }
    fn usage(&self, id: String) -> StorageFuture<'_, Option<UsageSnapshotRecord>> {
        self.execute(move |s| s.latest_usage_snapshot_for_account(&id))
    }
    fn save_usage(&self, snapshot: UsageSnapshotRecord, retain: usize) -> StorageFuture<'_, usize> {
        self.execute(move |s| s.insert_usage_snapshot_and_prune(&snapshot, retain))
    }
}

impl CatalogBillingStore for SqliteDomainStorage {
    fn models(&self, include_hidden: bool) -> StorageFuture<'_, Vec<ManagedModelV2>> {
        self.execute(move |s| s.list_managed_models_v2(include_hidden))
    }
    fn model(&self, slug: String) -> StorageFuture<'_, Option<ManagedModelV2>> {
        self.execute(move |s| s.get_managed_model_v2(&slug))
    }
    fn wallet(&self, kind: String, id: String) -> StorageFuture<'_, Option<AppWallet>> {
        self.execute(move |s| s.find_wallet_by_owner(&kind, &id))
    }
    fn ensure_wallet(&self, kind: String, id: String) -> StorageFuture<'_, AppWallet> {
        self.execute(move |s| {
            let wallet_id = format!("wlt_domain_{kind}_{id}");
            s.ensure_wallet_for_owner(&wallet_id, &kind, &id)
        })
    }
    fn adjust_wallet(
        &self,
        entry: AppWalletLedgerEntry,
    ) -> StorageFuture<'_, AppWalletLedgerEntry> {
        self.execute(move |s| s.adjust_wallet_balance(&entry))
    }
}

impl SettingsPluginsStore for SqliteDomainStorage {
    fn settings(&self) -> StorageFuture<'_, Vec<(String, String)>> {
        self.execute(move |s| s.list_app_settings())
    }
    fn setting(&self, key: String) -> StorageFuture<'_, Option<String>> {
        self.execute(move |s| s.get_app_setting(&key))
    }
    fn save_setting(&self, key: String, value: String, now: i64) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.set_app_setting(&key, &value, now))
    }
    fn remove_setting(&self, key: String) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.delete_app_setting(&key))
    }
    fn plugins(&self) -> StorageFuture<'_, Vec<PluginInstallListSummary>> {
        self.execute(move |s| s.list_plugin_install_summaries())
    }
    fn plugin_tasks(&self, id: Option<String>) -> StorageFuture<'_, Vec<PluginTaskListSummary>> {
        self.execute(move |s| s.list_plugin_task_summaries(id.as_deref()))
    }
    fn update_plugin_status(
        &self,
        id: String,
        status: String,
        last_error: Option<String>,
    ) -> StorageFuture<'_, ()> {
        self.execute(move |s| s.update_plugin_install_status(&id, &status, last_error.as_deref()))
    }
    fn repair_plugin_schedules(&self, id: Option<String>, now: i64) -> StorageFuture<'_, usize> {
        self.execute(move |s| s.repair_plugin_task_schedules(id.as_deref(), now))
    }
}
