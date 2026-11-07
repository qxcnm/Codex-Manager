use super::{
    scheduler::{self, Fetched},
    store,
    types::{Record, Source},
    *,
};
use codexmanager_core::{
    rpc::types::AggregateApiAssociateModelsResult,
    storage::{now_ts, Account, Storage},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn source(id: &str) -> Source {
    Source::parse("openai_account", id).unwrap()
}

fn associated() -> AggregateApiAssociateModelsResult {
    AggregateApiAssociateModelsResult {
        created_models: vec!["new-model".into()],
        added_routes: vec!["new-model".into()],
        unchanged_routes: Vec::new(),
    }
}

#[test]
fn defaults_validation_and_idempotent_updates() {
    let mut record = Record::new(source("selected-account"));
    assert!(!record.config.enabled);
    assert_eq!(record.config.interval_minutes, 60);
    assert_eq!(record.task().discovered_count, 0);
    for interval in [0, 4, 10081, u64::MAX] {
        assert!(record.update(true, Some(interval), 100).is_err());
    }
    assert!(record.update(true, None, 100).unwrap());
    assert_eq!(record.status.next_run_at, Some(100));
    assert!(!record.update(true, Some(60), 200).unwrap());
    assert_eq!(record.config.generation, 1);
    assert_eq!(record.status.next_run_at, Some(100));
    assert!(record.update(false, None, 200).unwrap());
    assert_eq!(record.config.generation, 2);
    assert_eq!(record.status.next_run_at, None);
    assert!(Source::parse("account", "a").is_err());
    assert!(Source::parse("openai_account", "\n").is_err());
    let json = serde_json::to_value(record.task()).unwrap();
    assert_eq!(json["intervalMinutes"], 60);
    assert!(json["lastSuccessAt"].is_null());
    assert!(json.get("consecutiveFailures").is_none());
}

#[test]
fn persisted_config_and_status_are_separate_and_corruption_is_an_error() {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    let mut record = Record::new(source("id.with/delimiters"));
    record.update(true, Some(5), 123).unwrap();
    storage
        .save_app_settings(&store::changes(&record, true).unwrap(), 123)
        .unwrap();
    record.status.discovered_count = 9;
    storage
        .save_app_settings(&store::changes(&record, false).unwrap(), 124)
        .unwrap();
    let mut settings: HashMap<_, _> = storage.list_app_settings().unwrap().into_iter().collect();
    let records = store::decode(&settings).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].config.enabled);
    assert_eq!(records[0].config.interval_minutes, 5);
    assert_eq!(records[0].status.discovered_count, 9);
    settings.insert(
        format!("model_discovery.config.{}", record.config.source.key()),
        "invalid-json".into(),
    );
    assert!(store::decode(&settings).is_err());
}

#[test]
fn failures_back_off_without_changing_last_success() {
    let mut record = Record::new(source("a"));
    record.update(true, Some(5), 100).unwrap();
    record.status.last_success_at = Some(80);
    record.finish_error("fetch failed", 100);
    assert_eq!(record.status.next_run_at, Some(400));
    record.finish_error("fetch failed", 400);
    assert_eq!(record.status.next_run_at, Some(1000));
    assert_eq!(record.status.last_success_at, Some(80));
    for _ in 0..100 {
        record.finish_error("fetch failed", 1000);
    }
    assert_eq!(record.status.next_run_at, Some(22600));
}

struct TestDb {
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
    path: PathBuf,
}
impl TestDb {
    fn new() -> Self {
        Self::new_with_prepared_guard(|_| ())
    }

