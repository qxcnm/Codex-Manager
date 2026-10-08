//! On-disk record format of the request payload spill segments.
//!
//! ```text
//! frame   := magic(4) version(1) payload_len(u32 LE) crc32(u32 LE) payload
//! payload := meta wire_body? body
//! ```
//!
//! `crc32` covers the whole payload. A record whose header or payload is cut
//! short (crash while appending) is reported as [`ReadOutcome::Torn`]; a
//! record with a bad magic, version, CRC or meta block is
//! [`ReadOutcome::Corrupt`]. Both are skipped and counted by the caller.
//!
//! A record whose body was rewritten before spilling (redaction) carries
//! [`SpillOriginalDigests`] of the bytes that were actually sent and never
//! carries a wire body, so no unredacted copy reaches the disk.

use std::io::{self, Read, Write};
use std::ops::Range;

pub const RECORD_MAGIC: [u8; 4] = *b"CMRL";
pub const RECORD_VERSION: u8 = 1;
pub const RECORD_HEADER_LEN: usize = 13;
/// Largest payload a single frame can carry (u32 length field).
pub const MAX_RECORD_PAYLOAD_BYTES: u64 = u32::MAX as u64;

const FLAG_REDACT: u8 = 1;
const FLAG_PREVIEW: u8 = 1 << 1;
const FLAG_GENERATION: u8 = 1 << 2;
const FLAG_CONVERSATION: u8 = 1 << 3;
const FLAG_ATTEMPT: u8 = 1 << 4;
const FLAG_CONTENT_ENCODING: u8 = 1 << 5;
const FLAG_WIRE_SEPARATE: u8 = 1 << 6;
const FLAG_ORIGINAL_DIGESTS: u8 = 1 << 7;

/// Transport metadata of an outbound attempt (the wire body travels
/// separately in the payload).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpillAttemptMeta {
    pub method: String,
    pub url: String,
    pub transport: String,
    pub content_encoding: Option<String>,
}

/// Digests of the original request bytes, kept when the spilled body is a
/// rewritten (redacted) copy so the stored hashes and sizes still describe
/// what was received and sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpillOriginalDigests {
    /// SHA-256 (hex) of the original body.
    pub body_sha256: String,
    /// Length of the original body in bytes.
    pub body_len: u64,
    /// SHA-256 (hex) of the attempt's wire bytes (equal to `body_sha256`
    /// when the wire body was the body itself).
    pub wire_sha256: Option<String>,
}

/// Job header persisted in front of the raw body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpillRecordMeta {
    pub trace_id: String,
    pub stage: String,
    /// Clear generation snapshot; `None` when the process mirror was not
    /// initialized at capture time (then `clear_epoch` + `boot_id` decide).
    pub generation: Option<i64>,
    pub clear_epoch: u64,
    pub boot_id: u64,
    pub redact: bool,
    pub preview: bool,
    pub conversation_key: Option<String>,
    pub attempt: Option<SpillAttemptMeta>,
    pub created_at: i64,
    /// Set when the body is a rewritten copy; such records never carry a
    /// wire body.
    pub original: Option<SpillOriginalDigests>,
}

/// Borrowed view used for encoding. `wire_body == None` means the wire body
/// is identical to `body` (or there is no attempt). The wire body is
/// dropped when `meta.original` is set.
#[derive(Debug, Clone, Copy)]
pub struct SpillRecordRef<'a> {
    pub meta: &'a SpillRecordMeta,
    pub body: &'a [u8],
    pub wire_body: Option<&'a [u8]>,
}

