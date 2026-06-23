use rusqlite::{params, Result, Row};
use url::{Host, Url};

use super::{
    now_ts, ProxyProfile, ProxyProfileCreateInput, ProxyProfileUpdateInput,
    ProxyProfileUrlMetadata, Storage,
};

impl Storage {
    pub fn create_proxy_profile(&self, input: &ProxyProfileCreateInput) -> Result<ProxyProfile> {
        let now = now_ts();
        let proxy_url = input.proxy_url.trim().to_string();
        let metadata = derive_proxy_profile_url_metadata(&proxy_url);
        let name = normalize_required_text(&input.name);
        let tags_json = normalize_optional_text(input.tags_json.as_deref());
        let notes = normalize_optional_text(input.notes.as_deref());

        self.conn.execute(
            "INSERT INTO proxy_profiles (
                id,
                name,
                proxy_url,
                proxy_url_redacted,
                scheme,
                host,
                port,
                enabled,
                status,
                last_error,
                last_url_latency_ms,
                last_download_mbps,
                last_upload_mbps,
                last_tested_at,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                asn,
                as_org,
                flag_img_url,
                flag_emoji,
                timezone_id,
                timezone_offset,
                timezone_utc,
                tags_json,
                notes,
                created_at,
                updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'unchecked', NULL,
                NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, NULL, ?9, ?10, ?11, ?12
            )",
            params![
                input.id.trim(),
                name,
                proxy_url,
                metadata.proxy_url_redacted,
                metadata.scheme,
                metadata.host,
                metadata.port,
                if input.enabled { 1 } else { 0 },
                tags_json,
                notes,
                now,
                now,
            ],
        )?;

