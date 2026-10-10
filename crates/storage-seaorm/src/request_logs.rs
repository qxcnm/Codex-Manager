//! Append-only request log snapshots for the optional Service storage adapter.
//!
//! IDs are supplied by the application/importer and never reassigned on retry.
//! The existing f64 amount remains a log estimate, not a billing ledger amount.

use codexmanager_core::storage::RequestLog;
use sea_orm::entity::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryOrder, QuerySelect, Set};

mod query;
pub use query::RequestLogFilter;

const MAX_PAGE_SIZE: u64 = 500;

#[derive(Debug, Clone)]
pub struct RequestLogRecord {
    pub id: i64,
    pub log: RequestLog,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "request_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    id: i64,
    #[sea_orm(column_type = "Text", nullable)]
    trace_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    key_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    account_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    initial_account_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    attempted_account_ids_json: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    initial_aggregate_api_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    attempted_aggregate_api_ids_json: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    original_path: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    adapted_path: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    request_type: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    gateway_mode: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    route_strategy: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    route_source: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    client_model: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    model: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    model_source: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    upstream_model: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    actual_source_kind: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    actual_source_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    client_reasoning_effort: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    reasoning_effort: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    reasoning_source: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    service_tier: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    effective_service_tier: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    service_tier_source: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    response_adapter: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    upstream_url: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    aggregate_api_supplier_name: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    aggregate_api_url: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    error: Option<String>,
    #[sea_orm(column_type = "Text")]
    request_path: String,
    #[sea_orm(column_type = "Text")]
    method: String,
    transparent_mode: Option<bool>,
    enhanced_mode: Option<bool>,
    status_code: Option<i64>,
    duration_ms: Option<i64>,
    first_response_ms: Option<i64>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    estimated_cost_usd: Option<f64>,
    created_at: i64,
    cleared_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub(crate) fn id_value(&self) -> i64 {
        self.id
    }
}

pub struct RequestLogsRepository;

impl RequestLogsRepository {
    /// Insert an immutable snapshot. Duplicate IDs fail without overwriting data.
    pub async fn insert(db: &impl ConnectionTrait, id: i64, log: RequestLog) -> Result<(), DbErr> {
        let model = ActiveModel {
            id: Set(id),
            trace_id: Set(log.trace_id),
            key_id: Set(log.key_id),
            account_id: Set(log.account_id),
            initial_account_id: Set(log.initial_account_id),
            attempted_account_ids_json: Set(log.attempted_account_ids_json),
            initial_aggregate_api_id: Set(log.initial_aggregate_api_id),
            attempted_aggregate_api_ids_json: Set(log.attempted_aggregate_api_ids_json),
            original_path: Set(log.original_path),
            adapted_path: Set(log.adapted_path),
            request_type: Set(log.request_type),
            gateway_mode: Set(log.gateway_mode),
            route_strategy: Set(log.route_strategy),
            route_source: Set(log.route_source),
            client_model: Set(log.client_model),
            model: Set(log.model),
            model_source: Set(log.model_source),
            upstream_model: Set(log.upstream_model),
            actual_source_kind: Set(log.actual_source_kind),
            actual_source_id: Set(log.actual_source_id),
            client_reasoning_effort: Set(log.client_reasoning_effort),
            reasoning_effort: Set(log.reasoning_effort),
            reasoning_source: Set(log.reasoning_source),
            service_tier: Set(log.service_tier),
            effective_service_tier: Set(log.effective_service_tier),
            service_tier_source: Set(log.service_tier_source),
            response_adapter: Set(log.response_adapter),
            upstream_url: Set(log.upstream_url),
            aggregate_api_supplier_name: Set(log.aggregate_api_supplier_name),
            aggregate_api_url: Set(log.aggregate_api_url),
            error: Set(log.error),
            request_path: Set(log.request_path),
            method: Set(log.method),
            transparent_mode: Set(log.transparent_mode),
            enhanced_mode: Set(log.enhanced_mode),
            status_code: Set(log.status_code),
            duration_ms: Set(log.duration_ms),
            first_response_ms: Set(log.first_response_ms),
            input_tokens: Set(log.input_tokens),
            cached_input_tokens: Set(log.cached_input_tokens),
            output_tokens: Set(log.output_tokens),
            total_tokens: Set(log.total_tokens),
            reasoning_output_tokens: Set(log.reasoning_output_tokens),
            estimated_cost_usd: Set(log.estimated_cost_usd),
            created_at: Set(log.created_at),
            cleared_at: Set(None),
        };
        Entity::insert(model).exec(db).await.map(|_| ())
    }

