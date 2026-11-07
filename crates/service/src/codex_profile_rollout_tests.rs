use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "codexmanager-rollout-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(managed) = super::super::managed_profile_root(&self.0) {
            let _ = fs::remove_dir_all(managed);
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn rollout_content(id: &str, provider: &str) -> String {
    format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"{provider}\",\"cwd\":\"/fixture\"}}}}\r\n{{ \"type\": \"event_msg\", \"payload\": {{\"message\":\"keep original spacing\"}} }}\r\n{{\"type\":\"response_item\",\"payload\":{{\"text\":\"preserve tail\"}}}}"
    )
}

fn write_rollout(home: &Path, area: &str, id: &str, content: &str, compressed: bool) -> PathBuf {
    let directory = home.join(area).join("2026/10/26");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "rollout-2026-10-26T00-00-00-{id}.jsonl{}",
        if compressed { ".zst" } else { "" }
    ));
    let bytes = if compressed {
        encode_compressed(content.as_bytes()).unwrap()
    } else {
        content.as_bytes().to_vec()
    };
    fs::write(&path, bytes).unwrap();
    path
}

fn decoded(path: &Path) -> String {
    String::from_utf8(zstd::stream::decode_all(File::open(path).unwrap()).unwrap()).unwrap()
}

#[test]
fn compressed_history_repair_preserves_both_areas_backups_and_non_metadata_events() {
    let fixture = Fixture::new("compressed");
    let mut originals = Vec::new();
    for (area, id) in [
        ("sessions", "active-fixture"),
        ("archived_sessions", "archived-fixture"),
    ] {
        let content = rollout_content(id, "openai");
        let path = write_rollout(&fixture.0, area, id, &content, true);
        originals.push((path.clone(), fs::read(path).unwrap(), content));
    }

    let repaired = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert!(repaired.warnings.is_empty(), "{:?}", repaired.warnings);
    assert_eq!(repaired.changed_rollout_file_count, 2);
    let backup = PathBuf::from(repaired.backup_dir.unwrap());
    for (path, original, content) in &originals {
        assert!(path.is_file());
        assert!(!path.with_extension("").exists());
        let next = decoded(path);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(next.lines().next().unwrap()).unwrap()
                ["payload"]["model_provider"],
            "cm"
        );
        assert_eq!(
            next.split_once('\n').unwrap().1,
            content.split_once('\n').unwrap().1
        );
        assert_eq!(
            fs::read(backup.join(path.strip_prefix(&fixture.0).unwrap())).unwrap(),
            *original
        );
        assert_eq!(
            zstd::zstd_safe::get_frame_content_size(&fs::read(path).unwrap()).unwrap(),
            Some(next.len() as u64)
        );
    }
    let bytes_after_first = originals
        .iter()
        .map(|(path, _, _)| fs::read(path).unwrap())
        .collect::<Vec<_>>();
    let second = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(second.changed_rollout_file_count, 0);
    assert!(second.backup_dir.is_none());
    for ((path, _, _), expected) in originals.iter().zip(bytes_after_first) {
        assert_eq!(fs::read(path).unwrap(), expected);
    }
}

#[test]
fn plain_history_wins_over_compressed_siblings_and_temp_files_are_ignored() {
    let fixture = Fixture::new("siblings");
    let content = rollout_content("sibling-fixture", "openai");
    let plain = write_rollout(&fixture.0, "sessions", "sibling-fixture", &content, false);
    let compressed = write_rollout(&fixture.0, "sessions", "sibling-fixture", &content, true);
    let original_compressed = fs::read(&compressed).unwrap();
    let temporary = compressed.with_extension("zst.tmp");
    fs::write(&temporary, b"incomplete zstd").unwrap();

    let repaired = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert!(repaired.warnings.is_empty(), "{:?}", repaired.warnings);
    assert_eq!(repaired.changed_rollout_file_count, 1);
    assert!(fs::read_to_string(&plain)
        .unwrap()
        .contains("\"model_provider\":\"cm\""));
    assert_eq!(fs::read(&compressed).unwrap(), original_compressed);
    assert_eq!(fs::read(&temporary).unwrap(), b"incomplete zstd");
    let second = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert_eq!(second.changed_rollout_file_count, 0);
}