        self.find_proxy_profile(input.id.trim())?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn update_proxy_profile(
        &self,
        input: &ProxyProfileUpdateInput,
    ) -> Result<Option<ProxyProfile>> {
        let Some(existing) = self.find_proxy_profile(input.id.trim())? else {
            return Ok(None);
        };

        let proxy_url = input
            .proxy_url
            .as_deref()
            .map(str::trim)
            .map(ToString::to_string)
            .unwrap_or_else(|| existing.proxy_url.clone());
        let metadata = if input.proxy_url.is_some() {
            derive_proxy_profile_url_metadata(&proxy_url)
        } else {
            ProxyProfileUrlMetadata {
                proxy_url_redacted: existing.proxy_url_redacted.clone(),
                scheme: existing.scheme.clone(),
                host: existing.host.clone(),
                port: existing.port,
            }
        };
        let now = now_ts();

        let name = input
            .name
            .as_deref()
            .map(normalize_required_text)
            .unwrap_or(existing.name);
        let enabled = input.enabled.unwrap_or(existing.enabled);
        let status = input
            .status
            .as_deref()
            .map(normalize_status)
            .unwrap_or(existing.status);
        let last_error = input
            .last_error
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.last_error);
        let ip = input
            .ip
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.ip);
        let country_code = input
            .country_code
            .as_deref()
            .map(normalize_country_code_value)
            .unwrap_or(existing.country_code);
        let country_name = input
            .country_name
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.country_name);
        let region_name = input
            .region_name
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.region_name);
        let city_name = input
            .city_name
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.city_name);
        let as_org = input
            .as_org
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.as_org);
        let isp = input
            .isp
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.isp);
        let as_domain = input
            .as_domain
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.as_domain);
        let flag_img_url = input
            .flag_img_url
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.flag_img_url);
        let flag_emoji = input
            .flag_emoji
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.flag_emoji);
        let timezone_id = input
            .timezone_id
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.timezone_id);
        let timezone_offset = input.timezone_offset.or(existing.timezone_offset);
        let timezone_utc = input
            .timezone_utc
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.timezone_utc);
        let tags_json = input
            .tags_json
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.tags_json);
        let notes = input
            .notes
            .as_deref()
            .map(normalize_optional_text_value)
            .unwrap_or(existing.notes);

        self.conn.execute(
            "UPDATE proxy_profiles
             SET name = ?2,
                 proxy_url = ?3,
                 proxy_url_redacted = ?4,
                 scheme = ?5,
                 host = ?6,
                 port = ?7,
                 enabled = ?8,
                 status = ?9,
                 last_error = ?10,
                 last_url_latency_ms = ?11,
                 last_download_mbps = ?12,
                 last_upload_mbps = ?13,
                 last_tested_at = ?14,
                 ip = ?15,
                 country_code = ?16,
                 country_name = ?17,
                 region_name = ?18,
                 city_name = ?19,
                 asn = ?20,
                 as_org = ?21,
                 isp = ?30,
                 as_domain = ?31,
                 flag_img_url = ?22,
                 flag_emoji = ?23,
                 timezone_id = ?24,
                 timezone_offset = ?25,
                 timezone_utc = ?26,
                 tags_json = ?27,
                 notes = ?28,
                 updated_at = ?29
             WHERE id = ?1",
            params![
                input.id.trim(),
                name,
                proxy_url,
                metadata.proxy_url_redacted,
                metadata.scheme,
                metadata.host,
                metadata.port,
                if enabled { 1 } else { 0 },
                status,
                last_error,
                input.last_url_latency_ms.or(existing.last_url_latency_ms),
                input.last_download_mbps.or(existing.last_download_mbps),
                input.last_upload_mbps.or(existing.last_upload_mbps),
                input.last_tested_at.or(existing.last_tested_at),
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                input.asn.or(existing.asn),
                as_org,
                flag_img_url,
                flag_emoji,
                timezone_id,
                timezone_offset,
                timezone_utc,
                tags_json,
                notes,
                now,
                isp,
                as_domain,
            ],
        )?;

        self.find_proxy_profile(input.id.trim())
    }

    pub fn delete_proxy_profile(&self, id: &str) -> Result<bool> {
        let normalized_id = id.trim();
        let deleted = self
            .conn
            .execute("DELETE FROM proxy_profiles WHERE id = ?1", [normalized_id])?;
        if deleted > 0 {
            let _ = self.delete_proxy_profile_url_tests_by_profile(normalized_id);
            let _ = self.delete_proxy_speed_tests_by_profile(normalized_id);
            let _ = self.delete_proxy_diagnostic_tests_by_profile(normalized_id);
        }
        Ok(deleted > 0)
    }

    pub fn find_proxy_profile(&self, id: &str) -> Result<Option<ProxyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                name,
                proxy_url,
                proxy_url_redacted,
                scheme,
                host,
                port,
                enabled,
                status,
                last_error,
                last_url_latency_ms,
                last_download_mbps,
                last_upload_mbps,
                last_tested_at,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                asn,
                as_org,
                flag_img_url,
                flag_emoji,
                timezone_id,
                timezone_offset,
                timezone_utc,
                tags_json,
                notes,
                created_at,
                updated_at,
                isp,
                as_domain
             FROM proxy_profiles
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([id.trim()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_proxy_profile_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_proxy_profiles(&self) -> Result<Vec<ProxyProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id,
                name,
                proxy_url,
                proxy_url_redacted,
                scheme,
                host,
                port,
                enabled,
                status,
                last_error,
                last_url_latency_ms,
                last_download_mbps,
                last_upload_mbps,
                last_tested_at,
                ip,
                country_code,
                country_name,
                region_name,
                city_name,
                asn,
                as_org,
                flag_img_url,
                flag_emoji,
                timezone_id,
                timezone_offset,
                timezone_utc,
                tags_json,
                notes,
                created_at,
                updated_at,
                isp,
                as_domain
             FROM proxy_profiles
             ORDER BY updated_at DESC, name COLLATE NOCASE ASC, id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut profiles = Vec::new();
        while let Some(row) = rows.next()? {
            profiles.push(map_proxy_profile_row(row)?);
        }
        Ok(profiles)
    }

    pub(super) fn ensure_proxy_profiles_table(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS proxy_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                proxy_url TEXT NOT NULL,
                proxy_url_redacted TEXT NOT NULL,
                scheme TEXT,
                host TEXT,
                port INTEGER,
                enabled INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'unchecked',
                last_error TEXT,
                last_url_latency_ms INTEGER,
                last_download_mbps REAL,
                last_upload_mbps REAL,
                last_tested_at INTEGER,
                ip TEXT,
                country_code TEXT,
                country_name TEXT,
                region_name TEXT,
                city_name TEXT,
                asn INTEGER,
                as_org TEXT,
                isp TEXT,
                as_domain TEXT,
                flag_img_url TEXT,
                flag_emoji TEXT,
                timezone_id TEXT,
                timezone_offset INTEGER,
                timezone_utc TEXT,
                tags_json TEXT,
                notes TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_proxy_profiles_status
                ON proxy_profiles(status);
            CREATE INDEX IF NOT EXISTS idx_proxy_profiles_enabled
                ON proxy_profiles(enabled);",
        )?;
        self.ensure_column("proxy_profiles", "name", "TEXT NOT NULL DEFAULT ''")?;
        self.ensure_column("proxy_profiles", "proxy_url", "TEXT NOT NULL DEFAULT ''")?;
        self.ensure_column(
            "proxy_profiles",
            "proxy_url_redacted",
            "TEXT NOT NULL DEFAULT '<invalid>'",
        )?;
        self.ensure_column("proxy_profiles", "scheme", "TEXT")?;
        self.ensure_column("proxy_profiles", "host", "TEXT")?;
        self.ensure_column("proxy_profiles", "port", "INTEGER")?;
        self.ensure_column("proxy_profiles", "enabled", "INTEGER NOT NULL DEFAULT 1")?;
        self.ensure_column(
            "proxy_profiles",
            "status",
            "TEXT NOT NULL DEFAULT 'unchecked'",
        )?;
        self.ensure_column("proxy_profiles", "last_error", "TEXT")?;
        self.ensure_column("proxy_profiles", "last_url_latency_ms", "INTEGER")?;
        self.ensure_column("proxy_profiles", "last_download_mbps", "REAL")?;
        self.ensure_column("proxy_profiles", "last_upload_mbps", "REAL")?;
        self.ensure_column("proxy_profiles", "last_tested_at", "INTEGER")?;
        self.ensure_column("proxy_profiles", "ip", "TEXT")?;
        self.ensure_column("proxy_profiles", "country_code", "TEXT")?;
        self.ensure_column("proxy_profiles", "country_name", "TEXT")?;
        self.ensure_column("proxy_profiles", "region_name", "TEXT")?;
        self.ensure_column("proxy_profiles", "city_name", "TEXT")?;
        self.ensure_column("proxy_profiles", "asn", "INTEGER")?;
        self.ensure_column("proxy_profiles", "as_org", "TEXT")?;
        self.ensure_column("proxy_profiles", "isp", "TEXT")?;
        self.ensure_column("proxy_profiles", "as_domain", "TEXT")?;
        self.ensure_column("proxy_profiles", "flag_img_url", "TEXT")?;
        self.ensure_column("proxy_profiles", "flag_emoji", "TEXT")?;
        self.ensure_column("proxy_profiles", "timezone_id", "TEXT")?;
        self.ensure_column("proxy_profiles", "timezone_offset", "INTEGER")?;
        self.ensure_column("proxy_profiles", "timezone_utc", "TEXT")?;
        self.ensure_column("proxy_profiles", "tags_json", "TEXT")?;
        self.ensure_column("proxy_profiles", "notes", "TEXT")?;
        self.ensure_column("proxy_profiles", "created_at", "INTEGER NOT NULL DEFAULT 0")?;
        self.ensure_column("proxy_profiles", "updated_at", "INTEGER NOT NULL DEFAULT 0")?;
        Ok(())
    }
}

