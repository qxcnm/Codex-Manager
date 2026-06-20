use rusqlite::{params, Result, Row};

use super::{
    ProxyProfileUrlTest, ProxyProfileUrlTestInsertInput, Storage,
    ProxySpeedTest, ProxySpeedTestInsertInput,
    ProxyDiagnosticTest, ProxyDiagnosticTestInsertInput,
    AccountProxyUrlTest, AccountProxyUrlTestInsertInput,
};

impl Storage {
    pub fn insert_proxy_profile_url_test(
        &self,
        input: &ProxyProfileUrlTestInsertInput,
    ) -> Result<ProxyProfileUrlTest> {
        self.conn.execute(
            "INSERT INTO proxy_profile_url_tests (
                proxy_profile_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                input.proxy_profile_id.trim(),
                normalize_test_status(&input.status),
                input.url_latency_ms,
                input.status_code,
                input.test_url.trim(),
                normalize_optional_text(input.final_url.as_deref()),
                if input.redirected { 1 } else { 0 },
                input.tested_at,
                normalize_optional_text(input.error_code.as_deref()),
                normalize_optional_text(input.error.as_deref()),
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        self.find_proxy_profile_url_test(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn find_proxy_profile_url_test(&self, id: i64) -> Result<Option<ProxyProfileUrlTest>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                proxy_profile_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
             FROM proxy_profile_url_tests
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_proxy_profile_url_test_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_proxy_profile_url_tests(
        &self,
        proxy_profile_id: &str,
        limit: usize,
    ) -> Result<Vec<ProxyProfileUrlTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                proxy_profile_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
             FROM proxy_profile_url_tests
             WHERE proxy_profile_id = ?1
             ORDER BY tested_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![proxy_profile_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_proxy_profile_url_test_row(row)?);
        }
        Ok(items)
    }

    pub(super) fn ensure_proxy_profile_url_tests_table(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS proxy_profile_url_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                proxy_profile_id TEXT NOT NULL,
                status TEXT NOT NULL,
                url_latency_ms INTEGER,
                status_code INTEGER,
                test_url TEXT NOT NULL,
                final_url TEXT,
                redirected INTEGER NOT NULL DEFAULT 0,
                tested_at INTEGER NOT NULL,
                error_code TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_proxy_profile_url_tests_profile_tested_at
                ON proxy_profile_url_tests(proxy_profile_id, tested_at DESC, id DESC);",
        )?;
        self.ensure_column(
            "proxy_profile_url_tests",
            "proxy_profile_id",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        self.ensure_column(
            "proxy_profile_url_tests",
            "status",
            "TEXT NOT NULL DEFAULT 'unchecked'",
        )?;
        self.ensure_column("proxy_profile_url_tests", "url_latency_ms", "INTEGER")?;
        self.ensure_column("proxy_profile_url_tests", "status_code", "INTEGER")?;
        self.ensure_column(
            "proxy_profile_url_tests",
            "test_url",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        self.ensure_column("proxy_profile_url_tests", "final_url", "TEXT")?;
        self.ensure_column(
            "proxy_profile_url_tests",
            "redirected",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "proxy_profile_url_tests",
            "tested_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("proxy_profile_url_tests", "error_code", "TEXT")?;
        self.ensure_column("proxy_profile_url_tests", "error", "TEXT")?;
        Ok(())
    }
}

fn map_proxy_profile_url_test_row(row: &Row<'_>) -> Result<ProxyProfileUrlTest> {
    Ok(ProxyProfileUrlTest {
        id: row.get(0)?,
        proxy_profile_id: row.get(1)?,
        status: row.get(2)?,
        url_latency_ms: row.get(3)?,
        status_code: row.get(4)?,
        test_url: row.get(5)?,
        final_url: row.get(6)?,
        redirected: row.get::<_, i64>(7)? != 0,
        tested_at: row.get(8)?,
        error_code: row.get(9)?,
        error: row.get(10)?,
    })
}


impl Storage {
    pub fn insert_proxy_speed_test(
        &self,
        input: &ProxySpeedTestInsertInput,
    ) -> Result<ProxySpeedTest> {
        self.conn.execute(
            "INSERT INTO proxy_speed_tests (
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                observed_ip,
                observed_country,
                observed_colo,
                max_payload_bytes,
                samples_json,
                download_summary_json,
                upload_summary_json,
                started_at,
                finished_at,
                error_code,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                input.scope.trim(),
                normalize_optional_text(input.proxy_profile_id.as_deref()),
                normalize_optional_text(input.account_id.as_deref()),
                input.status.trim(),
                input.provider.trim(),
                normalize_optional_text(input.observed_ip.as_deref()),
                normalize_optional_text(input.observed_country.as_deref()),
                normalize_optional_text(input.observed_colo.as_deref()),
                input.max_payload_bytes,
                normalize_optional_text(input.samples_json.as_deref()),
                normalize_optional_text(input.download_summary_json.as_deref()),
                normalize_optional_text(input.upload_summary_json.as_deref()),
                input.started_at,
                input.finished_at,
                normalize_optional_text(input.error_code.as_deref()),
                normalize_optional_text(input.error.as_deref()),
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        self.find_proxy_speed_test(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn find_proxy_speed_test(&self, id: i64) -> Result<Option<ProxySpeedTest>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                observed_ip,
                observed_country,
                observed_colo,
                max_payload_bytes,
                samples_json,
                download_summary_json,
                upload_summary_json,
                started_at,
                finished_at,
                error_code,
                error
             FROM proxy_speed_tests
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(ProxySpeedTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                observed_ip: row.get(6)?,
                observed_country: row.get(7)?,
                observed_colo: row.get(8)?,
                max_payload_bytes: row.get(9)?,
                samples_json: row.get(10)?,
                download_summary_json: row.get(11)?,
                upload_summary_json: row.get(12)?,
                started_at: row.get(13)?,
                finished_at: row.get(14)?,
                error_code: row.get(15)?,
                error: row.get(16)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_proxy_speed_tests_by_profile(
        &self,
        proxy_profile_id: &str,
        limit: usize,
    ) -> Result<Vec<ProxySpeedTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                observed_ip,
                observed_country,
                observed_colo,
                max_payload_bytes,
                samples_json,
                download_summary_json,
                upload_summary_json,
                started_at,
                finished_at,
                error_code,
                error
             FROM proxy_speed_tests
             WHERE proxy_profile_id = ?1 AND scope = 'system_proxy'
             ORDER BY started_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![proxy_profile_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(ProxySpeedTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                observed_ip: row.get(6)?,
                observed_country: row.get(7)?,
                observed_colo: row.get(8)?,
                max_payload_bytes: row.get(9)?,
                samples_json: row.get(10)?,
                download_summary_json: row.get(11)?,
                upload_summary_json: row.get(12)?,
                started_at: row.get(13)?,
                finished_at: row.get(14)?,
                error_code: row.get(15)?,
                error: row.get(16)?,
            });
        }
        Ok(items)
    }

    pub fn list_proxy_speed_tests_by_account(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<ProxySpeedTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                observed_ip,
                observed_country,
                observed_colo,
                max_payload_bytes,
                samples_json,
                download_summary_json,
                upload_summary_json,
                started_at,
                finished_at,
                error_code,
                error
             FROM proxy_speed_tests
             WHERE account_id = ?1 AND scope = 'account_proxy'
             ORDER BY started_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![account_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(ProxySpeedTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                observed_ip: row.get(6)?,
                observed_country: row.get(7)?,
                observed_colo: row.get(8)?,
                max_payload_bytes: row.get(9)?,
                samples_json: row.get(10)?,
                download_summary_json: row.get(11)?,
                upload_summary_json: row.get(12)?,
                started_at: row.get(13)?,
                finished_at: row.get(14)?,
                error_code: row.get(15)?,
                error: row.get(16)?,
            });
        }
        Ok(items)
    }

    pub fn insert_proxy_diagnostic_test(
        &self,
        input: &ProxyDiagnosticTestInsertInput,
    ) -> Result<ProxyDiagnosticTest> {
        self.conn.execute(
            "INSERT INTO proxy_diagnostics_history (
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                file_size_id,
                downloaded_bytes,
                duration_ms,
                mbps,
                tested_at,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                input.scope.trim(),
                normalize_optional_text(input.proxy_profile_id.as_deref()),
                normalize_optional_text(input.account_id.as_deref()),
                input.status.trim(),
                input.provider.trim(),
                input.file_size_id.trim(),
                input.downloaded_bytes,
                input.duration_ms,
                input.mbps,
                input.tested_at,
                normalize_optional_text(input.error.as_deref()),
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        self.find_proxy_diagnostic_test(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn find_proxy_diagnostic_test(&self, id: i64) -> Result<Option<ProxyDiagnosticTest>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                file_size_id,
                downloaded_bytes,
                duration_ms,
                mbps,
                tested_at,
                error
             FROM proxy_diagnostics_history
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(ProxyDiagnosticTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                file_size_id: row.get(6)?,
                downloaded_bytes: row.get(7)?,
                duration_ms: row.get(8)?,
                mbps: row.get(9)?,
                tested_at: row.get(10)?,
                error: row.get(11)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_proxy_diagnostic_tests_by_profile(
        &self,
        proxy_profile_id: &str,
        limit: usize,
    ) -> Result<Vec<ProxyDiagnosticTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                file_size_id,
                downloaded_bytes,
                duration_ms,
                mbps,
                tested_at,
                error
             FROM proxy_diagnostics_history
             WHERE proxy_profile_id = ?1 AND scope = 'system_proxy'
             ORDER BY tested_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![proxy_profile_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(ProxyDiagnosticTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                file_size_id: row.get(6)?,
                downloaded_bytes: row.get(7)?,
                duration_ms: row.get(8)?,
                mbps: row.get(9)?,
                tested_at: row.get(10)?,
                error: row.get(11)?,
            });
        }
        Ok(items)
    }

    pub fn list_proxy_diagnostic_tests_by_account(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<ProxyDiagnosticTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                scope,
                proxy_profile_id,
                account_id,
                status,
                provider,
                file_size_id,
                downloaded_bytes,
                duration_ms,
                mbps,
                tested_at,
                error
             FROM proxy_diagnostics_history
             WHERE account_id = ?1 AND scope = 'account_proxy'
             ORDER BY tested_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![account_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(ProxyDiagnosticTest {
                id: row.get(0)?,
                scope: row.get(1)?,
                proxy_profile_id: row.get(2)?,
                account_id: row.get(3)?,
                status: row.get(4)?,
                provider: row.get(5)?,
                file_size_id: row.get(6)?,
                downloaded_bytes: row.get(7)?,
                duration_ms: row.get(8)?,
                mbps: row.get(9)?,
                tested_at: row.get(10)?,
                error: row.get(11)?,
            });
        }
        Ok(items)
    }

    pub fn insert_account_proxy_url_test(
        &self,
        input: &AccountProxyUrlTestInsertInput,
    ) -> Result<AccountProxyUrlTest> {
        self.conn.execute(
            "INSERT INTO account_proxy_url_tests (
                account_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                input.account_id.trim(),
                normalize_test_status(&input.status),
                input.url_latency_ms,
                input.status_code,
                input.test_url.trim(),
                normalize_optional_text(input.final_url.as_deref()),
                if input.redirected { 1 } else { 0 },
                input.tested_at,
                normalize_optional_text(input.error_code.as_deref()),
                normalize_optional_text(input.error.as_deref()),
            ],
        )?;

        let id = self.conn.last_insert_rowid();
        self.find_account_proxy_url_test(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn find_account_proxy_url_test(&self, id: i64) -> Result<Option<AccountProxyUrlTest>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                account_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
             FROM account_proxy_url_tests
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(AccountProxyUrlTest {
                id: row.get(0)?,
                account_id: row.get(1)?,
                status: row.get(2)?,
                url_latency_ms: row.get(3)?,
                status_code: row.get(4)?,
                test_url: row.get(5)?,
                final_url: row.get(6)?,
                redirected: row.get::<_, i64>(7)? != 0,
                tested_at: row.get(8)?,
                error_code: row.get(9)?,
                error: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_account_proxy_url_tests(
        &self,
        account_id: &str,
        limit: usize,
    ) -> Result<Vec<AccountProxyUrlTest>> {
        let normalized_limit = limit.max(1).min(i64::MAX as usize) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                account_id,
                status,
                url_latency_ms,
                status_code,
                test_url,
                final_url,
                redirected,
                tested_at,
                error_code,
                error
             FROM account_proxy_url_tests
             WHERE account_id = ?1
             ORDER BY tested_at DESC, id DESC
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![account_id.trim(), normalized_limit])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(AccountProxyUrlTest {
                id: row.get(0)?,
                account_id: row.get(1)?,
                status: row.get(2)?,
                url_latency_ms: row.get(3)?,
                status_code: row.get(4)?,
                test_url: row.get(5)?,
                final_url: row.get(6)?,
                redirected: row.get::<_, i64>(7)? != 0,
                tested_at: row.get(8)?,
                error_code: row.get(9)?,
                error: row.get(10)?,
            });
        }
        Ok(items)
    }

    pub fn delete_proxy_profile_url_tests_by_profile(
        &self,
        proxy_profile_id: &str,
    ) -> Result<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM proxy_profile_url_tests WHERE proxy_profile_id = ?1",
            [proxy_profile_id.trim()],
        )?;
        Ok(deleted)
    }

    pub fn delete_proxy_speed_tests_by_profile(
        &self,
        proxy_profile_id: &str,
    ) -> Result<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM proxy_speed_tests WHERE proxy_profile_id = ?1",
            [proxy_profile_id.trim()],
        )?;
        Ok(deleted)
    }

    pub fn delete_proxy_diagnostic_tests_by_profile(
        &self,
        proxy_profile_id: &str,
    ) -> Result<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM proxy_diagnostics_history WHERE proxy_profile_id = ?1",
            [proxy_profile_id.trim()],
        )?;
        Ok(deleted)
    }

    pub(super) fn ensure_proxy_history_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS proxy_speed_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope TEXT NOT NULL,
                proxy_profile_id TEXT,
                account_id TEXT,
                status TEXT NOT NULL,
                provider TEXT NOT NULL,
                observed_ip TEXT,
                observed_country TEXT,
                observed_colo TEXT,
                max_payload_bytes INTEGER,
                samples_json TEXT,
                download_summary_json TEXT,
                upload_summary_json TEXT,
                started_at INTEGER NOT NULL,
                finished_at INTEGER NOT NULL,
                error_code TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_proxy_speed_tests_profile ON proxy_speed_tests(proxy_profile_id, started_at DESC);
            CREATE INDEX IF NOT EXISTS idx_proxy_speed_tests_account ON proxy_speed_tests(account_id, started_at DESC);

            CREATE TABLE IF NOT EXISTS proxy_diagnostics_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope TEXT NOT NULL,
                proxy_profile_id TEXT,
                account_id TEXT,
                status TEXT NOT NULL,
                provider TEXT NOT NULL,
                file_size_id TEXT NOT NULL,
                downloaded_bytes INTEGER,
                duration_ms INTEGER,
                mbps REAL,
                tested_at INTEGER NOT NULL,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_proxy_diagnostics_history_profile ON proxy_diagnostics_history(proxy_profile_id, tested_at DESC);
            CREATE INDEX IF NOT EXISTS idx_proxy_diagnostics_history_account ON proxy_diagnostics_history(account_id, tested_at DESC);

            CREATE TABLE IF NOT EXISTS account_proxy_url_tests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                status TEXT NOT NULL,
                url_latency_ms INTEGER,
                status_code INTEGER,
                test_url TEXT NOT NULL,
                final_url TEXT,
                redirected INTEGER NOT NULL DEFAULT 0,
                tested_at INTEGER NOT NULL,
                error_code TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_account_proxy_url_tests_account ON account_proxy_url_tests(account_id, tested_at DESC, id DESC);",
        )?;
        Ok(())
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_test_status(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "failed".to_string()
    } else {
        trimmed.to_string()
    }
}
