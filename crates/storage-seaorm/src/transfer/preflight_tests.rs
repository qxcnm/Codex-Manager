use super::*;
use crate::{SeaOrmStorage, SettingsRepository};
use codexmanager_core::storage::StorageBackendKind;

async fn empty_target() -> SeaOrmStorage {
    let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    target.migrate().await.unwrap();
    target
}

fn has(report: &TransferReport, code: &str) -> bool {
    report.failures.iter().any(|failure| failure.code == code)
}

fn write_settings_snapshot(path: &Path, rows: Vec<Vec<Cell>>) {
    let mut writer = BufWriter::new(File::create(path).unwrap());
    write_entry(&mut writer, &Entry::Header { version: VERSION }).unwrap();
    write_entry(
        &mut writer,
        &Entry::Table {
            name: "app_settings".into(),
            columns: vec!["key".into(), "value".into(), "updated_at".into()],
        },
    )
    .unwrap();
    let mut sum = [0; 32];
    for cells in &rows {
        add_hash(&mut sum, row_hash(cells).unwrap());
        write_entry(
            &mut writer,
            &Entry::Row {
                cells: cells.clone(),
            },
        )
        .unwrap();
    }
    write_entry(
        &mut writer,
        &Entry::EndTable {
            count: rows.len() as u64,
            sha256: hash_text(&sum),
        },
    )
    .unwrap();
    write_entry(&mut writer, &Entry::End { tables: 1 }).unwrap();
    writer.flush().unwrap();
}

#[tokio::test]
async fn dry_run_reports_types_conflicts_nonempty_and_reexecutes_without_mutation() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    let expected = export_sqlite(&source, &archive).await.unwrap();
    let target = empty_target().await;
    for _ in 0..2 {
        let report = dry_run(&archive, target.connection()).await;
        assert!(
            report.success,
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert!(!report.target_modified);
    }
    let original = std::fs::read(&archive).unwrap();
    write_settings_snapshot(
        &archive,
        vec![vec![
            Cell::Text("secret-key".into()),
            Cell::Text("private-token".into()),
            Cell::Text("invalid-time-secret".into()),
        ]],
    );
    let invalid = dry_run(&archive, target.connection()).await;
    assert!(has(&invalid, "source_value_type_or_range_invalid"));
    let output = serde_json::to_string(&invalid).unwrap();
    for secret in ["secret-key", "private-token", "invalid-time-secret"] {
        assert!(!output.contains(secret));
    }
    let duplicate = vec![
        Cell::Text("secret-key".into()),
        Cell::Text("private-token".into()),
        Cell::Integer(1),
    ];
    write_settings_snapshot(&archive, vec![duplicate.clone(), duplicate]);
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "source_unique_key_conflict"
    ));
    std::fs::write(&archive, &original).unwrap();
    assert_eq!(
        import_snapshot(&archive, target.connection())
            .await
            .unwrap(),
        expected
    );
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_not_empty"
    ));
    // The legacy adapter uses SQLite WAL. Opening a read-only SQLx handle can
    // checkpoint an already committed WAL into the main file, so a byte-for-
    // byte comparison would report a false mutation. Verify the source through
    // its public storage API instead: the fixture remains readable and its
    // committed setting survives every dry-run/import failure path.
    let source_db = codexmanager_core::storage::Storage::open(&source).unwrap();
    assert_eq!(
        source_db
            .get_app_setting("transfer_fixture")
            .unwrap()
            .as_deref(),
        Some("retained")
    );
    drop(source_db);
    assert_eq!(
        SettingsRepository::get(target.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    std::fs::remove_file(archive).unwrap();
    // Keep the source path until the process exits: the legacy SQLite facade
    // owns a shared SQLx runtime and Windows can retain a WAL handle briefly
    // after the final `Storage` value is dropped.
}

#[tokio::test]
async fn dry_run_rejects_populated_unmapped_target_table() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = empty_target().await;
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE unmanaged_fixture (value TEXT NOT NULL)",
        ))
        .await
        .unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO unmanaged_fixture (value) VALUES ('opaque')",
        ))
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_unmapped_populated_table"));
    assert!(report
        .tables
        .iter()
        .any(|table| !table.mapped && table.target_rows == Some(1)));
    assert!(!report.target_modified);
    let row = target
        .connection()
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS row_count FROM unmanaged_fixture",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "row_count").unwrap(), 1);
    std::fs::remove_file(archive).unwrap();
}

