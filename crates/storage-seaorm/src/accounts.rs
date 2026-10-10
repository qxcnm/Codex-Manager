//! Service-mode account repository.
//!
//! The SeaORM entity is private to this crate.  Callers receive the small
//! [`AccountRecord`] value type so HTTP/RPC code never depends on SeaORM's
//! generated `Entity` or `ActiveModel` types.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{EntityTrait, QueryOrder, Set};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: String,
    pub label: String,
    pub issuer: String,
    pub chatgpt_account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub subject_account_id: Option<String>,
    pub note: Option<String>,
    pub tags: Option<String>,
    pub group_name: Option<String>,
    pub sort: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "accounts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    id: String,
    label: String,
    issuer: String,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
    subject_account_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    note: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    tags: Option<String>,
    group_name: Option<String>,
    sort: i64,
    status: String,
    created_at: i64,
    updated_at: i64,
    #[sea_orm(default_value = false)]
    pub(crate) preferred: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub struct AccountsRepository;

impl AccountsRepository {
    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<AccountRecord>, DbErr> {
        Entity::find()
            .order_by_asc(Column::Sort)
            .order_by_desc(Column::UpdatedAt)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn get(db: &impl ConnectionTrait, id: &str) -> Result<Option<AccountRecord>, DbErr> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    /// Insert or replace an account by its stable id.
    pub async fn upsert(db: &impl ConnectionTrait, account: AccountRecord) -> Result<(), DbErr> {
        let model = ActiveModel {
            id: Set(account.id),
            label: Set(account.label),
            issuer: Set(account.issuer),
            chatgpt_account_id: Set(account.chatgpt_account_id),
            workspace_id: Set(account.workspace_id),
            subject_account_id: Set(account.subject_account_id),
            note: Set(account.note),
            tags: Set(account.tags),
            group_name: Set(account.group_name),
            sort: Set(account.sort),
            status: Set(account.status),
            created_at: Set(account.created_at),
            updated_at: Set(account.updated_at),
            preferred: sea_orm::NotSet,
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::Id)
                    .update_columns([
                        Column::Label,
                        Column::Issuer,
                        Column::ChatgptAccountId,
                        Column::WorkspaceId,
                        Column::SubjectAccountId,
                        Column::Note,
                        Column::Tags,
                        Column::GroupName,
                        Column::Sort,
                        Column::Status,
                        Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }

    pub async fn delete(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Entity::delete_by_id(id)
            .exec(db)
            .await
            .map(|result| result.rows_affected > 0)
    }
}

impl From<Model> for AccountRecord {
    fn from(model: Model) -> Self {
        Self {
            id: model.id,
            label: model.label,
            issuer: model.issuer,
            chatgpt_account_id: model.chatgpt_account_id,
            workspace_id: model.workspace_id,
            subject_account_id: model.subject_account_id,
            note: model.note,
            tags: model.tags,
            group_name: model.group_name,
            sort: model.sort,
            status: model.status,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    #[tokio::test]
    async fn accounts_repository_round_trip_and_upsert() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "CREATE TABLE accounts (id VARCHAR(255) PRIMARY KEY, label VARCHAR(255) NOT NULL, issuer VARCHAR(255) NOT NULL, chatgpt_account_id VARCHAR(255), workspace_id VARCHAR(255), subject_account_id VARCHAR(255), note TEXT, tags TEXT, group_name VARCHAR(255), sort BIGINT NOT NULL DEFAULT 0, status VARCHAR(64) NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, preferred BOOLEAN NOT NULL DEFAULT FALSE)",
        ))
        .await
        .unwrap();
        AccountsRepository::upsert(
            &db,
            AccountRecord {
                id: "a1".into(),
                label: "Primary".into(),
                issuer: "openai".into(),
                chatgpt_account_id: Some("cgpt".into()),
                workspace_id: None,
                subject_account_id: Some("subject".into()),
                note: None,
                tags: Some("prod".into()),
                group_name: None,
                sort: 1,
                status: "active".into(),
                created_at: 1,
                updated_at: 2,
            },
        )
        .await
        .unwrap();
        let mut updated = AccountsRepository::get(&db, "a1").await.unwrap().unwrap();
        updated.label = "Renamed".into();
        updated.updated_at = 3;
        AccountsRepository::upsert(&db, updated).await.unwrap();
        assert_eq!(
            AccountsRepository::list(&db).await.unwrap()[0].label,
            "Renamed"
        );
        assert!(AccountsRepository::delete(&db, "a1").await.unwrap());
        assert!(AccountsRepository::get(&db, "a1").await.unwrap().is_none());
    }
}
