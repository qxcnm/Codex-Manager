//! Read-only target preflight. Never call migrations or execute DDL/DML here.
use super::*;
use crate::{StorageConfig, StorageError};
use codexmanager_core::storage::StorageBackendKind;

#[derive(Clone, Debug, Serialize)]
pub struct TransferFailure {
    pub code: &'static str,
    pub table: Option<String>,
    pub column: Option<String>,
    pub row: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransferTableReport {
    pub table: String,
    pub source_rows: u64,
    pub target_rows: Option<u64>,
    pub mapped: bool,
}

/// Only allowlisted identifiers, counts and stable error codes are emitted.
/// Raw database errors, input values, paths and URLs must never enter this type.
#[derive(Clone, Debug, Serialize)]
pub struct TransferReport {
    pub report_version: u32,
    pub operation: String,
    pub success: bool,
    pub backend: Option<&'static str>,
    pub target_modified: bool,
    pub tables: Vec<TransferTableReport>,
    pub failures: Vec<TransferFailure>,
    pub failures_truncated: bool,
}

impl TransferReport {
    pub fn new(operation: &str) -> Self {
        Self {
            report_version: 1,
            operation: operation.into(),
            success: true,
            backend: None,
            target_modified: false,
            tables: vec![],
            failures: vec![],
            failures_truncated: false,
        }
    }

    pub fn fail(
        &mut self,
        code: &'static str,
        table: Option<&str>,
        column: Option<&str>,
        row: Option<u64>,
    ) {
        self.success = false;
        if self.failures.len() >= 100 {
            self.failures_truncated = true;
            return;
        }
        let supported = specs();
        let known = table.and_then(|name| supported.iter().find(|s| s.name == name));
        self.failures.push(TransferFailure {
            code,
            table: known.map(|s| s.name.clone()),
            column: column
                .filter(|name| known.is_some_and(|s| s.columns.contains_key(*name)))
                .map(str::to_owned),
            row,
        });
    }

    pub fn snapshot_error(&mut self, error: &TransferError) {
        let code = match error {
            TransferError::Io(_) => "snapshot_io_failed",
            TransferError::Json(_) => "snapshot_encoding_invalid",
            TransferError::Database(_) => "database_operation_failed",
            TransferError::Invalid(message) if message.starts_with("snapshot checksum") => {
                "snapshot_checksum_mismatch"
            }
            _ => "snapshot_structure_invalid",
        };
        self.fail(code, None, None, None);
    }