#[test]
fn corrupt_or_oversized_compressed_history_is_untouched_and_reported() {
    let fixture = Fixture::new("invalid-compressed");
    let corrupt = write_rollout(&fixture.0, "sessions", "corrupt-fixture", "unused", true);
    fs::write(&corrupt, b"invalid zstd stream").unwrap();
    let oversized = write_rollout(
        &fixture.0,
        "archived_sessions",
        "large-fixture",
        &format!(
            "{}{}",
            rollout_content("large-fixture", "openai"),
            "x".repeat(2048)
        ),
        true,
    );
    let original_large = fs::read(&oversized).unwrap();
    let mut warnings = Vec::new();
    let updates = collect_updates_with_limit(&fixture.0, "cm", 1024, &mut warnings);
    assert!(updates.is_empty());
    assert_eq!(warnings.len(), 2);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("safety limit")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("compressed rollout")),
        "{warnings:?}"
    );
    assert_eq!(fs::read(&corrupt).unwrap(), b"invalid zstd stream");
    assert_eq!(fs::read(&oversized).unwrap(), original_large);

    let repaired = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert!(repaired
        .warnings
        .iter()
        .any(|warning| warning.contains("corrupt-fixture")));
    assert_eq!(fs::read(&corrupt).unwrap(), b"invalid zstd stream");
}

#[test]
fn maintenance_busy_skips_rollouts_and_sqlite_together() {
    let fixture = Fixture::new("maintenance-busy");
    let path = write_rollout(
        &fixture.0,
        "sessions",
        "locked-fixture",
        &rollout_content("locked-fixture", "openai"),
        true,
    );
    let original = fs::read(&path).unwrap();
    let db = rusqlite::Connection::open(fixture.0.join(super::super::STATE_DB_FILE)).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('locked-fixture', 'openai')",
        [],
    )
    .unwrap();
    let _maintenance = try_maintenance_lock(&fixture.0).unwrap();

    let repaired = super::super::repair_history_for_provider(&fixture.0, "cm");
    assert_eq!(repaired.changed_rollout_file_count, 0);
    assert_eq!(repaired.updated_sqlite_row_count, 0);
    assert!(repaired
        .warnings
        .iter()
        .any(|warning| warning.contains("maintenance is busy")));
    assert_eq!(fs::read(&path).unwrap(), original);
    let provider: String = db
        .query_row("SELECT model_provider FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(provider, "openai");
}

#[test]
fn active_thread_writer_and_changed_source_prevent_rollout_publication() {
    let fixture = Fixture::new("writer-busy");
    let path = write_rollout(
        &fixture.0,
        "sessions",
        "writer-fixture",
        &rollout_content("writer-fixture", "openai"),
        true,
    );
    let original = fs::read(&path).unwrap();
    let backup = fixture.0.join("fixture-backup");
    fs::create_dir_all(fixture.0.join("thread-writer-locks")).unwrap();
    let thread_lock = try_lock_file(
        &fixture.0.join("thread-writer-locks/writer-fixture.lock"),
        true,
    )
    .unwrap();
    let update = prepare_update(&path, "cm", MAX_DECOMPRESSED_ROLLOUT_BYTES)
        .unwrap()
        .unwrap();
    assert!(update
        .apply(&fixture.0, &backup)
        .unwrap_err()
        .contains("busy"));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!backup.exists());
    drop(thread_lock);

    let update = prepare_update(&path, "cm", MAX_DECOMPRESSED_ROLLOUT_BYTES)
        .unwrap()
        .unwrap();
    let mut same_length_change = original.clone();
    let last = same_length_change.last_mut().unwrap();
    *last ^= 1;
    fs::write(&path, &same_length_change).unwrap();
    assert!(update
        .apply(&fixture.0, &backup)
        .unwrap_err()
        .contains("changed before repair"));
    assert_eq!(fs::read(&path).unwrap(), same_length_change);
    assert!(!backup.exists());
    fs::write(&path, &original).unwrap();

    let update = prepare_update(&path, "cm", MAX_DECOMPRESSED_ROLLOUT_BYTES)
        .unwrap()
        .unwrap();
    fs::write(&path, b"changed after planning").unwrap();
    assert!(update
        .apply(&fixture.0, &backup)
        .unwrap_err()
        .contains("changed before repair"));
    assert_eq!(fs::read(&path).unwrap(), b"changed after planning");
    assert!(!backup.exists());
}
