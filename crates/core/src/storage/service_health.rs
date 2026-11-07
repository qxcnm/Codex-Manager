//! Small, bounded health metadata store. Payloads are typed by the service;
//! this table never stores request bodies, raw errors or credentials.
use super::Storage;
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone)]
pub struct ServiceHealthRecord {
    pub id: String,
    pub kind: String,
    pub owner: String,
    pub payload: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct ServiceHealthLogSample {
    pub provider: String,
    pub status_code: Option<i64>,
    pub first_response_ms: Option<i64>,
    pub has_error: bool,
    pub upstream_present: bool,
    pub confirmed_upstream_error: bool,
    pub network_error: bool,
    pub created_at: i64,
}

/// All interpolated values are clamped integers; the same bounded, metadata-only
/// query is used on SQLite, MySQL and PostgreSQL. Protocol paths select inference
/// traffic, never identify a provider (protocol conversion is supported).
pub fn service_health_sample_sql(start: i64, end: i64, limit: usize) -> String {
    format!("SELECT CASE WHEN r.actual_source_kind = 'openai_account' THEN 'codex'
        WHEN r.actual_source_kind = 'aggregate_api' AND p.provider_type IN ('codex', 'claude') THEN p.provider_type
        ELSE 'unknown' END AS provider,
        r.status_code, r.first_response_ms,
        CASE WHEN TRIM(COALESCE(r.error, '')) <> '' THEN 1 ELSE 0 END AS has_error,
        CASE WHEN TRIM(COALESCE(r.upstream_url, '')) <> '' THEN 1 ELSE 0 END AS upstream_present,
        CASE WHEN LOWER(COALESCE(r.error, '')) = 'upstream server error' THEN 1 ELSE 0 END AS confirmed_upstream_error,
        CASE WHEN LOWER(COALESCE(r.error, '')) LIKE '%connect error%' OR LOWER(COALESCE(r.error, '')) LIKE '%dns error%' THEN 1 ELSE 0 END AS network_error,
        r.created_at
        FROM request_logs r LEFT JOIN aggregate_apis p ON r.actual_source_kind = 'aggregate_api' AND p.id = r.actual_source_id
        WHERE r.cleared_at IS NULL AND r.created_at >= {} AND r.created_at < {}
        AND r.method = 'POST' AND (r.request_path LIKE '/v1/responses%' OR r.request_path LIKE '/v1/chat/completions%' OR r.request_path = '/v1/messages')
        ORDER BY r.created_at DESC, r.id DESC LIMIT {}", start.max(0), end.max(0), limit.min(2001))
}

fn read_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ServiceHealthRecord> {
    Ok(ServiceHealthRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        owner: row.get(2)?,
        payload: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        expires_at: row.get(6)?,
    })
}

impl Storage {
    pub fn service_health_get(&self, id: &str) -> rusqlite::Result<Option<ServiceHealthRecord>> {
        self.conn.query_row("SELECT id,kind,owner,payload,created_at,updated_at,expires_at FROM service_health_records WHERE id=?1", [id], read_record).optional()
    }

