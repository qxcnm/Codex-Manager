use super::*;
use codexmanager_core::storage::{Account, Storage};
use rusqlite::Connection;

fn temp_profile(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-{name}-{unique}"))
}

fn cleanup_profile(dir: &Path) {
    if let Ok(root) = managed_profile_root(dir) {
        let _ = fs::remove_dir_all(root);
    }
    let _ = fs::remove_dir_all(dir);
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn test_account(id: &str, status: &str) -> Account {
    Account {
        id: id.to_string(),
        label: format!("Label {id}"),
        issuer: format!("issuer-{id}"),
        chatgpt_account_id: Some(format!("cgpt-{id}")),
        workspace_id: Some(format!("ws-{id}")),
        group_name: Some("test-group".to_string()),
        sort: 0,
        status: status.to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

fn test_token(account_id: &str, access_token: &str, refresh_token: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        api_key_access_token: None,
        last_refresh: 123,
    }
}

fn write_test_rollout(dir: &Path, thread_id: &str, provider: &str) -> (PathBuf, String) {
    let rollout_dir = dir.join("sessions").join("2026").join("06").join("06");
    fs::create_dir_all(&rollout_dir).expect("mkdir rollout");
    let path = rollout_dir.join(format!("rollout-2026-06-06T00-00-00-{thread_id}.jsonl"));
    let event_line = r#"{"timestamp":"2026-06-06T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"keep me"}}"#.to_string();
    let content = format!(
        "{{\"timestamp\":\"2026-06-06T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"model_provider\":\"{provider}\",\"cwd\":\"/tmp\"}}}}\n{event_line}\n"
    );
    fs::write(&path, content).expect("write rollout");
    (path, event_line)
}

fn create_state_db(dir: &Path, thread_id: &str, provider: &str) {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at INTEGER,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000, 1770000000000)",
        params![thread_id, provider],
    )
    .expect("insert thread");
}

fn sqlite_provider(dir: &Path, thread_id: &str) -> String {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.query_row(
        "SELECT model_provider FROM threads WHERE id = ?1",
        params![thread_id],
        |row| row.get::<_, String>(0),
    )
    .expect("read provider")
}

#[test]
fn direct_config_removes_only_managed_provider() {
    let input = r#"
model_provider = "cm"
model = "gpt-5.4"

[model_providers.cm]
name = "CodexManager"
base_url = "http://localhost:48760/v1"
wire_api = "responses"

[model_providers.other]
name = "Other"
base_url = "https://example.test/v1"
"#;

    let output = patch_config_for_direct(Some(input.to_string())).expect("patch direct");

    assert!(!output.contains("model_provider = \"cm\""));
    assert!(!output.contains("[model_providers.cm]"));
    assert!(output.contains("[model_providers.other]"));
    assert!(output.contains("model = \"gpt-5.4\""));
}

#[test]
fn gateway_config_sets_managed_provider_and_preserves_other_values() {
    let input = r#"
model = "gpt-5.4"

[model_providers.other]
name = "Other"
"#;

    let output = patch_config_for_gateway(Some(input.to_string()), "http://127.0.0.1:48770/v1")
        .expect("patch gateway");

    assert!(output.contains("model_provider = \"cm\""));
    assert!(output.contains("[model_providers.cm]"));
    assert!(output.contains("base_url = \"http://127.0.0.1:48770/v1\""));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(output.contains("[model_providers.other]"));
}

#[test]
fn invalid_toml_is_rejected() {
    assert!(patch_config_for_gateway(Some("bad = [".to_string()), "http://x/v1").is_err());
}

#[test]
fn usable_account_token_candidates_by_account_indexes_candidates() {
    let candidates = usable_account_token_candidates_by_account(vec![
        AccountTokenCandidate {
            account_id: "acc-ready".to_string(),
            has_access_token: true,
            has_refresh_token: true,
            last_refresh: 10,
        },
        AccountTokenCandidate {
            account_id: "acc-no-access".to_string(),
            has_access_token: false,
            has_refresh_token: true,
            last_refresh: 11,
        },
        AccountTokenCandidate {
            account_id: "acc-no-refresh".to_string(),
            has_access_token: true,
            has_refresh_token: false,
            last_refresh: 12,
        },
    ]);

    assert_eq!(candidates.len(), 3);
    assert_eq!(
        candidates
            .get("acc-ready")
            .map(|candidate| candidate.last_refresh),
        Some(10)
    );
    assert_eq!(
        candidates
            .get("acc-no-access")
            .map(|candidate| candidate.last_refresh),
        Some(11)
    );
    assert_eq!(
        candidates
            .get("acc-no-refresh")
            .map(|candidate| candidate.last_refresh),
        Some(12)
    );
}