#[tokio::test]
async fn dry_run_missing_and_malformed_schema_remains_unmodified() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_table_missing"));
    let rows = target
        .connection()
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table'".to_owned(),
        ))
        .await
        .unwrap();
    assert!(rows.is_empty(), "preflight must not create schema");
    target.migrate().await.unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE app_settings RENAME COLUMN updated_at TO old_updated_at".to_owned(),
        ))
        .await
        .unwrap();
    target
        .connection()
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE app_settings ADD COLUMN updated_at TEXT".to_owned(),
        ))
        .await
        .unwrap();
    let report = dry_run(&archive, target.connection()).await;
    assert!(has(&report, "target_column_type_mismatch"));
    assert!(SettingsRepository::list(target.connection())
        .await
        .unwrap()
        .is_empty());
    std::fs::remove_file(archive).unwrap();
    std::fs::remove_file(source).unwrap();
}

#[tokio::test]
async fn failed_checksum_and_connection_preserve_source_and_allow_clean_restore() {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let original = std::fs::read_to_string(&archive).unwrap();
    let target = empty_target().await;
    std::fs::write(&archive, original.replace("retained", "tampered-secret")).unwrap();
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "snapshot_checksum_mismatch"
    ));
    std::fs::write(&archive, &original).unwrap();
    let missing = source.with_extension("missing-target.sqlite");
    let config = read_only_target_config(crate::StorageConfig {
        backend: StorageBackendKind::Sqlite,
        database_url: format!(
            "sqlite://{}?mode=rwc",
            missing.to_string_lossy().replace('\\', "/")
        ),
        max_connections: 1,
        acquire_timeout_ms: 50,
    })
    .unwrap();
    assert!(SeaOrmStorage::connect_with_config(config).await.is_err());
    assert!(!missing.exists());
    // A physical backup is safe after the fixture connection is closed. Restore
    // to a new pathname and verify through the legacy adapter after failed cutover.
    let restored = source.with_extension("restored.sqlite");
    std::fs::copy(&source, &restored).unwrap();
    let restored_db = codexmanager_core::storage::Storage::open(&restored).unwrap();
    assert_eq!(
        restored_db
            .get_app_setting("transfer_fixture")
            .unwrap()
            .as_deref(),
        Some("retained")
    );
    drop(restored_db);
    assert!(dry_run(&archive, target.connection()).await.success);
    import_snapshot(&archive, target.connection())
        .await
        .unwrap();
    let source_db = codexmanager_core::storage::Storage::open(&source).unwrap();
    assert_eq!(
        source_db
            .get_app_setting("transfer_fixture")
            .unwrap()
            .as_deref(),
        Some("retained")
    );
    drop(source_db);
    std::fs::remove_file(restored).unwrap();
    std::fs::remove_file(archive).unwrap();
    // See the note above about the legacy facade's WAL handle on Windows.
}

async fn remote_dry_run(backend: StorageBackendKind, variable: &str) {
    let (source, archive) = tests::paths();
    tests::legacy_fixture(&source);
    export_sqlite(&source, &archive).await.unwrap();
    let target = SeaOrmStorage::connect(
        backend,
        &std::env::var(variable).expect("isolated new fixture URL"),
    )
    .await
    .unwrap();
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_table_missing"
    ));
    target.migrate().await.unwrap();
    for _ in 0..2 {
        let report = dry_run(&archive, target.connection()).await;
        assert!(
            report.success,
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert!(!report.target_modified);
        assert!(
            SettingsRepository::list(target.connection())
                .await
                .unwrap()
                .is_empty(),
            "dry-run must not import source settings into the real target"
        );
    }
    import_snapshot(&archive, target.connection())
        .await
        .unwrap();
    assert!(has(
        &dry_run(&archive, target.connection()).await,
        "target_not_empty"
    ));
    assert_eq!(
        SettingsRepository::get(target.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    // Await pool shutdown before deleting the fixture on Windows. Dropping the
    // synchronous compatibility facade only schedules SQLx handle cleanup.
    let file_url = url::Url::from_file_path(&source).unwrap();
    let source_db = SeaOrmStorage::connect(
        StorageBackendKind::Sqlite,
        &format!("sqlite://{}?mode=ro", file_url.path()),
    )
    .await
    .unwrap();
    assert_eq!(
        SettingsRepository::get(source_db.connection(), "transfer_fixture")
            .await
            .unwrap()
            .unwrap()
            .value,
        "retained"
    );
    source_db.connection().clone().close().await.unwrap();
    target.connection().clone().close().await.unwrap();
    std::fs::remove_file(archive).unwrap();
    std::fs::remove_file(source).unwrap();
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore = "requires a new isolated MySQL database"]
async fn mysql_dry_run_is_read_only_and_retries_before_import() {
    remote_dry_run(
        StorageBackendKind::Mysql,
        "CODEXMANAGER_TEST_DRYRUN_MYSQL_URL",
    )
    .await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires a new isolated PostgreSQL database"]
async fn postgres_dry_run_is_read_only_and_retries_before_import() {
    remote_dry_run(
        StorageBackendKind::Postgres,
        "CODEXMANAGER_TEST_DRYRUN_POSTGRES_URL",
    )
    .await;
}
