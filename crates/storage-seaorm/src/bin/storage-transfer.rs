use codexmanager_storage_seaorm::{transfer, SeaOrmStorage, StorageConfig};
use std::path::Path;

#[tokio::main]
async fn main() {
    let report = run().await;
    // Every outcome is a redacted JSON document; redirect stdout to retain it.
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report serialization")
    );
    if !report.success {
        std::process::exit(1);
    }
}

async fn run() -> transfer::TransferReport {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    let mut report = transfer::TransferReport::new(match command {
        "export" | "inspect" | "dry-run" | "prepare-target" | "import" => command,
        _ => "invalid-command",
    });
    match args.as_slice() {
        #[cfg(feature = "sqlite")]
        [command, source, archive] if command == "export" => {
            match transfer::export_sqlite(Path::new(source), Path::new(archive)).await {
                Ok(tables) => report.set_snapshot(&tables),
                Err(error) => report.snapshot_error(&error),
            }
            return report;
        }
        [command, archive] if matches!(command.as_str(), "inspect" | "dry-run" | "import") => {
            match transfer::inspect_snapshot(Path::new(archive)) {
                Ok(tables) => report.set_snapshot(&tables),
                Err(error) => {
                    report.snapshot_error(&error);
                    return report;
                }
            }
            if command == "inspect" {
                return report;
            }
        }
        [command] if command == "prepare-target" => {}
        _ => {
            report.fail(
                "usage_export_source_snapshot_or_inspect_dry_run_import_snapshot_or_prepare_target",
                None,
                None,
                None,
            );
            return report;
        }
    }
    let config = match StorageConfig::from_env().and_then(|config| {
        config.validate()?;
        if command == "dry-run" {
            transfer::read_only_target_config(config)
        } else {
            Ok(config)
        }
    }) {
        Ok(config) => config,
        Err(_) => {
            report.fail("target_configuration_invalid", None, None, None);
            return report;
        }
    };
    report.backend = Some(config.backend.as_str());
    let db = match SeaOrmStorage::connect_with_config(config).await {
        Ok(db) => db,
        Err(_) => {
            report.fail(
                "target_connection_or_authentication_failed",
                None,
                None,
                None,
            );
            return report;
        }
    };
    if command == "prepare-target" {
        // MySQL DDL may commit before a later migration fails. Never label
        // schema preparation as a read-only operation or a rollback guarantee.
        report.target_modified = true;
        if db.migrate().await.is_err() {
            report.fail("target_schema_preparation_failed", None, None, None);
        }
    } else {
        report = transfer::dry_run(Path::new(&args[1]), db.connection()).await;
        report.operation = command.into();
        if command == "import" && report.success {
            match transfer::import_snapshot(Path::new(&args[1]), db.connection()).await {
                Ok(_) => report.target_modified = true,
                Err(_) => report.fail("import_failed_transaction_rolled_back", None, None, None),
            }
        }
    }
    if db.connection().clone().close().await.is_err() {
        report.fail("target_connection_close_failed", None, None, None);
    }
    report
}