#[test]
fn list_candidates_uses_active_account_projection_and_usable_tokens() {
    let _lock = crate::test_env_guard();
    let dir = temp_profile("codex-profile-candidates");
    fs::create_dir_all(&dir).expect("mkdir temp dir");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    let mut active = test_account("acc-active-candidate", "active");
    active.label = "Active Candidate".to_string();
    active.group_name = Some("candidate-group".to_string());
    let mut disabled = test_account("acc-disabled-candidate", "disabled");
    disabled.label = "Disabled Candidate".to_string();
    storage
        .insert_account(&active)
        .expect("insert active account");
    storage
        .insert_account(&disabled)
        .expect("insert disabled account");
    storage
        .insert_token(&test_token("acc-active-candidate", "access", "refresh"))
        .expect("insert active token");
    storage
        .insert_token(&test_token("acc-disabled-candidate", "access", "refresh"))
        .expect("insert disabled token");
    storage
        .insert_account(&test_account("acc-missing-refresh", "active"))
        .expect("insert missing refresh account");
    storage
        .insert_token(&test_token("acc-missing-refresh", "access", ""))
        .expect("insert missing refresh token");
    drop(storage);

    let result = list_candidates().expect("list candidates");

    assert_eq!(result.accounts.len(), 1);
    let account = &result.accounts[0];
    assert_eq!(account.id, "acc-active-candidate");
    assert_eq!(account.label, "Active Candidate");
    assert_eq!(account.group_name.as_deref(), Some("candidate-group"));
    assert_eq!(account.status, "active");
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("cgpt-acc-active-candidate")
    );
    assert_eq!(
        account.workspace_id.as_deref(),
        Some("ws-acc-active-candidate")
    );
    assert_eq!(account.issuer, "issuer-acc-active-candidate");
    assert_eq!(account.last_refresh, 123);
    cleanup_profile(&dir);
}

#[test]
fn restore_optional_file_removes_files_that_were_missing() {
    let dir = temp_profile("restore-missing");
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("auth.json");
    fs::write(&path, "{}").expect("write");

    restore_optional_file(&path, None).expect("restore missing");

    assert!(!path.exists());
    cleanup_profile(&dir);
}

#[test]
fn auth_json_shapes_match_codex_modes() {
    let now = now_ts();
    let account = AccountDirectAuthProfile {
        id: "acc-1".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("chatgpt-1".to_string()),
        status: "active".to_string(),
    };
    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    let direct = build_direct_auth_json(&account, &token).expect("direct auth");
    let gateway = build_gateway_auth_json("cm-key").expect("gateway auth");

    assert!(auth_json_has_tokens(&direct));
    assert!(!auth_json_is_gateway(&direct));
    assert!(auth_json_is_gateway(&gateway));
}

#[test]
fn write_profile_files_uses_internal_marker() {
    let dir = temp_profile("internal-marker");
    let state = ManagedState {
        profile_dir: profile_key(&dir),
        mode: CodexProfileMode::Gateway,
        account_id: None,
        api_key_id: Some("key-1".to_string()),
        gateway_base_url: Some("http://localhost:48760/v1".to_string()),
        provider_id: PROVIDER_ID.to_string(),
        updated_at: now_ts(),
    };

    write_profile_files(&dir, "{}", "", state).expect("write profile");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!dir.join(MARKER_FILE).exists());
    let status = status_for_profile(&dir).expect("status");
    assert!(matches!(status.mode, CodexProfileMode::Gateway));
    assert_eq!(
        status.marker_path,
        paths.marker_path.to_string_lossy().to_string()
    );
    cleanup_profile(&dir);
}

#[test]
fn legacy_marker_migrates_to_internal_marker() {
    let dir = temp_profile("legacy-marker");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: CodexProfileMode::DirectAccount,
        account_id: Some("acc-1".to_string()),
        api_key_id: None,
        gateway_base_url: None,
        provider_id: PROVIDER_ID.to_string(),
        updated_at: now_ts(),
    };
    fs::write(
        dir.join(MARKER_FILE),
        serde_json::to_string_pretty(&marker).expect("marker json"),
    )
    .expect("write legacy marker");

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!paths.legacy_marker_path.exists());
    assert!(matches!(status.mode, CodexProfileMode::DirectAccount));
    cleanup_profile(&dir);
}