impl<'a> SpillRecordRef<'a> {
    /// Wire body that is actually written: only for attempts, and never for
    /// a rewritten body.
    pub fn wire_to_write(&self) -> Option<&'a [u8]> {
        self.wire_body
            .filter(|_| self.meta.attempt.is_some() && self.meta.original.is_none())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordEncodeError {
    TooLarge,
}

/// Frame header plus encoded meta block; body slices are written after it
/// without being copied into an intermediate buffer.
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub header: [u8; RECORD_HEADER_LEN],
    pub meta: Vec<u8>,
    pub frame_len: u64,
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

/// Encode the frame header and meta block for `record`.
pub fn encode_frame(record: &SpillRecordRef<'_>) -> Result<EncodedFrame, RecordEncodeError> {
    let meta = record.meta;
    let mut flags = 0_u8;
    if meta.redact {
        flags |= FLAG_REDACT;
    }
    if meta.preview {
        flags |= FLAG_PREVIEW;
    }
    if meta.generation.is_some() {
        flags |= FLAG_GENERATION;
    }
    if meta.conversation_key.is_some() {
        flags |= FLAG_CONVERSATION;
    }
    let wire = record.wire_to_write();
    if let Some(attempt) = meta.attempt.as_ref() {
        flags |= FLAG_ATTEMPT;
        if attempt.content_encoding.is_some() {
            flags |= FLAG_CONTENT_ENCODING;
        }
        if wire.is_some() {
            flags |= FLAG_WIRE_SEPARATE;
        }
    }
    if meta.original.is_some() {
        flags |= FLAG_ORIGINAL_DIGESTS;
    }
    let mut out = Vec::with_capacity(128 + meta.trace_id.len() + meta.stage.len());
    out.push(flags);
    out.extend_from_slice(&meta.generation.unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&meta.clear_epoch.to_le_bytes());
    out.extend_from_slice(&meta.boot_id.to_le_bytes());
    out.extend_from_slice(&meta.created_at.to_le_bytes());
    put_str(&mut out, &meta.trace_id);
    put_str(&mut out, &meta.stage);
    if let Some(key) = meta.conversation_key.as_deref() {
        put_str(&mut out, key);
    }
    if let Some(attempt) = meta.attempt.as_ref() {
        put_str(&mut out, &attempt.method);
        put_str(&mut out, &attempt.url);
        put_str(&mut out, &attempt.transport);
        if let Some(encoding) = attempt.content_encoding.as_deref() {
            put_str(&mut out, encoding);
        }
    }
    if let Some(original) = meta.original.as_ref() {
        put_str(&mut out, &original.body_sha256);
        out.extend_from_slice(&original.body_len.to_le_bytes());
        match original.wire_sha256.as_deref() {
            Some(wire_sha256) => {
                out.push(1);
                put_str(&mut out, wire_sha256);
            }
            None => out.push(0),
        }
    }
    if let Some(wire) = wire {
        out.extend_from_slice(&(wire.len() as u64).to_le_bytes());
    }
    out.extend_from_slice(&(record.body.len() as u64).to_le_bytes());

    let payload_len =
        out.len() as u64 + wire.map_or(0, |wire| wire.len() as u64) + record.body.len() as u64;
    if payload_len > MAX_RECORD_PAYLOAD_BYTES {
        return Err(RecordEncodeError::TooLarge);
    }
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&out);
    if let Some(wire) = wire {
        hasher.update(wire);
    }
    hasher.update(record.body);
    let crc = hasher.finalize();
    let mut header = [0_u8; RECORD_HEADER_LEN];
    header[..4].copy_from_slice(&RECORD_MAGIC);
    header[4] = RECORD_VERSION;
    header[5..9].copy_from_slice(&(payload_len as u32).to_le_bytes());
    header[9..13].copy_from_slice(&crc.to_le_bytes());
    Ok(EncodedFrame {
        header,
        meta: out,
        frame_len: RECORD_HEADER_LEN as u64 + payload_len,
    })
}

/// Encode and write one record. Returns the number of bytes written.
pub fn write_record<W: Write>(writer: &mut W, record: &SpillRecordRef<'_>) -> io::Result<u64> {
    let frame = encode_frame(record)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "spill record too large"))?;
    writer.write_all(&frame.header)?;
    writer.write_all(&frame.meta)?;
    if let Some(wire) = record.wire_to_write() {
        writer.write_all(wire)?;
    }
    writer.write_all(record.body)?;
    Ok(frame.frame_len)
}

/// A decoded record. Body and wire body are ranges into `payload` so the
/// caller can turn the payload into a shared buffer without copying.
#[derive(Debug, Clone)]
pub struct DecodedRecord {
    pub meta: SpillRecordMeta,
    pub payload: Vec<u8>,
    pub body_range: Range<usize>,
    pub wire_range: Option<Range<usize>>,
}

impl DecodedRecord {
    pub fn body(&self) -> &[u8] {
        &self.payload[self.body_range.clone()]
    }

    pub fn wire_body(&self) -> Option<&[u8]> {
        self.wire_range.clone().map(|range| &self.payload[range])
    }
}

#[derive(Debug)]
pub enum ReadOutcome {
    /// One record and the total frame length it occupied.
    Record(DecodedRecord, u64),
    /// Clean end: no bytes left before the limit.
    End,
    /// Header or payload cut short (crash during append).
    Torn,
    /// Bad magic / version / CRC / meta block.
    Corrupt(&'static str),
}

fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(filled)
}

