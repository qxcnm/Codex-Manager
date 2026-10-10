//! Database-side usage aggregation over raw records and retained hourly totals.
use codexmanager_core::storage::{
    DailyTokenUsageRollup, ModelTokenUsageRollup, SourceTokenUsageRollup, TokenUsageRollup,
    UserTokenUsageRollup,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, QueryResult, Statement, Value};

pub struct UsageAnalyticsRepository;
pub(crate) const OWNER: &str = "COALESCE((SELECT MIN(NULLIF(TRIM(w.owner_id),'')) FROM app_wallet_ledger_entries l JOIN app_wallets w ON w.id=l.wallet_id WHERE l.request_log_id=t.request_log_id AND l.entry_kind='request_charge' AND w.owner_kind='user'),NULLIF(TRIM(owner.owner_user_id),''),NULLIF(TRIM(stat_owner.owner_user_id),''))";
pub(crate) const OWNER_JOINS: &str = "LEFT JOIN api_key_owners owner ON owner.key_id=r.key_id AND owner.owner_kind='user' LEFT JOIN api_key_owners stat_owner ON stat_owner.key_id=t.key_id AND stat_owner.owner_kind='user'";
const TOKEN_FIELDS: [&str; 4] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
];
const COUNTERS: [&str; 8] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
    "request_count",
    "success_count",
    "error_count",
];

#[derive(Clone, Copy)]
enum Dimension {
    Daily(i64),
    Models(i64),
    Users,
    Source(&'static str),
}

fn source_id(kind: &str, hourly: bool) -> &'static str {
    match (kind,hourly) {
        ("openai_account",false) => "CASE WHEN t.actual_source_kind='openai_account' THEN COALESCE(NULLIF(TRIM(t.actual_source_id),''),NULLIF(TRIM(t.account_id),'')) WHEN r.actual_source_kind='openai_account' THEN COALESCE(NULLIF(TRIM(r.actual_source_id),''),NULLIF(TRIM(r.account_id),''),NULLIF(TRIM(t.account_id),'')) WHEN COALESCE(TRIM(t.actual_source_kind),'')='' AND COALESCE(TRIM(r.actual_source_kind),'')='' THEN COALESCE(NULLIF(TRIM(r.account_id),''),NULLIF(TRIM(t.account_id),'')) ELSE NULL END",
        ("aggregate_api",false) => "CASE WHEN t.actual_source_kind='aggregate_api' THEN NULLIF(TRIM(t.actual_source_id),'') WHEN r.actual_source_kind='aggregate_api' THEN COALESCE(NULLIF(TRIM(r.actual_source_id),''),NULLIF(TRIM(r.initial_aggregate_api_id),'')) WHEN COALESCE(TRIM(t.actual_source_kind),'')='' AND COALESCE(TRIM(r.actual_source_kind),'')='' THEN NULLIF(TRIM(r.initial_aggregate_api_id),'') ELSE NULL END",
        ("openai_account",true) => "CASE WHEN h.actual_source_kind='openai_account' THEN COALESCE(NULLIF(TRIM(h.actual_source_id),''),NULLIF(TRIM(h.account_id),'')) WHEN COALESCE(TRIM(h.actual_source_kind),'')='' THEN NULLIF(TRIM(h.account_id),'') ELSE NULL END",
        ("aggregate_api",true) => "CASE WHEN h.actual_source_kind='aggregate_api' THEN NULLIF(TRIM(h.actual_source_id),'') ELSE NULL END",
        _ => "NULL",
    }
}

