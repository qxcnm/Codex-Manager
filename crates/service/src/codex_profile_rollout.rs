use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// Bound both the input and expanded compressed history. Plain history keeps its
// existing read behavior; an oversized compressed file remains untouched.
const MAX_COMPRESSED_ROLLOUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECOMPRESSED_ROLLOUT_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct RolloutUpdate {
    path: PathBuf,
    content: Vec<u8>,
    source_digest: [u8; 32],
    source_len: u64,
    thread_id: String,
}

/// Use Codex's maintenance lock namespace to exclude compression and migration.
pub(super) fn try_maintenance_lock(profile_dir: &Path) -> Result<File, String> {
    let directory = profile_dir.join(".tmp");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("prepare Codex rollout maintenance lock failed: {error}"))?;
    try_lock_file(&directory.join("rollout-maintenance.lock"), true)
        .map_err(|error| format!("Codex history repair skipped: rollout maintenance {error}"))
}

pub(super) fn collect_updates(
    profile_dir: &Path,
    target_provider: &str,
    warnings: &mut Vec<String>,
) -> Vec<RolloutUpdate> {
    collect_updates_with_limit(
        profile_dir,
        target_provider,
        MAX_DECOMPRESSED_ROLLOUT_BYTES,
        warnings,
    )
}

fn collect_updates_with_limit(
    profile_dir: &Path,
    target_provider: &str,
    decoded_limit: u64,
    warnings: &mut Vec<String>,
) -> Vec<RolloutUpdate> {
    let mut files = Vec::new();
    for name in super::SESSION_DIRS {
        if let Err(error) = collect_rollout_files(&profile_dir.join(name), &mut files) {
            warnings.push(error);
        }
    }
    files.sort();
    let mut updates = Vec::new();
    for path in files {
        if is_compressed(&path) && path.with_extension("").is_file() {
            // Codex resolves a plain rollout before its compressed sibling.
            continue;
        }
        match prepare_update(&path, target_provider, decoded_limit) {
            Ok(Some(update)) => updates.push(update),
            Ok(None) => {}
            Err(error) => {
                warnings.push(format!("read rollout failed ({}): {error}", path.display()))
            }
        }
    }
    updates
}

fn collect_rollout_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "read session dir failed ({}): {error}",
                directory.display()
            ))
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("read session dir entry failed: {error}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("read rollout file type failed: {error}"))?;
        if kind.is_dir() {
            collect_rollout_files(&path, files)?;
        } else if kind.is_file()
            && (is_compressed(&path)
                || path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl")))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn is_compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".jsonl.zst"))
}

fn prepare_update(
    path: &Path,
    target_provider: &str,
    decoded_limit: u64,
) -> Result<Option<RolloutUpdate>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let source = if is_compressed(path) {
        read_bounded(file, MAX_COMPRESSED_ROLLOUT_BYTES)?
    } else {
        let mut source = Vec::new();
        std::io::BufReader::new(file)
            .read_to_end(&mut source)
            .map_err(|error| error.to_string())?;
        source
    };
    let decoded = if is_compressed(path) {
        let mut decoder = zstd::stream::read::Decoder::new(source.as_slice())
            .map_err(|error| format!("invalid compressed rollout: {error}"))?;
        // A small output limit alone does not bound the decoder's window allocation.
        decoder
            .window_log_max(27)
            .map_err(|error| format!("invalid compressed rollout window: {error}"))?;
        Some(read_bounded(decoder, decoded_limit)?)
    } else {
        None
    };
    let content = std::str::from_utf8(decoded.as_deref().unwrap_or(&source))
        .map_err(|error| format!("rollout is not UTF-8: {error}"))?;
    let Some(patched) = patch_rollout_provider(content, target_provider)? else {
        return Ok(None);
    };
    let thread_id = rollout_thread_id(content)?;
    let content = if is_compressed(path) {
        encode_compressed(patched.as_bytes())?
    } else {
        patched.into_bytes()
    };
    Ok(Some(RolloutUpdate {
        path: path.to_path_buf(),
        content,
        source_digest: Sha256::digest(&source).into(),
        source_len: source.len() as u64,
        thread_id,
    }))
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("decode/read compressed rollout failed: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "compressed rollout exceeds the {limit} byte safety limit"
        ));
    }
    Ok(bytes)
}

