//! Spill segment files: `request-log-spill/seg-<20 digits>.log`.
//!
//! Portability rules (macOS / Windows / Linux):
//! * only `std::fs`, no mmap;
//! * a file is never deleted or renamed while this process still has it
//!   open; deletes are retried with backoff because Windows scanners may
//!   hold short-lived handles;
//! * file names are lower-case ASCII digits, paths are built with
//!   `PathBuf::join`;
//! * the directory is owned by one process through an exclusive lock on
//!   `.lock` (`File::try_lock`); unix directories are created with 0700.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::record::{encode_frame, read_record, DecodedRecord, ReadOutcome, SpillRecordRef};

pub const SPILL_DIR_NAME: &str = "request-log-spill";
pub const SPILL_LOCK_FILE_NAME: &str = ".lock";
pub const SEGMENT_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const SPILL_SYNC_INTERVAL: Duration = Duration::from_secs(1);
const SEGMENT_PREFIX: &str = "seg-";
const SEGMENT_SUFFIX: &str = ".log";
const DELETE_ATTEMPTS: u32 = 4;
const DELETE_BACKOFF: Duration = Duration::from_millis(25);

/// Position in the segment stream: `(segment id, byte offset)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpillPos {
    pub segment: u64,
    pub offset: u64,
}

impl SpillPos {
    pub const fn new(segment: u64, offset: u64) -> Self {
        Self { segment, offset }
    }
}

pub fn segment_file_name(id: u64) -> String {
    format!("{SEGMENT_PREFIX}{id:020}{SEGMENT_SUFFIX}")
}

pub fn parse_segment_file_name(name: &str) -> Option<u64> {
    let digits = name
        .strip_prefix(SEGMENT_PREFIX)?
        .strip_suffix(SEGMENT_SUFFIX)?;
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub fn segment_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(segment_file_name(id))
}

/// Existing segments `(id, len)` sorted by id.
pub fn list_segments(dir: &Path) -> io::Result<Vec<(u64, u64)>> {
    let mut segments = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(segments),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let entry = entry?;
        let Some(id) = entry.file_name().to_str().and_then(parse_segment_file_name) else {
            continue;
        };
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            segments.push((id, metadata.len()));
        }
    }
    segments.sort_unstable();
    Ok(segments)
}

/// Create the spill directory (0700 on unix).
pub fn prepare_spill_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Delete a closed file, retrying with backoff. A missing file counts as
/// deleted.
pub fn remove_file_with_retry(path: &Path, attempts: u32, backoff: Duration) -> io::Result<()> {
    let mut delay = backoff;
    let mut last_error = None;
    for attempt in 0..attempts.max(1) {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => last_error = Some(err),
        }
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
            delay = delay.saturating_mul(2);
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("remove failed")))
}

#[derive(Debug)]
pub enum SpillOpenError {
    /// Another process owns the spill directory.
    Locked,
    Io(io::Error),
}

