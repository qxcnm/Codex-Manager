//! Atomic, integer-only wallet accounting and immutable request charges.
pub(crate) mod wallets {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_wallets")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub owner_kind: String,
        pub owner_id: String,
        pub balance_credit_micros: i64,
        pub frozen_credit_micros: i64,
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<wallets::Model> for codexmanager_core::storage::AppWallet {
    fn from(row: wallets::Model) -> Self {
        Self {
            id: row.id,
            owner_kind: row.owner_kind,
            owner_id: row.owner_id,
            balance_credit_micros: row.balance_credit_micros,
            frozen_credit_micros: row.frozen_credit_micros,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::AppWallet> for wallets::ActiveModel {
    fn from(row: codexmanager_core::storage::AppWallet) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            owner_kind: sea_orm::Set(row.owner_kind),
            owner_id: sea_orm::Set(row.owner_id),
            balance_credit_micros: sea_orm::Set(row.balance_credit_micros),
            frozen_credit_micros: sea_orm::Set(row.frozen_credit_micros),
            status: sea_orm::Set(row.status),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod ledger {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_wallet_ledger_entries")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub wallet_id: String,
        pub entry_kind: String,
        pub amount_credit_micros: i64,
        pub balance_after_credit_micros: i64,
        pub request_log_id: Option<i64>,
        pub api_key_id: Option<String>,
        pub pricing_rule_id: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub raw_usage_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub note: Option<String>,
        pub created_by_user_id: Option<String>,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<ledger::Model> for codexmanager_core::storage::AppWalletLedgerEntry {
    fn from(row: ledger::Model) -> Self {
        Self {
            id: row.id,
            wallet_id: row.wallet_id,
            entry_kind: row.entry_kind,
            amount_credit_micros: row.amount_credit_micros,
            balance_after_credit_micros: row.balance_after_credit_micros,
            request_log_id: row.request_log_id,
            api_key_id: row.api_key_id,
            pricing_rule_id: row.pricing_rule_id,
            raw_usage_json: row.raw_usage_json,
            note: row.note,
            created_by_user_id: row.created_by_user_id,
            created_at: row.created_at,
        }
    }
}
impl From<codexmanager_core::storage::AppWalletLedgerEntry> for ledger::ActiveModel {
    fn from(row: codexmanager_core::storage::AppWalletLedgerEntry) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            wallet_id: sea_orm::Set(row.wallet_id),
            entry_kind: sea_orm::Set(row.entry_kind),
            amount_credit_micros: sea_orm::Set(row.amount_credit_micros),
            balance_after_credit_micros: sea_orm::Set(row.balance_after_credit_micros),
            request_log_id: sea_orm::Set(row.request_log_id),
            api_key_id: sea_orm::Set(row.api_key_id),
            pricing_rule_id: sea_orm::Set(row.pricing_rule_id),
            raw_usage_json: sea_orm::Set(row.raw_usage_json),
            note: sea_orm::Set(row.note),
            created_by_user_id: sea_orm::Set(row.created_by_user_id),
            created_at: sea_orm::Set(row.created_at),
        }
    }
}
pub(crate) mod snapshots {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "request_charge_snapshots")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub request_log_id: i64,
        pub model_id: Option<String>,
        pub model_slug: String,
        pub tier_min_input_tokens: i64,
        pub usage_source: String,
        pub input_tokens: i64,
        pub cached_input_tokens: i64,
        pub cache_write_tokens: i64,
        pub output_tokens: i64,
        pub input_microusd_per_1m: i64,
        pub cached_input_microusd_per_1m: i64,
        pub cache_write_microusd_per_1m: i64,
        pub output_microusd_per_1m: i64,
        pub rate_multiplier_millis: i64,
        pub base_cost_microusd: i64,
        pub charged_cost_microusd: i64,
        pub currency: String,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<snapshots::Model> for codexmanager_core::storage::ChargeSnapshotV2 {
    fn from(row: snapshots::Model) -> Self {
        Self {
            request_log_id: row.request_log_id,
            model_id: row.model_id,
            model_slug: row.model_slug,
            tier_min_input_tokens: row.tier_min_input_tokens,
            usage_source: row.usage_source,
            input_tokens: row.input_tokens,
            cached_input_tokens: row.cached_input_tokens,
            cache_write_tokens: row.cache_write_tokens,
            output_tokens: row.output_tokens,
            input_microusd_per_1m: row.input_microusd_per_1m,
            cached_input_microusd_per_1m: row.cached_input_microusd_per_1m,
            cache_write_microusd_per_1m: row.cache_write_microusd_per_1m,
            output_microusd_per_1m: row.output_microusd_per_1m,
            rate_multiplier_millis: row.rate_multiplier_millis,
            base_cost_microusd: row.base_cost_microusd,
            charged_cost_microusd: row.charged_cost_microusd,
            currency: row.currency,
            created_at: row.created_at,
        }
    }
}
impl From<codexmanager_core::storage::ChargeSnapshotV2> for snapshots::ActiveModel {
    fn from(row: codexmanager_core::storage::ChargeSnapshotV2) -> Self {
        Self {
            request_log_id: sea_orm::Set(row.request_log_id),
            model_id: sea_orm::Set(row.model_id),
            model_slug: sea_orm::Set(row.model_slug),
            tier_min_input_tokens: sea_orm::Set(row.tier_min_input_tokens),
            usage_source: sea_orm::Set(row.usage_source),
            input_tokens: sea_orm::Set(row.input_tokens),
            cached_input_tokens: sea_orm::Set(row.cached_input_tokens),
            cache_write_tokens: sea_orm::Set(row.cache_write_tokens),
            output_tokens: sea_orm::Set(row.output_tokens),
            input_microusd_per_1m: sea_orm::Set(row.input_microusd_per_1m),
            cached_input_microusd_per_1m: sea_orm::Set(row.cached_input_microusd_per_1m),
            cache_write_microusd_per_1m: sea_orm::Set(row.cache_write_microusd_per_1m),
            output_microusd_per_1m: sea_orm::Set(row.output_microusd_per_1m),
            rate_multiplier_millis: sea_orm::Set(row.rate_multiplier_millis),
            base_cost_microusd: sea_orm::Set(row.base_cost_microusd),
            charged_cost_microusd: sea_orm::Set(row.charged_cost_microusd),
            currency: sea_orm::Set(row.currency),
            created_at: sea_orm::Set(row.created_at),
        }
    }
}
use crate::{ModelCatalogRepository, ModelPriceTiersRepository, ModelPricesRepository};
use codexmanager_core::storage::{
    compute_charge_v2, now_ts, AppWallet, AppWalletLedgerEntry, ChargeSnapshotInputV2,
    ChargeSnapshotV2,
};
use sea_orm::{
    entity::prelude::*, ActiveModelTrait, DatabaseConnection, DatabaseTransaction, QueryOrder,
    QuerySelect, TransactionTrait,
};

pub struct BillingRepository;
impl BillingRepository {
    pub async fn wallet(db: &impl ConnectionTrait, id: &str) -> Result<Option<AppWallet>, DbErr> {
        Ok(wallets::Entity::find_by_id(id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn wallet_by_owner(
        db: &impl ConnectionTrait,
        kind: &str,
        owner: &str,
    ) -> Result<Option<AppWallet>, DbErr> {
        Ok(wallets::Entity::find()
            .filter(wallets::Column::OwnerKind.eq(kind))
            .filter(wallets::Column::OwnerId.eq(owner))
            .one(db)
            .await?
            .map(Into::into))
    }
    /// Insert only: balances on an existing wallet may change only with a ledger entry.
    pub async fn create_wallet(db: &impl ConnectionTrait, wallet: AppWallet) -> Result<(), DbErr> {
        if wallet.id.trim().is_empty()
            || wallet.owner_id.trim().is_empty()
            || !matches!(wallet.owner_kind.as_str(), "user" | "project")
            || !matches!(wallet.status.as_str(), "active" | "disabled")
            || wallet.frozen_credit_micros < 0
            || wallet.balance_credit_micros < wallet.frozen_credit_micros
        {
            return Err(DbErr::Custom("invalid wallet".into()));
        }
        let model: wallets::ActiveModel = wallet.into();
        model.insert(db).await?;
        Ok(())
    }
    pub async fn snapshot(
        db: &impl ConnectionTrait,
        request: i64,
    ) -> Result<Option<ChargeSnapshotV2>, DbErr> {
        Ok(snapshots::Entity::find_by_id(request)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn ledger(
        db: &impl ConnectionTrait,
        wallet_id: &str,
        limit: u64,
    ) -> Result<Vec<AppWalletLedgerEntry>, DbErr> {
        Ok(ledger::Entity::find()
            .filter(ledger::Column::WalletId.eq(wallet_id))
            .order_by_desc(ledger::Column::CreatedAt)
            .order_by_desc(ledger::Column::Id)
            .limit(limit.min(1000))
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    async fn lock_wallet(db: &DatabaseTransaction, id: &str) -> Result<AppWallet, DbErr> {
        // The no-op UPDATE locks the wallet before reading the snapshot/balance,
        // including SQLite where SELECT FOR UPDATE is unavailable.
        wallets::Entity::update_many()
            .col_expr(
                wallets::Column::UpdatedAt,
                Expr::col(wallets::Column::UpdatedAt).into(),
            )
            .filter(wallets::Column::Id.eq(id))
            .exec(db)
            .await?;
        Self::wallet(db, id)
            .await?
            .ok_or_else(|| DbErr::Custom("wallet_not_found".into()))
    }
    pub async fn adjust_balance(
        db: &DatabaseConnection,
        entry: AppWalletLedgerEntry,
    ) -> Result<AppWalletLedgerEntry, DbErr> {
        let tx = db.begin().await?;
        let result = Self::adjust_in_transaction(&tx, entry).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn adjust_in_transaction(
        tx: &DatabaseTransaction,
        mut entry: AppWalletLedgerEntry,
    ) -> Result<AppWalletLedgerEntry, DbErr> {
        if entry.entry_kind == "request_charge"
            || entry.request_log_id.is_some()
            || entry.id.trim().is_empty()
        {
            return Err(DbErr::Custom(
                "request charges require an immutable charge snapshot".into(),
            ));
        }
        let wallet = Self::lock_wallet(tx, &entry.wallet_id).await?;
        if let Some(existing) = ledger::Entity::find_by_id(&entry.id).one(tx).await? {
            if existing.wallet_id != entry.wallet_id
                || existing.amount_credit_micros != entry.amount_credit_micros
                || existing.entry_kind != entry.entry_kind
            {
                return Err(DbErr::Custom("ledger_idempotency_conflict".into()));
            }
            return Ok(existing.into());
        }
        if wallet.status != "active" {
            return Err(DbErr::Custom("wallet_inactive".into()));
        }
        let next = wallet
            .balance_credit_micros
            .checked_add(entry.amount_credit_micros)
            .ok_or_else(|| DbErr::Custom("wallet_balance_overflow".into()))?;
        if next < wallet.frozen_credit_micros {
            return Err(DbErr::Custom("wallet_insufficient_balance".into()));
        }
        wallets::Entity::update_many()
            .col_expr(wallets::Column::BalanceCreditMicros, Expr::value(next))
            .col_expr(wallets::Column::UpdatedAt, Expr::value(entry.created_at))
            .filter(wallets::Column::Id.eq(&wallet.id))
            .exec(tx)
            .await?;
        entry.balance_after_credit_micros = next;
        let row: ledger::ActiveModel = entry.clone().into();
        row.insert(tx).await?;
        Ok(entry)
    }
    pub async fn record_charge(
        db: &DatabaseConnection,
        input: &ChargeSnapshotInputV2,
    ) -> Result<ChargeSnapshotV2, DbErr> {
        if input.request_log_id <= 0
            || input.model_slug.trim().is_empty()
            || !matches!(input.usage_source.as_str(), "actual" | "estimated")
        {
            return Err(DbErr::Custom("invalid request charge".into()));
        }
        let tx = db.begin().await?;
        let result = Self::record_in_transaction(&tx, input).await;
        match result {
            Ok(snapshot) => {
                tx.commit().await?;
                Ok(snapshot)
            }
            Err(err) => {
                tx.rollback().await?;
                // A concurrent writer may have won the snapshot unique key.
                // Only an identical request identity is eligible for a replay.
                if let Some(existing) = Self::snapshot(db, input.request_log_id).await? {
                    Self::check_replay(db, input, &existing).await?;
                    return Ok(existing);
                }
                Err(err)
            }
        }
    }
    async fn check_replay(
        db: &impl ConnectionTrait,
        input: &ChargeSnapshotInputV2,
        existing: &ChargeSnapshotV2,
    ) -> Result<(), DbErr> {
        if existing.model_slug != input.model_slug.trim() {
            return Err(DbErr::Custom("charge_idempotency_conflict".into()));
        }
        let prior = ledger::Entity::find()
            .filter(ledger::Column::RequestLogId.eq(input.request_log_id))
            .filter(ledger::Column::EntryKind.eq("request_charge"))
            .one(db)
            .await?;
        let requested = input.wallet_id.as_deref().filter(|v| !v.trim().is_empty());
        if prior.as_ref().map(|row| row.wallet_id.as_str()) != requested {
            return Err(DbErr::Custom("charge_idempotency_conflict".into()));
        }
        Ok(())
    }
    async fn record_in_transaction(
        tx: &DatabaseTransaction,
        input: &ChargeSnapshotInputV2,
    ) -> Result<ChargeSnapshotV2, DbErr> {
        let wallet_id = input
            .wallet_id
            .as_deref()
            .filter(|id| !id.trim().is_empty());
        let wallet = match wallet_id {
            Some(id) => Some(Self::lock_wallet(tx, id).await?),
            None => None,
        };
        if let Some(existing) = Self::snapshot(tx, input.request_log_id).await? {
            Self::check_replay(tx, input, &existing).await?;
            Self::sync_estimate(tx, &existing).await?;
            return Ok(existing);
        }
        let slug = input
            .pricing_model_slug
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(input.model_slug.trim());
        let model = ModelCatalogRepository::find_by_slug(tx, slug)
            .await?
            .ok_or_else(|| DbErr::Custom("model_not_found".into()))?;
        let price = ModelPricesRepository::get(tx, &model.id)
            .await?
            .ok_or_else(|| DbErr::Custom("model_price_missing".into()))?;
        if price.price.price_status == "missing" {
            return Err(DbErr::Custom("model_price_missing".into()));
        }
        let tier = ModelPriceTiersRepository::list_for_model(tx, &model.id)
            .await?
            .into_iter()
            .filter(|row| row.tier.min_input_tokens <= input.input_tokens)
            .max_by_key(|row| row.tier.min_input_tokens)
            .ok_or_else(|| DbErr::Custom("model_price_tier_missing".into()))?
            .tier;
        let computation = compute_charge_v2(
            input.input_tokens,
            input.cached_input_tokens,
            input.cache_write_tokens,
            input.output_tokens,
            &tier,
            input.rate_multiplier_millis,
        )
        .map_err(|err| DbErr::Custom(err.to_string()))?;
        let cached = input.cached_input_tokens.min(input.input_tokens);
        let now = now_ts();
        let snapshot = ChargeSnapshotV2 {
            request_log_id: input.request_log_id,
            model_id: Some(model.id),
            model_slug: input.model_slug.trim().into(),
            tier_min_input_tokens: tier.min_input_tokens,
            usage_source: input.usage_source.clone(),
            input_tokens: input.input_tokens,
            cached_input_tokens: cached,
            cache_write_tokens: input
                .cache_write_tokens
                .min(input.input_tokens.saturating_sub(cached)),
            output_tokens: input.output_tokens,
            input_microusd_per_1m: tier.input_microusd_per_1m,
            cached_input_microusd_per_1m: tier.cached_input_microusd_per_1m,
            cache_write_microusd_per_1m: tier
                .cache_write_microusd_per_1m
                .unwrap_or(tier.input_microusd_per_1m),
            output_microusd_per_1m: tier.output_microusd_per_1m,
            rate_multiplier_millis: input.rate_multiplier_millis,
            base_cost_microusd: computation.base_cost_microusd,
            charged_cost_microusd: computation.charged_cost_microusd,
            currency: "USD".into(),
            created_at: now,
        };
        let row: snapshots::ActiveModel = snapshot.clone().into();
        row.insert(tx).await?;
        Self::sync_estimate(tx, &snapshot).await?;
        if let Some(wallet) = wallet {
            let charge = snapshot.charged_cost_microusd;
            let next = wallet
                .balance_credit_micros
                .checked_sub(charge)
                .ok_or_else(|| DbErr::Custom("wallet_insufficient_balance".into()))?;
            if wallet.status != "active" || next < wallet.frozen_credit_micros {
                return Err(DbErr::Custom("wallet_insufficient_balance".into()));
            }
            let updated = wallets::Entity::update_many()
                .col_expr(wallets::Column::BalanceCreditMicros, Expr::value(next))
                .col_expr(wallets::Column::UpdatedAt, Expr::value(now))
                .filter(wallets::Column::Id.eq(&wallet.id))
                .filter(wallets::Column::BalanceCreditMicros.eq(wallet.balance_credit_micros))
                .exec(tx)
                .await?;
            if updated.rows_affected != 1 {
                return Err(DbErr::Custom("wallet_concurrent_change".into()));
            }
            let entry = AppWalletLedgerEntry {
                id: format!("wl_request_{}", input.request_log_id),
                wallet_id: wallet.id,
                entry_kind: "request_charge".into(),
                amount_credit_micros: -charge,
                balance_after_credit_micros: next,
                request_log_id: Some(input.request_log_id),
                api_key_id: input.api_key_id.clone(),
                pricing_rule_id: input.pricing_rule_id.clone(),
                raw_usage_json: input.raw_usage_json.clone(),
                note: Some(
                    input
                        .ledger_note
                        .clone()
                        .unwrap_or_else(|| "model_catalog_v2".into()),
                ),
                created_by_user_id: None,
                created_at: now,
            };
            let row: ledger::ActiveModel = entry.into();
            row.insert(tx).await?;
        }
        Ok(snapshot)
    }
    async fn sync_estimate(
        db: &impl ConnectionTrait,
        snapshot: &ChargeSnapshotV2,
    ) -> Result<(), DbErr> {
        crate::request_token_stats::Entity::update_many()
            .col_expr(
                crate::request_token_stats::Column::EstimatedCostUsd,
                Expr::value(snapshot.base_cost_microusd as f64 / 1_000_000.0),
            )
            .filter(crate::request_token_stats::Column::RequestLogId.eq(snapshot.request_log_id))
            .exec(db)
            .await?;
        Ok(())
    }
}

impl BillingRepository {
    pub async fn ensure_wallet(
        db: &impl ConnectionTrait,
        id: &str,
        kind: &str,
        owner: &str,
    ) -> Result<AppWallet, DbErr> {
        if let Some(wallet) = Self::wallet_by_owner(db, kind, owner).await? {
            return Ok(wallet);
        }
        let now = now_ts();
        Self::create_wallet(
            db,
            AppWallet {
                id: id.into(),
                owner_kind: kind.into(),
                owner_id: owner.into(),
                balance_credit_micros: 0,
                frozen_credit_micros: 0,
                status: "active".into(),
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        Self::wallet_by_owner(db, kind, owner)
            .await?
            .ok_or_else(|| DbErr::Custom("wallet_not_found".into()))
    }
    pub async fn set_available(
        db: &DatabaseConnection,
        mut entry: AppWalletLedgerEntry,
        available: i64,
    ) -> Result<AppWallet, DbErr> {
        if available < 0 {
            return Err(DbErr::Custom("invalid wallet available balance".into()));
        }
        let tx = db.begin().await?;
        let wallet = Self::lock_wallet(&tx, &entry.wallet_id).await?;
        let target = available
            .checked_add(wallet.frozen_credit_micros)
            .ok_or_else(|| DbErr::Custom("wallet_balance_overflow".into()))?;
        entry.amount_credit_micros = target
            .checked_sub(wallet.balance_credit_micros)
            .ok_or_else(|| DbErr::Custom("wallet_balance_overflow".into()))?;
        if entry.amount_credit_micros != 0 {
            Self::adjust_in_transaction(&tx, entry).await?;
        }
        let result = Self::wallet(&tx, &wallet.id)
            .await?
            .ok_or_else(|| DbErr::Custom("wallet_not_found".into()))?;
        tx.commit().await?;
        Ok(result)
    }
}
#[cfg(test)]
#[path = "billing_tests.rs"]
mod tests;
