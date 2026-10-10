use crate::desktop_history::{
    account_quota_capacity_templates as templates, quota_source_model_assignments as assignments,
};
use codexmanager_core::storage::{AccountQuotaCapacityTemplate, QuotaSourceModelAssignment};
use sea_orm::sea_query::OnConflict;
use sea_orm::{entity::prelude::*, QueryOrder, Set};

pub struct QuotaConfigurationRepository;

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capacity_templates_normalize_slots_and_clear_nonpositive_limits() {
        let storage = crate::SeaOrmStorage::connect(
            codexmanager_core::storage::StorageBackendKind::Sqlite,
            "sqlite::memory:",
        )
        .await
        .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        QuotaConfigurationRepository::set_template(db, " Pro ", Some(200), Some(-1))
            .await
            .unwrap();
        QuotaConfigurationRepository::set_template(db, "", Some(20), None)
            .await
            .unwrap();
        let rows = QuotaConfigurationRepository::templates(db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].plan_type, "pro");
        assert_eq!(rows[0].primary_window_tokens, Some(200));
        assert_eq!(rows[0].secondary_window_tokens, None);
        QuotaConfigurationRepository::set_template(db, "PRO", Some(0), Some(1000))
            .await
            .unwrap();
        let rows = QuotaConfigurationRepository::templates(db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].primary_window_tokens, None);
        assert_eq!(rows[0].secondary_window_tokens, Some(1000));
    }
}
impl QuotaConfigurationRepository {
    pub async fn templates(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<AccountQuotaCapacityTemplate>, DbErr> {
        Ok(templates::Entity::find()
            .order_by_asc(templates::Column::PlanType)
            .all(db)
            .await?
            .into_iter()
            .map(|row| AccountQuotaCapacityTemplate {
                plan_type: row.plan_type,
                primary_window_tokens: row.primary_window_tokens,
                secondary_window_tokens: row.secondary_window_tokens,
                updated_at: row.updated_at,
            })
            .collect())
    }
    pub async fn set_template(
        db: &impl ConnectionTrait,
        plan: &str,
        primary: Option<i64>,
        secondary: Option<i64>,
    ) -> Result<(), DbErr> {
        let plan = plan.trim().to_ascii_lowercase();
        if plan.is_empty() {
            return Ok(());
        }
        templates::Entity::insert(templates::ActiveModel {
            plan_type: Set(plan),
            primary_window_tokens: Set(primary.filter(|value| *value > 0)),
            secondary_window_tokens: Set(secondary.filter(|value| *value > 0)),
            updated_at: Set(codexmanager_core::storage::now_ts()),
        })
        .on_conflict(
            OnConflict::column(templates::Column::PlanType)
                .update_columns([
                    templates::Column::PrimaryWindowTokens,
                    templates::Column::SecondaryWindowTokens,
                    templates::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
        Ok(())
    }
    pub async fn assignments(
        db: &impl ConnectionTrait,
        source_kind: Option<&str>,
        source_ids: Option<&[String]>,
    ) -> Result<Vec<QuotaSourceModelAssignment>, DbErr> {
        if source_ids.is_some_and(|ids| ids.is_empty()) {
            return Ok(Vec::new());
        }
        let mut query = assignments::Entity::find();
        if let Some(kind) = source_kind {
            query = query.filter(assignments::Column::SourceKind.eq(kind));
        }
        if let Some(ids) = source_ids {
            query = query.filter(assignments::Column::SourceId.is_in(ids.iter().cloned()));
        }
        Ok(query
            .order_by_asc(assignments::Column::SourceKind)
            .order_by_asc(assignments::Column::SourceId)
            .order_by_asc(assignments::Column::ModelSlug)
            .all(db)
            .await?
            .into_iter()
            .map(|row| QuotaSourceModelAssignment {
                source_kind: row.source_kind,
                source_id: row.source_id,
                model_slug: row.model_slug,
                updated_at: row.updated_at,
            })
            .collect())
    }
}
