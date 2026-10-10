//! Domain-shaped account APIs shared by RPC, authentication and refresh workers.
use crate::account_details::*;
use crate::{
    AccountRecord, AccountTokenRecord, AccountTokensRepository, AccountsRepository,
    UsageSnapshotsRepository,
};
use codexmanager_core::storage::*;
use sea_orm::{
    entity::prelude::*, ActiveModelTrait, DatabaseConnection, QueryOrder, Set, TransactionTrait,
};
use std::collections::{HashMap, HashSet};
fn text(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}
fn ids(v: &[String]) -> Vec<String> {
    let mut v = v.iter().filter_map(|v| text(Some(v))).collect::<Vec<_>>();
    v.sort();
    v.dedup();
    v
}
impl From<AccountRecord> for Account {
    fn from(v: AccountRecord) -> Self {
        Self {
            id: v.id,
            label: v.label,
            issuer: v.issuer,
            chatgpt_account_id: v.chatgpt_account_id,
            workspace_id: v.workspace_id,
            group_name: v.group_name,
            sort: v.sort,
            status: v.status,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}
impl From<AccountTokenRecord> for Token {
    fn from(v: AccountTokenRecord) -> Self {
        Self {
            account_id: v.account_id,
            id_token: v.id_token,
            access_token: v.access_token,
            refresh_token: v.refresh_token,
            api_key_access_token: v.api_key_access_token,
            last_refresh: v.last_refresh,
        }
    }
}
impl AccountsRepository {
    pub async fn list_accounts(db: &impl ConnectionTrait) -> Result<Vec<Account>, DbErr> {
        Ok(Self::list(db).await?.into_iter().map(Into::into).collect())
    }
    pub async fn find_account_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<Account>, DbErr> {
        Ok(Self::get(db, id).await?.map(Into::into))
    }
    pub async fn find_token_by_account_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<Token>, DbErr> {
        Ok(AccountTokensRepository::get(db, id).await?.map(Into::into))
    }
    pub async fn find_account_with_token_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<(Account, Token)>, DbErr> {
        Ok(
            match (
                Self::find_account_by_id(db, id).await?,
                Self::find_token_by_account_id(db, id).await?,
            ) {
                (Some(a), Some(t)) => Some((a, t)),
                _ => None,
            },
        )
    }
    pub async fn list_accounts_for_ids(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<Account>, DbErr> {
        let wanted = ids(account_ids).into_iter().collect::<HashSet<_>>();
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .filter(|a| wanted.contains(&a.id))
            .collect())
    }
    pub async fn list_tokens_for_accounts(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<Token>, DbErr> {
        let mut out = Vec::new();
        for id in ids(account_ids) {
            if let Some(t) = Self::find_token_by_account_id(db, &id).await? {
                out.push(t)
            }
        }
        Ok(out)
    }
    pub async fn list_account_token_plans_for_accounts(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenPlan>, DbErr> {
        Ok(Self::list_tokens_for_accounts(db, account_ids)
            .await?
            .into_iter()
            .map(|t| AccountTokenPlan {
                account_id: t.account_id,
                id_token: t.id_token,
                access_token: t.access_token,
            })
            .collect())
    }
    pub async fn list_account_import_token_subjects(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountImportTokenSubject>, DbErr> {
        let accounts = Self::list_accounts(db).await?;
        Ok(Self::list_tokens_for_accounts(
            db,
            &accounts.into_iter().map(|a| a.id).collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .map(|t| AccountImportTokenSubject {
            account_id: t.account_id,
            id_token: t.id_token,
            access_token: t.access_token,
            refresh_token: t.refresh_token,
        })
        .collect())
    }
    pub async fn list_account_import_snapshots(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountImportSnapshot>, DbErr> {
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .map(|a| AccountImportSnapshot {
                id: a.id,
                label: a.label,
                issuer: a.issuer,
                chatgpt_account_id: a.chatgpt_account_id,
                workspace_id: a.workspace_id,
                sort: a.sort,
                created_at: a.created_at,
            })
            .collect())
    }
    pub async fn list_account_summary_rows(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountListSummaryRow>, DbErr> {
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .map(|a| AccountListSummaryRow {
                id: a.id,
                label: a.label,
                group_name: a.group_name,
                sort: a.sort,
                status: a.status,
            })
            .collect())
    }
    pub async fn list_account_ids_for_ids(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<String>, DbErr> {
        Ok(Self::list_accounts_for_ids(db, account_ids)
            .await?
            .into_iter()
            .map(|a| a.id)
            .collect())
    }
    pub async fn account_count(db: &impl ConnectionTrait) -> Result<i64, DbErr> {
        Ok(crate::accounts::Entity::find().count(db).await? as i64)
    }
    pub async fn account_exists(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Ok(Self::get(db, id).await?.is_some())
    }
    pub async fn max_account_sort(db: &impl ConnectionTrait) -> Result<Option<i64>, DbErr> {
        Ok(Self::list(db).await?.iter().map(|a| a.sort).max())
    }
    pub async fn find_account_status_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<String>, DbErr> {
        Ok(Self::get(db, id).await?.map(|a| a.status))
    }
    pub async fn find_account_upsert_state_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountUpsertState>, DbErr> {
        Ok(Self::get(db, id).await?.map(|a| AccountUpsertState {
            group_name: a.group_name,
            sort: a.sort,
            created_at: a.created_at,
        }))
    }
    pub async fn find_account_workspace_identity_by_id(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountWorkspaceIdentity>, DbErr> {
        Ok(Self::get(db, id).await?.map(|a| AccountWorkspaceIdentity {
            id: a.id,
            chatgpt_account_id: a.chatgpt_account_id,
            workspace_id: a.workspace_id,
        }))
    }
    pub async fn insert_account(db: &impl ConnectionTrait, a: &Account) -> Result<(), DbErr> {
        let previous = Self::get(db, &a.id).await?;
        Self::upsert(
            db,
            AccountRecord {
                id: a.id.clone(),
                label: a.label.clone(),
                issuer: a.issuer.clone(),
                chatgpt_account_id: a.chatgpt_account_id.clone(),
                workspace_id: a.workspace_id.clone(),
                subject_account_id: previous.as_ref().and_then(|a| a.subject_account_id.clone()),
                note: previous.as_ref().and_then(|a| a.note.clone()),
                tags: previous.as_ref().and_then(|a| a.tags.clone()),
                group_name: a.group_name.clone(),
                sort: a.sort,
                status: a.status.clone(),
                created_at: a.created_at,
                updated_at: a.updated_at,
            },
        )
        .await
    }
    pub async fn insert_token(db: &impl ConnectionTrait, t: &Token) -> Result<(), DbErr> {
        use crate::account_tokens::{ActiveModel, Column, Entity};
        Entity::insert(ActiveModel {
            account_id: Set(t.account_id.clone()),
            id_token: Set(t.id_token.clone()),
            access_token: Set(t.access_token.clone()),
            refresh_token: Set(t.refresh_token.clone()),
            api_key_access_token: Set(t.api_key_access_token.clone()),
            last_refresh: Set(t.last_refresh),
            ..Default::default()
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(Column::AccountId)
                .update_columns([
                    Column::IdToken,
                    Column::AccessToken,
                    Column::RefreshToken,
                    Column::ApiKeyAccessToken,
                    Column::LastRefresh,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }

    pub async fn list_account_cleanup_candidates_by_statuses(
        db: &impl ConnectionTrait,
        statuses: &[String],
    ) -> Result<Vec<AccountCleanupCandidate>, DbErr> {
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .filter(|a| {
                statuses
                    .iter()
                    .any(|s| s.trim() == a.status.trim().to_ascii_lowercase())
            })
            .map(|a| AccountCleanupCandidate {
                id: a.id,
                status: a.status,
            })
            .collect())
    }
    pub async fn list_account_token_refresh_issuers_for_ids(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenRefreshIssuer>, DbErr> {
        Ok(Self::list_accounts_for_ids(db, account_ids)
            .await?
            .into_iter()
            .map(|a| AccountTokenRefreshIssuer {
                id: a.id,
                issuer: a.issuer,
            })
            .collect())
    }
    pub async fn list_account_auth_refresh_targets(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountAuthRefreshTarget>, DbErr> {
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .map(|a| AccountAuthRefreshTarget {
                id: a.id,
                label: a.label,
                issuer: a.issuer,
            })
            .collect())
    }
    pub async fn list_account_workspace_identities_for_subject(
        db: &impl ConnectionTrait,
        subject: &str,
    ) -> Result<Vec<AccountWorkspaceIdentity>, DbErr> {
        Ok(Self::list(db)
            .await?
            .into_iter()
            .filter(|a| {
                a.subject_account_id.as_deref() == Some(subject)
                    || (a.subject_account_id.is_none()
                        && (a.id == subject || a.id.starts_with(&format!("{subject}::"))))
            })
            .map(|a| AccountWorkspaceIdentity {
                id: a.id,
                chatgpt_account_id: a.chatgpt_account_id,
                workspace_id: a.workspace_id,
            })
            .collect())
    }
    pub async fn find_account_with_token_by_identity(
        db: &impl ConnectionTrait,
        id: Option<&str>,
        chatgpt: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<Option<(Account, Token)>, DbErr> {
        if let Some(id) = text(id) {
            if let Some(v) = Self::find_account_with_token_by_id(db, &id).await? {
                return Ok(Some(v));
            }
        }
        let mut accounts = Self::list_accounts(db).await?;
        accounts.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        for (value, is_chatgpt) in [(text(chatgpt), true), (text(workspace), false)] {
            if let Some(value) = value {
                for account in &accounts {
                    let key = if is_chatgpt {
                        account.chatgpt_account_id.as_ref()
                    } else {
                        account.workspace_id.as_ref()
                    };
                    if key == Some(&value) {
                        if let Some(token) = Self::find_token_by_account_id(db, &account.id).await?
                        {
                            return Ok(Some((account.clone(), token)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}
macro_rules! account_field_update {
    ($name:ident,$column:ident,$typ:ty) => {
        impl AccountsRepository {
            pub async fn $name(
                db: &impl ConnectionTrait,
                id: &str,
                value: $typ,
            ) -> Result<(), DbErr> {
                crate::accounts::Entity::update_many()
                    .col_expr(crate::accounts::Column::$column, Expr::value(value))
                    .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(now_ts()))
                    .filter(crate::accounts::Column::Id.eq(id))
                    .exec(db)
                    .await?;
                Ok(())
            }
        }
    };
}
account_field_update!(update_account_label, Label, &str);
account_field_update!(update_account_group_name, GroupName, Option<&str>);
account_field_update!(update_account_sort, Sort, i64);
account_field_update!(update_account_status, Status, &str);
impl AccountsRepository {
    pub async fn touch_account_updated_at(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<(), DbErr> {
        crate::accounts::Entity::update_many()
            .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(crate::accounts::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_account_subject_identity(
        db: &impl ConnectionTrait,
        id: &str,
        subject: &str,
    ) -> Result<(), DbErr> {
        if subject.trim().is_empty() {
            return Ok(());
        }
        crate::accounts::Entity::update_many()
            .col_expr(
                crate::accounts::Column::SubjectAccountId,
                Expr::value(text(Some(subject))),
            )
            .filter(crate::accounts::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_account_workspace_identity(
        db: &impl ConnectionTrait,
        id: &str,
        chatgpt: Option<&str>,
        workspace: Option<&str>,
        updated_at: i64,
    ) -> Result<bool, DbErr> {
        Ok(crate::accounts::Entity::update_many()
            .col_expr(
                crate::accounts::Column::ChatgptAccountId,
                Expr::value(chatgpt),
            )
            .col_expr(crate::accounts::Column::WorkspaceId, Expr::value(workspace))
            .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(updated_at))
            .filter(crate::accounts::Column::Id.eq(id))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
    pub async fn update_account_status_if_context_matches(
        db: &impl ConnectionTrait,
        id: &str,
        expected: &str,
        at: i64,
        next: &str,
    ) -> Result<bool, DbErr> {
        Ok(crate::accounts::Entity::update_many()
            .col_expr(crate::accounts::Column::Status, Expr::value(next))
            .col_expr(
                crate::accounts::Column::UpdatedAt,
                Expr::value(now_ts().max(at.saturating_add(1))),
            )
            .filter(crate::accounts::Column::Id.eq(id))
            .filter(crate::accounts::Column::Status.eq(expected))
            .filter(crate::accounts::Column::UpdatedAt.eq(at))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
    pub async fn update_account_status_if_changed_with_existence(
        db: &impl ConnectionTrait,
        id: &str,
        status: &str,
    ) -> Result<(bool, bool), DbErr> {
        let Some(account) = Self::get(db, id).await? else {
            return Ok((false, false));
        };
        if account.status == status {
            return Ok((true, false));
        }
        Ok((
            true,
            Self::update_account_status_if_context_matches(
                db,
                id,
                &account.status,
                account.updated_at,
                status,
            )
            .await?,
        ))
    }
    pub async fn update_account_sorts(
        db: &DatabaseConnection,
        values: &[(String, i64)],
        updated_at: i64,
    ) -> Result<usize, DbErr> {
        let tx = db.begin().await?;
        for (id, sort) in values {
            if !Self::account_exists(&tx, id).await? {
                return Err(DbErr::Custom("account not found".into()));
            }
            crate::accounts::Entity::update_many()
                .col_expr(crate::accounts::Column::Sort, Expr::value(*sort))
                .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(updated_at))
                .filter(crate::accounts::Column::Id.eq(id))
                .exec(&tx)
                .await?;
        }
        tx.commit().await?;
        Ok(values.len())
    }
    pub async fn preferred_account_id(db: &impl ConnectionTrait) -> Result<Option<String>, DbErr> {
        Ok(crate::accounts::Entity::find()
            .filter(crate::accounts::Column::Preferred.eq(true))
            .one(db)
            .await?
            .map(|a| AccountRecord::from(a).id))
    }
    pub async fn set_preferred_account(
        db: &DatabaseConnection,
        id: Option<&str>,
    ) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "accounts").await?;
        crate::accounts::Entity::update_many()
            .col_expr(crate::accounts::Column::Preferred, Expr::value(false))
            .exec(&tx)
            .await?;
        if let Some(id) = text(id) {
            crate::accounts::Entity::update_many()
                .col_expr(crate::accounts::Column::Preferred, Expr::value(true))
                .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(now_ts()))
                .filter(crate::accounts::Column::Id.eq(id))
                .exec(&tx)
                .await?;
        }
        tx.commit().await
    }
    pub async fn clear_preferred_account_if(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<bool, DbErr> {
        Ok(crate::accounts::Entity::update_many()
            .col_expr(crate::accounts::Column::Preferred, Expr::value(false))
            .col_expr(crate::accounts::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(crate::accounts::Column::Id.eq(id.trim()))
            .filter(crate::accounts::Column::Preferred.eq(true))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
}
impl AccountsRepository {
    pub async fn find_account_metadata(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountMetadata>, DbErr> {
        Ok(metadata::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn upsert_account_metadata(
        db: &impl ConnectionTrait,
        id: &str,
        note: Option<&str>,
        tags: Option<&str>,
    ) -> Result<(), DbErr> {
        let note = text(note);
        let tags = text(tags);
        if note.is_none() && tags.is_none() {
            metadata::Entity::delete_by_id(id).exec(db).await?;
            return Ok(());
        }
        let existing = metadata::Entity::find_by_id(id).one(db).await?.is_some();
        let row = metadata::ActiveModel {
            account_id: Set(id.into()),
            note: Set(note),
            tags: Set(tags),
            updated_at: Set(now_ts()),
        };
        if existing {
            row.update(db).await?;
        } else {
            row.insert(db).await?;
        }
        Ok(())
    }
    pub async fn find_account_subscription(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountSubscription>, DbErr> {
        Ok(subscriptions::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn upsert_account_subscription(
        db: &impl ConnectionTrait,
        id: &str,
        has: bool,
        account_plan: Option<&str>,
        plan: Option<&str>,
        expires: Option<i64>,
        renews: Option<i64>,
    ) -> Result<(), DbErr> {
        let existing = subscriptions::Entity::find_by_id(id)
            .one(db)
            .await?
            .is_some();
        let row = subscriptions::ActiveModel {
            account_id: Set(id.into()),
            has_subscription: Set(has),
            account_plan_type: Set(text(account_plan)),
            plan_type: Set(text(plan)),
            expires_at: Set(expires),
            renews_at: Set(renews),
            updated_at: Set(now_ts()),
        };
        if existing {
            row.update(db).await?;
        } else {
            row.insert(db).await?;
        }
        Ok(())
    }
    pub async fn find_account_agent_identity(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountAgentIdentity>, DbErr> {
        Ok(agent_identities::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn upsert_account_agent_identity(
        db: &impl ConnectionTrait,
        identity: &AccountAgentIdentity,
    ) -> Result<(), DbErr> {
        let previous = agent_identities::Entity::find_by_id(&identity.account_id)
            .one(db)
            .await?;
        let mut row: agent_identities::ActiveModel = identity.clone().into();
        if let Some(previous) = previous {
            row.created_at = Set(previous.created_at);
            row.update(db).await?;
        } else {
            row.insert(db).await?;
        }
        Ok(())
    }
    pub async fn upsert_imported_account_bundle(
        db: &DatabaseConnection,
        account: &Account,
        note: Option<&str>,
        tags: Option<&str>,
        token: &Token,
        identity: Option<&AccountAgentIdentity>,
    ) -> Result<(), DbErr> {
        if account.id != token.account_id || identity.is_some_and(|i| i.account_id != account.id) {
            return Err(DbErr::Custom(
                "account/token/agent identity mismatch".into(),
            ));
        }
        let tx = db.begin().await?;
        Self::insert_account(&tx, account).await?;
        let previous = Self::find_account_metadata(&tx, &account.id).await?;
        let note = text(note).or_else(|| previous.as_ref().and_then(|v| v.note.clone()));
        let tags = text(tags).or_else(|| previous.as_ref().and_then(|v| v.tags.clone()));
        Self::upsert_account_metadata(&tx, &account.id, note.as_deref(), tags.as_deref()).await?;
        Self::insert_token(&tx, token).await?;
        if let Some(identity) = identity {
            Self::upsert_account_agent_identity(&tx, identity).await?;
        } else {
            agent_identities::Entity::delete_by_id(&account.id)
                .exec(&tx)
                .await?;
        }
        tx.commit().await
    }
    pub async fn insert_event(db: &impl ConnectionTrait, event: &Event) -> Result<(), DbErr> {
        events::ActiveModel {
            id: Default::default(),
            account_id: Set(event.account_id.clone()),
            event_type: Set(event.event_type.clone()),
            message: Set(event.message.clone()),
            created_at: Set(event.created_at),
        }
        .insert(db)
        .await?;
        Ok(())
    }
    pub async fn latest_account_status_reasons(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<HashMap<String, String>, DbErr> {
        let mut result = HashMap::new();
        for id in ids(account_ids) {
            if let Some(event) = events::Entity::find()
                .filter(events::Column::AccountId.eq(&id))
                .filter(events::Column::EventType.eq("account_status_update"))
                .order_by_desc(events::Column::CreatedAt)
                .order_by_desc(events::Column::Id)
                .one(db)
                .await?
            {
                if let Some((_, reason)) = event.message.split_once(" reason=") {
                    let reason = reason.trim();
                    if !reason.is_empty() {
                        result.insert(id, reason.into());
                    }
                }
            }
        }
        Ok(result)
    }
    pub async fn upsert_account_quota_capacity_override(
        db: &impl ConnectionTrait,
        id: &str,
        primary: Option<i64>,
        secondary: Option<i64>,
    ) -> Result<(), DbErr> {
        let id = id.trim();
        if id.is_empty() {
            return Ok(());
        }
        let primary = primary.filter(|v| *v > 0);
        let secondary = secondary.filter(|v| *v > 0);
        if primary.is_none() && secondary.is_none() {
            quota_overrides::Entity::delete_by_id(id).exec(db).await?;
            return Ok(());
        }
        let exists = quota_overrides::Entity::find_by_id(id)
            .one(db)
            .await?
            .is_some();
        let row = quota_overrides::ActiveModel {
            account_id: Set(id.into()),
            primary_window_tokens: Set(primary),
            secondary_window_tokens: Set(secondary),
            updated_at: Set(now_ts()),
        };
        if exists {
            row.update(db).await?;
        } else {
            row.insert(db).await?;
        }
        Ok(())
    }
    pub async fn load_account_summary_storage_snapshot_with_options(
        db: &impl ConnectionTrait,
        account_ids: &[String],
        options: AccountSummaryStorageSnapshotOptions,
    ) -> Result<AccountSummaryStorageSnapshot, DbErr> {
        if account_ids.is_empty() {
            return Ok(Default::default());
        }
        Ok(AccountSummaryStorageSnapshot {
            preferred_account_id: if options.include_preferred {
                Self::preferred_account_id(db).await?
            } else {
                None
            },
            status_reasons: if options.include_status_reasons {
                Self::latest_account_status_reasons(db, account_ids).await?
            } else {
                Default::default()
            },
            tokens: if options.include_tokens {
                Self::list_account_token_plans_for_accounts(db, account_ids).await?
            } else {
                vec![]
            },
            usage_snapshots: UsageSnapshotsRepository::latest_for_accounts(db, account_ids).await?,
            metadata: if options.include_details {
                Self::list_account_metadata_for_accounts(db, account_ids).await?
            } else {
                vec![]
            },
            subscriptions: if options.include_details {
                Self::list_account_subscriptions_for_accounts(db, account_ids).await?
            } else {
                vec![]
            },
            quota_overrides: if options.include_details {
                Self::list_account_quota_capacity_overrides_for_accounts(db, account_ids).await?
            } else {
                vec![]
            },
            model_assignments: vec![],
        })
    }
    pub async fn delete_accounts(
        db: &DatabaseConnection,
        account_ids: &[String],
    ) -> Result<usize, DbErr> {
        let tx = db.begin().await?;
        let mut count = 0;
        for id in ids(account_ids) {
            Self::delete_account_related_history(&tx, &id).await?;
            crate::desktop_history::conversation_bindings::Entity::delete_many()
                .filter(crate::desktop_history::conversation_bindings::Column::AccountId.eq(&id))
                .exec(&tx)
                .await?;
            metadata::Entity::delete_by_id(&id).exec(&tx).await?;
            subscriptions::Entity::delete_by_id(&id).exec(&tx).await?;
            agent_identities::Entity::delete_by_id(&id)
                .exec(&tx)
                .await?;
            proxy_settings::Entity::delete_by_id(&id).exec(&tx).await?;
            quota_overrides::Entity::delete_by_id(&id).exec(&tx).await?;
            warmups::Entity::delete_by_id(&id).exec(&tx).await?;
            crate::account_tokens::Entity::delete_by_id(&id)
                .exec(&tx)
                .await?;
            crate::usage_snapshots::Entity::delete_many()
                .filter(crate::usage_snapshots::Column::AccountId.eq(&id))
                .exec(&tx)
                .await?;
            events::Entity::delete_many()
                .filter(events::Column::AccountId.eq(&id))
                .exec(&tx)
                .await?;
            count += crate::accounts::Entity::delete_by_id(&id)
                .exec(&tx)
                .await?
                .rows_affected as usize;
        }
        tx.commit().await?;
        Ok(count)
    }
    pub async fn delete_account(db: &DatabaseConnection, id: &str) -> Result<(), DbErr> {
        Self::delete_accounts(db, &[id.into()]).await.map(|_| ())
    }
}
macro_rules! list_details {
    ($name:ident,$entity:ident,$typ:ty) => {
        impl AccountsRepository {
            pub async fn $name(
                db: &impl ConnectionTrait,
                account_ids: &[String],
            ) -> Result<Vec<$typ>, DbErr> {
                let mut result = Vec::new();
                for batch in ids(account_ids).chunks(200) {
                    result.extend(
                        $entity::Entity::find()
                            .filter($entity::Column::AccountId.is_in(batch.to_vec()))
                            .all(db)
                            .await?
                            .into_iter()
                            .map(<$typ>::from),
                    );
                }
                Ok(result)
            }
        }
    };
}
list_details!(
    list_account_metadata_for_accounts,
    metadata,
    AccountMetadata
);
list_details!(
    list_account_subscriptions_for_accounts,
    subscriptions,
    AccountSubscription
);
list_details!(
    list_account_quota_capacity_overrides_for_accounts,
    quota_overrides,
    AccountQuotaCapacityOverride
);
impl AccountsRepository {
    pub async fn find_account_proxy_settings(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountProxySettings>, DbErr> {
        Ok(proxy_settings::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_account_proxy_settings(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountProxySettings>, DbErr> {
        Ok(proxy_settings::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn upsert_account_proxy_settings(
        db: &impl ConnectionTrait,
        settings: &AccountProxySettings,
    ) -> Result<(), DbErr> {
        let exists = proxy_settings::Entity::find_by_id(&settings.account_id)
            .one(db)
            .await?
            .is_some();
        let row: proxy_settings::ActiveModel = settings.clone().into();
        if exists {
            row.update(db).await?;
        } else {
            row.insert(db).await?;
        }
        Ok(())
    }
    pub async fn clear_account_proxy_settings(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<(), DbErr> {
        proxy_settings::Entity::delete_by_id(id).exec(db).await?;
        Ok(())
    }
    pub async fn find_proxy_profile(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<ProxyProfile>, DbErr> {
        Ok(proxy_profiles::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_proxy_profiles(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<ProxyProfile>, DbErr> {
        Ok(proxy_profiles::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}
impl AccountsRepository {
    pub async fn insert_login_session(
        db: &impl ConnectionTrait,
        s: &LoginSession,
    ) -> Result<(), DbErr> {
        let row: login_sessions::ActiveModel = s.clone().into();
        row.insert(db).await?;
        Ok(())
    }
    pub async fn get_login_session(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<LoginSession>, DbErr> {
        Ok(login_sessions::Entity::find_by_id(id)
            .one(db)
            .await?
            .filter(|session| session.login_id == id)
            .map(Into::into))
    }
    pub async fn claim_login_session_for_completion(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<bool, DbErr> {
        Ok(login_sessions::Entity::update_many()
            .col_expr(login_sessions::Column::Status, Expr::value("completing"))
            .col_expr(login_sessions::Column::Error, Expr::value(None::<String>))
            .col_expr(login_sessions::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(login_sessions::Column::LoginId.eq(id))
            .filter(login_sessions::Column::Status.eq("pending"))
            .exec(db)
            .await?
            .rows_affected
            == 1)
    }
    async fn finish_login(
        db: &impl ConnectionTrait,
        id: &str,
        status: &str,
        error: Option<&str>,
        pending_only: bool,
    ) -> Result<bool, DbErr> {
        let states = if pending_only {
            vec!["pending"]
        } else {
            vec!["pending", "completing"]
        };
        Ok(login_sessions::Entity::update_many()
            .col_expr(login_sessions::Column::Status, Expr::value(status))
            .col_expr(login_sessions::Column::Error, Expr::value(error))
            .col_expr(login_sessions::Column::CodeVerifier, Expr::value(""))
            .col_expr(login_sessions::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(login_sessions::Column::LoginId.eq(id))
            .filter(login_sessions::Column::Status.is_in(states))
            .exec(db)
            .await?
            .rows_affected
            == 1)
    }
    pub async fn finish_login_session(
        db: &impl ConnectionTrait,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool, DbErr> {
        Self::finish_login(db, id, status, error, false).await
    }
    pub async fn finish_claimed_login_session(
        db: &impl ConnectionTrait,
        expected: &LoginSession,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool, DbErr> {
        // MySQL commonly defaults to case-insensitive text collation. OAuth
        // state and PKCE verifier ownership must compare the original bytes.
        let state_matches = if db.get_database_backend() == sea_orm::DbBackend::MySql {
            Expr::cust_with_values("BINARY `state` = ?", [expected.state.clone()])
        } else {
            login_sessions::Column::State.eq(&expected.state)
        };
        let verifier_matches = if db.get_database_backend() == sea_orm::DbBackend::MySql {
            Expr::cust_with_values(
                "BINARY `code_verifier` = ?",
                [expected.code_verifier.clone()],
            )
        } else {
            login_sessions::Column::CodeVerifier.eq(&expected.code_verifier)
        };
        Ok(login_sessions::Entity::update_many()
            .col_expr(login_sessions::Column::Status, Expr::value(status))
            .col_expr(login_sessions::Column::Error, Expr::value(error))
            .col_expr(login_sessions::Column::CodeVerifier, Expr::value(""))
            .col_expr(login_sessions::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(login_sessions::Column::LoginId.eq(&expected.login_id))
            .filter(login_sessions::Column::Status.eq("completing"))
            .filter(state_matches)
            .filter(verifier_matches)
            .filter(login_sessions::Column::CreatedAt.eq(expected.created_at))
            .exec(db)
            .await?
            .rows_affected
            == 1)
    }
    pub async fn fail_pending_login_session(
        db: &impl ConnectionTrait,
        id: &str,
        error: Option<&str>,
    ) -> Result<bool, DbErr> {
        Self::finish_login(db, id, "failed", error, true).await
    }
    pub async fn cancel_login_session(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Self::finish_login(db, id, "cancelled", None, true).await
    }
    pub async fn update_login_session_code_verifier_if_pending(
        db: &impl ConnectionTrait,
        id: &str,
        verifier: &str,
    ) -> Result<bool, DbErr> {
        Ok(login_sessions::Entity::update_many()
            .col_expr(login_sessions::Column::CodeVerifier, Expr::value(verifier))
            .col_expr(login_sessions::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(login_sessions::Column::LoginId.eq(id))
            .filter(login_sessions::Column::Status.eq("pending"))
            .exec(db)
            .await?
            .rows_affected
            == 1)
    }
    pub async fn latest_usage_cleanup_rows_for_accounts(
        db: &impl ConnectionTrait,
        account_ids: &[String],
    ) -> Result<Vec<UsageSnapshotCleanupRow>, DbErr> {
        Ok(
            UsageSnapshotsRepository::latest_for_accounts(db, account_ids)
                .await?
                .into_iter()
                .map(|s| UsageSnapshotCleanupRow {
                    account_id: s.account_id,
                    used_percent: s.used_percent,
                    window_minutes: s.window_minutes,
                    secondary_used_percent: s.secondary_used_percent,
                    secondary_window_minutes: s.secondary_window_minutes,
                    credits_json: s.credits_json,
                })
                .collect(),
        )
    }
    pub async fn list_account_usage_refresh_targets_by_statuses(
        db: &impl ConnectionTrait,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTarget>, DbErr> {
        Ok(Self::list_accounts(db)
            .await?
            .into_iter()
            .filter(|a| {
                statuses
                    .iter()
                    .any(|s| s.trim() == a.status.trim().to_ascii_lowercase())
            })
            .map(|a| AccountUsageRefreshTarget {
                id: a.id,
                status: a.status,
                workspace_id: a.workspace_id.or(a.chatgpt_account_id),
            })
            .collect())
    }
    pub async fn list_account_usage_refresh_token_targets_by_statuses(
        db: &impl ConnectionTrait,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTokenTarget>, DbErr> {
        let mut out = Vec::new();
        for a in Self::list_accounts(db).await?.into_iter().filter(|a| {
            statuses
                .iter()
                .any(|s| s.trim() == a.status.trim().to_ascii_lowercase())
        }) {
            if let Some(token) = Self::find_token_by_account_id(db, &a.id).await? {
                out.push(AccountUsageRefreshTokenTarget {
                    account_id: a.id,
                    workspace_id: a.workspace_id.or(a.chatgpt_account_id),
                    token,
                });
            }
        }
        Ok(out)
    }
}

impl AccountsRepository {
    async fn gateway_candidates(
        db: &impl ConnectionTrait,
        account_ids: Option<&[String]>,
        usage: bool,
    ) -> Result<Vec<(Account, Token)>, DbErr> {
        let accounts = if let Some(ids) = account_ids {
            Self::list_accounts_for_ids(db, ids).await?
        } else {
            Self::list_accounts(db).await?
        };
        let mut out = Vec::new();
        for account in accounts {
            let status = account.status.trim().to_ascii_lowercase();
            if ["inactive", "disabled", "unavailable", "banned"].contains(&status.as_str())
                || (usage && status == "limited")
            {
                continue;
            }
            if usage && status != "force_enabled" {
                if let Some(row) =
                    UsageSnapshotsRepository::latest_for_account(db, &account.id).await?
                {
                    let snap = row.snapshot;
                    if !(snap.used_percent.is_some_and(|v| v < 100.)
                        && snap.window_minutes.is_some()
                        && snap.secondary_used_percent.is_some()
                            == snap.secondary_window_minutes.is_some()
                        && snap.secondary_used_percent.is_none_or(|v| v < 100.))
                    {
                        continue;
                    }
                }
            }
            if let Some(token) = Self::find_token_by_account_id(db, &account.id).await? {
                out.push((account, token));
            }
        }
        Ok(out)
    }
    pub async fn list_gateway_candidates(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<(Account, Token)>, DbErr> {
        Self::gateway_candidates(db, None, true).await
    }
    pub async fn list_gateway_candidates_unfiltered(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<(Account, Token)>, DbErr> {
        Self::gateway_candidates(db, None, false).await
    }
    pub async fn list_gateway_candidates_for_accounts(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<(Account, Token)>, DbErr> {
        Self::gateway_candidates(db, Some(ids), true).await
    }
    pub async fn list_gateway_candidates_unfiltered_for_accounts(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<(Account, Token)>, DbErr> {
        Self::gateway_candidates(db, Some(ids), false).await
    }
    pub async fn list_tokens(db: &impl ConnectionTrait) -> Result<Vec<Token>, DbErr> {
        Ok(crate::account_tokens::Entity::find()
            .order_by_asc(crate::account_tokens::Column::AccountId)
            .all(db)
            .await?
            .into_iter()
            .map(|v| AccountTokenRecord::from(v).into())
            .collect())
    }
    pub async fn list_tokens_due_for_refresh(
        db: &impl ConnectionTrait,
        due: i64,
        expiry: i64,
        limit: usize,
    ) -> Result<Vec<Token>, DbErr> {
        if limit == 0 {
            return Ok(vec![]);
        }
        let mut tokens = crate::account_tokens::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(AccountTokenRecord::from)
            .filter(|t| {
                !t.refresh_token.trim().is_empty()
                    && (t.next_refresh_at.is_none_or(|v| v <= due)
                        || t.access_token_exp.is_some_and(|v| v <= expiry))
            })
            .collect::<Vec<_>>();
        tokens.sort_by(|a, b| {
            a.next_refresh_at
                .unwrap_or(0)
                .cmp(&b.next_refresh_at.unwrap_or(0))
                .then_with(|| a.account_id.cmp(&b.account_id))
        });
        let mut out = Vec::new();
        for token in tokens {
            let latest = events::Entity::find()
                .filter(events::Column::AccountId.eq(&token.account_id))
                .filter(events::Column::EventType.eq("account_status_update"))
                .order_by_desc(events::Column::CreatedAt)
                .order_by_desc(events::Column::Id)
                .one(db)
                .await?;
            if latest.is_some_and(|e| {
                let e = e.message.to_ascii_lowercase();
                e.ends_with(" reason=account_deactivated")
                    || e.ends_with(" reason=workspace_deactivated")
            }) {
                continue;
            }
            out.push(token.into());
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
    pub async fn update_token_refresh_schedule(
        db: &impl ConnectionTrait,
        id: &str,
        exp: Option<i64>,
        next: Option<i64>,
    ) -> Result<(), DbErr> {
        AccountTokensRepository::update_refresh_schedule(db, id, exp, next)
            .await
            .map(|_| ())
    }
    pub async fn touch_token_refresh_attempt(
        db: &impl ConnectionTrait,
        id: &str,
        at: i64,
    ) -> Result<(), DbErr> {
        AccountTokensRepository::touch_refresh_attempt(db, id, at)
            .await
            .map(|_| ())
    }
    pub async fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses(
        db: &impl ConnectionTrait,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTarget>, DbErr> {
        let mut out = vec![];
        for target in Self::list_account_usage_refresh_targets_by_statuses(db, statuses).await? {
            if Self::find_token_by_account_id(db, &target.id)
                .await?
                .is_some_and(|t| {
                    !t.access_token.trim().is_empty() || !t.refresh_token.trim().is_empty()
                })
            {
                out.push(target)
            }
        }
        Ok(out)
    }
    pub async fn delete_account_agent_identity(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<(), DbErr> {
        agent_identities::Entity::delete_by_id(id).exec(db).await?;
        Ok(())
    }
}

impl AccountsRepository {
    pub async fn update_account_agent_identity_task_id(
        db: &impl ConnectionTrait,
        id: &str,
        runtime: &str,
        private_key: &str,
        task: Option<&str>,
    ) -> Result<bool, DbErr> {
        let Some(identity) = agent_identities::Entity::find_by_id(id).one(db).await? else {
            return Ok(false);
        };
        if !identity
            .auth_mode
            .trim()
            .eq_ignore_ascii_case("agentidentity")
        {
            return Ok(false);
        }
        Ok(agent_identities::Entity::update_many()
            .col_expr(agent_identities::Column::TaskId, Expr::value(task))
            .col_expr(agent_identities::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(agent_identities::Column::AccountId.eq(id))
            .filter(agent_identities::Column::AgentRuntimeId.eq(runtime))
            .filter(agent_identities::Column::AgentPrivateKey.eq(private_key))
            .filter(agent_identities::Column::AuthMode.eq(identity.auth_mode))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
}
