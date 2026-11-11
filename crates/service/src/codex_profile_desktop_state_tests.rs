use super::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn auth_with_claims(claims: Value) -> String {
    let payload = json!({"exp": 4_000_000_000u64, "https://api.openai.com/auth": claims});
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
    json!({"tokens": {"access_token": format!("header.{payload}.signature")}}).to_string()
}

fn target_auth() -> String {
    auth_with_claims(json!({"user_id": "user-123", "chatgpt_account_id": "account-456"}))
}

fn temporary_profile(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "codexmanager-desktop-state-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn root(value: &mut Value) -> &mut Map<String, Value> {
    value.as_object_mut().unwrap()
}

#[test]
fn account_owner_matches_codex_hash_and_preferred_claims() {
    let expected = "2bd1472347800188afce3c8de99bf53d4ffe6b3717b858f3f88e9e9fe774ba1c";
    assert_eq!(auth_owner(Some(&target_auth())).unwrap(), expected);
    assert_eq!(
        auth_owner(Some(&auth_with_claims(json!({
            "user_id": "user-123", "chatgpt_user_id": "ignored-user",
            "chatgpt_account_id": "account-456", "account_id": "ignored-account"
        }))))
        .unwrap(),
        expected
    );
    assert_eq!(
        auth_owner(Some(&auth_with_claims(json!({
            "chatgpt_user_id": "user-123", "account_id": "account-456"
        }))))
        .unwrap(),
        expected
    );
}

#[test]
fn account_owner_distinguishes_signed_out_from_unknown_token_identity() {
    for auth in [
        None,
        Some("{}"),
        Some(r#"{"tokens":null}"#),
        Some(r#"{"OPENAI_API_KEY":"local-key"}"#),
        Some(r#"{"auth_mode":"apikey","tokens":{"access_token":"stale-token"}}"#),
    ] {
        assert_eq!(auth_owner(auth).unwrap(), "signed-out");
    }
    for auth in [
        "not-json".to_string(),
        json!({"tokens": {"access_token": "opaque-token", "account_id": "insufficient"}})
            .to_string(),
        auth_with_claims(json!({"user_id": "user-123"})),
        auth_with_claims(json!({"chatgpt_account_id": "account-456"})),
        auth_with_claims(json!({"user_id": " ", "chatgpt_account_id": "account-456"})),
    ] {
        assert!(
            auth_owner(Some(&auth)).is_err(),
            "accepted unknown identity: {auth}"
        );
    }
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
        "exp": 0, "https://api.openai.com/auth": {"user_id": "user-123", "chatgpt_account_id": "account-456"}
    })).unwrap());
    let no_expiry =
        json!({"tokens": {"access_token": format!("header.{payload}.signature")}}).to_string();
    assert!(auth_owner(Some(&no_expiry)).is_err());
}

fn legacy_fixture() -> Value {
    json!({
        "electron-renderer-content-owner": "account-a",
        "electron-account-renderer-state-v1": {
            "owner": "account-a", "inactive": {}
        },
        "local-projects": {"local-a": {"name": "A", "roots": ["C:/code/a"]}},
        "project-order": ["remote-a", "local-a"],
        "pinned-project-ids": ["local-a", "remote-a"],
        "project-appearances": {"local-a": {"color": "blue"}, "remote-a": {"color": "red"}},
        "sidebar-project-thread-orders": {"local-a": ["thread-a"], "remote-a": ["remote-thread"]},
        "selected-project": {"type": "local", "projectId": "local-a"},
        "thread-project-assignments": {
            "thread-a": {"projectKind": "local", "projectId": "local-a"},
            "remote-thread": {"projectKind": "remote", "projectId": "remote-a"},
            "missing-thread": {"projectKind": "local", "projectId": "missing"}
        },
        "electron-saved-workspace-roots": ["C:/code/a", "/srv/remote", ""],
        "active-workspace-roots": ["C:/code/a", "/srv/remote"],
        "electron-workspace-root-labels": {"C:/code/a": "A", "/srv/remote": "Remote", "C:/unused": "Unused"},
        "remote-cwds-by-host-and-workspace": {"remote-host": {"/srv/remote": "/srv/remote"}},
        "remote-projects": {"remote-a": {"hostId": "remote-host"}},
        "drafts": {"thread-a": "private draft"},
        "project-writable-roots": {"local-a": ["C:/permissions"]},
        "electron-persisted-atom-state": {"private-account-preference": true},
        "electron-owner-atoms:account-a": {"private-account-preference": true}
    })
}