impl std::fmt::Display for SpillOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Locked => f.write_str("spill directory is locked by another process"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

struct ActiveSegment {
    id: u64,
    file: BufWriter<File>,
    len: u64,
    flushed_len: u64,
    dirty: bool,
    last_sync: Instant,
}

/// Append side of the spill directory. Owned by a single thread.
pub struct SpillStore {
    dir: PathBuf,
    _lock: File,
    segment_max: u64,
    active: Option<ActiveSegment>,
    /// Live segments `id -> len` (closed ones and the active one).
    segments: BTreeMap<u64, u64>,
    next_id: u64,
    pending_deletes: Vec<u64>,
    replay_start: SpillPos,
    committed: SpillPos,
}

impl SpillStore {
    /// Lock the directory and index the segments left by an earlier run.
    pub fn open(dir: &Path, segment_max: u64) -> Result<Self, SpillOpenError> {
        prepare_spill_dir(dir).map_err(SpillOpenError::Io)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(SPILL_LOCK_FILE_NAME))
            .map_err(SpillOpenError::Io)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(SpillOpenError::Locked),
            Err(fs::TryLockError::Error(err)) => return Err(SpillOpenError::Io(err)),
        }
        let existing = list_segments(dir).map_err(SpillOpenError::Io)?;
        let segments: BTreeMap<u64, u64> = existing.iter().copied().collect();
        let next_id = existing.last().map_or(1, |(id, _)| id + 1);
        let replay_start = existing
            .first()
            .map_or(SpillPos::new(next_id, 0), |(id, _)| SpillPos::new(*id, 0));
        let committed = existing
            .last()
            .map_or(SpillPos::new(next_id, 0), |(id, len)| {
                SpillPos::new(*id, *len)
            });
        Ok(Self {
            dir: dir.to_path_buf(),
            _lock: lock,
            segment_max: segment_max.max(1),
            active: None,
            segments,
            next_id,
            pending_deletes: Vec::new(),
            replay_start,
            committed,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// First position to replay (segments left by an earlier run).
    pub fn replay_start(&self) -> SpillPos {
        self.replay_start
    }

    /// End of the data that is flushed and therefore readable.
    pub fn committed(&self) -> SpillPos {
        self.committed
    }

    /// Bytes of all live segment files.
    pub fn live_bytes(&self) -> u64 {
        self.segments.values().sum()
    }

    pub fn segment_ids(&self) -> Vec<u64> {
        self.segments.keys().copied().collect()
    }

    pub fn active_segment(&self) -> Option<u64> {
        self.active.as_ref().map(|active| active.id)
    }

    /// End of the last appended record (flushed or not).
    pub fn write_position(&self) -> Option<SpillPos> {
        self.active
            .as_ref()
            .map(|active| SpillPos::new(active.id, active.len))
    }

    /// Whether a record of `segment` ending at byte `end` is complete in
    /// the stream. After a failed write or flush the segment is abandoned
    /// and its length is the size actually on disk, so records past it are
    /// lost (torn). Missing segments were consumed and deleted.
    pub fn record_survives(&self, segment: u64, end: u64) -> bool {
        self.segments.get(&segment).is_none_or(|len| *len >= end)
    }

    fn open_active(&mut self) -> io::Result<()> {
        let id = self.next_id;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(segment_path(&self.dir, id))?;
        self.next_id += 1;
        self.segments.insert(id, 0);
        self.active = Some(ActiveSegment {
            id,
            file: BufWriter::with_capacity(256 * 1024, file),
            len: 0,
            flushed_len: 0,
            dirty: false,
            last_sync: Instant::now(),
        });
        Ok(())
    }

    /// Append one record, rolling to a new segment when the active one
    /// would exceed the size limit. Returns the frame length.
    pub fn append(&mut self, record: &SpillRecordRef<'_>) -> io::Result<u64> {
        let frame = encode_frame(record)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "spill record too large"))?;
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.len > 0 && active.len + frame.frame_len > self.segment_max)
        {
            self.close_active()?;
        }
        if self.active.is_none() {
            self.open_active()?;
        }
        let wire = record.wire_body.filter(|_| record.meta.attempt.is_some());
        let result = {
            let active = self.active.as_mut().expect("active segment");
            active
                .file
                .write_all(&frame.header)
                .and_then(|_| active.file.write_all(&frame.meta))
                .and_then(|_| match wire {
                    Some(wire) => active.file.write_all(wire),
                    None => Ok(()),
                })
                .and_then(|_| active.file.write_all(record.body))
        };
        match result {
            Ok(()) => {
                let active = self.active.as_mut().expect("active segment");
                active.len += frame.frame_len;
                active.dirty = true;
                self.segments.insert(active.id, active.len);
                Ok(frame.frame_len)
            }
            Err(err) => {
                // The segment may now end with a partial record: never append
                // to it again. Replay treats the garbage as a torn tail.
                self.abandon_active();
                Err(err)
            }
        }
    }

    fn abandon_active(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.file.flush();
            let id = active.id;
            drop(active);
            if let Ok(metadata) = fs::metadata(segment_path(&self.dir, id)) {
                self.segments.insert(id, metadata.len());
            }
            self.committed = SpillPos::new(self.next_id, 0).max(self.committed);
        }
    }

    /// Make everything appended so far readable by [`SpillReader`].
    pub fn flush(&mut self) -> io::Result<SpillPos> {
        if let Some(active) = self.active.as_mut() {
            if active.flushed_len != active.len {
                if let Err(err) = active.file.flush() {
                    self.abandon_active();
                    return Err(err);
                }
                active.flushed_len = active.len;
            }
            self.committed = SpillPos::new(active.id, active.len);
        }
        Ok(self.committed)
    }

    /// `sync_data` at most once per [`SPILL_SYNC_INTERVAL`].
    pub fn maybe_sync(&mut self, now: Instant) -> io::Result<()> {
        if let Some(active) = self.active.as_mut() {
            if active.dirty && now.duration_since(active.last_sync) >= SPILL_SYNC_INTERVAL {
                if let Err(err) = active.file.flush() {
                    self.abandon_active();
                    return Err(err);
                }
                active.flushed_len = active.len;
                active.file.get_ref().sync_data()?;
                active.dirty = false;
                active.last_sync = now;
                self.committed = SpillPos::new(active.id, active.len);
            }
        }
        Ok(())
    }

    /// Flush, `sync_data` and close the active segment.
    pub fn close_active(&mut self) -> io::Result<()> {
        let Some(mut active) = self.active.take() else {
            return Ok(());
        };
        let flushed = active.file.flush();
        let id = active.id;
        let len = active.len;
        let result = flushed.and_then(|_| active.file.get_ref().sync_data());
        let flush_failed = result.is_err() && active.file.buffer().len() > 0;
        drop(active);
        let durable_len = if flush_failed {
            // Only what reached the file counts; later records are torn.
            fs::metadata(segment_path(&self.dir, id)).map_or(0, |metadata| metadata.len())
        } else {
            len
        };
        self.segments.insert(id, durable_len.min(len));
        self.committed = SpillPos::new(id, durable_len.min(len)).max(self.committed);
        if flush_failed {
            self.committed = SpillPos::new(self.next_id, 0).max(self.committed);
        }
        result
    }

    fn delete_segment(&mut self, id: u64) {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            return;
        }
        match remove_file_with_retry(
            &segment_path(&self.dir, id),
            DELETE_ATTEMPTS,
            DELETE_BACKOFF,
        ) {
            Ok(()) => {
                self.segments.remove(&id);
                self.pending_deletes.retain(|pending| *pending != id);
            }
            Err(_) => {
                if !self.pending_deletes.contains(&id) {
                    self.pending_deletes.push(id);
                }
            }
        }
    }

    /// Delete segments fully consumed up to `consumed`. When everything
    /// committed was consumed the active segment is closed and removed too.
    /// The caller guarantees that no reader holds a handle on them.
    pub fn release_consumed(&mut self, consumed: SpillPos) -> usize {
        let mut doomed: Vec<u64> = self
            .segments
            .keys()
            .copied()
            .filter(|id| *id < consumed.segment)
            .collect();
        if consumed >= self.committed {
            if let Some(active) = self.active.as_ref() {
                if active.len == active.flushed_len && active.id <= consumed.segment {
                    let id = active.id;
                    let _ = self.close_active();
                    doomed.push(id);
                }
            } else if self.segments.contains_key(&consumed.segment) {
                doomed.push(consumed.segment);
            }
        }
        let before = self.segments.len();
        for id in doomed {
            self.delete_segment(id);
        }
        before.saturating_sub(self.segments.len())
    }

    /// Drop everything (log clear): close the active segment, delete every
    /// segment and return the new empty stream position.
    pub fn purge_all(&mut self) -> SpillPos {
        let _ = self.close_active();
        let ids: Vec<u64> = self.segments.keys().copied().collect();
        for id in ids {
            self.delete_segment(id);
        }
        let position = SpillPos::new(self.next_id, 0);
        self.committed = position;
        self.replay_start = position;
        position
    }

    /// Close the active segment so later appends start a new one, and
    /// return the boundary: every record appended so far lies before it.
    /// A log clear rolls when it starts and later deletes only the
    /// segments before its boundary ([`Self::purge_before`]), so records
    /// captured while the clear runs survive it.
    /// A failed final flush leaves at most a torn tail, which the reader
    /// skips; the boundary is valid either way.
    pub fn roll(&mut self) -> SpillPos {
        let _ = self.close_active();
        let boundary = SpillPos::new(self.next_id, 0);
        self.committed = self.committed.max(boundary);
        boundary
    }

    /// Delete every segment before `boundary` (from [`Self::roll`]).
    /// Returns the number of segments deleted now.
    pub fn purge_before(&mut self, boundary: SpillPos) -> usize {
        let ids: Vec<u64> = self
            .segments
            .keys()
            .copied()
            .filter(|id| *id < boundary.segment)
            .collect();
        let before = self.segments.len();
        for id in ids {
            self.delete_segment(id);
        }
        self.replay_start = self.replay_start.max(boundary);
        self.committed = self.committed.max(boundary);
        before.saturating_sub(self.segments.len())
    }

    /// Retry deletes that failed earlier (e.g. a scanner held the file).
    pub fn retry_pending_deletes(&mut self) -> usize {
        let pending = std::mem::take(&mut self.pending_deletes);
        for id in pending {
            self.delete_segment(id);
        }
        self.pending_deletes.len()
    }

    pub fn pending_delete_count(&self) -> usize {
        self.pending_deletes.len()
    }
}

