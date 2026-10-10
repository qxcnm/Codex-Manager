//! Offline, versioned SQLite snapshots and transactional imports.
//!
//! Archives contain secrets, just as the source database does. Diagnostics
//! deliberately contain only table/column names and counts. Import never
//! overwrites data, silently drops a populated table, or changes live settings.

use sea_orm::sea_query::{Alias, ColumnType, Query};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait, Iterable,
    Statement, TransactionTrait, Value,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
#[cfg(feature = "sqlite")]
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
#[cfg(feature = "sqlite")]
use std::io::{BufWriter, Write};
use std::path::Path;

mod preflight;
pub use preflight::{
    dry_run, read_only_target_config, TransferFailure, TransferReport, TransferTableReport,
};
#[cfg(all(test, feature = "sqlite"))]
mod preflight_tests;

const VERSION: u32 = 1;
const PAGE_SIZE: u64 = 200;

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("snapshot I/O failed")]
    Io(#[from] std::io::Error),
    #[error("invalid snapshot encoding")]
    Json(#[from] serde_json::Error),
    #[error("snapshot database operation failed")]
    Database(#[from] DbErr),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Entry {
    Header { version: u32 },
    Table { name: String, columns: Vec<String> },
    Row { cells: Vec<Cell> },
    EndTable { count: u64, sha256: String },
    End { tables: usize },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableVerification {
    pub table: String,
    pub rows: u64,
    pub sha256: String,
}

#[derive(Clone)]
struct TableSpec {
    name: String,
    columns: BTreeMap<String, ColumnType>,
    keys: Vec<String>,
    nullable: BTreeSet<String>,
    defaulted: BTreeSet<String>,
    unique: Vec<Vec<String>>,
}

fn spec<E: EntityTrait>(entity: E) -> TableSpec {
    use sea_orm::{IdenStatic, PrimaryKeyToColumn, PrimaryKeyTrait};
    let keys: Vec<String> = E::PrimaryKey::iter()
        .map(|k| k.into_column().as_str().into())
        .collect();
    let mut unique = vec![keys.clone()];
    unique.extend(
        E::Column::iter()
            .filter(|c| c.def().is_unique())
            .map(|c| vec![c.as_str().into()]),
    );
    // These uniqueness constraints are created by migration.rs.
    match entity.table_name() {
        "app_wallets" => unique.push(vec!["owner_kind".into(), "owner_id".into()]),
        "app_wallet_ledger_entries" => {
            unique.push(vec!["request_log_id".into(), "entry_kind".into()])
        }
        _ => {}
    }
    TableSpec {
        name: entity.table_name().into(),
        columns: E::Column::iter()
            .map(|c| (c.as_str().into(), c.def().get_column_type().clone()))
            .collect(),
        nullable: E::Column::iter()
            .filter(|c| c.def().is_null())
            .map(|c| c.as_str().into())
            .collect(),
        defaulted: E::Column::iter()
            .filter(|c| {
                c.def().get_column_default().is_some()
                    || (E::PrimaryKey::auto_increment() && keys.iter().any(|k| k == c.as_str()))
            })
            .map(|c| c.as_str().into())
            .collect(),
        keys,
        unique,
    }
}

fn specs() -> Vec<TableSpec> {
    vec![
        spec(crate::desktop_history::api_key_profiles::Entity),
        spec(crate::desktop_history::schema_migrations::Entity),
        spec(crate::desktop_history::conversation_bindings::Entity),
        spec(crate::desktop_history::model_catalog_scopes::Entity),
        spec(crate::desktop_history::model_catalog_models::Entity),
        spec(crate::desktop_history::model_catalog_reasoning_levels::Entity),
        spec(crate::desktop_history::model_catalog_string_items::Entity),
        spec(crate::desktop_history::model_price_rules::Entity),
        spec(crate::desktop_history::quota_source_model_assignments::Entity),
        spec(crate::desktop_history::account_quota_capacity_templates::Entity),
        spec(crate::desktop_history::app_projects::Entity),
        spec(crate::desktop_history::app_project_members::Entity),
        spec(crate::desktop_history::redeem_code_batches::Entity),
        spec(crate::desktop_history::redeem_codes::Entity),
        spec(crate::desktop_history::redeem_records::Entity),
        spec(crate::desktop_history::model_source_models::Entity),
        spec(crate::desktop_history::model_source_mappings::Entity),
        spec(crate::desktop_history::model_source_mapping_preferences::Entity),
        spec(crate::desktop_history::model_catalog_v2_meta::Entity),
        spec(crate::desktop_history::codex_skill_repositories::Entity),
        spec(crate::desktop_history::codex_skill_repository_skills::Entity),
        spec(crate::proxy_history::proxy_profile_url_test::Entity),
        spec(crate::proxy_history::proxy_speed_test::Entity),
        spec(crate::proxy_history::proxy_diagnostic_test::Entity),
        spec(crate::proxy_history::account_proxy_url_test::Entity),
        spec(crate::plugins::installs::Entity),
        spec(crate::plugins::tasks::Entity),
        spec(crate::plugins::logs::Entity),
        spec(crate::api_key_details::secrets::Entity),
        spec(crate::api_key_details::quotas::Entity),
        spec(crate::api_key_rollups::hourly::Entity),
        spec(crate::api_key_rollups::legacy::Entity),
        spec(crate::aggregate_apis::providers::Entity),
        spec(crate::aggregate_apis::suppliers::Entity),
        spec(crate::aggregate_apis::secrets::Entity),
        spec(crate::aggregate_apis::balance_secrets::Entity),
        spec(crate::settings::Entity),
        spec(crate::accounts::Entity),
        spec(crate::account_tokens::Entity),
        spec(crate::api_keys::Entity),
        spec(crate::request_logs::Entity),
        spec(crate::request_token_stats::Entity),
        spec(crate::usage_snapshots::Entity),
        spec(crate::model_catalog::models::Entity),
        spec(crate::model_catalog::prices::Entity),
        spec(crate::model_catalog::price_tiers::Entity),
        spec(crate::model_catalog::routes::Entity),
        spec(crate::model_groups::groups::Entity),
        spec(crate::model_groups::group_models::Entity),
        spec(crate::model_groups::users::Entity),
        spec(crate::users::users::Entity),
        spec(crate::users::sessions::Entity),
        spec(crate::users::owners::Entity),
        spec(crate::users::rules::Entity),
        spec(crate::billing::wallets::Entity),
        spec(crate::billing::snapshots::Entity),
        spec(crate::billing::ledger::Entity),
        spec(crate::model_groups::group_models_v2::Entity),
        spec(crate::account_details::metadata::Entity),
        spec(crate::account_details::subscriptions::Entity),
        spec(crate::account_details::proxy_profiles::Entity),
        spec(crate::account_details::proxy_settings::Entity),
        spec(crate::account_details::quota_overrides::Entity),
        spec(crate::account_details::agent_identities::Entity),
        spec(crate::account_details::login_sessions::Entity),
        spec(crate::account_details::events::Entity),
        spec(crate::account_details::warmups::Entity),
    ]
}

#[cfg(feature = "sqlite")]
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(feature = "sqlite")]
fn write_entry(writer: &mut impl Write, entry: &Entry) -> Result<(), TransferError> {
    serde_json::to_writer(&mut *writer, entry)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn row_hash(cells: &[Cell]) -> Result<[u8; 32], TransferError> {
    Ok(Sha256::digest(serde_json::to_vec(cells)?).into())
}

// A commutative sum of row hashes allows verification across DB collations
// without holding a large table in memory. The count is checked separately.
fn add_hash(sum: &mut [u8; 32], row: [u8; 32]) {
    let mut carry = 0u16;
    for i in (0..32).rev() {
        carry += sum[i] as u16 + row[i] as u16;
        sum[i] = carry as u8;
        carry >>= 8;
    }
}
fn hash_text(sum: &[u8; 32]) -> String {
    sum.iter().map(|v| format!("{v:02x}")).collect()
}

/// Export every user table, including tables not yet supported by Service.
/// A read-only connection and one read transaction provide a consistent view
/// including committed WAL rows. Existing output files are never overwritten.
#[cfg(feature = "sqlite")]
pub async fn export_sqlite(
    source: &Path,
    output: &Path,
) -> Result<Vec<TableVerification>, TransferError> {
    let absolute = source.canonicalize()?;
    let file_url = url::Url::from_file_path(&absolute)
        .map_err(|_| TransferError::Invalid("invalid source path".into()))?;
    let source_url = format!("sqlite://{}?mode=ro", file_url.path());
    let mut options = sea_orm::ConnectOptions::new(source_url);
    options.max_connections(1).sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await?;
    let tx = db.begin().await?;
    let tables = tx.query_all(Statement::from_string(DbBackend::Sqlite,
        // Request payloads, upstream attempts and response-ID links are local
        // diagnostics only; the remote backend has no matching schema.
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name <> 'app_domain_locks' AND name NOT LIKE 'request_log_payload%' AND name <> 'request_log_response_links' AND name <> 'request_log_upstream_attempts' ORDER BY name".to_owned())).await?;
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?,
    );
    write_entry(&mut writer, &Entry::Header { version: VERSION })?;
    let mut report = Vec::new();
    for table in tables {
        let name: String = table.try_get("", "name")?;
        let columns = tx
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                format!("PRAGMA table_info({})", quote(&name)),
            ))
            .await?
            .into_iter()
            .map(|r| r.try_get::<String>("", "name"))
            .collect::<Result<Vec<_>, _>>()?;
        write_entry(
            &mut writer,
            &Entry::Table {
                name: name.clone(),
                columns: columns.clone(),
            },
        )?;
        let select = columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} AS c{i}, typeof({}) AS t{i}", quote(c), quote(c)))
            .collect::<Vec<_>>()
            .join(",");
        let mut count = 0;
        let mut sum = [0; 32];
        loop {
            let rows = tx
                .query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT {select} FROM {} LIMIT {PAGE_SIZE} OFFSET {count}",
                        quote(&name)
                    ),
                ))
                .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let mut cells = Vec::with_capacity(columns.len());
                for i in 0..columns.len() {
                    let c = format!("c{i}");
                    cells.push(
                        match row.try_get::<String>("", &format!("t{i}"))?.as_str() {
                            "null" => Cell::Null,
                            "integer" => Cell::Integer(row.try_get("", &c)?),
                            "real" => Cell::Real(row.try_get("", &c)?),
                            "text" => Cell::Text(row.try_get("", &c)?),
                            "blob" => Cell::Blob(row.try_get("", &c)?),
                            _ => {
                                return Err(TransferError::Invalid(
                                    "unsupported SQLite value type".into(),
                                ))
                            }
                        },
                    );
                }
                add_hash(&mut sum, row_hash(&cells)?);
                write_entry(&mut writer, &Entry::Row { cells })?;
                count += 1;
            }
        }
        let sha256 = hash_text(&sum);
        write_entry(
            &mut writer,
            &Entry::EndTable {
                count,
                sha256: sha256.clone(),
            },
        )?;
        report.push(TableVerification {
            table: name,
            rows: count,
            sha256,
        });
    }
    write_entry(
        &mut writer,
        &Entry::End {
            tables: report.len(),
        },
    )?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    tx.commit().await?;
    db.close().await?;
    Ok(report)
}