    pub async fn get(
        db: &impl ConnectionTrait,
        id: i64,
    ) -> Result<Option<RequestLogRecord>, DbErr> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    /// Page backward by stable ID. The upper bound is exclusive; at most 500
    /// records are loaded even if the caller requests an unbounded page.
    pub async fn list_before(
        db: &impl ConnectionTrait,
        before_id: Option<i64>,
        limit: u64,
    ) -> Result<Vec<RequestLogRecord>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut query = Entity::find()
            .order_by_desc(Column::Id)
            .limit(limit.min(MAX_PAGE_SIZE));
        if let Some(before_id) = before_id {
            query = query.filter(Column::Id.lt(before_id));
        }
        query
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }
}

impl From<Model> for RequestLogRecord {
    fn from(model: Model) -> Self {
        Self {
            id: model.id,
            log: RequestLog {
                trace_id: model.trace_id,
                key_id: model.key_id,
                account_id: model.account_id,
                initial_account_id: model.initial_account_id,
                attempted_account_ids_json: model.attempted_account_ids_json,
                initial_aggregate_api_id: model.initial_aggregate_api_id,
                attempted_aggregate_api_ids_json: model.attempted_aggregate_api_ids_json,
                original_path: model.original_path,
                adapted_path: model.adapted_path,
                request_type: model.request_type,
                gateway_mode: model.gateway_mode,
                route_strategy: model.route_strategy,
                route_source: model.route_source,
                client_model: model.client_model,
                model: model.model,
                model_source: model.model_source,
                upstream_model: model.upstream_model,
                actual_source_kind: model.actual_source_kind,
                actual_source_id: model.actual_source_id,
                client_reasoning_effort: model.client_reasoning_effort,
                reasoning_effort: model.reasoning_effort,
                reasoning_source: model.reasoning_source,
                service_tier: model.service_tier,
                effective_service_tier: model.effective_service_tier,
                service_tier_source: model.service_tier_source,
                response_adapter: model.response_adapter,
                upstream_url: model.upstream_url,
                aggregate_api_supplier_name: model.aggregate_api_supplier_name,
                aggregate_api_url: model.aggregate_api_url,
                error: model.error,
                request_path: model.request_path,
                method: model.method,
                transparent_mode: model.transparent_mode,
                enhanced_mode: model.enhanced_mode,
                status_code: model.status_code,
                duration_ms: model.duration_ms,
                first_response_ms: model.first_response_ms,
                input_tokens: model.input_tokens,
                cached_input_tokens: model.cached_input_tokens,
                output_tokens: model.output_tokens,
                total_tokens: model.total_tokens,
                reasoning_output_tokens: model.reasoning_output_tokens,
                estimated_cost_usd: model.estimated_cost_usd,
                created_at: model.created_at,
            },
        }
    }
}

