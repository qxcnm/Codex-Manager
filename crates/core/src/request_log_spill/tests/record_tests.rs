use super::record::*;
use super::{classify_generation, GenerationCheck, CLEAR_EPOCH_IN_FLIGHT};

pub(super) fn sample_meta(trace_id: &str) -> SpillRecordMeta {
    SpillRecordMeta {
        trace_id: trace_id.to_string(),
        stage: "upstream:00000000000000000007".to_string(),
        generation: Some(3),
        clear_epoch: 2,
        boot_id: 99,
        redact: true,
        preview: false,
        conversation_key: Some("gk_1|conv".to_string()),
        attempt: Some(SpillAttemptMeta {
            method: "POST".to_string(),
            url: "https://example.test/v1/responses".to_string(),
            transport: "http".to_string(),
            content_encoding: Some("zstd".to_string()),
        }),
        created_at: 1_700_000_123,
    }
}

fn encode(meta: &SpillRecordMeta, body: &[u8], wire: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    write_record(
        &mut out,
        &SpillRecordRef {
            meta,
            body,
            wire_body: wire,
        },
    )
    .unwrap();
    out
}

fn decode_one(bytes: &[u8]) -> ReadOutcome {
    let mut reader = bytes;
    read_record(&mut reader, bytes.len() as u64).unwrap()
}

#[test]
fn record_round_trips_with_separate_wire_body() {
    let meta = sample_meta("trc_1");
    let bytes = encode(&meta, br#"{"input":[1,2,3]}"#, Some(b"\x28\xb5compressed"));
    let ReadOutcome::Record(record, frame_len) = decode_one(&bytes) else {
        panic!("record expected");
    };
    assert_eq!(frame_len, bytes.len() as u64);
    assert_eq!(record.meta, meta);
    assert_eq!(record.body(), br#"{"input":[1,2,3]}"#);
    assert_eq!(record.wire_body(), Some(&b"\x28\xb5compressed"[..]));
}

#[test]
fn record_round_trips_minimal_meta_and_binary_body() {
    let meta = SpillRecordMeta {
        trace_id: "trc_min".to_string(),
        stage: "client".to_string(),
        generation: None,
        ..Default::default()
    };
    let body: Vec<u8> = (0..=255_u8).collect();
    // A wire body without attempt metadata is ignored.
    let bytes = encode(&meta, &body, Some(b"ignored"));
    let ReadOutcome::Record(record, _) = decode_one(&bytes) else {
        panic!("record expected");
    };
    assert_eq!(record.meta, meta);
    assert_eq!(record.body(), &body[..]);
    assert_eq!(record.wire_body(), None);
}

#[test]
fn truncated_records_are_torn_and_bad_crc_is_corrupt() {
    let meta = sample_meta("trc_torn");
    let bytes = encode(&meta, b"0123456789abcdef", None);
    for cut in [1, RECORD_HEADER_LEN - 1, RECORD_HEADER_LEN, bytes.len() - 1] {
        let mut reader = &bytes[..cut];
        // The caller does not know the real length (crash): use the file end.
        assert!(
            matches!(
                read_record(&mut reader, cut as u64).unwrap(),
                ReadOutcome::Torn
            ),
            "cut at {cut}"
        );
    }
    let mut damaged = bytes.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0x55;
    assert!(matches!(decode_one(&damaged), ReadOutcome::Corrupt("crc")));
    let mut wrong_magic = bytes.clone();
    wrong_magic[0] = b'X';
    assert!(matches!(
        decode_one(&wrong_magic),
        ReadOutcome::Corrupt("magic")
    ));
    let mut wrong_version = bytes;
    wrong_version[4] = 200;
    assert!(matches!(
        decode_one(&wrong_version),
        ReadOutcome::Corrupt("version")
    ));
    assert!(matches!(decode_one(&[]), ReadOutcome::End));
}

#[test]
fn corrupt_length_never_reads_past_available_bytes() {
    let meta = sample_meta("trc_len");
    let mut bytes = encode(&meta, b"body", None);
    bytes[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(decode_one(&bytes), ReadOutcome::Torn));
}

#[test]
fn generation_classification_rejects_unverifiable_jobs() {
    assert_eq!(
        classify_generation(Some(4), 0, 1, 2, 9),
        GenerationCheck::Known(4)
    );
    assert_eq!(
        classify_generation(None, 3, 7, 7, 3),
        GenerationCheck::ResolveAtWrite
    );
    assert_eq!(
        classify_generation(Some(-1), 3, 7, 7, 3),
        GenerationCheck::ResolveAtWrite
    );
    // A clear started after capture.
    assert_eq!(
        classify_generation(None, 3, 7, 7, 4),
        GenerationCheck::Reject
    );
    // Captured while a clear was in flight.
    assert_eq!(
        classify_generation(None, CLEAR_EPOCH_IN_FLIGHT, 7, 7, CLEAR_EPOCH_IN_FLIGHT),
        GenerationCheck::Reject
    );
    // Replayed from an earlier process run.
    assert_eq!(
        classify_generation(None, 0, 6, 7, 0),
        GenerationCheck::Reject
    );
}
