use super::*;
use crate::{RequestTokenStatRecord, RequestTokenStatsRepository, SeaOrmStorage};
use codexmanager_core::storage::StorageBackendKind;

pub(crate) async fn exercise(db: &DatabaseConnection, suffix: &str, stamp: i64) {
    let id = format!("api-details-{suffix}");
    let key = ApiKeyRecord {
        id: id.clone(),
        name: Some("original".into()),
        model_slug: Some("gpt-test".into()),
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".into(),
        aggregate_api_id: None,
        account_plan_filter: None,
        account_group_filter: Some("group-a".into()),
        client_type: "codex".into(),
        protocol_type: "openai_compat".into(),
        auth_scheme: "authorization_bearer".into(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{suffix}"),
        status: "active".into(),
        created_at: stamp,
        last_used_at: None,
    };
    ApiKeyDetailsRepository::create(db, key.clone(), "fixture-secret".into(), Some(100))
        .await
        .expect("atomic create");
    assert_eq!(
        ApiKeyDetailsRepository::secret(db, &id)
            .await
            .unwrap()
            .as_deref(),
        Some("fixture-secret")
    );
    assert_eq!(
        ApiKeyDetailsRepository::quota(db, &id).await.unwrap(),
        Some(100)
    );
    let mut duplicate = key.clone();
    duplicate.id.push_str("-duplicate");
    assert!(ApiKeyDetailsRepository::create(
        db,
        duplicate.clone(),
        "replacement".into(),
        Some(500)
    )
    .await
    .is_err());
    assert!(ApiKeysRepository::get(db, &duplicate.id)
        .await
        .unwrap()
        .is_none());
    assert!(ApiKeyDetailsRepository::secret(db, &duplicate.id)
        .await
        .unwrap()
        .is_none());
    assert!(
        ApiKeyDetailsRepository::update(db, &id, Some(Some(500)), |key| {
            key.name = Some("must roll back".into());
            Err("invalid routing".into())
        })
        .await
        .is_err()
    );
    assert_eq!(
        ApiKeysRepository::get(db, &id)
            .await
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("original")
    );
    assert_eq!(
        ApiKeyDetailsRepository::quota(db, &id).await.unwrap(),
        Some(100)
    );
    ApiKeyDetailsRepository::update(db, &id, Some(None), |key| {
        key.status = "disabled".into();
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        ApiKeysRepository::get(db, &id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "disabled"
    );
    assert_eq!(ApiKeyDetailsRepository::quota(db, &id).await.unwrap(), None);

    let stat = RequestTokenStatRecord {
        request_log_id: stamp,
        key_id: Some(id.clone()),
        account_id: None,
        model: Some("gpt-test".into()),
        actual_source_kind: None,
        actual_source_id: None,
        input_tokens: Some(40),
        cached_input_tokens: Some(10),
        output_tokens: Some(5),
        total_tokens: None,
        reasoning_output_tokens: Some(3),
        estimated_cost_usd: Some(0.1),
        usage_included: true,
        created_at: stamp,
    };
    RequestTokenStatsRepository::upsert(db, stat.clone())
        .await
        .unwrap();
    let mut excluded = stat;
    excluded.request_log_id += 1;
    excluded.usage_included = false;
    excluded.total_tokens = Some(10000);
    RequestTokenStatsRepository::upsert(db, excluded)
        .await
        .unwrap();
    crate::api_key_rollups::hourly::ActiveModel {
        bucket_start: Set(stamp),
        bucket_end: Set(stamp + 3600),
        key_id: Set(id.clone()),
        account_id: Set(String::new()),
        model: Set("gpt-test".into()),
        actual_source_kind: Set(String::new()),
        actual_source_id: Set(String::new()),
        owner_user_id: Set(String::new()),
        input_tokens: Set(20),
        cached_input_tokens: Set(0),
        output_tokens: Set(5),
        total_tokens: Set(25),
        reasoning_output_tokens: Set(2),
        estimated_cost_usd: Set(0.2),
        request_count: Set(1),
        success_count: Set(1),
        error_count: Set(0),
        updated_at: Set(stamp),
    }
    .insert(db)
    .await
    .unwrap();
    crate::api_key_rollups::legacy::ActiveModel {
        key_id: Set(id.clone()),
        account_id: Set(String::new()),
        model: Set("gpt-test".into()),
        input_tokens: Set(8),
        cached_input_tokens: Set(0),
        output_tokens: Set(2),
        total_tokens: Set(10),
        reasoning_output_tokens: Set(0),
        estimated_cost_usd: Set(0.3),
        source_rows: Set(1),
        updated_at: Set(stamp),
    }
    .insert(db)
    .await
    .unwrap();
    assert_eq!(
        ApiKeyDetailsRepository::token_usage(db, &id).await.unwrap(),
        70
    );
    let keys = [id.clone()];
    let bounded =
        ApiKeyDetailsRepository::usage_by_key_model(db, Some(stamp), Some(stamp + 1), Some(&keys))
            .await
            .unwrap();
    assert_eq!(bounded.len(), 1);
    assert_eq!(bounded[0].total_tokens, 60);
    assert_eq!(bounded[0].input_tokens, 60);
    assert_eq!(bounded[0].reasoning_output_tokens, 5);
    assert!((bounded[0].estimated_cost_usd - 0.3).abs() < 0.00001);
    assert!(
        ApiKeyDetailsRepository::usage_by_key_model(db, None, None, Some(&[]))
            .await
            .unwrap()
            .is_empty()
    );
    let hostile = [format!("{id}' OR 1=1 --")];
    assert!(
        ApiKeyDetailsRepository::usage_by_key_model(db, None, None, Some(&hostile))
            .await
            .unwrap()
            .is_empty()
    );
    ApiKeyDetailsRepository::delete(db, &id).await.unwrap();
    assert!(ApiKeysRepository::get(db, &id).await.unwrap().is_none());
    assert!(ApiKeyDetailsRepository::secret(db, &id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        ApiKeyDetailsRepository::token_usage(db, &id).await.unwrap(),
        70,
        "deleting keys preserves historical accounting"
    );
}

#[tokio::test]
async fn atomic_api_keys_preserve_secrets_quotas_and_archived_accounting() {
    let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection(), "sqlite", 1000).await;
}
