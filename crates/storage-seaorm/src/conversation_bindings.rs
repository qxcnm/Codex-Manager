use crate::{desktop_history::conversation_bindings as bindings, UsersRepository};
use codexmanager_core::storage::ConversationBinding;
use sea_orm::{entity::prelude::*, sea_query::OnConflict, Set, TransactionTrait};
use std::collections::HashMap;

pub struct ConversationBindingsRepository;
impl From<bindings::Model> for ConversationBinding {
    fn from(row: bindings::Model) -> Self {
        Self {
            platform_key_hash: row.platform_key_hash,
            conversation_id: row.conversation_id,
            account_id: row.account_id,
            thread_epoch: row.thread_epoch,
            thread_anchor: row.thread_anchor,
            status: row.status,
            last_model: row.last_model,
            last_switch_reason: row.last_switch_reason,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_used_at: row.last_used_at,
        }
    }
}
impl ConversationBindingsRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        key: &str,
        conversation: &str,
    ) -> Result<Option<ConversationBinding>, DbErr> {
        bindings::Entity::find_by_id((key.to_owned(), conversation.to_owned()))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }
    pub async fn upsert(
        db: &impl ConnectionTrait,
        binding: &ConversationBinding,
    ) -> Result<(), DbErr> {
        bindings::Entity::insert(bindings::ActiveModel {
            platform_key_hash: Set(binding.platform_key_hash.clone()),
            conversation_id: Set(binding.conversation_id.clone()),
            account_id: Set(binding.account_id.clone()),
            thread_epoch: Set(binding.thread_epoch),
            thread_anchor: Set(binding.thread_anchor.clone()),
            status: Set(binding.status.clone()),
            last_model: Set(binding.last_model.clone()),
            last_switch_reason: Set(binding.last_switch_reason.clone()),
            created_at: Set(binding.created_at),
            updated_at: Set(binding.updated_at),
            last_used_at: Set(binding.last_used_at),
        })
        .on_conflict(
            OnConflict::columns([
                bindings::Column::PlatformKeyHash,
                bindings::Column::ConversationId,
            ])
            .update_columns([
                bindings::Column::AccountId,
                bindings::Column::ThreadEpoch,
                bindings::Column::ThreadAnchor,
                bindings::Column::Status,
                bindings::Column::LastModel,
                bindings::Column::LastSwitchReason,
                bindings::Column::UpdatedAt,
                bindings::Column::LastUsedAt,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
    /// Serialize first claims across service instances. A concurrent request
    /// receives the persisted winner without changing its account or epoch.
    pub async fn claim(
        db: &DatabaseConnection,
        binding: &ConversationBinding,
    ) -> Result<(ConversationBinding, bool), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "conversation_bindings").await?;
        let result = if let Some(existing) =
            Self::get(&tx, &binding.platform_key_hash, &binding.conversation_id).await?
        {
            (existing, false)
        } else {
            Self::upsert(&tx, binding).await?;
            (binding.clone(), true)
        };
        tx.commit().await?;
        Ok(result)
    }
    pub async fn touch(
        db: &impl ConnectionTrait,
        key: &str,
        conversation: &str,
        account: &str,
        model: Option<&str>,
        now: i64,
    ) -> Result<bool, DbErr> {
        Ok(bindings::Entity::update_many()
            .col_expr(
                bindings::Column::LastModel,
                Expr::value(model.map(ToOwned::to_owned)),
            )
            .col_expr(bindings::Column::LastUsedAt, Expr::value(now))
            .col_expr(bindings::Column::UpdatedAt, Expr::value(now))
            .filter(bindings::Column::PlatformKeyHash.eq(key))
            .filter(bindings::Column::ConversationId.eq(conversation))
            .filter(bindings::Column::AccountId.eq(account))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
    pub async fn active_account_counts(
        db: &impl ConnectionTrait,
        key: &str,
    ) -> Result<HashMap<String, usize>, DbErr> {
        let rows = bindings::Entity::find()
            .filter(bindings::Column::PlatformKeyHash.eq(key))
            .filter(bindings::Column::Status.eq("active"))
            .all(db)
            .await?;
        let mut counts = HashMap::new();
        for row in rows {
            *counts.entry(row.account_id).or_default() += 1;
        }
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn first_claim_wins_and_touch_cannot_change_the_bound_account() {
        let storage = crate::SeaOrmStorage::connect(
            codexmanager_core::storage::StorageBackendKind::Sqlite,
            "sqlite::memory:",
        )
        .await
        .unwrap();
        storage.migrate().await.unwrap();
        let first = ConversationBinding {
            platform_key_hash: "key".into(),
            conversation_id: "conversation".into(),
            account_id: "account-a".into(),
            thread_epoch: 1,
            thread_anchor: "anchor".into(),
            status: "active".into(),
            last_model: None,
            last_switch_reason: None,
            created_at: 10,
            updated_at: 10,
            last_used_at: 10,
        };
        let mut competitor = first.clone();
        competitor.account_id = "account-b".into();
        competitor.thread_epoch = 99;
        let db = storage.connection();
        assert!(
            ConversationBindingsRepository::claim(db, &first)
                .await
                .unwrap()
                .1
        );
        let (won, created) = ConversationBindingsRepository::claim(db, &competitor)
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(won.account_id, "account-a");
        assert_eq!(won.thread_epoch, 1);
        assert!(!ConversationBindingsRepository::touch(
            db,
            "key",
            "conversation",
            "account-b",
            Some("wrong"),
            20
        )
        .await
        .unwrap());
        assert!(ConversationBindingsRepository::touch(
            db,
            "key",
            "conversation",
            "account-a",
            Some("right"),
            21
        )
        .await
        .unwrap());
        let current = ConversationBindingsRepository::get(db, "key", "conversation")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.last_model.as_deref(), Some("right"));
        assert_eq!(current.last_used_at, 21);
        assert_eq!(
            ConversationBindingsRepository::active_account_counts(db, "key")
                .await
                .unwrap()
                .get("account-a"),
            Some(&1)
        );
    }
}