/// Read the next record. `available` is the number of bytes that may still
/// be consumed (end of the committed region); a length field pointing past
/// it is treated as a torn tail so corrupt lengths never trigger huge
/// allocations.
pub fn read_record<R: Read>(reader: &mut R, available: u64) -> io::Result<ReadOutcome> {
    if available == 0 {
        return Ok(ReadOutcome::End);
    }
    if available < RECORD_HEADER_LEN as u64 {
        return Ok(ReadOutcome::Torn);
    }
    let mut header = [0_u8; RECORD_HEADER_LEN];
    let read = read_full(reader, &mut header)?;
    if read == 0 {
        return Ok(ReadOutcome::End);
    }
    if read < RECORD_HEADER_LEN {
        return Ok(ReadOutcome::Torn);
    }
    if header[..4] != RECORD_MAGIC {
        return Ok(ReadOutcome::Corrupt("magic"));
    }
    if header[4] != RECORD_VERSION {
        return Ok(ReadOutcome::Corrupt("version"));
    }
    let payload_len = u32::from_le_bytes([header[5], header[6], header[7], header[8]]) as u64;
    let crc = u32::from_le_bytes([header[9], header[10], header[11], header[12]]);
    if payload_len > available - RECORD_HEADER_LEN as u64 {
        return Ok(ReadOutcome::Torn);
    }
    let mut payload = vec![0_u8; payload_len as usize];
    let read = read_full(reader, &mut payload)?;
    if read < payload.len() {
        return Ok(ReadOutcome::Torn);
    }
    if crc32fast::hash(&payload) != crc {
        return Ok(ReadOutcome::Corrupt("crc"));
    }
    match decode_payload(payload) {
        Ok(record) => Ok(ReadOutcome::Record(
            record,
            RECORD_HEADER_LEN as u64 + payload_len,
        )),
        Err(reason) => Ok(ReadOutcome::Corrupt(reason)),
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], &'static str> {
        let end = self.pos.checked_add(len).ok_or("length overflow")?;
        if end > self.data.len() {
            return Err("meta truncated");
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, &'static str> {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn i64(&mut self) -> Result<i64, &'static str> {
        Ok(self.u64()? as i64)
    }

    fn string(&mut self) -> Result<String, &'static str> {
        let mut len = [0_u8; 4];
        len.copy_from_slice(self.take(4)?);
        let bytes = self.take(u32::from_le_bytes(len) as usize)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| "meta string not utf-8")
    }
}

/// Decode a CRC-verified payload.
pub fn decode_payload(payload: Vec<u8>) -> Result<DecodedRecord, &'static str> {
    let mut cursor = Cursor {
        data: &payload,
        pos: 0,
    };
    let flags = cursor.u8()?;
    let generation = cursor.i64()?;
    let clear_epoch = cursor.u64()?;
    let boot_id = cursor.u64()?;
    let created_at = cursor.i64()?;
    let trace_id = cursor.string()?;
    let stage = cursor.string()?;
    let conversation_key = if flags & FLAG_CONVERSATION != 0 {
        Some(cursor.string()?)
    } else {
        None
    };
    let attempt = if flags & FLAG_ATTEMPT != 0 {
        let method = cursor.string()?;
        let url = cursor.string()?;
        let transport = cursor.string()?;
        let content_encoding = if flags & FLAG_CONTENT_ENCODING != 0 {
            Some(cursor.string()?)
        } else {
            None
        };
        Some(SpillAttemptMeta {
            method,
            url,
            transport,
            content_encoding,
        })
    } else {
        None
    };
    let original = if flags & FLAG_ORIGINAL_DIGESTS != 0 {
        let body_sha256 = cursor.string()?;
        let body_len = cursor.u64()?;
        let wire_sha256 = match cursor.u8()? {
            0 => None,
            1 => Some(cursor.string()?),
            _ => return Err("bad wire digest flag"),
        };
        Some(SpillOriginalDigests {
            body_sha256,
            body_len,
            wire_sha256,
        })
    } else {
        None
    };
    if original.is_some() && flags & FLAG_WIRE_SEPARATE != 0 {
        return Err("rewritten body with a wire body");
    }
    let wire_len = if flags & FLAG_WIRE_SEPARATE != 0 {
        Some(cursor.u64()? as usize)
    } else {
        None
    };
    let body_len = cursor.u64()? as usize;
    let wire_range = match wire_len {
        Some(len) => {
            let start = cursor.pos;
            cursor.take(len)?;
            Some(start..cursor.pos)
        }
        None => None,
    };
    let body_start = cursor.pos;
    cursor.take(body_len)?;
    let body_range = body_start..cursor.pos;
    if cursor.pos != payload.len() {
        return Err("trailing bytes");
    }
    Ok(DecodedRecord {
        meta: SpillRecordMeta {
            trace_id,
            stage,
            generation: (flags & FLAG_GENERATION != 0).then_some(generation),
            clear_epoch,
            boot_id,
            redact: flags & FLAG_REDACT != 0,
            preview: flags & FLAG_PREVIEW != 0,
            conversation_key,
            attempt,
            created_at,
            original,
        },
        payload,
        body_range,
        wire_range,
    })
}
