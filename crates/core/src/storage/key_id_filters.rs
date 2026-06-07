use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{types::Value, Result};

use super::Storage;

// SQLite commonly defaults to 999 host parameters. Keep a little room for
// companion predicates when callers still need an IN-list based lookup.
pub(super) const SQLITE_IN_CLAUSE_BATCH_SIZE: usize = 900;

static NEXT_TEMP_FILTER_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn normalize_key_ids(key_ids: &[String]) -> Vec<String> {
    let mut normalized = key_ids
        .iter()
        .map(|key_id| key_id.trim())
        .filter(|key_id| !key_id.is_empty())
        .map(|key_id| key_id.to_string())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(super) fn key_id_in_clause(column: &str, key_ids: &[String]) -> Option<(String, Vec<Value>)> {
    let key_ids = normalize_key_ids(key_ids);
    if key_ids.is_empty() {
        return None;
    }

    let placeholders = std::iter::repeat("?")
        .take(key_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let params = key_ids.into_iter().map(Value::Text).collect::<Vec<_>>();
    Some((format!("{column} IN ({placeholders})"), params))
}

pub(super) struct KeyIdSqlFilter<'a> {
    condition: String,
    params: Vec<Value>,
    // Keep the temporary table alive for as long as the generated SQL can run.
    _temp_filter: Option<TempKeyIdFilter<'a>>,
}

impl<'a> KeyIdSqlFilter<'a> {
    pub(super) fn create(
        storage: &'a Storage,
        column: &str,
        key_ids: &[String],
    ) -> Result<Option<Self>> {
        let key_ids = normalize_key_ids(key_ids);
        if key_ids.is_empty() {
            return Ok(None);
        }

        if key_ids.len() <= SQLITE_IN_CLAUSE_BATCH_SIZE {
            let Some((condition, params)) = key_id_in_clause(column, &key_ids) else {
                return Ok(None);
            };
            return Ok(Some(Self {
                condition,
                params,
                _temp_filter: None,
            }));
        }

        let Some(temp_filter) = TempKeyIdFilter::create(storage, &key_ids)? else {
            return Ok(None);
        };
        let condition = temp_filter.condition(column);
        Ok(Some(Self {
            condition,
            params: Vec::new(),
            _temp_filter: Some(temp_filter),
        }))
    }

    pub(super) fn condition(&self) -> &str {
        &self.condition
    }

    pub(super) fn params(&self) -> &[Value] {
        &self.params
    }
}

pub(super) struct TempKeyIdFilter<'a> {
    storage: &'a Storage,
    table_name: String,
}

impl<'a> TempKeyIdFilter<'a> {
    pub(super) fn create(storage: &'a Storage, key_ids: &[String]) -> Result<Option<Self>> {
        let key_ids = normalize_key_ids(key_ids);
        if key_ids.is_empty() {
            return Ok(None);
        }

        let id = NEXT_TEMP_FILTER_ID.fetch_add(1, Ordering::Relaxed);
        let table_name = format!("cm_temp_key_id_filter_{id}");
        storage.conn.execute(
            &format!(
                "CREATE TEMP TABLE {table_name} (
                    key_id TEXT PRIMARY KEY
                 ) WITHOUT ROWID"
            ),
            [],
        )?;

        let savepoint_name = format!("cm_key_filter_insert_{id}");
        storage
            .conn
            .execute_batch(&format!("SAVEPOINT {savepoint_name}"))?;
        let insert_result = (|| -> Result<()> {
            let mut stmt = storage
                .conn
                .prepare(&format!("INSERT INTO {table_name} (key_id) VALUES (?1)"))?;
            for key_id in key_ids {
                stmt.execute([key_id])?;
            }
            Ok(())
        })();
        if let Err(err) = insert_result {
            let _ = storage.conn.execute_batch(&format!(
                "ROLLBACK TO {savepoint_name}; RELEASE {savepoint_name}"
            ));
            let _ = storage
                .conn
                .execute(&format!("DROP TABLE IF EXISTS {table_name}"), []);
            return Err(err);
        }
        storage
            .conn
            .execute_batch(&format!("RELEASE {savepoint_name}"))?;

        Ok(Some(Self {
            storage,
            table_name,
        }))
    }

    pub(super) fn condition(&self, column: &str) -> String {
        format!(
            "EXISTS (
                SELECT 1
                FROM {} key_filter
                WHERE key_filter.key_id = {column}
             )",
            self.table_name
        )
    }

    pub(super) fn exists_clause(&self, column: &str) -> String {
        format!(" AND {}", self.condition(column))
    }
}

impl Drop for TempKeyIdFilter<'_> {
    fn drop(&mut self) {
        let _ = self
            .storage
            .conn
            .execute(&format!("DROP TABLE IF EXISTS {}", self.table_name), []);
    }
}
