use rusqlite::{Result, Row};

use super::{now_ts, AccountProxySettings, Storage};

impl Storage {
    pub fn upsert_account_proxy_settings(
        &self,
        account_id: &str,
        enabled: bool,
        proxy_url: Option<&str>,
        status: &str,
        latency_ms: Option<i64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
        ip: Option<&str>,
        country_code: Option<&str>,
        country_name: Option<&str>,
        region_name: Option<&str>,
        city_name: Option<&str>,
        geo_checked_at: Option<i64>,
        geo_error: Option<&str>,
        asn: Option<i64>,
        as_org: Option<&str>,
        isp: Option<&str>,
        as_domain: Option<&str>,
        timezone_id: Option<&str>,
        timezone_offset: Option<i64>,
        timezone_utc: Option<&str>,
        flag_img_url: Option<&str>,
        flag_emoji: Option<&str>,
    ) -> Result<()> {
        let now = now_ts();
        let created_at = self
            .find_account_proxy_settings(account_id)?
            .map(|settings| settings.created_at)
            .unwrap_or(now);

        self.conn.execute(
            "INSERT INTO account_proxy_settings (
                account_id,
                enabled,
                proxy_url,
                status,
                latency_ms,
                last_check_at,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                asn,
                as_org,
                isp,
                as_domain,
                timezone_id,
                timezone_offset,
                timezone_utc,
                flag_img_url,
                flag_emoji,
                created_at,
                updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                ?21, ?22, ?23, ?24, ?25
            )
            ON CONFLICT(account_id) DO UPDATE SET
                enabled = excluded.enabled,
                proxy_url = excluded.proxy_url,
                status = excluded.status,
                latency_ms = excluded.latency_ms,
                last_check_at = excluded.last_check_at,
                last_error = excluded.last_error,
                ip = excluded.ip,
                country_code = excluded.country_code,
                country_name = excluded.country_name,
                region_name = excluded.region_name,
                city_name = excluded.city_name,
                geo_checked_at = excluded.geo_checked_at,
                geo_error = excluded.geo_error,
                asn = excluded.asn,
                as_org = excluded.as_org,
                isp = excluded.isp,
                as_domain = excluded.as_domain,
                timezone_id = excluded.timezone_id,
                timezone_offset = excluded.timezone_offset,
                timezone_utc = excluded.timezone_utc,
                flag_img_url = excluded.flag_img_url,
                flag_emoji = excluded.flag_emoji,
                updated_at = excluded.updated_at",
            rusqlite::params![
                account_id,
                if enabled { 1 } else { 0 },
                normalize_optional_text(proxy_url),
                normalize_status(status),
                latency_ms,
                last_check_at,
                normalize_optional_text(last_error),
                normalize_optional_text(ip),
                normalize_country_code(country_code),
                normalize_optional_text(country_name),
                normalize_optional_text(region_name),
                normalize_optional_text(city_name),
                geo_checked_at,
                normalize_optional_text(geo_error),
                asn,
                normalize_optional_text(as_org),
                normalize_optional_text(isp),
                normalize_optional_text(as_domain),
                normalize_optional_text(timezone_id),
                timezone_offset,
                normalize_optional_text(timezone_utc),
                normalize_optional_text(flag_img_url),
                normalize_optional_text(flag_emoji),
                created_at,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn update_account_proxy_check_status(
        &self,
        account_id: &str,
        status: &str,
        latency_ms: Option<i64>,
        last_check_at: Option<i64>,
        last_error: Option<&str>,
        ip: Option<&str>,
        country_code: Option<&str>,
        country_name: Option<&str>,
        region_name: Option<&str>,
        city_name: Option<&str>,
        geo_checked_at: Option<i64>,
        geo_error: Option<&str>,
        asn: Option<i64>,
        as_org: Option<&str>,
        isp: Option<&str>,
        as_domain: Option<&str>,
        timezone_id: Option<&str>,
        timezone_offset: Option<i64>,
        timezone_utc: Option<&str>,
        flag_img_url: Option<&str>,
        flag_emoji: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE account_proxy_settings
             SET status = ?2,
                 latency_ms = ?3,
                 last_check_at = ?4,
                 last_error = ?5,
                 ip = ?6,
                 country_code = ?7,
                 country_name = ?8,
                 region_name = ?9,
                 city_name = ?10,
                 geo_checked_at = ?11,
                 geo_error = ?12,
                 asn = ?13,
                 as_org = ?14,
                 isp = ?15,
                 as_domain = ?16,
                 timezone_id = ?17,
                 timezone_offset = ?18,
                 timezone_utc = ?19,
                 flag_img_url = ?20,
                 flag_emoji = ?21,
                 updated_at = ?22
             WHERE account_id = ?1",
            rusqlite::params![
                account_id,
                normalize_status(status),
                latency_ms,
                last_check_at,
                normalize_optional_text(last_error),
                normalize_optional_text(ip),
                normalize_country_code(country_code),
                normalize_optional_text(country_name),
                normalize_optional_text(region_name),
                normalize_optional_text(city_name),
                geo_checked_at,
                normalize_optional_text(geo_error),
                asn,
                normalize_optional_text(as_org),
                normalize_optional_text(isp),
                normalize_optional_text(as_domain),
                normalize_optional_text(timezone_id),
                timezone_offset,
                normalize_optional_text(timezone_utc),
                normalize_optional_text(flag_img_url),
                normalize_optional_text(flag_emoji),
                now_ts(),
            ],
        )?;
        Ok(())
    }

    pub fn clear_account_proxy_settings(&self, account_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM account_proxy_settings WHERE account_id = ?1",
            [account_id],
        )?;
        Ok(())
    }

