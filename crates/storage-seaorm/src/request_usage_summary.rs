use crate::ApiKeyDetailsRepository;
use codexmanager_core::storage::RequestLogTodaySummary;
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement, Value};

impl ApiKeyDetailsRepository {
    /// Aggregate every model, including administrative requests without an API key.
    pub async fn usage_by_model(
        db: &impl ConnectionTrait,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<codexmanager_core::storage::TokenUsageSummary>, DbErr> {
        let backend = db.get_database_backend();
        let mut values: Vec<Value> = Vec::new();
        let mut parameter = |value: Value| {
            values.push(value);
            if backend == DatabaseBackend::Postgres {
                format!("${}", values.len())
            } else {
                "?".into()
            }
        };
        let mut sources = Vec::new();
        for (table, clock, included) in [
            (
                "request_token_stats",
                "created_at",
                " AND usage_included=TRUE",
            ),
            ("request_token_stat_hourly_rollups", "bucket_start", ""),
            ("request_token_stat_rollups", "", ""),
        ] {
            if clock.is_empty() && (start.is_some() || end.is_some()) {
                continue;
            }
            let mut sql=format!("SELECT COALESCE(NULLIF(TRIM(model),''),'unknown') AS model,input_tokens,cached_input_tokens,output_tokens,reasoning_output_tokens,total_tokens,estimated_cost_usd FROM {table} WHERE 1=1{included}");
            if let Some(start) = start {
                sql.push_str(&format!(" AND {clock}>={}", parameter(start.into())));
            }
            if let Some(end) = end {
                let (end_clock, comparison) = if clock == "bucket_start" {
                    ("bucket_end", "<=")
                } else {
                    (clock, "<")
                };
                sql.push_str(&format!(
                    " AND {end_clock}{comparison}{}",
                    parameter(end.into())
                ));
            }
            sources.push(sql);
        }
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
        let sums = [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
        ]
        .iter()
        .map(|field| format!("CAST(COALESCE(SUM(COALESCE({field},0)),0) AS {integer}) AS {field}"))
        .collect::<Vec<_>>()
        .join(",");
        let fallback =
            "COALESCE(input_tokens,0)-COALESCE(cached_input_tokens,0)+COALESCE(output_tokens,0)";
        let tokens=format!("CASE WHEN total_tokens IS NOT NULL THEN CASE WHEN total_tokens>0 THEN total_tokens ELSE 0 END WHEN {fallback}>0 THEN {fallback} ELSE 0 END");
        let sql=format!("SELECT model,{sums},CAST(COALESCE(SUM({tokens}),0) AS {integer}) AS total_tokens,CAST(COALESCE(SUM(COALESCE(estimated_cost_usd,0)),0) AS {float}) AS estimated_cost_usd FROM ({}) usage_rows GROUP BY model ORDER BY total_tokens DESC,model ASC",sources.join(" UNION ALL "));
        db.query_all(Statement::from_sql_and_values(backend, sql, values))
            .await?
            .iter()
            .map(|row| {
                Ok(codexmanager_core::storage::TokenUsageSummary {
                    model: row.try_get("", "model")?,
                    input_tokens: row.try_get("", "input_tokens")?,
                    cached_input_tokens: row.try_get("", "cached_input_tokens")?,
                    output_tokens: row.try_get("", "output_tokens")?,
                    reasoning_output_tokens: row.try_get("", "reasoning_output_tokens")?,
                    total_tokens: row.try_get("", "total_tokens")?,
                    estimated_cost_usd: row.try_get("", "estimated_cost_usd")?,
                })
            })
            .collect()
    }
    /// Date-bounded raw plus hourly usage; cleared logs retain their accounting.
    pub async fn today_summary(
        db: &impl ConnectionTrait,
        key_ids: Option<&[String]>,
        start: i64,
        end: i64,
    ) -> Result<RequestLogTodaySummary, DbErr> {
        let backend = db.get_database_backend();
        let mut values: Vec<Value> = Vec::new();
        let mut parameter = |value: Value| {
            values.push(value);
            if backend == DatabaseBackend::Postgres {
                format!("${}", values.len())
            } else {
                "?".to_string()
            }
        };
        let mut sources = Vec::new();
        for (table, clock, included) in [
            (
                "request_token_stats",
                "created_at",
                " AND usage_included = TRUE",
            ),
            ("request_token_stat_hourly_rollups", "bucket_start", ""),
        ] {
            let (end_clock, comparator) = if table == "request_token_stat_hourly_rollups" {
                ("bucket_end", "<=")
            } else {
                (clock, "<")
            };
            let mut source = format!("SELECT input_tokens,cached_input_tokens,output_tokens,reasoning_output_tokens,estimated_cost_usd FROM {table} WHERE {clock} >= {} AND {end_clock} {comparator} {}{included}",parameter(start.into()),parameter(end.into()));
            if let Some(keys) = key_ids {
                if keys.is_empty() {
                    source.push_str(" AND 1=0");
                } else {
                    source.push_str(&format!(
                        " AND key_id IN ({})",
                        keys.iter()
                            .map(|key| parameter(key.as_str().into()))
                            .collect::<Vec<_>>()
                            .join(",")
                    ));
                }
            }
            sources.push(source);
        }
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
        let fields = [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
        ]
        .iter()
        .map(|field| format!("CAST(COALESCE(SUM(COALESCE({field},0)),0) AS {integer}) AS {field}"))
        .collect::<Vec<_>>()
        .join(",");
        let sql = format!("SELECT {fields}, CAST(COALESCE(SUM(COALESCE(estimated_cost_usd,0)),0) AS {float}) AS cost FROM ({}) selected_stats",sources.join(" UNION ALL "));
        let row = db
            .query_one(Statement::from_sql_and_values(backend, sql, values))
            .await?
            .ok_or_else(|| DbErr::Custom("request usage summary returned no row".into()))?;
        Ok(RequestLogTodaySummary {
            input_tokens: row.try_get("", "input_tokens")?,
            cached_input_tokens: row.try_get("", "cached_input_tokens")?,
            output_tokens: row.try_get("", "output_tokens")?,
            reasoning_output_tokens: row.try_get("", "reasoning_output_tokens")?,
            estimated_cost_usd: row.try_get("", "cost")?,
        })
    }
}