#[test]
fn legacy_history_backups_migrate_and_are_pruned() {
    let dir = temp_profile("legacy-history-backups");
    let legacy_root = dir.join(HISTORY_BACKUP_DIR);
    fs::create_dir_all(&legacy_root).expect("mkdir legacy root");
    for index in 0..5 {
        let backup_dir = legacy_root.join(format!("backup-{index}"));
        fs::create_dir_all(&backup_dir).expect("mkdir legacy backup");
        fs::write(backup_dir.join("file.txt"), format!("backup-{index}"))
            .expect("write legacy backup");
    }

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(!paths.legacy_history_backup_root.exists());
    assert!(paths.history_backup_root.exists());
    assert_eq!(status.history_backup_count, MAX_HISTORY_BACKUPS_PER_PROFILE);
    cleanup_profile(&dir);
}

#[test]
fn history_repair_aligns_direct_and_gateway_providers() {
    let dir = temp_profile("history-provider");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-provider";
    let (rollout_path, event_line) = write_test_rollout(&dir, thread_id, PROVIDER_ID);
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");

    let direct = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(direct.warnings.is_empty(), "{:?}", direct.warnings);
    assert_eq!(direct.changed_rollout_file_count, 1);
    assert_eq!(direct.updated_sqlite_row_count, 1);
    assert_eq!(
        sqlite_provider(&dir, thread_id),
        DEFAULT_HISTORY_PROVIDER_ID
    );
    let direct_rollout = fs::read_to_string(&rollout_path).expect("read direct rollout");
    assert!(direct_rollout.contains("\"model_provider\":\"openai\""));
    assert!(direct_rollout.contains(&event_line));
    assert!(!dir.join(HISTORY_BACKUP_DIR).exists());
    let direct_backup = direct.backup_dir.as_ref().expect("direct backup dir");
    assert!(direct_backup.contains(MANAGED_PROFILE_ROOT_DIR));
    let direct_backup_path = PathBuf::from(direct_backup);
    assert!(direct_backup_path.join(STATE_DB_FILE).exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-wal"))
        .exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-shm"))
        .exists());
    assert!(direct_backup_path
        .join(HISTORY_BACKUP_MANIFEST_FILE)
        .exists());

    let gateway = repair_history_for_provider(&dir, PROVIDER_ID);

    assert!(gateway.warnings.is_empty(), "{:?}", gateway.warnings);
    assert_eq!(gateway.changed_rollout_file_count, 1);
    assert_eq!(gateway.updated_sqlite_row_count, 1);
    assert_eq!(sqlite_provider(&dir, thread_id), PROVIDER_ID);
    let gateway_rollout = fs::read_to_string(&rollout_path).expect("read gateway rollout");
    assert!(gateway_rollout.contains("\"model_provider\":\"cm\""));
    assert!(gateway_rollout.contains(&event_line));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_appends_missing_session_index_once() {
    let dir = temp_profile("history-index");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index";
    create_state_db(&dir, thread_id, DEFAULT_HISTORY_PROVIDER_ID);

    let first = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);
    let second = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(first.added_session_index_entry_count, 1);
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(second.added_session_index_entry_count, 0);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert_eq!(index.lines().count(), 1);
    assert!(index.contains(thread_id));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_handles_sqlite_with_only_updated_at_ms() {
    let dir = temp_profile("history-index-updated-ms-only");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index-ms";
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000000)",
        params![thread_id, DEFAULT_HISTORY_PROVIDER_ID],
    )
    .expect("insert thread");
    drop(conn);

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert_eq!(summary.added_session_index_entry_count, 1);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert!(index.contains(thread_id));
    assert!(index.contains("2026"));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_reports_sqlite_lock_as_warning() {
    let dir = temp_profile("history-locked");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-locked";
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");
    let lock_conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open lock sqlite");
    lock_conn
        .execute("BEGIN IMMEDIATE", [])
        .expect("begin immediate");

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert_eq!(summary.updated_sqlite_row_count, 0);
    assert!(
        summary
            .warnings
            .iter()
            .any(|warning| warning.contains("update Codex history sqlite provider failed")),
        "{:?}",
        summary.warnings
    );
    drop(lock_conn);
    cleanup_profile(&dir);
}