    pub fn find_account_proxy_settings(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountProxySettings>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                account_id,
                enabled,
                proxy_url,
                status,
                latency_ms,
                last_check_at,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                asn,
                as_org,
                isp,
                as_domain,
                timezone_id,
                timezone_offset,
                timezone_utc,
                flag_img_url,
                flag_emoji,
                created_at,
                updated_at
             FROM account_proxy_settings
             WHERE account_id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_account_proxy_settings_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn find_cached_proxy_flag_by_country(&self, country_code: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_img_url
             FROM account_proxy_settings
             WHERE country_code = ?1
               AND flag_img_url LIKE 'data:image/%'
             LIMIT 1",
        )?;
        let mut rows = stmt.query([country_code])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }

    pub fn list_account_proxy_settings(&self) -> Result<Vec<AccountProxySettings>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                account_id,
                enabled,
                proxy_url,
                status,
                latency_ms,
                last_check_at,
                last_error,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                geo_checked_at,
                geo_error,
                asn,
                as_org,
                isp,
                as_domain,
                timezone_id,
                timezone_offset,
                timezone_utc,
                flag_img_url,
                flag_emoji,
                created_at,
                updated_at
             FROM account_proxy_settings
             ORDER BY updated_at DESC, account_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_account_proxy_settings_row(row)?);
        }
        Ok(out)
    }

    pub(super) fn ensure_account_proxy_settings_table(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS account_proxy_settings (
                account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
                enabled INTEGER NOT NULL DEFAULT 0,
                proxy_url TEXT,
                status TEXT NOT NULL DEFAULT 'unchecked',
                latency_ms INTEGER,
                last_check_at INTEGER,
                last_error TEXT,
                ip TEXT,
                country_code TEXT,
                country_name TEXT,
                region_name TEXT,
                city_name TEXT,
                geo_checked_at INTEGER,
                geo_error TEXT,
                asn INTEGER,
                as_org TEXT,
                isp TEXT,
                as_domain TEXT,
                timezone_id TEXT,
                timezone_offset INTEGER,
                timezone_utc TEXT,
                flag_img_url TEXT,
                flag_emoji TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_account_proxy_settings_updated_at
                ON account_proxy_settings(updated_at DESC, account_id ASC);",
        )?;
        self.ensure_column(
            "account_proxy_settings",
            "enabled",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("account_proxy_settings", "proxy_url", "TEXT")?;
        self.ensure_column(
            "account_proxy_settings",
            "status",
            "TEXT NOT NULL DEFAULT 'unchecked'",
        )?;
        self.ensure_column("account_proxy_settings", "latency_ms", "INTEGER")?;
        self.ensure_column("account_proxy_settings", "last_check_at", "INTEGER")?;
        self.ensure_column("account_proxy_settings", "last_error", "TEXT")?;
        self.ensure_column("account_proxy_settings", "ip", "TEXT")?;
        self.ensure_column("account_proxy_settings", "country_code", "TEXT")?;
        self.ensure_column("account_proxy_settings", "country_name", "TEXT")?;
        self.ensure_column("account_proxy_settings", "region_name", "TEXT")?;
        self.ensure_column("account_proxy_settings", "city_name", "TEXT")?;
        self.ensure_column("account_proxy_settings", "geo_checked_at", "INTEGER")?;
        self.ensure_column("account_proxy_settings", "geo_error", "TEXT")?;
        self.ensure_column(
            "account_proxy_settings",
            "created_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "account_proxy_settings",
            "updated_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("account_proxy_settings", "asn", "INTEGER")?;
        self.ensure_column("account_proxy_settings", "as_org", "TEXT")?;
        self.ensure_column("account_proxy_settings", "isp", "TEXT")?;
        self.ensure_column("account_proxy_settings", "as_domain", "TEXT")?;
        self.ensure_column("account_proxy_settings", "timezone_id", "TEXT")?;
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN is_proxy",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN geo_provider",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN continent_code",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN continent_name",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN region_code",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN latitude",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN longitude",
                [],
            )
            .ok();
        self.conn
            .execute("ALTER TABLE account_proxy_settings DROP COLUMN is_eu", [])
            .ok();
        self.conn
            .execute("ALTER TABLE account_proxy_settings DROP COLUMN postal", [])
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN timezone_abbr",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN timezone_is_dst",
                [],
            )
            .ok();
        self.conn
            .execute(
                "ALTER TABLE account_proxy_settings DROP COLUMN flag_emoji_unicode",
                [],
            )
            .ok();
        self.ensure_column("account_proxy_settings", "timezone_offset", "INTEGER")?;
        self.ensure_column("account_proxy_settings", "timezone_utc", "TEXT")?;
        self.ensure_column("account_proxy_settings", "flag_img_url", "TEXT")?;
        self.ensure_column("account_proxy_settings", "flag_emoji", "TEXT")?;
        Ok(())
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn normalize_country_code(value: Option<&str>) -> Option<String> {
    normalize_optional_text(value).map(|text| text.to_ascii_uppercase())
}

fn normalize_status(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unchecked".to_string()
    } else {
        trimmed.to_string()
    }
}

fn map_account_proxy_settings_row(row: &Row<'_>) -> Result<AccountProxySettings> {
    Ok(AccountProxySettings {
        account_id: row.get(0)?,
        enabled: row.get::<_, i64>(1)? != 0,
        proxy_url: row.get(2)?,
        status: row.get(3)?,
        latency_ms: row.get(4)?,
        last_check_at: row.get(5)?,
        last_error: row.get(6)?,
        ip: row.get(7)?,
        country_code: row.get(8)?,
        country_name: row.get(9)?,
        region_name: row.get(10)?,
        city_name: row.get(11)?,
        geo_checked_at: row.get(12)?,
        geo_error: row.get(13)?,
        asn: row.get(14)?,
        as_org: row.get(15)?,
        isp: row.get(16)?,
        as_domain: row.get(17)?,
        timezone_id: row.get(18)?,
        timezone_offset: row.get(19)?,
        timezone_utc: row.get(20)?,
        flag_img_url: row.get(21)?,
        flag_emoji: row.get(22)?,
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
    })
}