#[cfg(test)]
pub(crate) fn fixture() -> RequestLog {
    RequestLog {
        trace_id: Some("trace_id-fixture".into()),
        key_id: Some("key_id-fixture".into()),
        account_id: Some("account_id-fixture".into()),
        initial_account_id: Some("initial_account_id-fixture".into()),
        attempted_account_ids_json: Some("[\"first\",\"second\"]".into()),
        initial_aggregate_api_id: Some("initial_aggregate_api_id-fixture".into()),
        attempted_aggregate_api_ids_json: Some("[\"first\",\"second\"]".into()),
        original_path: Some("original_path-fixture".into()),
        adapted_path: Some("adapted_path-fixture".into()),
        request_type: Some("request_type-fixture".into()),
        gateway_mode: Some("gateway_mode-fixture".into()),
        route_strategy: Some("route_strategy-fixture".into()),
        route_source: Some("route_source-fixture".into()),
        client_model: Some("client_model-fixture".into()),
        model: Some("model-fixture".into()),
        model_source: Some("model_source-fixture".into()),
        upstream_model: Some("upstream_model-fixture".into()),
        actual_source_kind: Some("actual_source_kind-fixture".into()),
        actual_source_id: Some("actual_source_id-fixture".into()),
        client_reasoning_effort: Some("client_reasoning_effort-fixture".into()),
        reasoning_effort: Some("reasoning_effort-fixture".into()),
        reasoning_source: Some("reasoning_source-fixture".into()),
        service_tier: Some("service_tier-fixture".into()),
        effective_service_tier: Some("effective_service_tier-fixture".into()),
        service_tier_source: Some("service_tier_source-fixture".into()),
        response_adapter: Some("response_adapter-fixture".into()),
        upstream_url: Some("upstream_url-fixture".into()),
        aggregate_api_supplier_name: Some("aggregate_api_supplier_name-fixture".into()),
        aggregate_api_url: Some("aggregate_api_url-fixture".into()),
        error: Some("diagnostic".repeat(80)),
        request_path: "/v1/responses".into(),
        method: "POST".into(),
        transparent_mode: Some(false),
        enhanced_mode: Some(true),
        status_code: Some(200),
        duration_ms: Some(i64::from(i32::MAX) + 35),
        first_response_ms: Some(i64::from(i32::MAX) + 36),
        input_tokens: Some(i64::from(i32::MAX) + 37),
        cached_input_tokens: Some(i64::from(i32::MAX) + 38),
        output_tokens: Some(i64::from(i32::MAX) + 39),
        total_tokens: Some(i64::from(i32::MAX) + 40),
        reasoning_output_tokens: Some(i64::from(i32::MAX) + 41),
        estimated_cost_usd: Some(0.125),
        created_at: 1_721_234_567,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeaOrmStorage;
    use codexmanager_core::storage::StorageBackendKind;
    use sea_orm::TransactionTrait;

    #[tokio::test]
    async fn request_log_round_trip_preserves_every_field_and_rejects_duplicate_id() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        let log = fixture();
        RequestLogsRepository::insert(db, 10, log.clone())
            .await
            .unwrap();
        let actual = RequestLogsRepository::get(db, 10).await.unwrap().unwrap();
        assert_eq!(actual.id, 10);
        assert_eq!(format!("{:?}", actual.log), format!("{:?}", log));
        assert!(RequestLogsRepository::insert(db, 10, RequestLog::default())
            .await
            .is_err());
        let unchanged = RequestLogsRepository::get(db, 10).await.unwrap().unwrap();
        assert_eq!(format!("{:?}", unchanged.log), format!("{:?}", log));
    }

    #[tokio::test]
    async fn request_log_cursor_is_exclusive_bounded_and_rolls_back() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        let db = storage.connection();
        let tx = db.begin().await.unwrap();
        for id in 1..=MAX_PAGE_SIZE as i64 + 1 {
            RequestLogsRepository::insert(&tx, id, RequestLog::default())
                .await
                .unwrap();
        }
        let page = RequestLogsRepository::list_before(&tx, None, u64::MAX)
            .await
            .unwrap();
        assert_eq!(page.len(), MAX_PAGE_SIZE as usize);
        assert_eq!(page[0].id, MAX_PAGE_SIZE as i64 + 1);
        let tail = RequestLogsRepository::list_before(&tx, Some(3), 10)
            .await
            .unwrap();
        assert_eq!(
            tail.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(RequestLogsRepository::list_before(&tx, None, 0)
            .await
            .unwrap()
            .is_empty());
        tx.rollback().await.unwrap();
        assert!(RequestLogsRepository::get(db, 1).await.unwrap().is_none());
    }
}