async fn aggregate(
    db: &impl ConnectionTrait,
    start: i64,
    end: i64,
    dimension: Dimension,
    user: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<QueryResult>, DbErr> {
    if end <= start || limit == Some(0) || user.is_some_and(|id| id.trim().is_empty()) {
        return Ok(Vec::new());
    }
    let backend = db.get_database_backend();
    let integer = if backend == DatabaseBackend::MySql {
        "SIGNED"
    } else {
        "BIGINT"
    };
    let float = if backend == DatabaseBackend::Postgres {
        "DOUBLE PRECISION"
    } else {
        "DOUBLE"
    };
    let owner_needed = matches!(dimension, Dimension::Users) || user.is_some();
    let raw_owner = if owner_needed { OWNER } else { "NULL" };
    let joins = if owner_needed { OWNER_JOINS } else { "" };
    let (raw_source, hourly_source) = if let Dimension::Source(kind) = dimension {
        (source_id(kind, false), source_id(kind, true))
    } else {
        ("NULL", "NULL")
    };
    let raw_fields = TOKEN_FIELDS
        .iter()
        .map(|field| {
            format!(
                "CASE WHEN t.usage_included=TRUE THEN COALESCE(t.{field},0) ELSE 0 END AS {field}"
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let fallback =
        "COALESCE(t.input_tokens,0)-COALESCE(t.cached_input_tokens,0)+COALESCE(t.output_tokens,0)";
    let token_value=format!("CASE WHEN t.usage_included<>TRUE THEN 0 WHEN t.total_tokens IS NOT NULL THEN CASE WHEN t.total_tokens>0 THEN t.total_tokens ELSE 0 END WHEN {fallback}>0 THEN {fallback} ELSE 0 END");
    let hourly_fields = TOKEN_FIELDS
        .iter()
        .map(|field| format!("h.{field}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut values: Vec<Value> = Vec::new();
    let mut param = |value: Value| {
        values.push(value);
        if backend == DatabaseBackend::Postgres {
            format!("${}", values.len())
        } else {
            "?".to_owned()
        }
    };
    let raw_start = param(start.into());
    let raw_end = param(end.into());
    let hourly_start = param(start.into());
    let hourly_end = param(end.into());
    let raw=format!("SELECT t.created_at AS event_ts,COALESCE(NULLIF(TRIM(t.model),''),'unknown') AS model,{raw_owner} AS user_id,{raw_source} AS source_id,{raw_fields},{token_value} AS total_tokens,CASE WHEN t.usage_included=TRUE THEN COALESCE(t.estimated_cost_usd,0) ELSE 0 END AS estimated_cost_usd,1 AS request_count,CASE WHEN r.status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END AS success_count,CASE WHEN r.status_code>=400 OR TRIM(COALESCE(r.error,''))<>'' THEN 1 ELSE 0 END AS error_count FROM request_token_stats t LEFT JOIN request_logs r ON r.id=t.request_log_id {joins} WHERE t.created_at>={raw_start} AND t.created_at<{raw_end}");
    let hourly=format!("SELECT h.bucket_start AS event_ts,COALESCE(NULLIF(TRIM(h.model),''),'unknown') AS model,NULLIF(TRIM(h.owner_user_id),'') AS user_id,{hourly_source} AS source_id,{hourly_fields},h.total_tokens,h.estimated_cost_usd,h.request_count,h.success_count,h.error_count FROM request_token_stat_hourly_rollups h WHERE h.bucket_start>={hourly_start} AND h.bucket_end<={hourly_end}");
    let mut predicates = Vec::new();
    if let Some(user) = user {
        predicates.push(format!("user_id={}", param(user.trim().into())));
    }
    let (dimension_fields, group, order) = match dimension {
        Dimension::Daily(bucket) | Dimension::Models(bucket) => {
            let bucket = bucket.max(1);
            let div = if backend == DatabaseBackend::MySql {
                format!("((event_ts - {start}) DIV {bucket})")
            } else {
                format!("CAST((event_ts - {start}) / {bucket} AS BIGINT)")
            };
            let bucket_expr = format!("CAST({start}+{div}*{bucket} AS {integer}) AS bucket_start");
            if matches!(dimension, Dimension::Models(_)) {
                (
                    format!("{bucket_expr},model"),
                    "bucket_start,model",
                    "bucket_start,model",
                )
            } else {
                (bucket_expr, "bucket_start", "bucket_start")
            }
        }
        Dimension::Users => {
            predicates.push("user_id IS NOT NULL".into());
            ("user_id".into(), "user_id", "total_tokens DESC,user_id")
        }
        Dimension::Source(_) => {
            predicates.push("source_id IS NOT NULL".into());
            (
                "source_id".into(),
                "source_id",
                "total_tokens DESC,source_id",
            )
        }
    };
    let sums = COUNTERS
        .iter()
        .map(|field| format!("CAST(COALESCE(SUM({field}),0) AS {integer}) AS {field}"))
        .collect::<Vec<_>>()
        .join(",");
    let condition = if predicates.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", predicates.join(" AND "))
    };
    let limit = limit
        .map(|value| format!(" LIMIT {}", value.min(i64::MAX as usize)))
        .unwrap_or_default();
    let sql=format!("SELECT {dimension_fields},{sums},CAST(COALESCE(SUM(estimated_cost_usd),0) AS {float}) AS estimated_cost_usd FROM ({raw} UNION ALL {hourly}) events {condition} GROUP BY {group} ORDER BY {order}{limit}");
    db.query_all(Statement::from_sql_and_values(backend, sql, values))
        .await
}
fn usage(row: &QueryResult) -> Result<TokenUsageRollup, DbErr> {
    Ok(TokenUsageRollup {
        input_tokens: row.try_get::<i64>("", "input_tokens")?.max(0),
        cached_input_tokens: row.try_get::<i64>("", "cached_input_tokens")?.max(0),
        output_tokens: row.try_get::<i64>("", "output_tokens")?.max(0),
        reasoning_output_tokens: row.try_get::<i64>("", "reasoning_output_tokens")?.max(0),
        total_tokens: row.try_get::<i64>("", "total_tokens")?.max(0),
        estimated_cost_usd: row.try_get::<f64>("", "estimated_cost_usd")?.max(0.0),
        request_count: row.try_get::<i64>("", "request_count")?.max(0),
        success_count: row.try_get::<i64>("", "success_count")?.max(0),
        error_count: row.try_get::<i64>("", "error_count")?.max(0),
    })
}
impl UsageAnalyticsRepository {
    pub async fn daily(
        db: &impl ConnectionTrait,
        start: i64,
        end: i64,
        bucket: i64,
        user: Option<&str>,
    ) -> Result<Vec<DailyTokenUsageRollup>, DbErr> {
        aggregate(db, start, end, Dimension::Daily(bucket), user, None)
            .await?
            .iter()
            .map(|row| {
                let begin = row.try_get::<i64>("", "bucket_start")?;
                Ok(DailyTokenUsageRollup {
                    day_start_ts: begin,
                    day_end_ts: begin.saturating_add(bucket.max(1)).min(end),
                    usage: usage(row)?,
                })
            })
            .collect()
    }
    pub async fn models(
        db: &impl ConnectionTrait,
        start: i64,
        end: i64,
        bucket: i64,
    ) -> Result<Vec<ModelTokenUsageRollup>, DbErr> {
        aggregate(db, start, end, Dimension::Models(bucket), None, None)
            .await?
            .iter()
            .map(|row| {
                let begin = row.try_get::<i64>("", "bucket_start")?;
                Ok(ModelTokenUsageRollup {
                    bucket_start_ts: begin,
                    bucket_end_ts: begin.saturating_add(bucket.max(1)).min(end),
                    model: row.try_get("", "model")?,
                    usage: usage(row)?,
                })
            })
            .collect()
    }
    pub async fn users(
        db: &impl ConnectionTrait,
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<Vec<UserTokenUsageRollup>, DbErr> {
        aggregate(db, start, end, Dimension::Users, None, limit)
            .await?
            .iter()
            .map(|row| {
                Ok(UserTokenUsageRollup {
                    user_id: row.try_get("", "user_id")?,
                    usage: usage(row)?,
                })
            })
            .collect()
    }
    pub async fn sources(
        db: &impl ConnectionTrait,
        kinds: &[String],
        start: i64,
        end: i64,
        limit: Option<usize>,
    ) -> Result<Vec<SourceTokenUsageRollup>, DbErr> {
        let mut result = Vec::new();
        for kind in ["openai_account", "aggregate_api"] {
            if kinds.iter().any(|candidate| candidate.trim() == kind) {
                for row in aggregate(db, start, end, Dimension::Source(kind), None, limit).await? {
                    result.push(SourceTokenUsageRollup {
                        source_kind: kind.into(),
                        source_id: row.try_get("", "source_id")?,
                        usage: usage(&row)?,
                    });
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BillingRepository, RequestLogsRepository, RequestTokenStatRecord,
        RequestTokenStatsRepository, SeaOrmStorage, UsersRepository,
    };
    use codexmanager_core::storage::{
        ApiKeyOwner, AppWallet, AppWalletLedgerEntry, RequestLog, StorageBackendKind,
    };
    use sea_orm::{ActiveModelTrait, Set, TransactionTrait};

    async fn verify(storage: SeaOrmStorage) {
        storage.migrate().await.unwrap();
        let tx = storage.connection().begin().await.unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        let key = format!("analytics-{stamp}");
        let former = format!("former-{stamp}");
        let current = format!("current-{stamp}");
        for id in [&former, &current] {
            UsersRepository::put(
                &tx,
                codexmanager_core::storage::AppUser {
                    id: id.clone(),
                    username: id.clone(),
                    display_name: None,
                    password_hash: "fixture-only".into(),
                    role: "member".into(),
                    status: "active".into(),
                    created_at: 100,
                    updated_at: 100,
                    last_login_at: None,
                },
            )
            .await
            .unwrap();
        }
        let log = RequestLog {
            key_id: Some(key.clone()),
            account_id: Some("account-a".into()),
            model: Some("model-a".into()),
            status_code: Some(200),
            created_at: 100,
            ..Default::default()
        };
        RequestLogsRepository::insert(&tx, stamp, log)
            .await
            .unwrap();
        RequestTokenStatsRepository::upsert(
            &tx,
            RequestTokenStatRecord {
                request_log_id: stamp,
                key_id: Some(key.clone()),
                account_id: Some("account-a".into()),
                model: Some("model-a".into()),
                actual_source_kind: None,
                actual_source_id: None,
                input_tokens: Some(10),
                cached_input_tokens: Some(3),
                output_tokens: Some(2),
                total_tokens: None,
                reasoning_output_tokens: Some(1),
                estimated_cost_usd: Some(0.25),
                usage_included: true,
                created_at: 100,
            },
        )
        .await
        .unwrap();
        UsersRepository::put_owner(
            &tx,
            ApiKeyOwner {
                key_id: key.clone(),
                owner_kind: "user".into(),
                owner_user_id: Some(current.clone()),
                project_id: None,
                updated_at: 100,
            },
        )
        .await
        .unwrap();
        BillingRepository::create_wallet(
            &tx,
            AppWallet {
                id: former.clone(),
                owner_kind: "user".into(),
                owner_id: former.clone(),
                balance_credit_micros: 10,
                frozen_credit_micros: 0,
                status: "active".into(),
                created_at: 100,
                updated_at: 100,
            },
        )
        .await
        .unwrap();
        let entry = AppWalletLedgerEntry {
            id: format!("ledger-{stamp}"),
            wallet_id: former.clone(),
            entry_kind: "request_charge".into(),
            amount_credit_micros: -1,
            balance_after_credit_micros: 10,
            request_log_id: Some(stamp),
            api_key_id: Some(key.clone()),
            pricing_rule_id: None,
            raw_usage_json: None,
            note: None,
            created_by_user_id: None,
            created_at: 100,
        };
        let ledger: crate::billing::ledger::ActiveModel = entry.into();
        ledger.insert(&tx).await.unwrap();
        crate::api_key_rollups::hourly::ActiveModel {
            bucket_start: Set(0),
            bucket_end: Set(3600),
            key_id: Set(key.clone()),
            account_id: Set("account-a".into()),
            model: Set("model-a".into()),
            actual_source_kind: Set("openai_account".into()),
            actual_source_id: Set("account-a".into()),
            owner_user_id: Set(former.clone()),
            input_tokens: Set(20),
            cached_input_tokens: Set(5),
            output_tokens: Set(4),
            total_tokens: Set(19),
            reasoning_output_tokens: Set(2),
            estimated_cost_usd: Set(0.5),
            request_count: Set(2),
            success_count: Set(1),
            error_count: Set(1),
            updated_at: Set(100),
        }
        .insert(&tx)
        .await
        .unwrap();
        let daily = UsageAnalyticsRepository::daily(&tx, 0, 3600, 3600, Some(&former))
            .await
            .unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].usage.total_tokens, 28);
        assert_eq!(daily[0].usage.request_count, 3);
        assert_eq!(daily[0].usage.error_count, 1);
        assert!(
            UsageAnalyticsRepository::daily(&tx, 0, 3600, 3600, Some(&current))
                .await
                .unwrap()
                .is_empty(),
            "historical billed owner must override a reassigned API key"
        );
        assert_eq!(
            UsageAnalyticsRepository::daily(&tx, 0, 3599, 3600, Some(&former))
                .await
                .unwrap()[0]
                .usage
                .total_tokens,
            9,
            "partial hourly bucket is excluded"
        );
        let models = UsageAnalyticsRepository::models(&tx, 0, 3600, 3600)
            .await
            .unwrap();
        assert!(models
            .iter()
            .any(|row| row.model == "model-a" && row.usage.total_tokens >= 28));
        let users = UsageAnalyticsRepository::users(&tx, 0, 3600, None)
            .await
            .unwrap();
        assert_eq!(
            users
                .iter()
                .find(|row| row.user_id == former)
                .unwrap()
                .usage
                .total_tokens,
            28
        );
        let sources =
            UsageAnalyticsRepository::sources(&tx, &["openai_account".into()], 0, 3600, None)
                .await
                .unwrap();
        assert!(sources
            .iter()
            .any(|row| row.source_id == "account-a" && row.usage.total_tokens >= 28));
        let scope = crate::RequestLogFilter {
            key_ids: Some(vec![key.clone()]),
            ..Default::default()
        };
        assert_eq!(
            RequestLogsRepository::summarize_filtered(&tx, &scope)
                .await
                .unwrap()
                .total_tokens,
            9
        );
        assert_eq!(
            RequestLogsRepository::list_filtered(&tx, &scope, 0, 10)
                .await
                .unwrap()[0]
                .input_tokens,
            Some(10)
        );
        assert_eq!(
            crate::ApiKeyDetailsRepository::today_summary(&tx, Some(&[key]), 0, 3600)
                .await
                .unwrap()
                .input_tokens,
            30
        );
        // Compaction and pruning share the same lock/transaction as ID allocation.
        // Repeating maintenance must not double bill or duplicate hourly counts.
        UsersRepository::lock(&tx, "request_logs").await.unwrap();
        crate::request_log_retention::compact(&tx, 3600, 7200)
            .await
            .unwrap();
        crate::request_log_retention::prune(&tx, 3600, 7200)
            .await
            .unwrap();
        crate::request_log_retention::compact(&tx, 3600, 7201)
            .await
            .unwrap();
        crate::request_log_retention::prune(&tx, 3600, 7201)
            .await
            .unwrap();
        assert!(RequestTokenStatsRepository::get(&tx, stamp)
            .await
            .unwrap()
            .is_none());
        assert!(
            RequestLogsRepository::get(&tx, stamp)
                .await
                .unwrap()
                .is_some(),
            "ledger-linked log must remain immutable"
        );
        assert_eq!(
            RequestLogsRepository::count_filtered(&tx, &scope)
                .await
                .unwrap(),
            0,
            "billed log is hidden after retention"
        );
        let retained = UsageAnalyticsRepository::daily(&tx, 0, 3600, 3600, Some(&former))
            .await
            .unwrap();
        assert_eq!(retained[0].usage.total_tokens, 28);
        assert_eq!(retained[0].usage.request_count, 3);
        assert_eq!(retained[0].usage.error_count, 1);
        tx.rollback().await.unwrap();
    }
    #[tokio::test]
    async fn sqlite_usage_keeps_historical_owner_and_hourly_boundaries() {
        verify(
            SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
                .await
                .unwrap(),
        )
        .await;
    }
    #[cfg(feature = "mysql")]
    #[tokio::test]
    #[ignore = "requires isolated MySQL URL"]
    async fn mysql_usage_keeps_historical_owner_and_hourly_boundaries() {
        verify(
            SeaOrmStorage::connect(
                StorageBackendKind::Mysql,
                &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
    }
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore = "requires isolated PostgreSQL URL"]
    async fn postgres_usage_keeps_historical_owner_and_hourly_boundaries() {
        verify(
            SeaOrmStorage::connect(
                StorageBackendKind::Postgres,
                &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
    }
}