#[test]
fn legacy_inheritance_seeds_only_local_project_navigation() {
    let mut state = legacy_fixture();
    let original = state.clone();
    assert!(inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap());
    let target = &state[LEGACY_STATE]["inactive"]["account-b"];
    assert_eq!(target["atoms"], json!({}));
    assert_eq!(
        target["globals"],
        json!({
            "local-projects": {"local-a": {"name": "A", "roots": ["C:/code/a"]}},
            "project-order": ["local-a"],
            "pinned-project-ids": ["local-a"],
            "project-appearances": {"local-a": {"color": "blue"}},
            "sidebar-project-thread-orders": {"local-a": ["thread-a"]},
            "selected-project": {"type": "local", "projectId": "local-a"},
            "thread-project-assignments": {"thread-a": {"projectKind": "local", "projectId": "local-a"}},
            "electron-saved-workspace-roots": ["C:/code/a"],
            "active-workspace-roots": ["C:/code/a"],
            "electron-workspace-root-labels": {"C:/code/a": "A"}
        })
    );
    for key in [
        "remote-projects",
        "drafts",
        "project-writable-roots",
        "electron-persisted-atom-state",
        "electron-owner-atoms:account-a",
    ] {
        assert_eq!(state[key], original[key]);
        assert!(target["globals"].get(key).is_none());
    }
    assert!(state.get("electron-owner-atoms:account-b").is_none());
    assert_eq!(state[OWNER], "account-a");
    let inherited = state.clone();
    assert!(!inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap());
    assert_eq!(state, inherited);
}

#[test]
fn target_project_details_take_precedence_while_missing_items_and_empty_lists_are_filled() {
    let mut state = legacy_fixture();
    state["local-projects"]["local-b"] = json!({"name": "Additional B"});
    state[LEGACY_STATE]["inactive"]["account-b"] = json!({
        "atoms": {"keep-preference": true},
        "globals": {
            "local-projects": {"target-project": {"name": "B"}, "local-a": {"name": "Target A"}},
            "project-order": [],
            "pinned-project-ids": [],
            "selected-project": {"type": "local", "projectId": "target-project"},
            "drafts": {"target-thread": "keep draft"}
        }
    });
    state["electron-owner-value:active-workspace-roots:account-b"] = json!([]);
    let original_target = state[LEGACY_STATE]["inactive"]["account-b"].clone();
    inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap();
    let target = &state[LEGACY_STATE]["inactive"]["account-b"];
    for key in ["selected-project", "drafts"] {
        assert_eq!(target["globals"][key], original_target["globals"][key]);
    }
    assert_eq!(
        target["globals"]["local-projects"],
        json!({
            "target-project": {"name": "B"},
            "local-a": {"name": "Target A"},
            "local-b": {"name": "Additional B"}
        })
    );
    assert_eq!(target["globals"]["project-order"], json!(["local-a"]));
    assert_eq!(target["globals"]["pinned-project-ids"], json!(["local-a"]));
    assert_eq!(target["atoms"], original_target["atoms"]);
    assert_eq!(
        state["electron-owner-value:active-workspace-roots:account-b"],
        json!(["C:/code/a"])
    );

    state[LEGACY_STATE]["inactive"]["account-b"]["globals"]["project-order"] =
        json!(["target-project", "local-a"]);
    inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap();
    assert_eq!(
        state[LEGACY_STATE]["inactive"]["account-b"]["globals"]["project-order"],
        json!(["target-project", "local-a"])
    );
}

#[test]
fn owner_remote_preferences_exclude_remote_roots_without_migrating_connections() {
    let mut state = json!({
        "electron-renderer-content-owner": "account-a",
        "electron-account-renderer-state-v1": {"owner": "account-a", "inactive": {}},
        "electron-saved-workspace-roots": ["C:/code/a", "/srv/remote"],
        "active-workspace-roots": ["/srv/remote", "C:/code/a"],
        "electron-workspace-root-labels": {"C:/code/a": "A", "/srv/remote": "Remote"},
        "electron-account-remote-preference:remote-cwds-by-host-and-workspace:account-a": {
            "remote-host": {"/srv/remote": "/srv/remote"}
        }
    });
    inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap();
    let globals = &state[LEGACY_STATE]["inactive"]["account-b"]["globals"];
    assert_eq!(
        globals["electron-saved-workspace-roots"],
        json!(["C:/code/a"])
    );
    assert_eq!(globals["active-workspace-roots"], json!(["C:/code/a"]));
    assert_eq!(
        globals["electron-workspace-root-labels"],
        json!({"C:/code/a": "A"})
    );
    assert!(state
        .get("electron-account-remote-preference:remote-cwds-by-host-and-workspace:account-b")
        .is_none());
    assert!(globals.get("remote-cwds-by-host-and-workspace").is_none());
}

