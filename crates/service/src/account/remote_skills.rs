use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{
    CodexSkillRepositoryCatalogSnapshot, CodexSkillRepositoryRecord,
    CodexSkillRepositorySkillRecord, CodexSkillRepositoryUpsert,
};
use codexmanager_storage_seaorm::SkillRepositoriesRepository;
use std::ops::Deref;
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
impl AccountStorage<'_> {
    pub(crate) fn list_codex_skill_repositories(
        &self,
    ) -> rusqlite::Result<Vec<CodexSkillRepositoryRecord>> {
        if !seaorm_enabled() {
            return self.deref().list_codex_skill_repositories();
        }
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::ensure_builtins(s.connection())
                .await
                .map_err(|e| e.to_string())?;
            SkillRepositoriesRepository::list(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn get_codex_skill_repository(
        &self,
        id: &str,
    ) -> rusqlite::Result<Option<CodexSkillRepositoryRecord>> {
        if !seaorm_enabled() {
            return self.deref().get_codex_skill_repository(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::get(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn get_codex_skill_repository_skill(
        &self,
        id: &str,
        skill: &str,
    ) -> rusqlite::Result<Option<CodexSkillRepositorySkillRecord>> {
        if !seaorm_enabled() {
            return self.deref().get_codex_skill_repository_skill(id, skill);
        }
        let id = id.to_owned();
        let skill = skill.to_owned();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::skill(s.connection(), &id, &skill)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn codex_skill_repository_catalog_snapshot(
        &self,
    ) -> rusqlite::Result<CodexSkillRepositoryCatalogSnapshot> {
        if !seaorm_enabled() {
            return self.deref().codex_skill_repository_catalog_snapshot();
        }
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::ensure_builtins(s.connection())
                .await
                .map_err(|e| e.to_string())?;
            SkillRepositoriesRepository::snapshot(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn upsert_codex_skill_repository(
        &self,
        input: &CodexSkillRepositoryUpsert,
    ) -> rusqlite::Result<CodexSkillRepositoryRecord> {
        if !seaorm_enabled() {
            return self.deref().upsert_codex_skill_repository(input);
        }
        let input = input.clone();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::upsert(s.connection(), &input)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_codex_skill_repository(&self, id: &str) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.deref().delete_codex_skill_repository(id);
        }
        let id = id.to_owned();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::delete(s.connection(), &id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn replace_codex_skill_repository_snapshot(
        &self,
        id: &str,
        items: &[CodexSkillRepositorySkillRecord],
        scanned: i64,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .replace_codex_skill_repository_snapshot(id, items, scanned);
        }
        let id = id.to_owned();
        let items = items.to_vec();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::replace_snapshot(s.connection(), &id, &items, scanned)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn record_codex_skill_repository_error(
        &self,
        id: &str,
        err: &str,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self.deref().record_codex_skill_repository_error(id, err);
        }
        let id = id.to_owned();
        let err = err.to_owned();
        seaorm_block_on(move |s| async move {
            SkillRepositoriesRepository::record_error(s.connection(), &id, &err)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