fn encode_compressed(content: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)
        .map_err(|error| format!("create rollout compressor failed: {error}"))?;
    encoder
        .set_pledged_src_size(Some(content.len() as u64))
        .map_err(|error| format!("set rollout size failed: {error}"))?;
    encoder
        .write_all(content)
        .map_err(|error| format!("compress rollout failed: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("finish rollout compression failed: {error}"))
}

impl RolloutUpdate {
    pub(super) fn apply(self, profile_dir: &Path, backup_dir: &Path) -> Result<(), String> {
        let _writer_guard = publication_guard(profile_dir, &self.thread_id).map_err(|error| {
            format!("rollout repair skipped ({}): {error}", self.path.display())
        })?;
        if is_compressed(&self.path) && self.path.with_extension("").exists() {
            return Err(format!(
                "rollout repair skipped because a plain sibling appeared ({})",
                self.path.display()
            ));
        }
        let permissions = self.verify_source()?;
        // Back up the physical representation before replacing it. For .zst this
        // preserves the exact original compressed bytes, not a decoded substitute.
        super::backup_existing_file(profile_dir, &self.path, backup_dir)?;
        let parent = self
            .path
            .parent()
            .ok_or("rollout has no parent directory")?;
        let temporary = super::temp_file_path(parent, &self.path);
        let mut created = false;
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| error.to_string())?;
            created = true;
            file.write_all(&self.content)
                .map_err(|error| error.to_string())?;
            file.set_permissions(permissions)
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            drop(file);
            fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|error| format!("replace rollout failed ({}): {error}", self.path.display()))
    }

    fn verify_source(&self) -> Result<fs::Permissions, String> {
        let metadata = fs::symlink_metadata(&self.path).map_err(|error| {
            format!(
                "rollout changed before repair ({}): {error}",
                self.path.display()
            )
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != self.source_len
        {
            return Err(format!(
                "rollout changed before repair ({})",
                self.path.display()
            ));
        }
        let mut file = File::open(&self.path).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 8192];
        loop {
            let length = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if length == 0 {
                break;
            }
            digest.update(&buffer[..length]);
        }
        let actual: [u8; 32] = digest.finalize().into();
        if actual != self.source_digest {
            return Err(format!(
                "rollout changed before repair ({})",
                self.path.display()
            ));
        }
        Ok(metadata.permissions())
    }
}

fn publication_guard(profile_dir: &Path, thread_id: &str) -> Result<(File, Option<File>), String> {
    let directory = profile_dir.join("thread-writer-locks");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    // Every official writer takes coordination before acquiring its thread lock.
    // Keep coordination through source verification, backup, and atomic publication.
    let coordination = try_lock_file(&directory.join(".coordination.lock"), true)?;
    let path = directory.join(format!("{thread_id}.lock"));
    let thread_lock = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => Some(lock_open_file(file).map_err(|error| format!("thread writer {error}"))?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("open thread writer lock failed: {error}")),
    };
    Ok((coordination, thread_lock))
}

fn try_lock_file(path: &Path, create: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)
        .map_err(|error| error.to_string())?;
    lock_open_file(file)
}

fn lock_open_file(file: File) -> Result<File, String> {
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err("is busy".to_string()),
        Err(std::fs::TryLockError::Error(error)) => Err(error.to_string()),
    }
}

fn rollout_thread_id(content: &str) -> Result<String, String> {
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|value| value.as_str()) != Some("session_meta") {
            continue;
        }
        let id = value["payload"]["id"].as_str().unwrap_or_default();
        if !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Ok(id.to_string());
        }
        return Err("rollout session_meta has no safe thread id; repair skipped".to_string());
    }
    Err("rollout has no session_meta thread id; repair skipped".to_string())
}

fn patch_rollout_provider(content: &str, target_provider: &str) -> Result<Option<String>, String> {
    let mut changed = false;
    let mut output = String::with_capacity(content.len());
    for segment in content.split_inclusive('\n') {
        let (line, suffix) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(patched) = patch_rollout_line_provider(line, target_provider)? {
            output.push_str(&patched);
            output.push_str(suffix);
            changed = true;
        } else {
            output.push_str(segment);
        }
    }
    Ok(changed.then_some(output))
}

fn patch_rollout_line_provider(
    line: &str,
    target_provider: &str,
) -> Result<Option<String>, String> {
    let json_text = line.trim_end_matches('\r');
    let mut value = match serde_json::from_str::<serde_json::Value>(json_text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if value.get("type").and_then(|value| value.as_str()) != Some("session_meta") {
        return Ok(None);
    }
    let Some(payload) = value
        .get_mut("payload")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(None);
    };
    if payload
        .get("model_provider")
        .and_then(|value| value.as_str())
        == Some(target_provider)
    {
        return Ok(None);
    }
    payload.insert(
        "model_provider".to_string(),
        serde_json::Value::String(target_provider.to_string()),
    );
    serde_json::to_string(&value)
        .map(|patched| Some(format!("{patched}{}", &line[json_text.len()..])))
        .map_err(|error| format!("serialize rollout session_meta failed: {error}"))
}

#[cfg(test)]
#[path = "codex_profile_rollout_tests.rs"]
mod tests;
