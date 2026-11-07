//! Database-independent persistence for bounded, privacy-preserving health data.
use codexmanager_core::storage::{
    service_health_sample_sql, ServiceHealthLogSample, ServiceHealthRecord,
};
use sea_orm::entity::prelude::*;
use sea_orm::{sea_query::Expr, QueryOrder, QuerySelect, Set, Statement};

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "service_health_records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub kind: String,
    pub owner: String,
    #[sea_orm(column_type = "Text")]
    pub payload: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
impl From<Model> for ServiceHealthRecord {
    fn from(v: Model) -> Self {
        Self {
            id: v.id,
            kind: v.kind,
            owner: v.owner,
            payload: v.payload,
            created_at: v.created_at,
            updated_at: v.updated_at,
            expires_at: v.expires_at,
        }
    }
}

pub struct ServiceHealthRepository;
impl ServiceHealthRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<ServiceHealthRecord>, DbErr> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map(|r| r.map(Into::into))
    }
    pub async fn list(
        db: &impl ConnectionTrait,
        kind: &str,
        now: i64,
        limit: usize,
    ) -> Result<Vec<ServiceHealthRecord>, DbErr> {
        Entity::find()
            .filter(Column::Kind.eq(kind))
            .filter(Column::ExpiresAt.gt(now))
            .order_by_desc(Column::UpdatedAt)
            .order_by_asc(Column::Id)
            .limit(limit.min(10001) as u64)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }
    pub async fn put(
        db: &impl ConnectionTrait,
        row: ServiceHealthRecord,
        expected: Option<i64>,
    ) -> Result<bool, DbErr> {
        if let Some(expected) = expected {
            let result = Entity::update_many()
                .col_expr(Column::Kind, Expr::value(row.kind))
                .col_expr(Column::Owner, Expr::value(row.owner))
                .col_expr(Column::Payload, Expr::value(row.payload))
                .col_expr(Column::CreatedAt, Expr::value(row.created_at))
                .col_expr(Column::UpdatedAt, Expr::value(row.updated_at))
                .col_expr(Column::ExpiresAt, Expr::value(row.expires_at))
                .filter(Column::Id.eq(row.id))
                .filter(Column::UpdatedAt.eq(expected))
                .exec(db)
                .await?;
            return Ok(result.rows_affected == 1);
        }
        match Entity::insert(ActiveModel {
            id: Set(row.id),
            kind: Set(row.kind),
            owner: Set(row.owner),
            payload: Set(row.payload),
            created_at: Set(row.created_at),
            updated_at: Set(row.updated_at),
            expires_at: Set(row.expires_at),
        })
        .exec_without_returning(db)
        .await
        {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.sql_err(),
                    Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
    pub async fn prune(db: &impl ConnectionTrait, now: i64) -> Result<(), DbErr> {
        Entity::delete_many()
            .filter(Column::ExpiresAt.lte(now))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn samples(
        db: &impl ConnectionTrait,
        start: i64,
        end: i64,
        limit: usize,
    ) -> Result<Vec<ServiceHealthLogSample>, DbErr> {
        let rows = db
            .query_all(Statement::from_string(
                db.get_database_backend(),
                service_health_sample_sql(start, end, limit),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ServiceHealthLogSample {
                    provider: row.try_get("", "provider")?,
                    status_code: row.try_get("", "status_code")?,
                    first_response_ms: row.try_get("", "first_response_ms")?,
                    has_error: row.try_get::<i32>("", "has_error")? != 0,
                    upstream_present: row.try_get::<i32>("", "upstream_present")? != 0,
                    confirmed_upstream_error: row.try_get::<i32>("", "confirmed_upstream_error")?
                        != 0,
                    network_error: row.try_get::<i32>("", "network_error")? != 0,
                    created_at: row.try_get("", "created_at")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn remote_samples_use_same_metadata_contract() {
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        crate::migration::migrate(&db).await.unwrap();
        crate::RequestLogsRepository::insert(
            &db,
            1,
            codexmanager_core::storage::RequestLog {
                request_path: "/v1/messages".into(),
                method: "POST".into(),
                actual_source_kind: Some("openai_account".into()),
                status_code: Some(502),
                upstream_url: Some("https://private.invalid".into()),
                error: Some("private diagnostic".into()),
                created_at: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let samples = ServiceHealthRepository::samples(&db, 100, 101, 2001)
            .await
            .unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].provider, "codex");
        assert!(samples[0].has_error);
        assert!(!samples[0].confirmed_upstream_error);
        assert!(ServiceHealthRepository::samples(&db, 101, 102, 2001)
            .await
            .unwrap()
            .is_empty());
    }
    #[tokio::test]
    async fn remote_records_cas_and_expiry() {
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        crate::migration::migrate(&db).await.unwrap();
        let mut row = ServiceHealthRecord {
            id: "a".into(),
            kind: "report".into(),
            owner: "member".into(),
            payload: "{}".into(),
            created_at: 1,
            updated_at: 1,
            expires_at: 20,
        };
        assert!(ServiceHealthRepository::put(&db, row.clone(), None)
            .await
            .unwrap());
        assert!(!ServiceHealthRepository::put(&db, row.clone(), None)
            .await
            .unwrap());
        row.updated_at = 2;
        assert!(ServiceHealthRepository::put(&db, row.clone(), Some(1))
            .await
            .unwrap());
        assert!(!ServiceHealthRepository::put(&db, row, Some(1))
            .await
            .unwrap());
        ServiceHealthRepository::prune(&db, 20).await.unwrap();
        assert!(ServiceHealthRepository::get(&db, "a")
            .await
            .unwrap()
            .is_none());
    }
}
