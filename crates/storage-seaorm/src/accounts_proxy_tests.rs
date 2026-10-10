use super::*;
pub(crate) async fn exercise(db: &DatabaseConnection, suffix: &str) {
    let id = format!("proxy-{suffix}");
    let profile = AccountsRepository::create_proxy_profile(
        db,
        &ProxyProfileCreateInput {
            id: id.clone(),
            name: " Fixture ".into(),
            proxy_url: " http://fixture:fake-pass@127.0.0.1:1080 ".into(),
            enabled: true,
            tags_json: None,
            notes: Some("old note".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(profile.name, "Fixture");
    assert!(!profile.proxy_url_redacted.contains("fake-pass"));
    let input = ProxyProfileUpdateInput {
        id: id.clone(),
        name: None,
        proxy_url: None,
        enabled: None,
        status: None,
        last_error: None,
        last_url_latency_ms: None,
        last_download_mbps: Some(12.5),
        last_upload_mbps: None,
        last_tested_at: None,
        ip: None,
        country_code: None,
        country_name: None,
        region_name: None,
        city_name: None,
        asn: None,
        as_org: None,
        isp: None,
        as_domain: None,
        flag_img_url: None,
        flag_emoji: None,
        timezone_id: None,
        timezone_offset: None,
        timezone_utc: None,
        tags_json: None,
        notes: Some(String::new()),
    };
    let changed = AccountsRepository::update_proxy_profile(db, &input)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.name, "Fixture");
    assert_eq!(changed.last_download_mbps, Some(12.5));
    assert_eq!(changed.notes, None);
    for stamp in [10, 20] {
        AccountsRepository::insert_proxy_profile_url_test(
            db,
            &ProxyProfileUrlTestInsertInput {
                proxy_profile_id: id.clone(),
                status: " ".into(),
                url_latency_ms: Some(100),
                status_code: Some(200),
                test_url: " https://example.invalid ".into(),
                final_url: None,
                redirected: false,
                tested_at: stamp,
                error_code: Some(" ".into()),
                error: None,
            },
        )
        .await
        .unwrap();
    }
    let history = AccountsRepository::list_proxy_profile_url_tests(db, &id, 0)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].tested_at, 20);
    assert_eq!(history[0].status, "failed");
    assert!(history[0].error_code.is_none());
    AccountsRepository::delete_proxy_profile(db, &id)
        .await
        .unwrap();
    assert!(
        AccountsRepository::list_proxy_profile_url_tests(db, &id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn sqlite_proxy_updates_and_history_cleanup() {
    let s = crate::SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    s.migrate().await.unwrap();
    exercise(s.connection(), "sqlite").await;
}