#[test]
fn modern_owner_scoped_roots_and_selection_survive_return_switches() {
    let mut state = json!({
        "electron-renderer-content-owner": "account-a",
        "local-projects": {"local-a": {"name": "A"}},
        "electron-owner-value:electron-saved-workspace-roots:account-a": ["C:/code/a"],
        "electron-owner-value:active-workspace-roots:account-a": ["C:/code/a"],
        "electron-owner-value:electron-workspace-root-labels:account-a": {"C:/code/a": "A"},
        "electron-owner-value:selected-project:account-a": {"type": "local", "projectId": "local-a"},
        "electron-owner-atoms:account-a": {"draft": "keep private"}
    });
    assert!(inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap());
    for key in WORKSPACE_KEYS.into_iter().chain(["selected-project"]) {
        assert_eq!(
            state[format!("electron-owner-value:{key}:account-a")],
            state[format!("electron-owner-value:{key}:account-b")]
        );
    }
    let inherited = state.clone();
    assert!(!inherit_local_projects(root(&mut state), "account-b", "account-a").unwrap());
    assert!(!inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap());
    assert_eq!(state, inherited);
    assert!(state.get(LEGACY_STATE).is_none());
    assert!(state.get("electron-owner-atoms:account-b").is_none());
}

#[test]
fn legacy_inactive_owner_can_supply_local_project_navigation() {
    let mut state = json!({
        "electron-account-renderer-state-v1": {
            "owner": "account-a", "inactive": {
                "account-a": {"atoms": {"private": true}, "globals": {
                    "local-projects": {"local-a": {"name": "A"}},
                    "project-order": ["local-a"],
                    "electron-saved-workspace-roots": ["C:/code/a"]
                }}
            }
        }
    });
    inherit_local_projects(root(&mut state), "account-a", "account-b").unwrap();
    assert_eq!(
        state[LEGACY_STATE]["inactive"]["account-b"]["globals"]["project-order"],
        json!(["local-a"])
    );
    assert_eq!(
        state[LEGACY_STATE]["inactive"]["account-b"]["atoms"],
        json!({})
    );
}

#[test]
fn protect_switch_leaves_missing_same_owner_and_unowned_state_untouched() {
    let profile = temporary_profile("no-op");
    let backup = profile.join("backups").join("desktop-state.json");
    let path = profile.join(GLOBAL_STATE_FILE);
    protect_account_switch(&profile, Some(&target_auth()), &backup).unwrap();
    assert!(!path.exists());
    assert!(!backup.exists());

    let owner = auth_owner(Some(&target_auth())).unwrap();
    let original = format!("{{\r\n  \"{OWNER}\" : \"{owner}\",\r\n  \"custom\" : true\r\n}}\r\n");
    fs::write(&path, &original).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    protect_account_switch(&profile, Some(&target_auth()), &backup).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert!(!backup.exists());

    let unowned = b"{\r\n  \"custom\" : true\r\n}\r\n";
    fs::write(&path, unowned).unwrap();
    protect_account_switch(&profile, Some("invalid-auth"), &backup).unwrap();
    assert_eq!(fs::read(&path).unwrap(), unowned);
    assert!(!backup.exists());
    fs::remove_dir_all(profile).unwrap();
}

#[test]
fn protect_switch_rejects_corrupt_state_without_overwriting_original_or_backup() {
    let profile = temporary_profile("corrupt");
    let backup = profile.join("desktop-state.backup.json");
    let path = profile.join(GLOBAL_STATE_FILE);
    fs::write(&backup, "previous backup").unwrap();
    for original in [
        "{broken",
        "[]",
        r#"{"electron-renderer-content-owner":"account-a","electron-saved-workspace-roots":["C:/code/a"],"electron-account-renderer-state-v1":[]}"#,
    ] {
        fs::write(&path, original).unwrap();
        assert!(protect_account_switch(&profile, Some(&target_auth()), &backup).is_err());
        assert_eq!(fs::read(&path).unwrap(), original.as_bytes());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "previous backup");
    }
    fs::remove_dir_all(profile).unwrap();
}

#[test]
fn protect_switch_backs_up_original_and_publishes_local_project_inheritance() {
    let profile = temporary_profile("switch");
    let backup = profile.join("backups").join("desktop-state.json");
    let path = profile.join(GLOBAL_STATE_FILE);
    let original = serde_json::to_string_pretty(&legacy_fixture()).unwrap();
    fs::write(&path, &original).unwrap();
    let auth = target_auth();
    let owner = auth_owner(Some(&auth)).unwrap();
    protect_account_switch(&profile, Some(&auth), &backup).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), original.as_bytes());
    let updated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(updated[OWNER], "account-a");
    assert_eq!(
        updated[LEGACY_STATE]["inactive"][&owner]["globals"]["project-order"],
        json!(["local-a"])
    );
    assert_eq!(
        updated[LEGACY_STATE]["inactive"][&owner]["atoms"],
        json!({})
    );
    assert_eq!(updated["drafts"], legacy_fixture()["drafts"]);
    let before_retry = fs::read(&path).unwrap();
    protect_account_switch(&profile, Some(&auth), &backup).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before_retry);
    assert_eq!(fs::read(&backup).unwrap(), original.as_bytes());
    fs::remove_dir_all(profile).unwrap();
}

