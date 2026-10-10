//! Parameterized, backend-independent log queries sharing the desktop parser.
use super::*;
use crate::{RequestTokenStatRecord, RequestTokenStatsRepository, UsersRepository};
use codexmanager_core::storage::{
    request_log_query::{parse_request_log_query, RequestLogQuery},
    RequestLogQuerySummary, RequestTokenStat,
};
use sea_orm::{
    DatabaseBackend, DatabaseConnection, FromQueryResult, Statement, TransactionTrait, Value,
};

#[derive(Debug, Clone, Default)]
pub struct RequestLogFilter {
    pub query: Option<String>,
    pub status: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    /// None is administrator scope. Some(empty) must never become unscoped.
    pub key_ids: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiKeyDetailsRepository, SeaOrmStorage};
    use codexmanager_core::storage::StorageBackendKind;

    #[tokio::test]
    async fn atomic_log_queries_preserve_scope_usage_and_clear_history() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        for (key, status, error) in [
            ("mine", 200, None),
            ("other", 500, Some("failed")),
            ("mine", 200, Some("partial")),
        ] {
            let log = RequestLog {
                key_id: Some(key.into()),
                model: Some("公开模型".into()),
                upstream_model: Some("private-source-model".into()),
                request_path: "/v1/responses".into(),
                method: "POST".into(),
                status_code: Some(status),
                error: error.map(str::to_owned),
                created_at: 100,
                ..Default::default()
            };
            let stat = RequestTokenStat {
                key_id: Some(key.into()),
                input_tokens: Some(10),
                cached_input_tokens: Some(3),
                output_tokens: Some(2),
                total_tokens: None,
                estimated_cost_usd: Some(0.25),
                created_at: 100,
                ..Default::default()
            };
            RequestLogsRepository::append_with_usage(db, log, stat)
                .await
                .unwrap();
        }
        let own = RequestLogFilter {
            key_ids: Some(vec!["mine".into()]),
            ..Default::default()
        };
        let summary = RequestLogsRepository::summarize_filtered(db, &own)
            .await
            .unwrap();
        assert_eq!(
            (
                summary.count,
                summary.success_count,
                summary.error_count,
                summary.total_tokens
            ),
            (2, 2, 1, 18)
        );
        assert_eq!(summary.estimated_cost_usd, 0.5);
        let page = RequestLogsRepository::list_filtered(db, &own, 1, 1)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].input_tokens, Some(10));
        assert_eq!(page[0].error, None);
        let mut searched = own.clone();
        for query in [
            "upstream_model:private",
            "private-source-model",
            "key:=other",
        ] {
            searched.query = Some(query.into());
            assert_eq!(
                RequestLogsRepository::count_filtered(db, &searched)
                    .await
                    .unwrap(),
                0
            );
        }
        searched.query = Some("model:=公开模型".into());
        assert_eq!(
            RequestLogsRepository::count_filtered(db, &searched)
                .await
                .unwrap(),
            2
        );
        searched.query = Some("status:2xx".into());
        assert_eq!(
            RequestLogsRepository::count_filtered(db, &searched)
                .await
                .unwrap(),
            2
        );
        searched.end_ts = Some(100);
        assert_eq!(
            RequestLogsRepository::count_filtered(db, &searched)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            RequestLogsRepository::count_filtered(
                db,
                &RequestLogFilter {
                    key_ids: Some(Vec::new()),
                    ..Default::default()
                }
            )
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            ApiKeyDetailsRepository::today_summary(db, own.key_ids.as_deref(), 0, 3600)
                .await
                .unwrap()
                .input_tokens,
            20
        );
        RequestLogsRepository::clear(db, 200).await.unwrap();
        assert_eq!(
            RequestLogsRepository::count_filtered(db, &RequestLogFilter::default())
                .await
                .unwrap(),
            0
        );
        assert!(RequestLogsRepository::get(db, 1).await.unwrap().is_none());
        assert_eq!(
            ApiKeyDetailsRepository::today_summary(db, own.key_ids.as_deref(), 0, 3600)
                .await
                .unwrap()
                .input_tokens,
            20
        );
        let id = RequestLogsRepository::append_with_usage(
            db,
            RequestLog::default(),
            RequestTokenStat::default(),
        )
        .await
        .unwrap();
        assert_eq!(id, 4, "clear must never recycle billing-linked IDs");
    }

    #[tokio::test]
    async fn usage_failure_rolls_back_log_and_id_allocation() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "DROP TABLE request_token_stats".to_string(),
        ))
        .await
        .unwrap();
        assert!(RequestLogsRepository::append_with_usage(
            db,
            RequestLog::default(),
            RequestTokenStat::default()
        )
        .await
        .is_err());
        assert!(RequestLogsRepository::get(db, 1).await.unwrap().is_none());
        storage.migrate().await.unwrap();
        assert_eq!(
            RequestLogsRepository::append_with_usage(
                db,
                RequestLog::default(),
                RequestTokenStat::default()
            )
            .await
            .unwrap(),
            1
        );
    }
}