    pub fn service_health_list(
        &self,
        kind: &str,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<ServiceHealthRecord>> {
        let mut stmt = self.conn.prepare("SELECT id,kind,owner,payload,created_at,updated_at,expires_at FROM service_health_records WHERE kind=?1 AND expires_at>?2 ORDER BY updated_at DESC,id LIMIT ?3")?;
        stmt.query_map(params![kind, now, limit.min(10001) as i64], read_record)?
            .collect()
    }

    /// Compare-and-swap makes report cooldowns work
    /// across listeners/processes without trusting a client-provided identity.
    pub fn service_health_put(
        &self,
        row: &ServiceHealthRecord,
        expected: Option<i64>,
    ) -> rusqlite::Result<bool> {
        let changed = if let Some(expected) = expected {
            self.conn.execute("UPDATE service_health_records SET kind=?2,owner=?3,payload=?4,created_at=?5,updated_at=?6,expires_at=?7 WHERE id=?1 AND updated_at=?8", params![row.id,row.kind,row.owner,row.payload,row.created_at,row.updated_at,row.expires_at,expected])?
        } else {
            self.conn.execute("INSERT OR IGNORE INTO service_health_records(id,kind,owner,payload,created_at,updated_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![row.id,row.kind,row.owner,row.payload,row.created_at,row.updated_at,row.expires_at])?
        };
        Ok(changed == 1)
    }

    pub fn service_health_prune(&self, now: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM service_health_records WHERE expires_at <= ?1",
            [now],
        )?;
        Ok(())
    }

    pub fn service_health_samples(
        &self,
        start: i64,
        end: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<ServiceHealthLogSample>> {
        let mut stmt = self
            .conn
            .prepare(&service_health_sample_sql(start, end, limit))?;
        stmt.query_map([], |row| {
            Ok(ServiceHealthLogSample {
                provider: row.get(0)?,
                status_code: row.get(1)?,
                first_response_ms: row.get(2)?,
                has_error: row.get::<_, i64>(3)? != 0,
                upstream_present: row.get::<_, i64>(4)? != 0,
                confirmed_upstream_error: row.get::<_, i64>(5)? != 0,
                network_error: row.get::<_, i64>(6)? != 0,
                created_at: row.get(7)?,
            })
        })?
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::RequestLog;

    #[test]
    fn samples_are_time_bounded_and_use_actual_provider_not_client_protocol() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage.conn.execute("INSERT INTO aggregate_apis(id,provider_type,url,created_at,updated_at) VALUES('claude-source','claude','https://private.invalid',1,1)", []).unwrap();
        let mut log = RequestLog {
            request_path: "/v1/messages".into(),
            method: "POST".into(),
            actual_source_kind: Some("openai_account".into()),
            status_code: Some(200),
            first_response_ms: Some(123),
            created_at: 100,
            ..Default::default()
        };
        storage.insert_request_log(&log).unwrap();
        log.request_path = "/v1/responses".into();
        log.actual_source_kind = Some("aggregate_api".into());
        log.actual_source_id = Some("claude-source".into());
        log.created_at = 101;
        storage.insert_request_log(&log).unwrap();
        log.actual_source_kind = None;
        log.actual_source_id = None;
        log.created_at = 102;
        storage.insert_request_log(&log).unwrap();
        log.created_at = 1;
        storage.insert_request_log(&log).unwrap();
        log.created_at = 103;
        log.request_path = "/v1/messages/count_tokens".into();
        storage.insert_request_log(&log).unwrap();
        let samples = storage.service_health_samples(100, 103, 2001).unwrap();
        assert_eq!(
            samples
                .iter()
                .map(|s| s.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["unknown", "claude", "codex"]
        );
        assert_eq!(samples[2].first_response_ms, Some(123));
        storage
            .conn
            .execute(
                "UPDATE request_logs SET cleared_at=104 WHERE created_at=101",
                [],
            )
            .unwrap();
        assert_eq!(
            storage
                .service_health_samples(100, 103, 2001)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            storage.service_health_samples(100, 103, 1).unwrap().len(),
            1
        );
        assert!(service_health_sample_sql(0, 200, usize::MAX).ends_with("LIMIT 2001"));
    }
    #[test]
    fn records_compare_and_swap_and_expiry() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let mut row = ServiceHealthRecord {
            id: "r".into(),
            kind: "report".into(),
            owner: "u".into(),
            payload: "{}".into(),
            created_at: 1,
            updated_at: 1,
            expires_at: 20,
        };
        assert!(storage.service_health_put(&row, None).unwrap());
        assert!(!storage.service_health_put(&row, None).unwrap());
        row.updated_at = 2;
        assert!(storage.service_health_put(&row, Some(1)).unwrap());
        assert!(!storage.service_health_put(&row, Some(1)).unwrap());
        assert_eq!(
            storage.service_health_list("report", 10, 10).unwrap().len(),
            1
        );
        storage.service_health_prune(20).unwrap();
        assert!(storage.service_health_get("r").unwrap().is_none());
    }
}