    pub fn set_snapshot(&mut self, source: &[TableVerification]) {
        let supported = specs();
        self.tables = source
            .iter()
            .map(|r| {
                let mapped = supported.iter().any(|s| s.name == r.table);
                TransferTableReport {
                    table: if mapped {
                        r.table.clone()
                    } else {
                        "[unmapped]".into()
                    },
                    source_rows: r.rows,
                    target_rows: None,
                    mapped,
                }
            })
            .collect();
    }
}

/// SQLx SQLite URLs can create missing files; force mode=ro before connecting.
/// Remote connections do not initialize a schema and preflight issues SELECTs only.
pub fn read_only_target_config(mut config: StorageConfig) -> Result<StorageConfig, StorageError> {
    config.validate()?;
    config.max_connections = 1;
    if config.backend == StorageBackendKind::Sqlite {
        if config.database_url.contains(":memory:") {
            return Err(StorageError::InvalidUrl { backend: "sqlite" });
        }
        let (base, query) = config
            .database_url
            .split_once('?')
            .unwrap_or((&config.database_url, ""));
        let mut params: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
            .filter(|(key, _)| key != "mode")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        params.push(("mode".into(), "ro".into()));
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();
        config.database_url = format!("{base}?{query}");
    }
    Ok(config)
}

struct TargetColumn {
    kind: String,
    nullable: bool,
    defaulted: bool,
    max_chars: Option<i64>,
    collation: Option<String>,
}

/// Return user tables visible to the target connection without touching the
/// schema.  The migration's domain-lock table is deliberately omitted from
/// the snapshot/export stream, so it is treated as an allowed empty support
/// table below.
async fn target_tables(db: &DatabaseConnection) -> Result<Vec<String>, DbErr> {
    let backend = db.get_database_backend();
    // MySQL reports INFORMATION_SCHEMA labels in uppercase unless explicitly
    // aliased. Keep stable labels for SQLx's case-sensitive column lookup.
    let statement = match backend {
        DbBackend::Sqlite => Statement::from_string(
            backend,
            "SELECT name AS table_name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ),
        DbBackend::MySql => Statement::from_string(
            backend,
            "SELECT table_name AS table_name FROM information_schema.tables WHERE table_schema=DATABASE() AND table_type='BASE TABLE' ORDER BY table_name",
        ),
        DbBackend::Postgres => Statement::from_string(
            backend,
            "SELECT table_name FROM information_schema.tables WHERE table_schema=current_schema() AND table_type='BASE TABLE' ORDER BY table_name",
        ),
    };
    db.query_all(statement)
        .await?
        .into_iter()
        .map(|row| row.try_get("", "table_name"))
        .collect()
}

async fn columns(
    db: &DatabaseConnection,
    table: &str,
) -> Result<BTreeMap<String, TargetColumn>, DbErr> {
    let backend = db.get_database_backend();
    if backend == DbBackend::Sqlite {
        return db
            .query_all(Statement::from_string(
                backend,
                format!("PRAGMA table_info(\"{table}\")"),
            ))
            .await?
            .into_iter()
            .map(|r| {
                Ok((
                    r.try_get("", "name")?,
                    TargetColumn {
                        kind: r.try_get::<String>("", "type")?.to_ascii_lowercase(),
                        nullable: r.try_get::<i64>("", "notnull")? == 0
                            && r.try_get::<i64>("", "pk")? == 0,
                        defaulted: r.try_get::<Option<String>>("", "dflt_value")?.is_some(),
                        max_chars: None,
                        collation: None,
                    },
                ))
            })
            .collect();
    }
    let (schema, parameter) = if backend == DbBackend::MySql {
        ("DATABASE()", "?")
    } else {
        ("current_schema()", "$1")
    };
    db.query_all(Statement::from_sql_and_values(backend, format!(
        "SELECT column_name AS column_name, data_type AS data_type, is_nullable AS is_nullable, column_default AS column_default, CAST(character_maximum_length AS SIGNED) AS max_chars, collation_name AS collation_name FROM information_schema.columns WHERE table_schema={schema} AND table_name={parameter}"
    ).replace("CAST(character_maximum_length AS SIGNED)", if backend == DbBackend::Postgres { "CAST(character_maximum_length AS BIGINT)" } else { "CAST(character_maximum_length AS SIGNED)" }), [table.into()])).await?
        .into_iter().map(|r| Ok((r.try_get("", "column_name")?, TargetColumn {
            kind: r.try_get::<String>("", "data_type")?.to_ascii_lowercase(),
            nullable: r.try_get::<String>("", "is_nullable")? == "YES",
            defaulted: r.try_get::<Option<String>>("", "column_default")?.is_some(),
            max_chars: r.try_get("", "max_chars")?, collation: r.try_get("", "collation_name")?,
        }))).collect()
}

fn compatible(kind: &ColumnType, target: &str) -> bool {
    match kind {
        ColumnType::Boolean => matches!(target, "boolean" | "bool" | "tinyint" | "integer"),
        ColumnType::BigInteger => matches!(target, "bigint" | "integer"),
        ColumnType::Integer => matches!(target, "int" | "integer" | "bigint"),
        ColumnType::Double | ColumnType::Float => {
            matches!(target, "real" | "double" | "double precision" | "float")
        }
        ColumnType::String(_) | ColumnType::Text | ColumnType::Char(_) => {
            target.contains("char") || matches!(target, "text" | "mediumtext" | "longtext")
        }
        _ => false,
    }
}

fn valid_cell(cell: &Cell, kind: &ColumnType, nullable: bool, max_chars: Option<i64>) -> bool {
    match cell {
        Cell::Null => nullable,
        Cell::Integer(v) => match kind {
            ColumnType::Boolean => *v == 0 || *v == 1,
            ColumnType::Integer => i32::try_from(*v).is_ok(),
            ColumnType::BigInteger => true,
            ColumnType::Double | ColumnType::Float => (*v as f64) as i64 == *v,
            _ => false,
        },
        Cell::Real(v) => v.is_finite() && matches!(kind, ColumnType::Double | ColumnType::Float),
        Cell::Text(v) => {
            matches!(
                kind,
                ColumnType::String(_) | ColumnType::Text | ColumnType::Char(_)
            ) && max_chars.is_none_or(|max| v.chars().count() as i64 <= max)
        }
        Cell::Blob(_) => false,
    }
}

/// Validates the archive first, then reads catalogs and counts; no migration,
/// insert, sequence update, transaction rollback simulation or lock-row write.
pub async fn dry_run(path: &Path, db: &DatabaseConnection) -> TransferReport {
    let mut report = TransferReport::new("dry-run");
    report.backend = Some(match db.get_database_backend() {
        DbBackend::Sqlite => "sqlite",
        DbBackend::MySql => "mysql",
        DbBackend::Postgres => "postgres",
    });
    let snapshot = match inspect_snapshot(path) {
        Ok(source) => source,
        Err(error) => {
            report.snapshot_error(&error);
            return report;
        }
    };
    report.set_snapshot(&snapshot);
    let supported = specs();
    let supported_names: BTreeSet<_> = supported.iter().map(|table| table.name.as_str()).collect();
    // A populated table outside the migration contract would make the
    // documented empty-target import unsafe. Empty auxiliary tables are
    // harmless and are still included in the per-table report for visibility.
    match target_tables(db).await {
        Ok(names) => {
            for name in names {
                if name == "app_domain_locks" || supported_names.contains(name.as_str()) {
                    continue;
                }
                let query = Query::select()
                    .expr(sea_orm::sea_query::Expr::cust("COUNT(*) AS row_count"))
                    .from(Alias::new(&name))
                    .to_owned();
                match db
                    .query_one(db.get_database_backend().build(&query))
                    .await
                    .and_then(|row| row.ok_or_else(|| DbErr::Custom("missing count".into())))
                    .and_then(|row| row.try_get::<i64>("", "row_count"))
                {
                    Ok(count) => {
                        report.tables.push(TransferTableReport {
                            table: "[unmapped]".into(),
                            source_rows: 0,
                            target_rows: Some(count as u64),
                            mapped: false,
                        });
                        if count != 0 {
                            report.fail("target_unmapped_populated_table", None, None, None);
                        }
                    }
                    Err(_) => report.fail("target_read_access_failed", None, None, None),
                }
            }
        }
        Err(_) => report.fail("target_catalog_access_failed", None, None, None),
    }
    for source in &snapshot {
        if source.rows > 0 && !supported.iter().any(|s| s.name == source.table) {
            report.fail("unmapped_populated_table", None, None, None);
        }
    }
    for table in &supported {
        let source = snapshot.iter().find(|s| s.table == table.name);
        let target = match columns(db, &table.name).await {
            Ok(target) => target,
            Err(_) => {
                report.fail(
                    "target_catalog_access_failed",
                    Some(&table.name),
                    None,
                    None,
                );
                continue;
            }
        };
        if target.is_empty() {
            report.fail("target_table_missing", Some(&table.name), None, None);
            continue;
        }
        for (column, kind) in &table.columns {
            match target.get(column) {
                None => report.fail(
                    "target_column_missing",
                    Some(&table.name),
                    Some(column),
                    None,
                ),
                Some(actual) if !compatible(kind, &actual.kind) => report.fail(
                    "target_column_type_mismatch",
                    Some(&table.name),
                    Some(column),
                    None,
                ),
                _ => {}
            }
        }
        let query = Query::select()
            .expr(sea_orm::sea_query::Expr::cust("COUNT(*) AS row_count"))
            .from(Alias::new(&table.name))
            .to_owned();
        match db
            .query_one(db.get_database_backend().build(&query))
            .await
            .and_then(|r| r.ok_or_else(|| DbErr::Custom("missing count".into())))
            .and_then(|r| r.try_get::<i64>("", "row_count"))
        {
            Ok(count) => {
                if let Some(row) = report.tables.iter_mut().find(|r| r.table == table.name) {
                    row.target_rows = Some(count as u64);
                } else {
                    report.tables.push(TransferTableReport {
                        table: table.name.clone(),
                        source_rows: 0,
                        target_rows: Some(count as u64),
                        mapped: true,
                    });
                }
                if count != 0 {
                    report.fail("target_not_empty", Some(&table.name), None, None);
                }
            }
            Err(_) => report.fail("target_read_access_failed", Some(&table.name), None, None),
        }
        if source.is_none_or(|s| s.rows == 0) {
            continue;
        }
        if let Err(error) = validate_rows(path, db, table, &target, &mut report).await {
            report.snapshot_error(&error);
        }
    }
    report
}

async fn validate_rows(
    path: &Path,
    db: &DatabaseConnection,
    table: &TableSpec,
    target: &BTreeMap<String, TargetColumn>,
    report: &mut TransferReport,
) -> Result<(), TransferError> {
    let mut active = None;
    let mut number = 0;
    // Digests, never raw credential or identifier values, are retained for duplicate detection.
    let mut unique: Vec<BTreeSet<[u8; 32]>> =
        table.unique.iter().map(|_| BTreeSet::new()).collect();
    for line in BufReader::new(File::open(path)?).lines() {
        match serde_json::from_str::<Entry>(&line?)? {
            Entry::Table { name, columns } if name == table.name => {
                if columns.iter().any(|c| !table.columns.contains_key(c)) {
                    report.fail("unmapped_source_column", Some(&table.name), None, None);
                    return Ok(());
                }
                active = Some(columns);
            }
            Entry::Table { .. } | Entry::EndTable { .. } => active = None,
            Entry::Row { cells } if active.is_some() => {
                number += 1;
                let original = active.as_ref().unwrap();
                let (cols, cells) = match complete_legacy_row(db, table, original, &cells).await {
                    Ok(row) => row,
                    Err(_) => {
                        report.fail(
                            "legacy_mapping_failed",
                            Some(&table.name),
                            None,
                            Some(number),
                        );
                        continue;
                    }
                };
                for column in table.columns.keys() {
                    if !cols.contains(column)
                        && !table.nullable.contains(column)
                        && !table.defaulted.contains(column)
                    {
                        report.fail(
                            "required_source_column_missing",
                            Some(&table.name),
                            Some(column),
                            Some(number),
                        );
                    }
                }
                for (column, cell) in cols.iter().zip(&cells) {
                    let actual = target.get(column);
                    if !valid_cell(
                        cell,
                        &table.columns[column],
                        actual.map_or(table.nullable.contains(column), |c| c.nullable),
                        actual.and_then(|c| c.max_chars),
                    ) {
                        report.fail(
                            "source_value_type_or_range_invalid",
                            Some(&table.name),
                            Some(column),
                            Some(number),
                        );
                    }
                }
                for (column, actual) in target {
                    if !cols.contains(column)
                        && !table.columns.contains_key(column)
                        && !actual.nullable
                        && !actual.defaulted
                    {
                        report.fail(
                            "unmapped_required_target_column",
                            Some(&table.name),
                            None,
                            Some(number),
                        );
                    }
                }
                for (index, keys) in table.unique.iter().enumerate() {
                    let mut values = Vec::new();
                    for key in keys {
                        let Some(value) =
                            cols.iter().position(|c| c == key).map(|i| cells[i].clone())
                        else {
                            values.clear();
                            break;
                        };
                        if value == Cell::Null {
                            values.clear();
                            break;
                        }
                        // MySQL compares text using the target collation, including accent/case
                        // and padding rules. WEIGHT_STRING is read-only and returns no raw text.
                        let value = if let (DbBackend::MySql, Cell::Text(text), Some(collation)) = (
                            db.get_database_backend(),
                            &value,
                            target.get(key).and_then(|c| c.collation.as_deref()),
                        ) {
                            if !collation
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_')
                            {
                                return Err(TransferError::Invalid(
                                    "invalid collation metadata".into(),
                                ));
                            }
                            let row = db.query_one(Statement::from_sql_and_values(DbBackend::MySql,
                                format!("SELECT HEX(WEIGHT_STRING(CONVERT(? USING utf8mb4) COLLATE {collation})) AS weight"), [text.clone().into()])).await?
                                .ok_or_else(|| TransferError::Invalid("missing comparison weight".into()))?;
                            Cell::Text(row.try_get("", "weight")?)
                        } else {
                            value
                        };
                        values.push(value);
                    }
                    if !values.is_empty() && !unique[index].insert(row_hash(&values)?) {
                        report.fail(
                            "source_unique_key_conflict",
                            Some(&table.name),
                            None,
                            Some(number),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
