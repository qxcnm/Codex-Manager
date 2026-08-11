use super::{now_ts, KiroVaultError, Storage};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

type VaultResult<T> = Result<T, KiroVaultError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexAgentIdentityRecord {
    pub account_id: String,
    pub agent_runtime_id: String,
    pub agent_private_key: String,
    pub task_id: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub is_fedramp: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct CodexAgentIdentityUpsert {
    pub account_id: String,
    pub agent_runtime_id: String,
    pub agent_private_key: String,
    pub task_id: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub is_fedramp: bool,
    pub status: String,
    pub last_error: Option<String>,
}

impl Storage {
    pub fn upsert_codex_agent_identity(&self, input: &CodexAgentIdentityUpsert) -> VaultResult<()> {
        let private_key = self.encrypt_vault_text(
            &format!("codex-agent-identity:{}:private-key", input.account_id),
            &input.agent_private_key,
        )?;
        let task_id = input
            .task_id
            .as_deref()
            .map(|value| {
                self.encrypt_vault_text(
                    &format!("codex-agent-identity:{}:task-id", input.account_id),
                    value,
                )
            })
            .transpose()?;
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO codex_agent_identities (
                account_id, agent_runtime_id, encrypted_private_key, encrypted_task_id,
                chatgpt_user_id, email, plan_type, is_fedramp, status, last_error,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
             ON CONFLICT(account_id) DO UPDATE SET
                agent_runtime_id = excluded.agent_runtime_id,
                encrypted_private_key = excluded.encrypted_private_key,
                encrypted_task_id = excluded.encrypted_task_id,
                chatgpt_user_id = COALESCE(excluded.chatgpt_user_id, codex_agent_identities.chatgpt_user_id),
                email = COALESCE(excluded.email, codex_agent_identities.email),
                plan_type = COALESCE(excluded.plan_type, codex_agent_identities.plan_type),
                is_fedramp = excluded.is_fedramp,
                status = excluded.status,
                last_error = excluded.last_error,
                updated_at = excluded.updated_at",
            params![
                input.account_id,
                input.agent_runtime_id,
                private_key,
                task_id,
                input.chatgpt_user_id,
                input.email,
                input.plan_type,
                input.is_fedramp,
                input.status,
                input.last_error,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn find_codex_agent_identity(
        &self,
        account_id: &str,
    ) -> VaultResult<Option<CodexAgentIdentityRecord>> {
        let row = self
            .conn
            .query_row(
                "SELECT account_id, agent_runtime_id, encrypted_private_key, encrypted_task_id,
                        chatgpt_user_id, email, plan_type, is_fedramp, status, last_error,
                        created_at, updated_at
                 FROM codex_agent_identities WHERE account_id = ?1",
                [account_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, bool>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            account_id,
            agent_runtime_id,
            encrypted_private_key,
            encrypted_task_id,
            chatgpt_user_id,
            email,
            plan_type,
            is_fedramp,
            status,
            last_error,
            created_at,
            updated_at,
        )) = row
        else {
            return Ok(None);
        };
        let agent_private_key = self.decrypt_vault_text(
            &format!("codex-agent-identity:{account_id}:private-key"),
            &encrypted_private_key,
        )?;
        let task_id = encrypted_task_id
            .as_deref()
            .map(|value| {
                self.decrypt_vault_text(
                    &format!("codex-agent-identity:{account_id}:task-id"),
                    value,
                )
            })
            .transpose()?;
        Ok(Some(CodexAgentIdentityRecord {
            account_id,
            agent_runtime_id,
            agent_private_key,
            task_id,
            chatgpt_user_id,
            email,
            plan_type,
            is_fedramp,
            status,
            last_error,
            created_at,
            updated_at,
        }))
    }

    pub fn list_codex_agent_identities(&self) -> VaultResult<Vec<CodexAgentIdentityRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id FROM codex_agent_identities ORDER BY created_at ASC, account_id ASC",
        )?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.iter()
            .filter_map(|id| self.find_codex_agent_identity(id).transpose())
            .collect()
    }

    pub fn update_codex_agent_identity_task(
        &self,
        account_id: &str,
        task_id: &str,
    ) -> VaultResult<bool> {
        let encrypted = self.encrypt_vault_text(
            &format!("codex-agent-identity:{account_id}:task-id"),
            task_id,
        )?;
        Ok(self.conn.execute(
            "UPDATE codex_agent_identities
             SET encrypted_task_id = ?2, status = 'ready', last_error = NULL, updated_at = ?3
             WHERE account_id = ?1",
            params![account_id, encrypted, now_ts()],
        )? > 0)
    }

    pub fn clear_codex_agent_identity_task(&self, account_id: &str) -> VaultResult<bool> {
        Ok(self.conn.execute(
            "UPDATE codex_agent_identities
             SET encrypted_task_id = NULL, status = 'task_pending', last_error = NULL, updated_at = ?2
             WHERE account_id = ?1",
            params![account_id, now_ts()],
        )? > 0)
    }

    pub fn update_codex_agent_identity_status(
        &self,
        account_id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> VaultResult<bool> {
        Ok(self.conn.execute(
            "UPDATE codex_agent_identities
             SET status = ?2, last_error = ?3, updated_at = ?4
             WHERE account_id = ?1",
            params![account_id, status, last_error, now_ts()],
        )? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Account;

    #[test]
    fn agent_identity_secrets_are_encrypted_and_round_trip() {
        let storage = Storage::open_in_memory().expect("storage");
        storage.init().expect("migrations");
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: "account-agent-1".to_string(),
                label: "agent@example.com".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some("workspace-1".to_string()),
                workspace_id: Some("workspace-1".to_string()),
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("account");
        let input = CodexAgentIdentityUpsert {
            account_id: "account-agent-1".to_string(),
            agent_runtime_id: "runtime-1".to_string(),
            agent_private_key: "private-key-secret".to_string(),
            task_id: Some("task-id-secret".to_string()),
            chatgpt_user_id: Some("user-1".to_string()),
            email: Some("agent@example.com".to_string()),
            plan_type: Some("plus".to_string()),
            is_fedramp: false,
            status: "ready".to_string(),
            last_error: None,
        };
        storage
            .upsert_codex_agent_identity(&input)
            .expect("upsert identity");

        let (stored_key, stored_task): (String, Option<String>) = storage
            .conn
            .query_row(
                "SELECT encrypted_private_key, encrypted_task_id FROM codex_agent_identities WHERE account_id = ?1",
                [&input.account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("stored ciphertext");
        assert_ne!(stored_key, input.agent_private_key);
        assert_ne!(stored_task.as_deref(), input.task_id.as_deref());

        let restored = storage
            .find_codex_agent_identity(&input.account_id)
            .expect("read identity")
            .expect("identity");
        assert_eq!(restored.agent_private_key, input.agent_private_key);
        assert_eq!(restored.task_id, input.task_id);
    }
}
