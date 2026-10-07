use super::*;
use crate::request_log_spill::record::{SpillAttemptMeta, SpillRecordMeta, RECORD_HEADER_LEN};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cm-spill-{name}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        Self(dir.join(SPILL_DIR_NAME))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn meta(trace_id: &str, generation: i64) -> SpillRecordMeta {
    SpillRecordMeta {
        trace_id: trace_id.to_string(),
        stage: "client".to_string(),
        generation: Some(generation),
        clear_epoch: 0,
        boot_id: 1,
        redact: false,
        preview: true,
        conversation_key: None,
        attempt: Some(SpillAttemptMeta {
            method: "POST".to_string(),
            url: "https://example.test/v1".to_string(),
            transport: "http".to_string(),
            content_encoding: None,
        }),
        created_at: 10,
    }
}

fn append(store: &mut SpillStore, trace_id: &str, body: &[u8]) -> u64 {
    let meta = meta(trace_id, 0);
    store
        .append(&SpillRecordRef {
            meta: &meta,
            body,
            wire_body: None,
        })
        .unwrap()
}

fn read_all(dir: &Path, from: SpillPos, committed: SpillPos) -> SpillReadBatch {
    SpillReader::new(dir)
        .read(from, committed, usize::MAX, u64::MAX)
        .unwrap()
}

fn traces(batch: &SpillReadBatch) -> Vec<String> {
    batch
        .records
        .iter()
        .map(|record| record.meta.trace_id.clone())
        .collect()
}

#[test]
fn segment_names_are_lowercase_fixed_width_digits() {
    assert_eq!(segment_file_name(1), "seg-00000000000000000001.log");
    assert_eq!(
        parse_segment_file_name("seg-00000000000000000042.log"),
        Some(42)
    );
    assert_eq!(parse_segment_file_name("seg-42.log"), None);
    assert_eq!(
        parse_segment_file_name("SEG-00000000000000000042.LOG"),
        None
    );
    assert_eq!(parse_segment_file_name(".lock"), None);
}

#[test]
fn appended_records_are_readable_only_after_flush() {
    let dir = TempDir::new("flush");
    let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let start = store.replay_start();
    assert_eq!(start, store.committed());
    append(&mut store, "trc_a", b"aaaa");
    assert_eq!(store.committed(), start, "unflushed data is not committed");
    let committed = store.flush().unwrap();
    append(&mut store, "trc_b", b"bbbb");
    let batch = read_all(&dir.0, start, committed);
    assert_eq!(traces(&batch), vec!["trc_a"]);
    assert_eq!(batch.next, committed);
    let committed = store.flush().unwrap();
    let batch = read_all(&dir.0, batch.next, committed);
    assert_eq!(traces(&batch), vec!["trc_b"]);
    assert_eq!(batch.records[0].body(), b"bbbb");
}

#[test]
fn segments_roll_over_and_replay_in_order_after_restart() {
    let dir = TempDir::new("roll");
    {
        let mut store = SpillStore::open(&dir.0, 256).unwrap();
        for index in 0..12 {
            append(&mut store, &format!("trc_{index:02}"), &[index as u8; 60]);
        }
        store.close_active().unwrap();
        assert!(store.segment_ids().len() > 3, "{:?}", store.segment_ids());
    }
    let mut store = SpillStore::open(&dir.0, 256).unwrap();
    let start = store.replay_start();
    let committed = store.committed();
    assert!(start < committed);
    // New appends go after the leftovers.
    append(&mut store, "trc_new", b"new");
    let committed_after = store.flush().unwrap();
    assert!(committed_after > committed);
    let batch = read_all(&dir.0, start, committed_after);
    let mut expected: Vec<String> = (0..12).map(|index| format!("trc_{index:02}")).collect();
    expected.push("trc_new".to_string());
    assert_eq!(traces(&batch), expected);
    assert_eq!(batch.torn + batch.corrupt, 0);
}

