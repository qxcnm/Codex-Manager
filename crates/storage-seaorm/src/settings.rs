use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryOrder, Set};

/// HTTP-independent domain value for a persisted application setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "app_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    key: String,
    #[sea_orm(column_type = "Text")]
    value: String,
    updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub struct SettingsRepository;

impl SettingsRepository {
    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<AppSetting>, DbErr> {
        Entity::find()
            .order_by_asc(Column::Key)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn get(db: &impl ConnectionTrait, key: &str) -> Result<Option<AppSetting>, DbErr> {
        Entity::find()
            .filter(Column::Key.eq(key))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn set(db: &impl ConnectionTrait, setting: AppSetting) -> Result<(), DbErr> {
        let model = ActiveModel {
            key: Set(setting.key),
            value: Set(setting.value),
            updated_at: Set(setting.updated_at),
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::Key)
                    .update_columns([Column::Value, Column::UpdatedAt])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }

    pub async fn delete(db: &impl ConnectionTrait, key: &str) -> Result<bool, DbErr> {
        Entity::delete_by_id(key)
            .exec(db)
            .await
            .map(|result| result.rows_affected > 0)
    }
}

impl From<Model> for AppSetting {
    fn from(model: Model) -> Self {
        Self {
            key: model.key,
            value: model.value,
            updated_at: model.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    #[tokio::test]
    async fn settings_repository_round_trip() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(db.get_database_backend(), "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at BIGINT NOT NULL)")).await.unwrap();
        SettingsRepository::set(
            &db,
            AppSetting {
                key: "theme".into(),
                value: "dark".into(),
                updated_at: 7,
            },
        )
        .await
        .unwrap();
        SettingsRepository::set(
            &db,
            AppSetting {
                key: "theme".into(),
                value: "light".into(),
                updated_at: 8,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            SettingsRepository::get(&db, "theme")
                .await
                .unwrap()
                .unwrap()
                .value,
            "light"
        );
        assert_eq!(SettingsRepository::list(&db).await.unwrap().len(), 1);
        assert!(SettingsRepository::delete(&db, "theme").await.unwrap());
        assert!(SettingsRepository::get(&db, "theme")
            .await
            .unwrap()
            .is_none());
    }
}