/// Validate the complete archive before starting a target transaction.
pub fn inspect_snapshot(path: &Path) -> Result<Vec<TableVerification>, TransferError> {
    let mut report = Vec::new();
    let mut active: Option<(String, usize, u64, [u8; 32])> = None;
    let mut header = false;
    let mut ended = false;
    let mut names = BTreeSet::new();
    for line in BufReader::new(File::open(path)?).lines() {
        if ended {
            return Err(TransferError::Invalid("trailing snapshot data".into()));
        }
        let entry: Entry = serde_json::from_str(&line?)?;
        match entry {
            Entry::Header { version } if !header && version == VERSION => {
                header = true;
            }
            Entry::Table { name, columns } if header && active.is_none() => {
                if columns.is_empty()
                    || columns.iter().collect::<BTreeSet<_>>().len() != columns.len()
                    || !names.insert(name.clone())
                {
                    return Err(TransferError::Invalid("duplicate table or column".into()));
                }
                active = Some((name, columns.len(), 0, [0; 32]));
            }
            Entry::Row { cells } => {
                let Some((_, width, count, sum)) = active.as_mut() else {
                    return Err(TransferError::Invalid("row outside table".into()));
                };
                if cells.len() != *width {
                    return Err(TransferError::Invalid("snapshot row width mismatch".into()));
                }
                add_hash(sum, row_hash(&cells)?);
                *count += 1;
            }
            Entry::EndTable { count, sha256 } => {
                let Some((table, _, actual, sum)) = active.take() else {
                    return Err(TransferError::Invalid("table trailer without table".into()));
                };
                if count != actual || sha256 != hash_text(&sum) {
                    return Err(TransferError::Invalid(format!(
                        "snapshot checksum mismatch: {table}"
                    )));
                }
                report.push(TableVerification {
                    table,
                    rows: count,
                    sha256,
                });
            }
            Entry::End { tables } if header && active.is_none() && tables == report.len() => {
                ended = true;
            }
            _ => {
                return Err(TransferError::Invalid(
                    "unsupported version or invalid snapshot order".into(),
                ))
            }
        }
    }
    if !ended {
        return Err(TransferError::Invalid("incomplete snapshot".into()));
    }
    Ok(report)
}

