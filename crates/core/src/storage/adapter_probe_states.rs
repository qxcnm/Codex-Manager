use rusqlite::{params, params_from_iter};
use serde::{Deserialize, Serialize};

use super::{key_id_filters::text_id_in_clause, Storage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterCredentialProbeState {
    pub pool_id: String,
    pub credential_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub checked_at: i64,
    pub retry_after: Option<i64>,
}

impl Storage {
    pub fn upsert_adapter_credential_probe_state(
        &self,
        state: &AdapterCredentialProbeState,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO adapter_credential_probe_states (
                pool_id, credential_id, status, error_code, checked_at, retry_after
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(pool_id, credential_id) DO UPDATE SET
                status = excluded.status,
                error_code = excluded.error_code,
                checked_at = excluded.checked_at,
                retry_after = excluded.retry_after",
            params![
                state.pool_id,
                state.credential_id,
                state.status,
                state.error_code,
                state.checked_at,
                state.retry_after,
            ],
        )?;
        Ok(())
    }

    pub fn mark_adapter_credential_unprobed(
        &self,
        pool_id: &str,
        credential_id: &str,
        checked_at: i64,
    ) -> rusqlite::Result<()> {
        self.upsert_adapter_credential_probe_state(&AdapterCredentialProbeState {
            pool_id: pool_id.to_string(),
            credential_id: credential_id.to_string(),
            status: "unprobed".to_string(),
            error_code: None,
            checked_at,
            retry_after: None,
        })
    }

    pub fn list_adapter_credential_probe_states(
        &self,
        pool_id: &str,
        credential_ids: &[String],
    ) -> rusqlite::Result<Vec<AdapterCredentialProbeState>> {
        let Some((id_clause, mut id_params)) = text_id_in_clause("credential_id", credential_ids)
        else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT pool_id, credential_id, status, error_code, checked_at, retry_after
             FROM adapter_credential_probe_states
             WHERE pool_id = ? AND {id_clause}"
        );
        let mut query_params = vec![rusqlite::types::Value::Text(pool_id.to_string())];
        query_params.append(&mut id_params);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(query_params), |row| {
            Ok(AdapterCredentialProbeState {
                pool_id: row.get(0)?,
                credential_id: row.get(1)?,
                status: row.get(2)?,
                error_code: row.get(3)?,
                checked_at: row.get(4)?,
                retry_after: row.get(5)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_state_round_trips_and_can_be_invalidated() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_adapter_credential_probe_state(&AdapterCredentialProbeState {
                pool_id: "codex".to_string(),
                credential_id: "account-1".to_string(),
                status: "available".to_string(),
                error_code: None,
                checked_at: 100,
                retry_after: None,
            })
            .expect("save available state");

        let ids = vec!["account-1".to_string()];
        let available = storage
            .list_adapter_credential_probe_states("codex", &ids)
            .expect("read available state");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].status, "available");

        storage
            .mark_adapter_credential_unprobed("codex", "account-1", 200)
            .expect("invalidate state");
        let invalidated = storage
            .list_adapter_credential_probe_states("codex", &ids)
            .expect("read invalidated state");
        assert_eq!(invalidated[0].status, "unprobed");
        assert_eq!(invalidated[0].checked_at, 200);
    }
}
