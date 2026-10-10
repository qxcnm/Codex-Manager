use super::*;

pub(crate) async fn exercise(db: &DatabaseConnection, suffix: &str) {
    let plugin = PluginInstall {
        plugin_id: format!("plugin-{suffix}"),
        source_url: None,
        name: "Fixture".into(),
        version: "1".into(),
        description: None,
        author: None,
        homepage_url: None,
        script_url: None,
        script_body: "return {}".into(),
        permissions_json: "[]".into(),
        manifest_json: r#"{"manifestVersion":"1","tags":["test"]}"#.into(),
        status: "enabled".into(),
        installed_at: 1,
        updated_at: 2,
        last_run_at: None,
        last_error: None,
    };
    let task = PluginTask {
        id: format!("task-{suffix}"),
        plugin_id: plugin.plugin_id.clone(),
        name: "Fixture task".into(),
        description: None,
        entrypoint: "run".into(),
        schedule_kind: "interval".into(),
        interval_seconds: Some(10),
        enabled: true,
        next_run_at: None,
        last_run_at: Some(10),
        last_status: None,
        last_error: None,
        task_json: "{}".into(),
        created_at: 1,
        updated_at: 1,
    };
    PluginsRepository::replace_plugin_install(db, &plugin, &[task.clone()])
        .await
        .unwrap();
    let mut invalid = task.clone();
    invalid.plugin_id = "another-plugin".into();
    assert!(
        PluginsRepository::replace_plugin_install(db, &plugin, &[invalid])
            .await
            .is_err()
    );
    assert!(
        PluginsRepository::find_plugin_task(db, &task.id)
            .await
            .unwrap()
            .is_some(),
        "failed replacement must retain existing tasks"
    );
    assert_eq!(
        PluginsRepository::repair_plugin_task_schedules(db, Some(&plugin.plugin_id), 15)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        PluginsRepository::find_plugin_task(db, &task.id)
            .await
            .unwrap()
            .unwrap()
            .next_run_at,
        Some(20)
    );
    assert!(!PluginsRepository::list_due_plugin_tasks(db, 19, 1000)
        .await
        .unwrap()
        .iter()
        .any(|r| r.id == task.id));
    assert!(PluginsRepository::list_due_plugin_tasks(db, 20, 1000)
        .await
        .unwrap()
        .iter()
        .any(|r| r.id == task.id));
    PluginsRepository::update_plugin_install_status(db, &plugin.plugin_id, "disabled", None)
        .await
        .unwrap();
    assert!(!PluginsRepository::list_due_plugin_tasks(db, 20, 1000)
        .await
        .unwrap()
        .iter()
        .any(|r| r.id == task.id));
    PluginsRepository::insert_plugin_run_log(
        db,
        &PluginRunLog {
            id: None,
            plugin_id: plugin.plugin_id.clone(),
            task_id: Some(task.id.clone()),
            run_type: "manual".into(),
            status: "ok".into(),
            started_at: 30,
            finished_at: Some(31),
            duration_ms: Some(1000),
            output_json: Some("{}".into()),
            error: None,
        },
    )
    .await
    .unwrap();
    let logs =
        PluginsRepository::list_plugin_run_log_summaries(db, Some(&plugin.plugin_id), None, 10)
            .await
            .unwrap();
    assert_eq!(logs[0].plugin_name.as_deref(), Some("Fixture"));
    assert_eq!(logs[0].task_name.as_deref(), Some("Fixture task"));
    PluginsRepository::delete_plugin_install(db, &plugin.plugin_id)
        .await
        .unwrap();
    assert!(PluginsRepository::find_plugin_task(db, &task.id)
        .await
        .unwrap()
        .is_none());
    let logs =
        PluginsRepository::list_plugin_run_log_summaries(db, Some(&plugin.plugin_id), None, 10)
            .await
            .unwrap();
    assert_eq!(logs.len(), 1, "uninstall keeps execution audit");
    assert!(logs[0].plugin_name.is_none());
}
#[tokio::test]
async fn sqlite_plugin_replacement_schedule_and_audit() {
    let s = crate::SeaOrmStorage::connect(
        codexmanager_core::storage::StorageBackendKind::Sqlite,
        "sqlite::memory:",
    )
    .await
    .unwrap();
    s.migrate().await.unwrap();
    exercise(s.connection(), "sqlite").await;
}