#[test]
fn project_reindex_is_limited_to_valid_local_projects_in_the_current_home() {
    let profile = temporary_profile("project-index");
    let identity = format!("local:{}", profile.display());
    let other_home = format!("local:{}", profile.join("other-home").display());
    let mut state = json!({
        "local-projects": {
            "local-a": {"id": "local-a", "name": "A", "rootPaths": ["C:/code/a"], "createdAt": 1, "updatedAt": 2},
            "codex:project-b": {"id": "codex:project-b", "name": "B", "rootPaths": ["C:/code/b"], "createdAt": 1, "updatedAt": 2},
            "mismatched-id": {"id": "different-id", "rootPaths": ["C:/code/c"]},
            "invalid-roots": {"id": "invalid-roots", "rootPaths": []},
            "missing-name": {"id": "missing-name", "rootPaths": ["C:/code/e"], "createdAt": 1, "updatedAt": 2},
            "missing-time": {"id": "missing-time", "name": "F", "rootPaths": ["C:/code/f"]}
        },
        "app-server-project-id-by-legacy-project-id-by-host": {
            (identity.clone()): {"local-a": "core-a", "codex:project-b": "core-b", "mismatched-id": "core-c", "invalid-roots": "core-d", "missing-name": "core-e", "missing-time": "core-f", "unknown-project": "core-unknown"},
            (other_home.clone()): {"local-a": "other-home-core"},
            "remote-host:/remote/home": {"local-a": "remote-core"}
        },
        "thread-project-assignments": {"thread-a": {"projectKind": "local", "projectId": "local-a"}}
    });
    let before = state.clone();
    assert!(invalidate_local_project_index(root(&mut state), &profile).unwrap());
    let index = &state["app-server-project-id-by-legacy-project-id-by-host"];
    assert_eq!(
        index[&identity],
        json!({"mismatched-id": "core-c", "invalid-roots": "core-d", "missing-name": "core-e", "missing-time": "core-f", "unknown-project": "core-unknown"})
    );
    for identity in [other_home.as_str(), "remote-host:/remote/home"] {
        assert_eq!(
            index[identity],
            before["app-server-project-id-by-legacy-project-id-by-host"][identity]
        );
    }
    assert_eq!(state["local-projects"], before["local-projects"]);
    assert_eq!(
        state["thread-project-assignments"],
        before["thread-project-assignments"]
    );
    assert!(!invalidate_local_project_index(root(&mut state), &profile).unwrap());
    fs::remove_dir_all(profile).unwrap();
}

#[test]
fn modern_switch_rebuilds_project_index_without_requiring_legacy_state() {
    let profile = temporary_profile("modern-reindex");
    let identity = format!("local:{}", profile.display());
    let mut state = json!({
        "electron-renderer-content-owner": "previous-owner",
        "local-projects": {"local-a": {"id": "local-a", "name": "A", "rootPaths": ["C:/code/a"], "createdAt": 1, "updatedAt": 2}},
        "project-order": ["local-a"],
        "app-server-project-id-by-legacy-project-id-by-host": {(identity.clone()): {"local-a": "missing-core-project"}}
    });
    let path = profile.join(GLOBAL_STATE_FILE);
    let backup = profile.join("before-switch.json");
    // Reapplying the same account must not invalidate a valid working index.
    state[OWNER] = json!(auth_owner(Some(&target_auth())).unwrap());
    let same_account = state.to_string();
    fs::write(&path, &same_account).unwrap();
    protect_account_switch(&profile, Some(&target_auth()), &backup).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), same_account);
    assert!(!backup.exists());

    state[OWNER] = json!("previous-owner");
    let original = state.to_string();
    fs::write(&path, &original).unwrap();
    protect_account_switch(&profile, Some(&target_auth()), &backup).unwrap();
    let updated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        updated["app-server-project-id-by-legacy-project-id-by-host"][&identity],
        json!({})
    );
    assert_eq!(updated["local-projects"], state["local-projects"]);
    assert_eq!(updated["project-order"], json!(["local-a"]));
    assert_eq!(fs::read_to_string(&backup).unwrap(), original);
    assert!(updated.get(LEGACY_STATE).is_none());
    fs::remove_dir_all(profile).unwrap();
}