#[test]
fn reader_respects_record_and_byte_limits() {
    let dir = TempDir::new("limits");
    let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let start = store.replay_start();
    for index in 0..5 {
        append(&mut store, &format!("trc_{index}"), &[1_u8; 100]);
    }
    let committed = store.flush().unwrap();
    let reader = SpillReader::new(&dir.0);
    let first = reader.read(start, committed, 2, u64::MAX).unwrap();
    assert_eq!(traces(&first), vec!["trc_0", "trc_1"]);
    let second = reader.read(first.next, committed, 64, 1).unwrap();
    assert_eq!(traces(&second), vec!["trc_2"], "at least one record");
    let rest = reader.read(second.next, committed, 64, u64::MAX).unwrap();
    assert_eq!(traces(&rest), vec!["trc_3", "trc_4"]);
    assert_eq!(rest.next, committed);
}

#[test]
fn torn_tail_after_crash_is_skipped_and_counted() {
    let dir = TempDir::new("torn");
    {
        let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
        append(&mut store, "trc_ok", b"complete");
        append(&mut store, "trc_cut", b"this record will be cut");
        store.close_active().unwrap();
        let (id, len) = list_segments(&dir.0).unwrap()[0];
        let file = OpenOptions::new()
            .write(true)
            .open(segment_path(&dir.0, id))
            .unwrap();
        file.set_len(len - 5).unwrap();
    }
    let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let start = store.replay_start();
    append(&mut store, "trc_next", b"next");
    let committed = store.flush().unwrap();
    let batch = read_all(&dir.0, start, committed);
    assert_eq!(traces(&batch), vec!["trc_ok", "trc_next"]);
    assert_eq!(batch.torn, 1);
}

#[test]
fn corrupt_crc_skips_rest_of_closed_segment() {
    let dir = TempDir::new("crc");
    {
        let mut store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
        append(&mut store, "trc_1", b"first");
        append(&mut store, "trc_2", b"second");
        append(&mut store, "trc_3", b"third");
        store.close_active().unwrap();
        let (id, len) = list_segments(&dir.0).unwrap()[0];
        let path = segment_path(&dir.0, id);
        let mut bytes = fs::read(&path).unwrap();
        // Flip a meta byte of the second record.
        let first_frame = {
            let mut reader = &bytes[..];
            match read_record(&mut reader, len).unwrap() {
                ReadOutcome::Record(_, frame_len) => frame_len as usize,
                other => panic!("{other:?}"),
            }
        };
        bytes[first_frame + RECORD_HEADER_LEN + 2] ^= 0xff;
        fs::write(&path, bytes).unwrap();
    }
    let store = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    let batch = read_all(&dir.0, store.replay_start(), store.committed());
    assert_eq!(traces(&batch), vec!["trc_1"]);
    assert_eq!(batch.corrupt, 1);
    assert_eq!(batch.next, store.committed());
}

#[test]
fn consumed_segments_are_deleted_including_idle_active_segment() {
    let dir = TempDir::new("release");
    let mut store = SpillStore::open(&dir.0, 200).unwrap();
    let start = store.replay_start();
    for index in 0..6 {
        append(&mut store, &format!("trc_{index}"), &[7_u8; 60]);
    }
    let committed = store.flush().unwrap();
    let segments_before = store.segment_ids().len();
    assert!(segments_before >= 3);
    let partial = SpillReader::new(&dir.0)
        .read(start, committed, 2, u64::MAX)
        .unwrap();
    store.release_consumed(partial.next);
    assert!(store.segment_ids().len() < segments_before);
    assert!(store
        .segment_ids()
        .iter()
        .all(|id| *id >= partial.next.segment));
    let rest = read_all(&dir.0, partial.next, committed);
    assert_eq!(rest.next, committed);
    store.release_consumed(rest.next);
    assert!(store.segment_ids().is_empty());
    assert_eq!(store.active_segment(), None);
    assert!(list_segments(&dir.0).unwrap().is_empty());
    // The next append starts a fresh segment after the deleted ones.
    append(&mut store, "trc_after", b"x");
    let committed_after = store.flush().unwrap();
    assert!(committed_after.segment > committed.segment);
    let batch = read_all(&dir.0, rest.next, committed_after);
    assert_eq!(traces(&batch), vec!["trc_after"]);
}