/// Result of one [`SpillReader::read`] call.
#[derive(Debug, Default)]
pub struct SpillReadBatch {
    pub records: Vec<DecodedRecord>,
    /// Position after the last consumed (or skipped) byte.
    pub next: SpillPos,
    pub torn: u64,
    pub corrupt: u64,
    pub bytes: u64,
}

/// Sequential reader. Opens a segment per call and closes it before
/// returning, so it never holds handles the store wants to delete.
pub struct SpillReader {
    dir: PathBuf,
}

impl SpillReader {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// Read records from `from` up to `committed`, stopping after
    /// `max_records` records or once `max_bytes` payload bytes were read
    /// (at least one record is returned when available). Damaged regions
    /// are skipped: in a closed segment to its end, in the committed
    /// segment up to the committed offset.
    pub fn read(
        &self,
        from: SpillPos,
        committed: SpillPos,
        max_records: usize,
        max_bytes: u64,
    ) -> io::Result<SpillReadBatch> {
        let mut batch = SpillReadBatch {
            next: from,
            ..Default::default()
        };
        while batch.next < committed
            && batch.records.len() < max_records.max(1)
            && (batch.records.is_empty() || batch.bytes < max_bytes)
        {
            let segment = batch.next.segment;
            let path = segment_path(&self.dir, segment);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    batch.next = if segment < committed.segment {
                        SpillPos::new(segment + 1, 0)
                    } else {
                        committed
                    };
                    continue;
                }
                Err(err) => return Err(err),
            };
            let file_len = file.metadata()?.len();
            let limit = if segment == committed.segment {
                committed.offset.min(file_len)
            } else {
                file_len
            };
            let skip_target = if segment < committed.segment {
                SpillPos::new(segment + 1, 0)
            } else {
                committed
            };
            if batch.next.offset >= limit {
                batch.next = skip_target;
                continue;
            }
            let mut reader = BufReader::with_capacity(256 * 1024, file);
            reader.seek(SeekFrom::Start(batch.next.offset))?;
            let mut offset = batch.next.offset;
            loop {
                if batch.records.len() >= max_records.max(1)
                    || (!batch.records.is_empty() && batch.bytes >= max_bytes)
                {
                    batch.next = SpillPos::new(segment, offset);
                    return Ok(batch);
                }
                match read_record(&mut reader, limit - offset)? {
                    ReadOutcome::Record(record, frame_len) => {
                        offset += frame_len;
                        batch.bytes += frame_len;
                        batch.records.push(record);
                    }
                    ReadOutcome::End => {
                        batch.next = skip_target;
                        break;
                    }
                    ReadOutcome::Torn => {
                        batch.torn += 1;
                        batch.next = skip_target;
                        break;
                    }
                    ReadOutcome::Corrupt(_) => {
                        batch.corrupt += 1;
                        batch.next = skip_target;
                        break;
                    }
                }
                if offset >= limit {
                    batch.next = if segment < committed.segment {
                        SpillPos::new(segment + 1, 0)
                    } else {
                        SpillPos::new(segment, offset)
                    };
                    break;
                }
            }
        }
        Ok(batch)
    }
}

#[cfg(test)]
#[path = "tests/segment_tests.rs"]
mod tests;