fn cell_value(cell: &Cell, column_type: &ColumnType) -> Result<Value, TransferError> {
    if matches!(column_type, ColumnType::Boolean) {
        return match cell {
            Cell::Integer(0) => Ok(false.into()),
            Cell::Integer(1) => Ok(true.into()),
            Cell::Null => Ok(Value::Bool(None)),
            _ => Err(TransferError::Invalid("invalid boolean in snapshot".into())),
        };
    }
    Ok(match cell {
        Cell::Null => match column_type {
            ColumnType::BigInteger | ColumnType::Integer => Value::BigInt(None),
            ColumnType::Double | ColumnType::Float => Value::Double(None),
            _ => Value::String(None),
        },
        Cell::Integer(v) if matches!(column_type, ColumnType::Double | ColumnType::Float) => {
            (*v as f64).into()
        }
        Cell::Integer(v) => (*v).into(),
        Cell::Real(v) => (*v).into(),
        Cell::Text(v) => v.clone().into(),
        Cell::Blob(v) => v.clone().into(),
    })
}

/// Import to an already migrated, empty target. Unknown populated tables or
/// columns stop the whole import; there is no unsafe partial-success mode.
pub async fn import_snapshot(
    path: &Path,
    db: &DatabaseConnection,
) -> Result<Vec<TableVerification>, TransferError> {
    let report = inspect_snapshot(path)?;
    let supported = specs();
    let unsupported: Vec<_> = report
        .iter()
        .filter(|r| r.rows > 0 && !supported.iter().any(|s| s.name == r.table))
        .map(|r| r.table.clone())
        .collect();
    if !unsupported.is_empty() {
        return Err(TransferError::Invalid(format!(
            "import blocked by unmigrated populated tables: {}",
            unsupported.join(", ")
        )));
    }
    let backend = db.get_database_backend();
    let tx = db.begin().await?;
    for table in &supported {
        let query = Query::select()
            .expr(sea_orm::sea_query::Expr::cust("COUNT(*) AS row_count"))
            .from(Alias::new(&table.name))
            .to_owned();
        let row = tx
            .query_one(backend.build(&query))
            .await?
            .ok_or_else(|| TransferError::Invalid("missing target count".into()))?;
        if row.try_get::<i64>("", "row_count")? > 0 {
            return Err(TransferError::Invalid(format!(
                "target table is not empty: {}",
                table.name
            )));
        }
    }
    // Replay in FK order, bounded to one row in memory. Prevalidation above
    // ensures malformed/truncated archives cannot leave partially imported data.
    for table in &supported {
        let mut columns = None;
        for line in BufReader::new(File::open(path)?).lines() {
            match serde_json::from_str::<Entry>(&line?)? {
                Entry::Table {
                    name,
                    columns: cols,
                } if name == table.name => {
                    let extra: Vec<_> = cols
                        .iter()
                        .filter(|c| !table.columns.contains_key(*c))
                        .cloned()
                        .collect();
                    if !extra.is_empty() && report.iter().any(|r| r.table == name && r.rows > 0) {
                        return Err(TransferError::Invalid(format!(
                            "unmapped columns in {name}: {}",
                            extra.join(", ")
                        )));
                    }
                    columns = Some(cols);
                }
                Entry::Table { .. } | Entry::EndTable { .. } => columns = None,
                Entry::Row { cells } if columns.is_some() => {
                    let cols = columns.as_ref().unwrap();
                    let (insert_columns, insert_cells) =
                        complete_legacy_row(&tx, table, cols, &cells).await?;
                    let values = insert_columns
                        .iter()
                        .zip(&insert_cells)
                        .map(|(c, v)| cell_value(v, &table.columns[c]))
                        .collect::<Result<Vec<_>, _>>()?;
                    let insert = Query::insert()
                        .into_table(Alias::new(&table.name))
                        .columns(insert_columns.iter().map(Alias::new))
                        .values_panic(values.into_iter().map(Into::into))
                        .to_owned();
                    tx.execute(backend.build(&insert)).await?;
                }
                _ => {}
            }
        }
    }
    // Compare every value, including nulls and large integer amounts, while
    // still inside the transaction. A mismatch rolls back all tables.
    for expected in &report {
        let Some(table) = supported.iter().find(|s| s.name == expected.table) else {
            continue;
        };
        if expected.rows == 0 {
            continue;
        }
        let columns = snapshot_columns(path, &table.name)?;
        let mut count = 0;
        let mut sum = [0; 32];
        loop {
            let mut select = Query::select();
            select
                .columns(columns.iter().map(Alias::new))
                .from(Alias::new(&table.name))
                .limit(PAGE_SIZE)
                .offset(count);
            for key in &table.keys {
                select.order_by(Alias::new(key), sea_orm::sea_query::Order::Asc);
            }
            let rows = tx.query_all(backend.build(&select)).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let cells = columns
                    .iter()
                    .map(|c| read_cell(&row, c, &table.columns[c]))
                    .collect::<Result<Vec<_>, _>>()?;
                add_hash(&mut sum, row_hash(&cells)?);
                count += 1;
            }
        }
        if count != expected.rows || hash_text(&sum) != expected.sha256 {
            return Err(TransferError::Invalid(format!(
                "target verification mismatch: {}",
                table.name
            )));
        }
    }
    // Imported explicit IDs must advance PostgreSQL's generated sequence.
    if backend == DbBackend::Postgres {
        for table in [
            "usage_snapshots",
            "events",
            "proxy_profile_url_tests",
            "proxy_speed_tests",
            "proxy_diagnostics_history",
            "account_proxy_url_tests",
            "plugin_run_logs",
        ] {
            tx.execute(Statement::from_string(backend,format!("SELECT setval(pg_get_serial_sequence('{table}','id'), COALESCE(MAX(id),1), COUNT(*)>0) FROM {table}"))).await?;
        }
    }
    tx.commit().await?;
    Ok(report)
}