#[test]
fn purge_after_clear_deletes_every_old_segment() {
    let dir = TempDir::new("purge");
    {
        let mut store = SpillStore::open(&dir.0, 200).unwrap();
        for index in 0..5 {
            append(&mut store, &format!("trc_old_{index}"), &[1_u8; 80]);
        }
        store.flush().unwrap();
    }
    let mut store = SpillStore::open(&dir.0, 200).unwrap();
    append(&mut store, "trc_unflushed", b"plaintext");
    assert!(!list_segments(&dir.0).unwrap().is_empty());
    let position = store.purge_all();
    assert!(list_segments(&dir.0).unwrap().is_empty());
    assert_eq!(store.live_bytes(), 0);
    assert_eq!(store.committed(), position);
    append(&mut store, "trc_new", b"new");
    let committed = store.flush().unwrap();
    let batch = read_all(&dir.0, position, committed);
    assert_eq!(traces(&batch), vec!["trc_new"]);
}

#[test]
fn clear_boundary_deletes_only_segments_written_before_the_roll() {
    let dir = TempDir::new("boundary");
    let mut store = SpillStore::open(&dir.0, 1 << 20).unwrap();
    append(&mut store, "trc_before_1", b"old secret");
    append(&mut store, "trc_before_2", b"old secret");
    let boundary = store.roll();
    assert_eq!(store.committed(), boundary);
    append(&mut store, "trc_during_clear", b"new body");
    let committed = store.flush().unwrap();
    assert_eq!(list_segments(&dir.0).unwrap().len(), 2);
    assert_eq!(store.purge_before(boundary), 1);
    let left = list_segments(&dir.0).unwrap();
    assert_eq!(left.len(), 1);
    assert!(left[0].0 >= boundary.segment);
    assert_eq!(store.replay_start(), boundary);
    let batch = read_all(&dir.0, boundary, committed);
    assert_eq!(traces(&batch), vec!["trc_during_clear"]);
    // Rolling an idle store still yields a boundary after all data.
    let idle = store.roll();
    assert!(idle > committed);
    assert_eq!(store.purge_before(idle), 1);
    assert!(list_segments(&dir.0).unwrap().is_empty());
}

#[test]
fn records_past_the_on_disk_length_of_an_abandoned_segment_are_lost() {
    let dir = TempDir::new("abandon");
    let mut store = SpillStore::open(&dir.0, 1 << 20).unwrap();
    let first_len = append(&mut store, "trc_kept", b"kept body");
    let second_len = append(&mut store, "trc_lost", b"lost body");
    let segment = store.active_segment().unwrap();
    store.flush().unwrap();
    // Simulate a write that only partially reached the disk.
    let path = segment_path(&dir.0, segment);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(first_len + second_len / 2)
        .unwrap();
    store.abandon_active();
    assert!(store.record_survives(segment, first_len));
    assert!(!store.record_survives(segment, first_len + second_len));
    assert!(store.active_segment().is_none());
    let batch = read_all(&dir.0, SpillPos::new(segment, 0), store.committed());
    assert_eq!(traces(&batch), vec!["trc_kept"]);
    assert_eq!(batch.torn, 1);
    // Deleted (consumed) segments count as surviving.
    assert!(store.record_survives(segment + 100, 1));
}

#[test]
fn second_owner_of_the_spill_directory_is_rejected() {
    let dir = TempDir::new("lock");
    let first = SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).unwrap();
    assert!(matches!(
        SpillStore::open(&dir.0, SEGMENT_MAX_BYTES),
        Err(SpillOpenError::Locked)
    ));
    drop(first);
    assert!(SpillStore::open(&dir.0, SEGMENT_MAX_BYTES).is_ok());
}

#[test]
fn remove_with_retry_treats_missing_files_as_deleted() {
    let dir = TempDir::new("remove");
    prepare_spill_dir(&dir.0).unwrap();
    let path = dir.0.join(segment_file_name(9));
    fs::write(&path, b"x").unwrap();
    remove_file_with_retry(&path, 3, Duration::from_millis(1)).unwrap();
    assert!(!path.exists());
    remove_file_with_retry(&path, 3, Duration::from_millis(1)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