    fn new_with_prepared_guard<T>(before_publish: impl FnOnce(&Path) -> T) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codexmanager-model-discovery-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // The environment guard serializes test bodies, not previously spawned workers.
        // Finish migrations before any worker can discover this new database path.
        let prepared = Storage::open(&path).expect("open unpublished discovery test database");
        prepared
            .init()
            .expect("initialize unpublished discovery test database");
        prepared
            .smoke_check_model_catalog_v2()
            .expect("validate unpublished discovery test catalog");
        drop(prepared);
        let _prepared_guard = before_publish(&path);
        let mut previous = Vec::new();
        for (key, value) in [
            (
                "CODEXMANAGER_DB_PATH",
                Some(path.to_string_lossy().into_owned()),
            ),
            ("CODEXMANAGER_STORAGE_BACKEND", Some("sqlite".into())),
            ("CODEXMANAGER_DATABASE_URL", None),
        ] {
            previous.push((key, std::env::var_os(key)));
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        crate::clear_shutdown_flag();
        scheduler::control().cooldowns.clear();
        crate::initialize_storage_if_needed().unwrap();
        let db = Self { previous, path };
        db.add_account("a");
        db
    }
    fn storage(&self) -> Storage {
        Storage::open(&self.path).unwrap()
    }
    fn add_account(&self, id: &str) {
        self.storage()
            .insert_account(&Account {
                id: id.into(),
                label: "Discovery test".into(),
                issuer: "https://auth.openai.com".into(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".into(),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
    }
}

#[test]
fn fixture_publishes_a_migrated_database_before_service_workers_can_initialize_it() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new_with_prepared_guard(|path| {
        assert_ne!(
            std::env::var_os("CODEXMANAGER_DB_PATH").as_deref(),
            Some(path.as_os_str())
        );
        // Holding the migration lock through publication deterministically proves the
        // service never needs to claim it for this already-prepared fixture.
        let migration_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(format!("{}.model-catalog-v2.lock", path.display()))
            .unwrap();
        migration_lock.try_lock().unwrap();
        migration_lock
    });
}
impl Drop for TestDb {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn disable_fences_old_generation_and_reenable_does_not_overlap() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new();
    assert!(list_tasks().unwrap().items.is_empty());
    let saved = update_task("openai_account", "a", true, None).unwrap();
    assert_eq!(saved.interval_minutes, 60);
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    assert!(scheduler::claim_due().unwrap().is_empty());
    assert!(
        update_task("openai_account", "a", true, None)
            .unwrap()
            .running
    );
    update_task("openai_account", "a", false, None).unwrap();
    assert!(run.cancelled.load(Ordering::Acquire));
    scheduler::complete_with(
        &run,
        Ok(Fetched::default()),
        |_, _| panic!("disabled task must not write"),
        || panic!("disabled task must not sync"),
    )
    .unwrap();
    update_task("openai_account", "a", true, Some(5)).unwrap();
    assert!(scheduler::claim_due().unwrap().is_empty());
    drop(run);
    let runs = scheduler::claim_due().unwrap();
    assert_eq!(runs.len(), 1);
    drop(runs);
    update_task("openai_account", "a", false, None).unwrap();
}

#[test]
fn restart_recovers_running_task_and_errors_never_expose_upstream_secrets() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new();
    update_task("openai_account", "a", true, Some(5)).unwrap();
    let interrupted = scheduler::claim_due().unwrap();
    drop(interrupted); // Same persisted state as shutdown or a process restart.
    scheduler::control().cooldowns.clear(); // A new process has no local cooldowns.
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    assert!(list_tasks().unwrap().items[0]
        .last_warning
        .as_deref()
        .unwrap()
        .contains("interrupted"));
    scheduler::complete_with(
        &run,
        Err("https://provider.test/models?key=SECRET credential SECRET".into()),
        |_, _| panic!("failed fetch must not write"),
        || Ok(()),
    )
    .unwrap();
    drop(run);
    let task = list_tasks().unwrap().items.remove(0);
    assert!(!task.running);
    assert!(task.last_success_at.is_none());
    assert!(task.next_run_at.unwrap() >= now_ts() + 295);
    assert!(!task.last_error.unwrap().contains("SECRET"));
    assert!(scheduler::claim_due().unwrap().is_empty());
    update_task("openai_account", "a", false, None).unwrap();
}

#[test]
fn source_disabled_or_deleted_after_fetch_prevents_catalog_write() {
    let _env = crate::test_env_guard();
    let db = TestDb::new();
    for removed in [false, true] {
        db.add_account("a");
        update_task("openai_account", "a", true, None).unwrap();
        let run = scheduler::claim_due().unwrap().pop().unwrap();
        if removed {
            db.storage().delete_account("a").unwrap();
        } else {
            db.storage().update_account_status("a", "disabled").unwrap();
        }
        scheduler::complete_with(
            &run,
            Ok(Fetched::default()),
            |_, _| panic!("unavailable source must not write"),
            || Ok(()),
        )
        .unwrap();
        drop(run);
        let task = list_tasks().unwrap().items.remove(0);
        assert!(!task.enabled);
        assert!(task.next_run_at.is_none());
        assert!(task.last_error.is_some());
        assert!(scheduler::claim_due().unwrap().is_empty());
        assert!(update_task("openai_account", "a", true, None).is_err());
    }
}

#[test]
fn successful_import_records_counts_and_profile_failure_is_only_a_warning() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    scheduler::complete_with(
        &run,
        Ok(Fetched {
            models: vec!["new-model".into()],
            ..Default::default()
        }),
        |_, _| Ok(associated()),
        || Err("sensitive local path".into()),
    )
    .unwrap();
    drop(run);
    let task = list_tasks().unwrap().items.remove(0);
    assert_eq!(task.discovered_count, 1);
    assert_eq!(task.created_model_count, 1);
    assert_eq!(task.added_route_count, 1);
    assert!(task.last_success_at.is_some());
    assert!(task.last_error.is_none());
    assert!(task.last_warning.is_some());
    assert!(!task.last_warning.unwrap().contains("sensitive"));
    assert!(task.next_run_at.unwrap() >= now_ts() + 3595);
    update_task("openai_account", "a", false, None).unwrap();
}