// Desktop stores API profiles separately and does not store adapter comparison
// hashes. Keep the original tables/values for checksum verification, while
// deriving the additional fields required by the authoritative repositories.
async fn complete_legacy_row(
    db: &impl ConnectionTrait,
    table: &TableSpec,
    columns: &[String],
    cells: &[Cell],
) -> Result<(Vec<String>, Vec<Cell>), TransferError> {
    let mut cols = columns.to_vec();
    let mut values = cells.to_vec();
    let text = |name: &str| -> Result<String, TransferError> {
        columns
            .iter()
            .position(|c| c == name)
            .and_then(|i| cells.get(i))
            .and_then(|v| {
                if let Cell::Text(v) = v {
                    Some(v.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                TransferError::Invalid(format!("missing text field {}.{name}", table.name))
            })
    };
    let mut add = |name: &str, value: Cell| {
        if !columns.iter().any(|c| c == name) {
            cols.push(name.into());
            values.push(value);
        }
    };
    match table.name.as_str() {
        "models" => add(
            "slug_key",
            Cell::Text(crate::model_catalog::comparison_key(&[&text("slug")?
                .trim()
                .to_ascii_lowercase()])),
        ),
        "model_routes" => add(
            "route_key",
            Cell::Text(crate::model_catalog::comparison_key(&[
                &text("model_id")?,
                &text("source_kind")?,
                &text("source_id")?,
                &text("upstream_model")?,
            ])),
        ),
        "model_price_tiers" => {
            add("created_at", Cell::Integer(0));
            add("updated_at", Cell::Integer(0));
        }
        "api_keys" => {
            let profile = crate::desktop_history::api_key_profiles::Entity::find_by_id(text("id")?)
                .one(db)
                .await?;
            add(
                "client_type",
                Cell::Text(
                    profile
                        .as_ref()
                        .map(|p| p.client_type.clone())
                        .unwrap_or_else(|| "codex".into()),
                ),
            );
            add(
                "protocol_type",
                Cell::Text(
                    profile
                        .as_ref()
                        .map(|p| p.protocol_type.clone())
                        .unwrap_or_else(|| "openai_compat".into()),
                ),
            );
            add(
                "auth_scheme",
                Cell::Text(
                    profile
                        .as_ref()
                        .map(|p| p.auth_scheme.clone())
                        .unwrap_or_else(|| "authorization_bearer".into()),
                ),
            );
            add(
                "upstream_base_url",
                profile
                    .as_ref()
                    .and_then(|p| p.upstream_base_url.clone())
                    .map(Cell::Text)
                    .unwrap_or(Cell::Null),
            );
            add(
                "static_headers_json",
                profile
                    .as_ref()
                    .and_then(|p| p.static_headers_json.clone())
                    .map(Cell::Text)
                    .unwrap_or(Cell::Null),
            );
            add(
                "service_tier",
                profile
                    .and_then(|p| p.service_tier)
                    .map(Cell::Text)
                    .unwrap_or(Cell::Null),
            );
        }
        _ => {}
    }
    Ok((cols, values))
}

fn snapshot_columns(path: &Path, table: &str) -> Result<Vec<String>, TransferError> {
    for line in BufReader::new(File::open(path)?).lines() {
        if let Entry::Table { name, columns } = serde_json::from_str(&line?)? {
            if name == table {
                return Ok(columns);
            }
        }
    }
    Err(TransferError::Invalid("missing snapshot table".into()))
}

fn read_cell(row: &sea_orm::QueryResult, col: &str, kind: &ColumnType) -> Result<Cell, DbErr> {
    Ok(match kind {
        ColumnType::Boolean => row
            .try_get::<Option<bool>>("", col)?
            .map(|v| Cell::Integer(i64::from(v)))
            .unwrap_or(Cell::Null),
        ColumnType::BigInteger | ColumnType::Integer => row
            .try_get::<Option<i64>>("", col)?
            .map(Cell::Integer)
            .unwrap_or(Cell::Null),
        ColumnType::Double | ColumnType::Float => row
            .try_get::<Option<f64>>("", col)?
            .map(Cell::Real)
            .unwrap_or(Cell::Null),
        _ => row
            .try_get::<Option<String>>("", col)?
            .map(Cell::Text)
            .unwrap_or(Cell::Null),
    })
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::{AppSetting, SeaOrmStorage, SettingsRepository};
    use codexmanager_core::storage::StorageBackendKind;

    pub(super) fn paths() -> (std::path::PathBuf, std::path::PathBuf) {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source = std::env::temp_dir().join(format!(
            "codexmanager-transfer-{}-{id}.sqlite",
            std::process::id()
        ));
        let archive = source.with_extension("jsonl");
        (source, archive)
    }

    #[tokio::test]
    async fn current_desktop_sqlite_imports_with_domain_data() {
        let (source, archive) = paths();
        legacy_fixture(&source);
        let report = export_sqlite(&source, &archive).await.unwrap();
        let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        target.migrate().await.unwrap();
        let actual = import_snapshot(&archive, target.connection())
            .await
            .unwrap();
        assert_eq!(actual, report);
        assert_eq!(
            SettingsRepository::get(target.connection(), "transfer_fixture")
                .await
                .unwrap()
                .unwrap()
                .value,
            "retained"
        );
        let model = crate::ManagedModelsRepository::get(target.connection(), "transfer-model")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.routes[0].upstream_model, "fixture");
        assert_eq!(model.price_tiers.len(), 1);
        let key = crate::ApiKeysRepository::get(target.connection(), "transfer-key")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(key.protocol_type, "anthropic_native");
        assert_eq!(key.service_tier.as_deref(), Some("priority"));
        assert_eq!(
            key.upstream_base_url.as_deref(),
            Some("https://provider.invalid")
        );
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    pub(super) fn legacy_fixture(source: &Path) {
        let legacy = codexmanager_core::storage::Storage::open(source).unwrap();
        legacy.init().unwrap();
        legacy
            .set_app_setting("transfer_fixture", "retained", 1)
            .unwrap();
        use codexmanager_core::storage::{
            ApiKey, ManagedModelV2, ManagedModelV2Upsert, ModelPriceTierV2, ModelPriceV2,
            ModelRouteV2,
        };
        legacy
            .upsert_managed_models_v2(&[ManagedModelV2Upsert {
                previous_slug: None,
                model: ManagedModelV2 {
                    slug: "transfer-model".into(),
                    display_name: "Transfer fixture".into(),
                    origin: "custom".into(),
                    enabled: true,
                    supported_in_api: true,
                    visibility: "list".into(),
                    instructions_mode: "passthrough".into(),
                    capabilities: serde_json::json!({"tools":true}),
                    price: ModelPriceV2 {
                        price_status: "custom".into(),
                        input_microusd_per_1m: Some(1_000_000),
                        cached_input_microusd_per_1m: Some(100_000),
                        output_microusd_per_1m: Some(2_000_000),
                        ..Default::default()
                    },
                    price_tiers: vec![ModelPriceTierV2 {
                        min_input_tokens: 0,
                        input_microusd_per_1m: 1_000_000,
                        cached_input_microusd_per_1m: 100_000,
                        cache_write_microusd_per_1m: None,
                        output_microusd_per_1m: 2_000_000,
                    }],
                    routes: vec![ModelRouteV2 {
                        source_kind: "account_pool".into(),
                        source_id: "default".into(),
                        upstream_model: "fixture".into(),
                        enabled: true,
                        weight: 1,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            }])
            .unwrap();
        legacy
            .insert_api_key(&ApiKey {
                id: "transfer-key".into(),
                name: Some("fixture".into()),
                model_slug: Some("transfer-model".into()),
                reasoning_effort: None,
                service_tier: Some("priority".into()),
                rotation_strategy: "account_rotation".into(),
                aggregate_api_id: None,
                aggregate_api_url: None,
                account_plan_filter: None,
                client_type: "claude_code".into(),
                protocol_type: "anthropic_native".into(),
                auth_scheme: "x_api_key".into(),
                upstream_base_url: Some("https://provider.invalid".into()),
                static_headers_json: Some("{}".into()),
                key_hash: "fake-transfer-hash".into(),
                status: "active".into(),
                created_at: 1,
                last_used_at: None,
            })
            .unwrap();
        legacy
            .insert_request_token_stat(&codexmanager_core::storage::RequestTokenStat {
                request_log_id: 99,
                key_id: Some("transfer-key".into()),
                model: Some("transfer-model".into()),
                input_tokens: Some(9_007_199_254_740_993),
                output_tokens: Some(7),
                total_tokens: Some(9_007_199_254_741_000),
                estimated_cost_usd: Some(0.125),
                created_at: 123,
                ..Default::default()
            })
            .unwrap();
    }

    async fn fixture(source: &Path) -> SeaOrmStorage {
        let source_url = format!(
            "sqlite://{}?mode=rwc",
            source.display().to_string().replace('\\', "/")
        );
        let db = SeaOrmStorage::connect(StorageBackendKind::Sqlite, &source_url)
            .await
            .unwrap();
        db.migrate().await.unwrap();
        SettingsRepository::set(
            db.connection(),
            AppSetting {
                key: "unicode".into(),
                value: "迁移\nsecret is never in the report".into(),
                updated_at: i64::from(i32::MAX) + 1,
            },
        )
        .await
        .unwrap();
        crate::RequestTokenStatsRepository::upsert(
            db.connection(),
            crate::RequestTokenStatRecord {
                request_log_id: 99,
                key_id: None,
                account_id: None,
                model: Some("fixture".into()),
                actual_source_kind: None,
                actual_source_id: None,
                input_tokens: Some(9_007_199_254_740_993),
                cached_input_tokens: None,
                output_tokens: Some(7),
                total_tokens: Some(9_007_199_254_741_000),
                reasoning_output_tokens: None,
                estimated_cost_usd: Some(0.125),
                usage_included: true,
                created_at: 123,
            },
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn offline_snapshot_preserves_source_and_round_trips_with_checksums() {
        let (source, archive) = paths();
        let source_db = fixture(&source).await;
        let expected = SettingsRepository::list(source_db.connection())
            .await
            .unwrap();
        let report = export_sqlite(&source, &archive).await.unwrap();
        assert_eq!(inspect_snapshot(&archive).unwrap(), report);
        assert!(
            export_sqlite(&source, &archive).await.is_err(),
            "never overwrite backup"
        );
        let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        target.migrate().await.unwrap();
        assert_eq!(
            import_snapshot(&archive, target.connection())
                .await
                .unwrap(),
            report
        );
        assert_eq!(
            SettingsRepository::list(target.connection()).await.unwrap(),
            expected
        );
        assert_eq!(
            SettingsRepository::list(source_db.connection())
                .await
                .unwrap(),
            expected
        );
        assert!(import_snapshot(&archive, target.connection())
            .await
            .unwrap_err()
            .to_string()
            .contains("not empty"));
        source_db.connection().clone().close().await.unwrap();
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    #[tokio::test]
    async fn unsupported_populated_tables_are_archived_and_block_partial_import() {
        let (source, archive) = paths();
        let source_db = fixture(&source).await;
        source_db
            .connection()
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE TABLE unsupported_domain (id INTEGER PRIMARY KEY, payload BLOB)".to_owned(),
            ))
            .await
            .unwrap();
        source_db
            .connection()
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO unsupported_domain VALUES (1, x'00FF')".to_owned(),
            ))
            .await
            .unwrap();
        export_sqlite(&source, &archive).await.unwrap();
        let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        target.migrate().await.unwrap();
        let err = import_snapshot(&archive, target.connection())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unsupported_domain"));
        assert!(SettingsRepository::list(target.connection())
            .await
            .unwrap()
            .is_empty());
        source_db.connection().clone().close().await.unwrap();
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    #[tokio::test]
    async fn truncated_or_tampered_archives_cannot_write_target() {
        let (source, archive) = paths();
        let source_db = fixture(&source).await;
        export_sqlite(&source, &archive).await.unwrap();
        let content = std::fs::read_to_string(&archive).unwrap();
        std::fs::write(&archive, content.replace("迁移", "tampered")).unwrap();
        let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        target.migrate().await.unwrap();
        assert!(import_snapshot(&archive, target.connection())
            .await
            .unwrap_err()
            .to_string()
            .contains("checksum"));
        std::fs::write(
            &archive,
            content.lines().take(2).collect::<Vec<_>>().join("\n"),
        )
        .unwrap();
        assert!(inspect_snapshot(&archive).is_err());
        assert!(SettingsRepository::list(target.connection())
            .await
            .unwrap()
            .is_empty());
        source_db.connection().clone().close().await.unwrap();
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    #[tokio::test]
    async fn late_unmapped_column_rolls_back_earlier_tables() {
        let (source, archive) = paths();
        let source_db = fixture(&source).await;
        source_db
            .connection()
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE request_token_stats ADD COLUMN future_accounting_field TEXT"
                    .to_owned(),
            ))
            .await
            .unwrap();
        export_sqlite(&source, &archive).await.unwrap();
        let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        target.migrate().await.unwrap();
        let err = import_snapshot(&archive, target.connection())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("future_accounting_field"));
        assert!(SettingsRepository::list(target.connection())
            .await
            .unwrap()
            .is_empty());
        assert!(
            crate::RequestTokenStatsRepository::list(target.connection())
                .await
                .unwrap()
                .is_empty()
        );
        source_db.connection().clone().close().await.unwrap();
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    async fn import_into_real_database(backend: StorageBackendKind, env: &str) {
        let (source, archive) = paths();
        legacy_fixture(&source);
        let expected = export_sqlite(&source, &archive).await.unwrap();
        let url = std::env::var(env).expect("isolated empty import test database URL");
        let target = SeaOrmStorage::connect(backend, &url)
            .await
            .expect("connect import test database");
        crate::migration::migrate(target.connection())
            .await
            .expect("migrate import target");
        assert_eq!(
            import_snapshot(&archive, target.connection())
                .await
                .unwrap(),
            expected
        );
        let stat = crate::RequestTokenStatsRepository::get(target.connection(), 99)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stat.input_tokens, Some(9_007_199_254_740_993));
        assert!(stat.usage_included);
        assert_eq!(stat.estimated_cost_usd, Some(0.125));
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_file(source).unwrap();
    }

    #[cfg(feature = "mysql")]
    #[tokio::test]
    #[ignore = "requires a separate empty MySQL import test database"]
    async fn sqlite_snapshot_imports_into_mysql() {
        import_into_real_database(
            StorageBackendKind::Mysql,
            "CODEXMANAGER_TEST_IMPORT_MYSQL_URL",
        )
        .await;
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore = "requires a separate empty PostgreSQL import test database"]
    async fn sqlite_snapshot_imports_into_postgres() {
        import_into_real_database(
            StorageBackendKind::Postgres,
            "CODEXMANAGER_TEST_IMPORT_POSTGRES_URL",
        )
        .await;
    }
}