pub fn derive_proxy_profile_url_metadata(proxy_url: &str) -> ProxyProfileUrlMetadata {
    let raw = proxy_url.trim();
    let Ok(parsed) = Url::parse(raw) else {
        return invalid_proxy_profile_url_metadata();
    };

    let Some(host) = parsed.host() else {
        return invalid_proxy_profile_url_metadata();
    };

    let scheme = parsed.scheme().to_string();
    let host_for_url = host_to_redacted_url_text(&host);
    let host = Some(host_to_metadata_text(&host));
    let port = parsed.port_or_known_default().map(i64::from);
    let port_suffix = port.map(|value| format!(":{value}")).unwrap_or_default();

    ProxyProfileUrlMetadata {
        proxy_url_redacted: format!("{scheme}://{host_for_url}{port_suffix}"),
        scheme: Some(scheme),
        host,
        port,
    }
}

fn invalid_proxy_profile_url_metadata() -> ProxyProfileUrlMetadata {
    ProxyProfileUrlMetadata {
        proxy_url_redacted: "<invalid>".to_string(),
        scheme: None,
        host: None,
        port: None,
    }
}

fn host_to_redacted_url_text(host: &Host<&str>) -> String {
    match host {
        Host::Domain(value) => value.to_string(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => format!("[{value}]"),
    }
}

fn host_to_metadata_text(host: &Host<&str>) -> String {
    match host {
        Host::Domain(value) => value.to_string(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    }
}

fn normalize_required_text(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.map(normalize_optional_text_value).unwrap_or(None)
}

fn normalize_optional_text_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_country_code_value(value: &str) -> Option<String> {
    normalize_optional_text_value(value).map(|text| text.to_ascii_uppercase())
}

fn normalize_status(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unchecked".to_string()
    } else {
        trimmed.to_string()
    }
}

fn map_proxy_profile_row(row: &Row<'_>) -> Result<ProxyProfile> {
    Ok(ProxyProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        proxy_url: row.get(2)?,
        proxy_url_redacted: row.get(3)?,
        scheme: row.get(4)?,
        host: row.get(5)?,
        port: row.get(6)?,
        enabled: row.get::<_, i64>(7)? != 0,
        status: row.get(8)?,
        last_error: row.get(9)?,
        last_url_latency_ms: row.get(10)?,
        last_download_mbps: row.get(11)?,
        last_upload_mbps: row.get(12)?,
        last_tested_at: row.get(13)?,
        ip: row.get(14)?,
        country_code: row.get(15)?,
        country_name: row.get(16)?,
        region_name: row.get(17)?,
        city_name: row.get(18)?,
        asn: row.get(19)?,
        as_org: row.get(20)?,
        flag_img_url: row.get(21)?,
        flag_emoji: row.get(22)?,
        timezone_id: row.get(23)?,
        timezone_offset: row.get(24)?,
        timezone_utc: row.get(25)?,
        tags_json: row.get(26)?,
        notes: row.get(27)?,
        created_at: row.get(28)?,
        updated_at: row.get(29)?,
        isp: row.get(30)?,
        as_domain: row.get(31)?,
    })
}