#[test]
fn disable_waits_for_admitted_write_and_no_write_can_begin_after_return() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let completed = Arc::new(AtomicBool::new(false));
    let write_completed = completed.clone();
    let worker = std::thread::spawn(move || {
        scheduler::complete_with(
            &run,
            Ok(Fetched::default()),
            |_, _| {
                entered.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                write_completed.store(true, Ordering::Release);
                Ok(associated())
            },
            || Ok(()),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (disabled, disabled_rx) = std::sync::mpsc::channel();
    let updater = std::thread::spawn(move || {
        let task = update_task("openai_account", "a", false, None).unwrap();
        assert!(completed.load(Ordering::Acquire));
        disabled.send(task).unwrap();
    });
    assert!(disabled_rx.recv_timeout(Duration::from_millis(30)).is_err());
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert!(
        !disabled_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .enabled
    );
    updater.join().unwrap();
    assert!(scheduler::claim_due().unwrap().is_empty());
}

#[tokio::test]
async fn cancelling_discovery_drops_pending_network_future() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let flag = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let marker = Dropped(dropped.clone());
    let cancel_flag = flag.clone();
    let cancel = async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel_flag.store(true, Ordering::Release);
    };
    let pending = async move {
        let _marker = marker;
        std::future::pending::<Result<Fetched, String>>().await
    };
    let (result, _) = tokio::join!(scheduler::fetch_until_cancelled(&flag, pending), cancel);
    assert!(result.is_none());
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn unsaved_completion_cools_down_instead_of_repeating_upstream_requests() {
    let _env = crate::test_env_guard();
    let db = TestDb::new();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    let record = store::load().unwrap().remove(0);
    // A storage read/write failure must not cause a just-ended run to be reclaimed.
    let key = format!("model_discovery.config.{}", source("a").key());
    db.storage()
        .save_app_settings(&[(key, Some("malformed".into()))], now_ts())
        .unwrap();
    assert!(scheduler::complete_with(
        &run,
        Ok(Fetched::default()),
        |_, _| panic!("unreadable state cannot write"),
        || Ok(())
    )
    .is_err());
    drop(run);
    store::save(&record, true).unwrap();
    let task = list_tasks().unwrap().items.remove(0);
    assert!(!task.running);
    assert!(task.next_run_at.unwrap() >= now_ts() + 295);
    assert!(scheduler::claim_due().unwrap().is_empty());
    scheduler::control()
        .cooldowns
        .insert(source("a").key(), now_ts() - 1);
    let retry = scheduler::claim_due().unwrap();
    assert_eq!(retry.len(), 1);
    update_task("openai_account", "a", false, None).unwrap();
    drop(retry);
}

#[test]
fn failed_attempt_resets_old_counts_but_association_failure_keeps_discovered_count() {
    let _env = crate::test_env_guard();
    let _db = TestDb::new();
    update_task("openai_account", "a", true, None).unwrap();
    let mut record = store::load().unwrap().remove(0);
    record.status.discovered_count = 8;
    record.status.created_model_count = 7;
    record.status.added_route_count = 6;
    store::save(&record, false).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    scheduler::complete_with(
        &run,
        Err("fetch failed".into()),
        |_, _| panic!("failed fetch"),
        || Ok(()),
    )
    .unwrap();
    drop(run);
    let task = list_tasks().unwrap().items.remove(0);
    assert_eq!(
        (
            task.discovered_count,
            task.created_model_count,
            task.added_route_count
        ),
        (0, 0, 0)
    );
    update_task("openai_account", "a", false, None).unwrap();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    scheduler::complete_with(
        &run,
        Ok(Fetched {
            models: vec!["m".into()],
            ..Default::default()
        }),
        |_, _| Err("insert failed".into()),
        || Ok(()),
    )
    .unwrap();
    drop(run);
    let task = list_tasks().unwrap().items.remove(0);
    assert_eq!(
        (
            task.discovered_count,
            task.created_model_count,
            task.added_route_count
        ),
        (1, 0, 0)
    );
    update_task("openai_account", "a", false, None).unwrap();
}

#[test]
fn scheduler_completion_imports_real_account_routes_without_overwriting_manual_changes() {
    let _env = crate::test_env_guard();
    let db = TestDb::new();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    scheduler::complete_with(
        &run,
        Ok(Fetched {
            models: vec!["discovery-new-model".into()],
            ..Default::default()
        }),
        |source, fetched| {
            crate::account::models::associate_account_models(
                &source.source_id,
                fetched.models,
                fetched.names,
            )
        },
        || Ok(()),
    )
    .unwrap();
    drop(run);
    let mut model = db
        .storage()
        .list_managed_models_v2(true)
        .unwrap()
        .into_iter()
        .find(|m| m.slug == "discovery-new-model")
        .unwrap();
    assert_eq!(model.routes[0].source_kind, "account_pool");
    model.display_name = "Manual label".into();
    model.routes[0].enabled = false;
    db.storage()
        .upsert_managed_models_v2(&[codexmanager_core::storage::ManagedModelV2Upsert {
            model,
            ..Default::default()
        }])
        .unwrap();
    update_task("openai_account", "a", false, None).unwrap();
    update_task("openai_account", "a", true, None).unwrap();
    let run = scheduler::claim_due().unwrap().pop().unwrap();
    scheduler::complete_with(
        &run,
        Ok(Fetched {
            models: vec!["discovery-new-model".into()],
            ..Default::default()
        }),
        |source, fetched| {
            crate::account::models::associate_account_models(
                &source.source_id,
                fetched.models,
                fetched.names,
            )
        },
        || panic!("unchanged catalog should not sync profile"),
    )
    .unwrap();
    drop(run);
    let model = db
        .storage()
        .list_managed_models_v2(true)
        .unwrap()
        .into_iter()
        .find(|m| m.slug == "discovery-new-model")
        .unwrap();
    assert_eq!(model.display_name, "Manual label");
    assert!(!model.routes[0].enabled);
    assert_eq!(model.routes.len(), 1);
    let task = list_tasks().unwrap().items.remove(0);
    assert_eq!(
        (
            task.discovered_count,
            task.created_model_count,
            task.added_route_count
        ),
        (1, 0, 0)
    );
    update_task("openai_account", "a", false, None).unwrap();
}

#[test]
fn scheduler_limits_global_concurrency_and_disables_missing_sources_before_fetch() {
    let _env = crate::test_env_guard();
    let db = TestDb::new();
    for id in ["a", "b", "c"] {
        db.add_account(id);
        update_task("openai_account", id, true, None).unwrap();
    }
    db.storage().delete_account("c").unwrap();
    let runs = scheduler::claim_due().unwrap();
    assert_eq!(runs.len(), 2);
    assert!(scheduler::claim_due().unwrap().is_empty());
    let deleted = list_tasks()
        .unwrap()
        .items
        .into_iter()
        .find(|t| t.source_id == "c")
        .unwrap();
    assert!(!deleted.enabled);
    assert!(!deleted.running);
    for id in ["a", "b"] {
        update_task("openai_account", id, false, None).unwrap();
    }
    drop(runs);
}