struct Parameters {
    backend: DatabaseBackend,
    values: Vec<Value>,
}
impl Parameters {
    fn bind(&mut self, value: impl Into<Value>) -> String {
        self.values.push(value.into());
        if self.backend == DatabaseBackend::Postgres {
            format!("${}", self.values.len())
        } else {
            "?".into()
        }
    }
    fn compare(&mut self, field: &str, exact: bool, value: &str) -> String {
        let parameter = self.bind(value);
        format!(
            "COALESCE({field}, '') {} {parameter}",
            if exact { "=" } else { "LIKE" }
        )
    }
}

fn filters(backend: DatabaseBackend, filter: &RequestLogFilter) -> (String, Parameters) {
    let mut params = Parameters {
        backend,
        values: Vec::new(),
    };
    let mut clauses = vec!["r.cleared_at IS NULL".to_string()];
    let admin = filter.key_ids.is_none();
    let account_fields = if admin {
        vec![
            "r.account_id",
            "a.label",
            "a.chatgpt_account_id",
            "a.workspace_id",
        ]
    } else {
        vec!["r.account_id"]
    };
    let query_condition = match parse_request_log_query(filter.query.as_deref()) {
        RequestLogQuery::All => None,
        RequestLogQuery::AccountLike(value) | RequestLogQuery::AccountExact(value) => {
            let exact = matches!(
                parse_request_log_query(filter.query.as_deref()),
                RequestLogQuery::AccountExact(_)
            );
            Some(
                account_fields
                    .iter()
                    .map(|field| params.compare(field, exact, &value))
                    .collect::<Vec<_>>()
                    .join(" OR "),
            )
        }
        RequestLogQuery::FieldLike { column, pattern } => {
            Some(if !admin && route_private(column) {
                "1=0".into()
            } else {
                params.compare(&format!("r.{column}"), false, &pattern)
            })
        }
        RequestLogQuery::FieldExact { column, value } => Some(if !admin && route_private(column) {
            "1=0".into()
        } else {
            params.compare(&format!("r.{column}"), true, &value)
        }),
        RequestLogQuery::StatusExact(status) => {
            Some(format!("r.status_code = {}", params.bind(status)))
        }
        RequestLogQuery::StatusRange(start, end) => Some(format!(
            "r.status_code >= {} AND r.status_code <= {}",
            params.bind(start),
            params.bind(end)
        )),
        RequestLogQuery::GlobalLike(pattern) => {
            let mut fields = vec![
                "request_path",
                "initial_account_id",
                "attempted_account_ids_json",
                "initial_aggregate_api_id",
                "attempted_aggregate_api_ids_json",
                "aggregate_api_supplier_name",
                "aggregate_api_url",
                "original_path",
                "adapted_path",
                "method",
                "request_type",
                "route_strategy",
                "route_source",
                "account_id",
                "client_model",
                "model",
                "model_source",
                "client_reasoning_effort",
                "reasoning_effort",
                "reasoning_source",
                "service_tier",
                "effective_service_tier",
                "service_tier_source",
                "response_adapter",
                "error",
                "key_id",
                "trace_id",
                "upstream_url",
            ];
            if admin {
                fields.extend(["upstream_model", "actual_source_kind", "actual_source_id"]);
            }
            let mut conditions = fields
                .into_iter()
                .map(|field| params.compare(&format!("r.{field}"), false, &pattern))
                .collect::<Vec<_>>();
            if admin {
                for field in &account_fields[1..] {
                    conditions.push(params.compare(field, false, &pattern));
                }
            }
            let cast = if backend == DatabaseBackend::MySql {
                "CHAR"
            } else {
                "TEXT"
            };
            for field in [
                "r.status_code",
                "t.input_tokens",
                "t.cached_input_tokens",
                "t.output_tokens",
                "t.total_tokens",
                "t.reasoning_output_tokens",
                "t.estimated_cost_usd",
            ] {
                conditions.push(params.compare(
                    &format!("CAST({field} AS {cast})"),
                    false,
                    &pattern,
                ));
            }
            Some(conditions.join(" OR "))
        }
    };
    if let Some(condition) = query_condition {
        clauses.push(format!("({condition})"));
    }
    match filter.status.as_deref() {
        Some("2xx") => clauses.push("r.status_code BETWEEN 200 AND 299".into()),
        Some("4xx") => clauses.push("r.status_code BETWEEN 400 AND 499".into()),
        Some("5xx") => clauses.push("r.status_code >= 500".into()),
        _ => {}
    }
    if let Some(start) = filter.start_ts {
        clauses.push(format!("r.created_at >= {}", params.bind(start)));
    }
    if let Some(end) = filter.end_ts {
        clauses.push(format!("r.created_at < {}", params.bind(end)));
    }
    if let Some(keys) = &filter.key_ids {
        if keys.is_empty() {
            clauses.push("1=0".into());
        } else {
            clauses.push(format!(
                "r.key_id IN ({})",
                keys.iter()
                    .map(|key| params.bind(key.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }
    (format!("FROM request_logs r LEFT JOIN request_token_stats t ON t.request_log_id=r.id LEFT JOIN accounts a ON a.id=r.account_id WHERE {}",clauses.join(" AND ")), params)
}
fn route_private(column: &str) -> bool {
    matches!(
        column,
        "upstream_model" | "actual_source_kind" | "actual_source_id"
    )
}

impl RequestLogsRepository {
    /// Allocate an ID under a database transaction lock, and atomically append
    /// the log and its usage. Import uses explicit IDs while the service is offline.
    pub async fn append_with_usage(
        db: &DatabaseConnection,
        log: RequestLog,
        stat: RequestTokenStat,
    ) -> Result<i64, DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "request_logs").await?;
        let id = crate::request_log_retention::allocate_id(&tx).await?;
        let usage_included = log
            .status_code
            .is_some_and(|status| (200..=299).contains(&status));
        Self::insert(&tx, id, log).await?;
        RequestTokenStatsRepository::upsert(
            &tx,
            RequestTokenStatRecord {
                request_log_id: id,
                key_id: stat.key_id,
                account_id: stat.account_id,
                model: stat.model,
                actual_source_kind: stat.actual_source_kind,
                actual_source_id: stat.actual_source_id,
                input_tokens: stat.input_tokens,
                cached_input_tokens: stat.cached_input_tokens,
                output_tokens: stat.output_tokens,
                total_tokens: stat.total_tokens,
                reasoning_output_tokens: stat.reasoning_output_tokens,
                estimated_cost_usd: stat.estimated_cost_usd,
                usage_included,
                created_at: stat.created_at,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn list_filtered(
        db: &impl ConnectionTrait,
        filter: &RequestLogFilter,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<RequestLog>, DbErr> {
        let backend = db.get_database_backend();
        let (clause, mut params) = filters(backend, filter);
        let limit_param = params.bind(limit.min(MAX_PAGE_SIZE) as i64);
        let offset_param = params.bind(offset.min(i64::MAX as u64) as i64);
        let token_columns = [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "total_tokens",
            "reasoning_output_tokens",
            "estimated_cost_usd",
        ];
        let selected = token_columns
            .iter()
            .map(|column| format!("t.{column} AS usage_{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let rows = db.query_all(Statement::from_sql_and_values(backend, format!("SELECT r.*, {selected} {clause} ORDER BY r.id DESC LIMIT {limit_param} OFFSET {offset_param}"), params.values)).await?;
        rows.iter()
            .map(|row| {
                let mut log = RequestLogRecord::from(Model::from_query_result(row, "")?).log;
                log.input_tokens = row.try_get("", "usage_input_tokens")?;
                log.cached_input_tokens = row.try_get("", "usage_cached_input_tokens")?;
                log.output_tokens = row.try_get("", "usage_output_tokens")?;
                log.total_tokens = row.try_get("", "usage_total_tokens")?;
                log.reasoning_output_tokens = row.try_get("", "usage_reasoning_output_tokens")?;
                log.estimated_cost_usd = row.try_get("", "usage_estimated_cost_usd")?;
                Ok(log)
            })
            .collect()
    }

    pub async fn count_filtered(
        db: &impl ConnectionTrait,
        filter: &RequestLogFilter,
    ) -> Result<i64, DbErr> {
        let backend = db.get_database_backend();
        let (clause, params) = filters(backend, filter);
        db.query_one(Statement::from_sql_and_values(
            backend,
            format!("SELECT COUNT(*) AS count {clause}"),
            params.values,
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("request log count returned no row".into()))?
        .try_get("", "count")
    }

    pub async fn summarize_filtered(
        db: &impl ConnectionTrait,
        filter: &RequestLogFilter,
    ) -> Result<RequestLogQuerySummary, DbErr> {
        let backend = db.get_database_backend();
        let (clause, params) = filters(backend, filter);
        // SUM(BIGINT) is NUMERIC on PostgreSQL and DECIMAL on MySQL. Cast its
        // bounded counters explicitly so every driver decodes the same i64.
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
        let usage = if backend == DatabaseBackend::Postgres {
            "TRUE"
        } else {
            "1"
        };
        let fallback = "COALESCE(t.input_tokens,0)-COALESCE(t.cached_input_tokens,0)+COALESCE(t.output_tokens,0)";
        let tokens = format!("CASE WHEN t.usage_included <> {usage} THEN 0 WHEN t.total_tokens IS NOT NULL THEN CASE WHEN t.total_tokens > 0 THEN t.total_tokens ELSE 0 END WHEN {fallback} > 0 THEN {fallback} ELSE 0 END");
        let sql = format!("SELECT COUNT(*) AS count, CAST(COALESCE(SUM(CASE WHEN r.status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END),0) AS {integer}) AS success_count, CAST(COALESCE(SUM(CASE WHEN r.status_code >= 400 OR TRIM(COALESCE(r.error,'')) <> '' THEN 1 ELSE 0 END),0) AS {integer}) AS error_count, CAST(COALESCE(SUM({tokens}),0) AS {integer}) AS total_tokens, CAST(COALESCE(SUM(CASE WHEN t.usage_included = {usage} THEN COALESCE(t.estimated_cost_usd,0) ELSE 0 END),0) AS {float}) AS cost {clause}");
        let row = db
            .query_one(Statement::from_sql_and_values(backend, sql, params.values))
            .await?
            .ok_or_else(|| DbErr::Custom("request log summary returned no row".into()))?;
        Ok(RequestLogQuerySummary {
            count: row.try_get("", "count")?,
            success_count: row.try_get("", "success_count")?,
            error_count: row.try_get("", "error_count")?,
            total_tokens: row.try_get("", "total_tokens")?,
            estimated_cost_usd: row.try_get("", "cost")?,
        })
    }
}
