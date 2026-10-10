use super::*;

#[test]
fn reset_warmup_rpc_persists_single_and_batch_settings_without_changing_account_status() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-reset-warmup-rpc");
    let storage = storage_helpers::open_storage().expect("open storage");
    let now = codexmanager_core::storage::now_ts();
    for (id, status) in [("reset-a", "active"), ("reset-b", "disabled")] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }
    let list = || {
        response_result(handle_request(rpc_request(
            "account/list",
            serde_json::json!({}),
        )))
        .result
    };
    assert!(list()["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["resetWarmupEnabled"] == true));

    let update = |params| {
        response_result(handle_request_with_actor(
            rpc_request("account/resetWarmup/update", params),
            RpcActor::from_parts(Some(ROLE_MEMBER), Some("member-reset-warmup")),
        ))
        .result
    };
    assert_eq!(
        update(
            serde_json::json!({"accountIds": ["reset-a", " reset-b ", "reset-a"], "enabled": false})
        )["updated"],
        2
    );
    assert!(list()["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["resetWarmupEnabled"] == false));
    assert_eq!(
        update(serde_json::json!({"accountIds": ["reset-a"], "enabled": true}))["updated"],
        1
    );
    let listed = list();
    let items = listed["items"].as_array().unwrap();
    assert_eq!(
        items.iter().find(|item| item["id"] == "reset-a").unwrap()["resetWarmupEnabled"],
        true
    );
    assert_eq!(
        items.iter().find(|item| item["id"] == "reset-b").unwrap()["resetWarmupEnabled"],
        false
    );

    for params in [
        serde_json::json!({"accountIds": [], "enabled": false}),
        serde_json::json!({"accountIds": [" "], "enabled": false}),
        serde_json::json!({"accountIds": ["reset-a", 123], "enabled": false}),
        serde_json::json!({"accountIds": ["reset-a"]}),
        serde_json::json!({"accountIds": ["reset-a"], "enabled": "false"}),
        serde_json::json!({"accountIds": ["reset-a", "missing"], "enabled": false}),
    ] {
        assert!(update(params).get("error").is_some());
    }
    let persisted = storage
        .list_account_reset_warmup_settings_for_accounts(&["reset-a".into(), "reset-b".into()])
        .unwrap();
    assert!(
        persisted.contains(&("reset-a".to_string(), true)),
        "invalid batch must not partially apply"
    );
    assert!(persisted.contains(&("reset-b".to_string(), false)));
    for (id, status) in [("reset-a", "active"), ("reset-b", "disabled")] {
        assert_eq!(
            storage.find_account_by_id(id).unwrap().unwrap().status,
            status
        );
    }
    let _ = std::fs::remove_file(db_path);
}
