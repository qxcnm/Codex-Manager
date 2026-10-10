//! Plugin persistence for service databases.
use codexmanager_core::storage::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use std::collections::HashMap;
pub struct PluginsRepository;
pub(crate) mod installs {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "plugin_installs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub plugin_id: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub source_url: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        #[sea_orm(column_type = "Text")]
        pub version: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub description: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub author: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub homepage_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub script_url: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub script_body: String,
        #[sea_orm(column_type = "Text")]
        pub permissions_json: String,
        #[sea_orm(column_type = "Text")]
        pub manifest_json: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub installed_at: i64,
        pub updated_at: i64,
        pub last_run_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<installs::Model> for PluginInstall {
    fn from(r: installs::Model) -> Self {
        Self {
            plugin_id: r.plugin_id,
            source_url: r.source_url,
            name: r.name,
            version: r.version,
            description: r.description,
            author: r.author,
            homepage_url: r.homepage_url,
            script_url: r.script_url,
            script_body: r.script_body,
            permissions_json: r.permissions_json,
            manifest_json: r.manifest_json,
            status: r.status,
            installed_at: r.installed_at,
            updated_at: r.updated_at,
            last_run_at: r.last_run_at,
            last_error: r.last_error,
        }
    }
}
fn installs_active(r: &PluginInstall) -> installs::ActiveModel {
    installs::ActiveModel {
        plugin_id: Set(r.plugin_id.clone()),
        source_url: Set(r.source_url.clone()),
        name: Set(r.name.clone()),
        version: Set(r.version.clone()),
        description: Set(r.description.clone()),
        author: Set(r.author.clone()),
        homepage_url: Set(r.homepage_url.clone()),
        script_url: Set(r.script_url.clone()),
        script_body: Set(r.script_body.clone()),
        permissions_json: Set(r.permissions_json.clone()),
        manifest_json: Set(r.manifest_json.clone()),
        status: Set(r.status.clone()),
        installed_at: Set(r.installed_at.clone()),
        updated_at: Set(r.updated_at.clone()),
        last_run_at: Set(r.last_run_at.clone()),
        last_error: Set(r.last_error.clone()),
    }
}
pub(crate) mod tasks {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "plugin_tasks")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub plugin_id: String,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub description: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub entrypoint: String,
        #[sea_orm(column_type = "Text")]
        pub schedule_kind: String,
        pub interval_seconds: Option<i64>,
        pub enabled: bool,
        pub next_run_at: Option<i64>,
        pub last_run_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_status: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub task_json: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<tasks::Model> for PluginTask {
    fn from(r: tasks::Model) -> Self {
        Self {
            id: r.id,
            plugin_id: r.plugin_id,
            name: r.name,
            description: r.description,
            entrypoint: r.entrypoint,
            schedule_kind: r.schedule_kind,
            interval_seconds: r.interval_seconds,
            enabled: r.enabled,
            next_run_at: r.next_run_at,
            last_run_at: r.last_run_at,
            last_status: r.last_status,
            last_error: r.last_error,
            task_json: r.task_json,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
fn tasks_active(r: &PluginTask) -> tasks::ActiveModel {
    tasks::ActiveModel {
        id: Set(r.id.clone()),
        plugin_id: Set(r.plugin_id.clone()),
        name: Set(r.name.clone()),
        description: Set(r.description.clone()),
        entrypoint: Set(r.entrypoint.clone()),
        schedule_kind: Set(r.schedule_kind.clone()),
        interval_seconds: Set(r.interval_seconds.clone()),
        enabled: Set(r.enabled.clone()),
        next_run_at: Set(r.next_run_at.clone()),
        last_run_at: Set(r.last_run_at.clone()),
        last_status: Set(r.last_status.clone()),
        last_error: Set(r.last_error.clone()),
        task_json: Set(r.task_json.clone()),
        created_at: Set(r.created_at.clone()),
        updated_at: Set(r.updated_at.clone()),
    }
}
pub(crate) mod logs {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "plugin_run_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_type = "Text")]
        pub plugin_id: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub task_id: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub run_type: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub started_at: i64,
        pub finished_at: Option<i64>,
        pub duration_ms: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub output_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<logs::Model> for PluginRunLog {
    fn from(r: logs::Model) -> Self {
        Self {
            id: Some(r.id),
            plugin_id: r.plugin_id,
            task_id: r.task_id,
            run_type: r.run_type,
            status: r.status,
            started_at: r.started_at,
            finished_at: r.finished_at,
            duration_ms: r.duration_ms,
            output_json: r.output_json,
            error: r.error,
        }
    }
}
fn logs_active(r: &PluginRunLog) -> logs::ActiveModel {
    logs::ActiveModel {
        plugin_id: Set(r.plugin_id.clone()),
        task_id: Set(r.task_id.clone()),
        run_type: Set(r.run_type.clone()),
        status: Set(r.status.clone()),
        started_at: Set(r.started_at.clone()),
        finished_at: Set(r.finished_at.clone()),
        duration_ms: Set(r.duration_ms.clone()),
        output_json: Set(r.output_json.clone()),
        error: Set(r.error.clone()),
        ..Default::default()
    }
}
fn text(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).filter(|v| !v.is_null()).map(|v| {
        v.as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| v.to_string())
    })
}
impl PluginsRepository {
    pub async fn upsert_plugin_install(
        db: &impl ConnectionTrait,
        plugin: &PluginInstall,
    ) -> Result<(), DbErr> {
        installs::Entity::insert(installs_active(plugin))
            .on_conflict(
                OnConflict::column(installs::Column::PluginId)
                    .update_columns([
                        installs::Column::SourceUrl,
                        installs::Column::Name,
                        installs::Column::Version,
                        installs::Column::Description,
                        installs::Column::Author,
                        installs::Column::HomepageUrl,
                        installs::Column::ScriptUrl,
                        installs::Column::ScriptBody,
                        installs::Column::PermissionsJson,
                        installs::Column::ManifestJson,
                        installs::Column::Status,
                        installs::Column::UpdatedAt,
                        installs::Column::LastRunAt,
                        installs::Column::LastError,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn replace_plugin_install(
        db: &DatabaseConnection,
        plugin: &PluginInstall,
        tasks: &[PluginTask],
    ) -> Result<(), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "plugins").await?;
        Self::upsert_plugin_install(&tx, plugin).await?;
        tasks::Entity::delete_many()
            .filter(tasks::Column::PluginId.eq(&plugin.plugin_id))
            .exec(&tx)
            .await?;
        for task in tasks {
            if task.plugin_id != plugin.plugin_id {
                return Err(DbErr::Custom("plugin task owner mismatch".into()));
            }
            tasks::Entity::insert(tasks_active(task)).exec(&tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn list_plugin_installs(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<PluginInstall>, DbErr> {
        Ok(installs::Entity::find()
            .order_by_desc(installs::Column::UpdatedAt)
            .order_by_desc(installs::Column::InstalledAt)
            .order_by_asc(installs::Column::PluginId)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn find_plugin_install(
        db: &impl ConnectionTrait,
        plugin_id: &str,
    ) -> Result<Option<PluginInstall>, DbErr> {
        Ok(installs::Entity::find_by_id(plugin_id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn find_plugin_runtime_install(
        db: &impl ConnectionTrait,
        plugin_id: &str,
    ) -> Result<Option<PluginRuntimeInstall>, DbErr> {
        Ok(Self::find_plugin_install(db, plugin_id)
            .await?
            .map(|r| PluginRuntimeInstall {
                plugin_id: r.plugin_id,
                source_url: r.source_url,
                name: r.name,
                version: r.version,
                script_body: r.script_body,
                permissions_json: r.permissions_json,
                status: r.status,
            }))
    }
    pub async fn list_plugin_install_summaries(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<PluginInstallListSummary>, DbErr> {
        Ok(Self::list_plugin_installs(db)
            .await?
            .into_iter()
            .map(|r| {
                let manifest =
                    serde_json::from_str::<serde_json::Value>(&r.manifest_json).unwrap_or_default();
                PluginInstallListSummary {
                    plugin_id: r.plugin_id,
                    source_url: r.source_url,
                    name: r.name,
                    version: r.version,
                    description: r.description,
                    author: r.author,
                    homepage_url: r.homepage_url,
                    script_url: r.script_url,
                    permissions_json: r.permissions_json,
                    status: r.status,
                    installed_at: r.installed_at,
                    updated_at: r.updated_at,
                    last_run_at: r.last_run_at,
                    last_error: r.last_error,
                    manifest_version: text(&manifest, "manifestVersion")
                        .or_else(|| text(&manifest, "manifest_version")),
                    category: text(&manifest, "category"),
                    runtime_kind: text(&manifest, "runtimeKind")
                        .or_else(|| text(&manifest, "runtime_kind")),
                    tags_json: manifest
                        .get("tags")
                        .filter(|v| !v.is_null())
                        .map(|v| v.to_string()),
                }
            })
            .collect())
    }
    pub async fn plugin_install_names_for_plugins(
        db: &impl ConnectionTrait,
        plugin_ids: &[String],
    ) -> Result<HashMap<String, String>, DbErr> {
        let ids = plugin_ids
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(installs::Entity::find()
            .filter(installs::Column::PluginId.is_in(ids))
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.plugin_id, r.name))
            .collect())
    }
    pub async fn plugin_task_names_for_tasks(
        db: &impl ConnectionTrait,
        task_ids: &[String],
    ) -> Result<HashMap<String, String>, DbErr> {
        let ids = task_ids
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(tasks::Entity::find()
            .filter(tasks::Column::Id.is_in(ids))
            .all(db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.name))
            .collect())
    }
    pub async fn list_plugin_tasks(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
    ) -> Result<Vec<PluginTask>, DbErr> {
        let mut q = tasks::Entity::find();
        if let Some(id) = plugin_id {
            q = q.filter(tasks::Column::PluginId.eq(id));
        }
        let mut rows = q
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect::<Vec<PluginTask>>();
        rows.sort_by_key(|r| (r.next_run_at, r.created_at));
        Ok(rows)
    }
    pub async fn find_plugin_task(
        db: &impl ConnectionTrait,
        task_id: &str,
    ) -> Result<Option<PluginTask>, DbErr> {
        Ok(tasks::Entity::find_by_id(task_id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list_plugin_task_summaries(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
    ) -> Result<Vec<PluginTaskListSummary>, DbErr> {
        let mut values = Vec::new();
        for r in Self::list_plugin_tasks(db, plugin_id).await? {
            let plugin_name = Self::find_plugin_install(db, &r.plugin_id)
                .await?
                .map(|p| p.name)
                .unwrap_or_else(|| r.plugin_id.clone());
            values.push(PluginTaskListSummary {
                id: r.id,
                plugin_id: r.plugin_id,
                plugin_name: plugin_name,
                name: r.name,
                description: r.description,
                entrypoint: r.entrypoint,
                schedule_kind: r.schedule_kind,
                interval_seconds: r.interval_seconds,
                enabled: r.enabled,
                next_run_at: r.next_run_at,
                last_run_at: r.last_run_at,
                last_status: r.last_status,
                last_error: r.last_error,
            });
        }
        Ok(values)
    }
    pub async fn plugin_task_counts_by_plugin(
        db: &impl ConnectionTrait,
    ) -> Result<HashMap<String, PluginTaskCount>, DbErr> {
        let mut counts: HashMap<String, PluginTaskCount> = HashMap::new();
        for r in Self::list_plugin_tasks(db, None).await? {
            let c = counts
                .entry(r.plugin_id.clone())
                .or_insert_with(|| PluginTaskCount {
                    plugin_id: r.plugin_id,
                    ..Default::default()
                });
            c.task_count += 1;
            c.enabled_task_count += i64::from(r.enabled);
        }
        Ok(counts)
    }
    pub async fn delete_plugin_install(
        db: &DatabaseConnection,
        plugin_id: &str,
    ) -> Result<(), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "plugins").await?;
        tasks::Entity::delete_many()
            .filter(tasks::Column::PluginId.eq(plugin_id))
            .exec(&tx)
            .await?;
        installs::Entity::delete_by_id(plugin_id).exec(&tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn list_plugin_tasks_needing_schedule_repair(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
    ) -> Result<Vec<PluginTaskScheduleRepairRow>, DbErr> {
        let mut rows = Self::list_plugin_tasks(db, plugin_id)
            .await?
            .into_iter()
            .filter(|r| {
                r.enabled
                    && r.schedule_kind != "manual"
                    && r.next_run_at.is_none()
                    && r.interval_seconds.is_some_and(|s| s > 0)
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        Ok(rows
            .into_iter()
            .map(|r| PluginTaskScheduleRepairRow {
                id: r.id,
                interval_seconds: r.interval_seconds,
                next_run_at: r.next_run_at,
                last_run_at: r.last_run_at,
                last_status: r.last_status,
                last_error: r.last_error,
            })
            .collect())
    }
    pub async fn repair_plugin_task_schedules(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
        now: i64,
    ) -> Result<usize, DbErr> {
        let mut q = tasks::Entity::update_many()
            .col_expr(
                tasks::Column::NextRunAt,
                sea_orm::sea_query::Func::coalesce([
                    Expr::col(tasks::Column::LastRunAt)
                        .add(Expr::col(tasks::Column::IntervalSeconds)),
                    Expr::value(now),
                ])
                .into(),
            )
            .col_expr(tasks::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(tasks::Column::Enabled.eq(true))
            .filter(tasks::Column::ScheduleKind.ne("manual"))
            .filter(tasks::Column::NextRunAt.is_null())
            .filter(tasks::Column::IntervalSeconds.gt(0));
        if let Some(id) = plugin_id {
            q = q.filter(tasks::Column::PluginId.eq(id));
        }
        Ok(q.exec(db).await?.rows_affected as usize)
    }
    pub async fn update_plugin_install_status(
        db: &impl ConnectionTrait,
        plugin_id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), DbErr> {
        installs::Entity::update_many()
            .col_expr(installs::Column::Status, Expr::value(status.to_owned()))
            .col_expr(
                installs::Column::LastError,
                Expr::value(last_error.map(ToOwned::to_owned)),
            )
            .col_expr(installs::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(installs::Column::PluginId.eq(plugin_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_plugin_install_last_run(
        db: &impl ConnectionTrait,
        plugin_id: &str,
        last_run_at: i64,
        last_error: Option<&str>,
    ) -> Result<(), DbErr> {
        installs::Entity::update_many()
            .col_expr(installs::Column::LastRunAt, Expr::value(Some(last_run_at)))
            .col_expr(
                installs::Column::LastError,
                Expr::value(last_error.map(ToOwned::to_owned)),
            )
            .col_expr(installs::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(installs::Column::PluginId.eq(plugin_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn set_plugin_task_enabled(
        db: &impl ConnectionTrait,
        task_id: &str,
        enabled: bool,
    ) -> Result<(), DbErr> {
        tasks::Entity::update_many()
            .col_expr(tasks::Column::Enabled, Expr::value(enabled))
            .col_expr(tasks::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(tasks::Column::Id.eq(task_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_plugin_task_definition(
        db: &impl ConnectionTrait,
        task_id: &str,
        name: &str,
        description: Option<&str>,
        entrypoint: &str,
        schedule_kind: &str,
        interval_seconds: Option<i64>,
        enabled: bool,
        next_run_at: Option<i64>,
        task_json: &str,
    ) -> Result<(), DbErr> {
        tasks::Entity::update_many()
            .col_expr(tasks::Column::Name, Expr::value(name.to_owned()))
            .col_expr(
                tasks::Column::Description,
                Expr::value(description.map(ToOwned::to_owned)),
            )
            .col_expr(
                tasks::Column::Entrypoint,
                Expr::value(entrypoint.to_owned()),
            )
            .col_expr(
                tasks::Column::ScheduleKind,
                Expr::value(schedule_kind.to_owned()),
            )
            .col_expr(
                tasks::Column::IntervalSeconds,
                Expr::value(interval_seconds),
            )
            .col_expr(tasks::Column::Enabled, Expr::value(enabled))
            .col_expr(tasks::Column::NextRunAt, Expr::value(next_run_at))
            .col_expr(tasks::Column::TaskJson, Expr::value(task_json.to_owned()))
            .col_expr(tasks::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(tasks::Column::Id.eq(task_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_plugin_task_schedule(
        db: &impl ConnectionTrait,
        task_id: &str,
        next_run_at: Option<i64>,
        last_run_at: Option<i64>,
        last_status: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<(), DbErr> {
        tasks::Entity::update_many()
            .col_expr(tasks::Column::NextRunAt, Expr::value(next_run_at))
            .col_expr(tasks::Column::LastRunAt, Expr::value(last_run_at))
            .col_expr(
                tasks::Column::LastStatus,
                Expr::value(last_status.map(ToOwned::to_owned)),
            )
            .col_expr(
                tasks::Column::LastError,
                Expr::value(last_error.map(ToOwned::to_owned)),
            )
            .col_expr(tasks::Column::UpdatedAt, Expr::value(now_ts()))
            .filter(tasks::Column::Id.eq(task_id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn list_due_plugin_tasks(
        db: &impl ConnectionTrait,
        now: i64,
        limit: i64,
    ) -> Result<Vec<PluginTaskExecutionRow>, DbErr> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let enabled = installs::Entity::find()
            .filter(installs::Column::Status.eq("enabled"))
            .all(db)
            .await?
            .into_iter()
            .map(|p| p.plugin_id)
            .collect::<std::collections::HashSet<_>>();
        let mut rows = Self::list_plugin_tasks(db, None)
            .await?
            .into_iter()
            .filter(|r| {
                r.enabled
                    && enabled.contains(&r.plugin_id)
                    && r.schedule_kind != "manual"
                    && r.next_run_at.is_none_or(|t| t <= now)
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|r| (r.next_run_at.unwrap_or(r.created_at), r.created_at));
        rows.truncate(limit as usize);
        Ok(rows
            .into_iter()
            .map(|r| PluginTaskExecutionRow {
                id: r.id,
                plugin_id: r.plugin_id,
                name: r.name,
                description: r.description,
                entrypoint: r.entrypoint,
                schedule_kind: r.schedule_kind,
                interval_seconds: r.interval_seconds,
                enabled: r.enabled,
            })
            .collect())
    }
    pub async fn next_enabled_plugin_task_run_at(
        db: &impl ConnectionTrait,
    ) -> Result<Option<i64>, DbErr> {
        let enabled = installs::Entity::find()
            .filter(installs::Column::Status.eq("enabled"))
            .all(db)
            .await?
            .into_iter()
            .map(|p| p.plugin_id)
            .collect::<std::collections::HashSet<_>>();
        Ok(Self::list_plugin_tasks(db, None)
            .await?
            .into_iter()
            .filter(|r| r.enabled && enabled.contains(&r.plugin_id) && r.schedule_kind != "manual")
            .filter_map(|r| r.next_run_at)
            .min())
    }
    pub async fn insert_plugin_run_log(
        db: &impl ConnectionTrait,
        log: &PluginRunLog,
    ) -> Result<i64, DbErr> {
        Ok(logs::Entity::insert(logs_active(log))
            .exec(db)
            .await?
            .last_insert_id)
    }
    pub async fn list_plugin_run_logs(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
        task_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<PluginRunLog>, DbErr> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let mut q = logs::Entity::find();
        if let Some(id) = plugin_id {
            q = q.filter(logs::Column::PluginId.eq(id));
        }
        if let Some(id) = task_id {
            q = q.filter(logs::Column::TaskId.eq(id));
        }
        Ok(q.order_by_desc(logs::Column::StartedAt)
            .order_by_desc(logs::Column::Id)
            .limit(limit as u64)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn list_plugin_run_log_summaries(
        db: &impl ConnectionTrait,
        plugin_id: Option<&str>,
        task_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<PluginRunLogListSummary>, DbErr> {
        let mut values = Vec::new();
        for r in Self::list_plugin_run_logs(db, plugin_id, task_id, limit).await? {
            let plugin_name = Self::find_plugin_install(db, &r.plugin_id)
                .await?
                .map(|p| p.name);
            let task_name = if let Some(id) = &r.task_id {
                Self::find_plugin_task(db, id).await?.map(|t| t.name)
            } else {
                None
            };
            values.push(PluginRunLogListSummary {
                id: r.id.unwrap_or_default(),
                plugin_id: r.plugin_id,
                plugin_name: plugin_name,
                task_id: r.task_id,
                task_name: task_name,
                run_type: r.run_type,
                status: r.status,
                started_at: r.started_at,
                finished_at: r.finished_at,
                duration_ms: r.duration_ms,
                output_json: r.output_json,
                error: r.error,
            });
        }
        Ok(values)
    }
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
pub(crate) mod tests;
