use crate::{
    desktop_history::{
        codex_skill_repositories as repositories, codex_skill_repository_skills as skills,
    },
    UsersRepository,
};
use codexmanager_core::storage::{
    now_ts, CodexSkillRepositoryCatalogSnapshot, CodexSkillRepositoryRecord,
    CodexSkillRepositorySkillRecord, CodexSkillRepositoryUpsert,
};
use sea_orm::{entity::prelude::*, ActiveModelTrait, Set, TransactionTrait};

pub struct SkillRepositoriesRepository;

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn repository_snapshots_roll_back_and_builtin_identity_stays_pinned() {
        let storage = crate::SeaOrmStorage::connect(
            codexmanager_core::storage::StorageBackendKind::Sqlite,
            "sqlite::memory:",
        )
        .await
        .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        SkillRepositoriesRepository::ensure_builtins(db)
            .await
            .unwrap();
        let builtin = SkillRepositoriesRepository::get(db, "builtin-anthropics-skills")
            .await
            .unwrap()
            .unwrap();
        let change = CodexSkillRepositoryUpsert {
            id: builtin.id.clone(),
            owner: "evil".into(),
            repository: "replacement".into(),
            ref_name: "other".into(),
            enabled: false,
            is_builtin: false,
            created_at: 1,
            updated_at: 2,
        };
        let saved = SkillRepositoriesRepository::upsert(db, &change)
            .await
            .unwrap();
        assert_eq!(saved.owner, builtin.owner);
        assert_eq!(saved.repository, builtin.repository);
        assert_eq!(saved.ref_name, builtin.ref_name);
        assert!(saved.is_builtin);
        assert!(!saved.enabled);
        assert!(!SkillRepositoriesRepository::delete(db, &saved.id)
            .await
            .unwrap());
        SkillRepositoriesRepository::ensure_builtins(db)
            .await
            .unwrap();
        assert!(
            !SkillRepositoriesRepository::get(db, &saved.id)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
        let custom = CodexSkillRepositoryUpsert {
            id: "custom".into(),
            owner: "fixture".into(),
            repository: "repo".into(),
            ref_name: "main".into(),
            enabled: true,
            is_builtin: false,
            created_at: 1,
            updated_at: 1,
        };
        SkillRepositoriesRepository::upsert(db, &custom)
            .await
            .unwrap();
        let skill = CodexSkillRepositorySkillRecord {
            repository_id: "custom".into(),
            skill_id: "one".into(),
            name: "One".into(),
            description: "kept".into(),
            path: "one".into(),
            source_url: "https://example.invalid/one".into(),
            revision: Some("rev-a".into()),
        };
        SkillRepositoriesRepository::replace_snapshot(
            db,
            "custom",
            std::slice::from_ref(&skill),
            10,
        )
        .await
        .unwrap();
        let mut replacement = skill.clone();
        replacement.description = "must roll back".into();
        replacement.revision = Some("rev-b".into());
        assert!(SkillRepositoriesRepository::replace_snapshot(
            db,
            "custom",
            &[replacement.clone(), replacement],
            20
        )
        .await
        .is_err());
        assert_eq!(
            SkillRepositoriesRepository::get(db, "custom")
                .await
                .unwrap()
                .unwrap()
                .revision
                .as_deref(),
            Some("rev-a")
        );
        assert_eq!(
            SkillRepositoriesRepository::skill(db, "custom", "one")
                .await
                .unwrap()
                .unwrap()
                .description,
            "kept"
        );
        assert!(SkillRepositoriesRepository::delete(db, "custom")
            .await
            .unwrap());
        assert!(SkillRepositoriesRepository::skill(db, "custom", "one")
            .await
            .unwrap()
            .is_none());
    }
}
impl From<repositories::Model> for CodexSkillRepositoryRecord {
    fn from(row: repositories::Model) -> Self {
        Self {
            id: row.id,
            owner: row.owner,
            repository: row.repository,
            ref_name: row.ref_name,
            enabled: row.enabled,
            is_builtin: row.is_builtin,
            revision: row.revision,
            last_scanned_at: row.last_scanned_at,
            last_error: row.last_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<skills::Model> for CodexSkillRepositorySkillRecord {
    fn from(row: skills::Model) -> Self {
        Self {
            repository_id: row.repository_id,
            skill_id: row.skill_id,
            name: row.name,
            description: row.description,
            path: row.path,
            source_url: row.source_url,
            revision: row.revision,
        }
    }
}
impl SkillRepositoriesRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<CodexSkillRepositoryRecord>, DbErr> {
        repositories::Entity::find_by_id(id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }
    pub async fn skill(
        db: &impl ConnectionTrait,
        id: &str,
        skill: &str,
    ) -> Result<Option<CodexSkillRepositorySkillRecord>, DbErr> {
        skills::Entity::find_by_id((id.to_owned(), skill.to_owned()))
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }
    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<CodexSkillRepositoryRecord>, DbErr> {
        let mut rows: Vec<CodexSkillRepositoryRecord> = repositories::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        rows.sort_by(|a, b| {
            b.is_builtin
                .cmp(&a.is_builtin)
                .then_with(|| {
                    a.owner
                        .to_ascii_lowercase()
                        .cmp(&b.owner.to_ascii_lowercase())
                })
                .then_with(|| {
                    a.repository
                        .to_ascii_lowercase()
                        .cmp(&b.repository.to_ascii_lowercase())
                })
                .then_with(|| a.ref_name.cmp(&b.ref_name))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(rows)
    }
    pub async fn snapshot(
        db: &impl ConnectionTrait,
    ) -> Result<CodexSkillRepositoryCatalogSnapshot, DbErr> {
        let mut rows: Vec<CodexSkillRepositorySkillRecord> = skills::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        rows.sort_by(|a, b| {
            a.repository_id
                .cmp(&b.repository_id)
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
                .then_with(|| {
                    a.path
                        .to_ascii_lowercase()
                        .cmp(&b.path.to_ascii_lowercase())
                })
                .then_with(|| a.skill_id.cmp(&b.skill_id))
        });
        Ok(CodexSkillRepositoryCatalogSnapshot {
            repositories: Self::list(db).await?,
            skills: rows,
        })
    }
    pub async fn upsert(
        db: &DatabaseConnection,
        input: &CodexSkillRepositoryUpsert,
    ) -> Result<CodexSkillRepositoryRecord, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "skill_repositories").await?;
        let id = input.id.trim();
        let current = repositories::Entity::find_by_id(id).one(&tx).await?;
        let pinned = current.as_ref().is_some_and(|row| row.is_builtin);
        let owner = if pinned {
            current.as_ref().unwrap().owner.clone()
        } else {
            input.owner.trim().to_owned()
        };
        let repository = if pinned {
            current.as_ref().unwrap().repository.clone()
        } else {
            input.repository.trim().to_owned()
        };
        let ref_name = if pinned {
            current.as_ref().unwrap().ref_name.clone()
        } else {
            input.ref_name.trim().to_owned()
        };
        if repositories::Entity::find()
            .filter(repositories::Column::Owner.eq(&owner))
            .filter(repositories::Column::Repository.eq(&repository))
            .filter(repositories::Column::RefName.eq(&ref_name))
            .filter(repositories::Column::Id.ne(id))
            .one(&tx)
            .await?
            .is_some()
        {
            return Err(DbErr::Custom("Skills repository already exists".into()));
        }
        let model = repositories::ActiveModel {
            id: Set(id.to_owned()),
            owner: Set(owner),
            repository: Set(repository),
            ref_name: Set(ref_name),
            enabled: Set(input.enabled),
            is_builtin: Set(pinned || input.is_builtin),
            revision: Set(current.as_ref().and_then(|row| row.revision.clone())),
            last_scanned_at: Set(current.as_ref().and_then(|row| row.last_scanned_at)),
            last_error: Set(current.as_ref().and_then(|row| row.last_error.clone())),
            created_at: Set(current
                .as_ref()
                .map(|row| row.created_at)
                .unwrap_or(input.created_at)),
            updated_at: Set(input.updated_at),
        };
        let row = if current.is_some() {
            model.update(&tx).await?
        } else {
            model.insert(&tx).await?
        };
        tx.commit().await?;
        Ok(row.into())
    }
    pub async fn delete(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "skill_repositories").await?;
        let Some(row) = repositories::Entity::find_by_id(id).one(&tx).await? else {
            return Ok(false);
        };
        if row.is_builtin {
            return Ok(false);
        }
        skills::Entity::delete_many()
            .filter(skills::Column::RepositoryId.eq(id))
            .exec(&tx)
            .await?;
        repositories::Entity::delete_by_id(id).exec(&tx).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn replace_snapshot(
        db: &DatabaseConnection,
        id: &str,
        items: &[CodexSkillRepositorySkillRecord],
        scanned_at: i64,
    ) -> Result<(), DbErr> {
        let revision = items
            .first()
            .and_then(|skill| skill.revision.as_deref())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                DbErr::Custom(
                    "repository snapshot must contain at least one revision-pinned Skill".into(),
                )
            })?;
        if items
            .iter()
            .any(|skill| skill.repository_id != id || skill.revision.as_deref() != Some(revision))
        {
            return Err(DbErr::Custom(
                "repository snapshot Skills must belong to the repository and use one revision"
                    .into(),
            ));
        }
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "skill_repositories").await?;
        let current = repositories::Entity::find_by_id(id)
            .one(&tx)
            .await?
            .ok_or_else(|| DbErr::Custom("Skills repository not found".into()))?;
        let mut model: repositories::ActiveModel = current.into();
        model.revision = Set(Some(revision.to_owned()));
        model.last_scanned_at = Set(Some(scanned_at));
        model.last_error = Set(None);
        model.updated_at = Set(scanned_at);
        model.update(&tx).await?;
        skills::Entity::delete_many()
            .filter(skills::Column::RepositoryId.eq(id))
            .exec(&tx)
            .await?;
        for skill in items {
            skills::ActiveModel {
                repository_id: Set(id.to_owned()),
                skill_id: Set(skill.skill_id.trim().to_owned()),
                name: Set(skill.name.trim().to_owned()),
                description: Set(skill.description.clone()),
                path: Set(skill.path.clone()),
                source_url: Set(skill.source_url.trim().to_owned()),
                revision: Set(skill.revision.clone()),
            }
            .insert(&tx)
            .await?;
        }
        tx.commit().await
    }
    pub async fn record_error(
        db: &impl ConnectionTrait,
        id: &str,
        error: &str,
    ) -> Result<bool, DbErr> {
        Ok(repositories::Entity::update_many()
            .col_expr(
                repositories::Column::LastError,
                Expr::value(Some(error.to_owned())),
            )
            .col_expr(repositories::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(repositories::Column::Id.eq(id))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
    /// Seed only absent built-ins when the live skill catalog is opened. The
    /// schema/import path stays empty and can verify archived data exactly.
    pub async fn ensure_builtins(db: &DatabaseConnection) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "skill_repositories").await?;
        for (id, owner, repository, ref_name) in [
            ("builtin-anthropics-skills", "anthropics", "skills", "main"),
            (
                "builtin-composiohq-awesome-claude-skills",
                "ComposioHQ",
                "awesome-claude-skills",
                "master",
            ),
            ("builtin-cexll-myclaude", "cexll", "myclaude", "master"),
            (
                "builtin-jimliu-baoyu-skills",
                "JimLiu",
                "baoyu-skills",
                "main",
            ),
        ] {
            if repositories::Entity::find_by_id(id)
                .one(&tx)
                .await?
                .is_none()
            {
                repositories::ActiveModel {
                    id: Set(id.into()),
                    owner: Set(owner.into()),
                    repository: Set(repository.into()),
                    ref_name: Set(ref_name.into()),
                    enabled: Set(true),
                    is_builtin: Set(true),
                    revision: Set(None),
                    last_scanned_at: Set(None),
                    last_error: Set(None),
                    created_at: Set(now_ts()),
                    updated_at: Set(now_ts()),
                }
                .insert(&tx)
                .await?;
            }
        }
        tx.commit().await
    }
}
