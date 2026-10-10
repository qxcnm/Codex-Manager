use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::ConversationBinding;
use codexmanager_storage_seaorm::ConversationBindingsRepository;
use std::{collections::HashMap, ops::Deref};
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
impl AccountStorage<'_> {
    pub(crate) fn get_conversation_binding(
        &self,
        key: &str,
        conversation: &str,
    ) -> rusqlite::Result<Option<ConversationBinding>> {
        if !seaorm_enabled() {
            return self.deref().get_conversation_binding(key, conversation);
        }
        let key = key.to_owned();
        let conversation = conversation.to_owned();
        seaorm_block_on(move |storage| async move {
            ConversationBindingsRepository::get(storage.connection(), &key, &conversation)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn upsert_conversation_binding(
        &self,
        binding: &ConversationBinding,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_conversation_binding(binding);
        }
        let binding = binding.clone();
        seaorm_block_on(move |storage| async move {
            ConversationBindingsRepository::upsert(storage.connection(), &binding)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn claim_conversation_binding(
        &self,
        binding: &ConversationBinding,
    ) -> rusqlite::Result<(ConversationBinding, bool)> {
        if !seaorm_enabled() {
            return self.deref().claim_conversation_binding(binding);
        }
        let binding = binding.clone();
        seaorm_block_on(move |storage| async move {
            ConversationBindingsRepository::claim(storage.connection(), &binding)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn touch_conversation_binding(
        &self,
        key: &str,
        conversation: &str,
        account: &str,
        model: Option<&str>,
        now: i64,
    ) -> rusqlite::Result<bool> {
        if !seaorm_enabled() {
            return self
                .deref()
                .touch_conversation_binding(key, conversation, account, model, now);
        }
        let key = key.to_owned();
        let conversation = conversation.to_owned();
        let account = account.to_owned();
        let model = model.map(ToOwned::to_owned);
        seaorm_block_on(move |storage| async move {
            ConversationBindingsRepository::touch(
                storage.connection(),
                &key,
                &conversation,
                &account,
                model.as_deref(),
                now,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn active_conversation_binding_account_counts(
        &self,
        key: &str,
    ) -> rusqlite::Result<HashMap<String, usize>> {
        if !seaorm_enabled() {
            return self.deref().active_conversation_binding_account_counts(key);
        }
        let key = key.to_owned();
        seaorm_block_on(move |storage| async move {
            ConversationBindingsRepository::active_account_counts(storage.connection(), &key)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
