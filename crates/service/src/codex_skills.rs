use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

#[path = "codex_skills_external.rs"]
mod external;

#[path = "codex_skill_frontmatter.rs"]
mod frontmatter;

pub(crate) const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SINGLE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SKILL_MD_BYTES: u64 = 512 * 1024;
const MAX_FILE_COUNT: usize = 512;
const MAX_ARCHIVE_ENTRY_COUNT: usize = MAX_FILE_COUNT * 2;
const MAX_SOURCE_ENTRY_COUNT: usize = MAX_FILE_COUNT * 2;
const MAX_PATH_DEPTH: usize = 16;
const MAX_DIRECTORY_NAME_BYTES: usize = 128;
const SYSTEM_DIRECTORY_NAME: &str = ".system";
const SKILL_FILE_NAME: &str = "SKILL.md";
const STAGING_PREFIX: &str = ".codexmanager-skill-staging-";
const QUARANTINE_PREFIX: &str = ".codexmanager-skill-delete-";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillSummary {
    pub directory_name: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub deletable: bool,
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillsInventory {
    pub codex_home: String,
    pub skills_root: String,
    pub items: Vec<CodexSkillSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillMetadata {
    name: String,
    description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannedEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
struct PlannedArchiveEntry {
    index: usize,
    relative_path: PathBuf,
    kind: PlannedEntryKind,
    unix_mode: Option<u32>,
}

#[derive(Debug, Clone)]
struct SourceFileEntry {
    source_path: PathBuf,
    canonical_path: PathBuf,
    relative_path: PathBuf,
    permissions: fs::Permissions,
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn list(codex_home: Option<&str>) -> Result<CodexSkillsInventory, String> {
    let codex_home = crate::codex_profile::resolve_profile_dir(codex_home)?;
    list_for_profile(&codex_home)
}

pub(crate) fn install_zip(
    file_name: Option<&str>,
    archive_base64: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillsInventory, String> {
    let file_name = file_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing fileName".to_string())?;
    if file_name.len() > 255 || !file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err("skill archive must be a .zip file".to_string());
    }
    let encoded = archive_base64
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing archiveBase64".to_string())?;
    let max_encoded_len = MAX_ARCHIVE_BYTES.div_ceil(3).saturating_mul(4);
    if encoded.len() > max_encoded_len {
        return Err(format!(
            "skill archive exceeds the {} MiB compressed size limit",
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        ));
    }
    let archive = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| "skill archive is not valid base64".to_string())?;
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "skill archive exceeds the {} MiB compressed size limit",
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        ));
    }

    let codex_home = crate::codex_profile::resolve_profile_dir(codex_home)?;
    let skills_root = codex_home.join("skills");
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "skills mutation lock poisoned".to_string())?;
    install_zip_into_root(&skills_root, &archive)?;
    list_for_profile(&codex_home)
}

pub(crate) fn install_archive_bytes(
    archive: &[u8],
    codex_home: Option<&str>,
) -> Result<CodexSkillsInventory, String> {
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "skill archive exceeds the {} MiB compressed size limit",
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        ));
    }
    let codex_home = crate::codex_profile::resolve_profile_dir(codex_home)?;
    let skills_root = codex_home.join("skills");
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "skills mutation lock poisoned".to_string())?;
    install_zip_into_root(&skills_root, archive)?;
    list_for_profile(&codex_home)
}

pub(crate) fn parse_skill_metadata_document(content: &str) -> Result<(String, String), String> {
    let metadata = parse_skill_metadata(content)?;
    Ok((metadata.name, metadata.description))
}

pub(crate) fn import_directory(
    source_path: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillsInventory, String> {
    let source_path = source_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing sourcePath".to_string())?;
    let source_path = PathBuf::from(source_path);
    if !source_path.is_absolute() {
        return Err("sourcePath must be an absolute path on the service host".to_string());
    }

    let codex_home = crate::codex_profile::resolve_profile_dir(codex_home)?;
    let skills_root = codex_home.join("skills");
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "skills mutation lock poisoned".to_string())?;
    import_directory_into_root(&skills_root, &source_path)?;
    list_for_profile(&codex_home)
}

pub(crate) fn delete(
    directory_name: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillsInventory, String> {
    let directory_name = directory_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing directoryName".to_string())?;
    let codex_home = crate::codex_profile::resolve_profile_dir(codex_home)?;
    let skills_root = codex_home.join("skills");
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "skills mutation lock poisoned".to_string())?;
    delete_from_root(&skills_root, directory_name)?;
    list_for_profile(&codex_home)
}

fn list_for_profile(codex_home: &Path) -> Result<CodexSkillsInventory, String> {
    let user_home = crate::codex_profile::user_home_dir().ok();
    list_with_user_home(codex_home, user_home.as_deref())
}

fn list_with_user_home(
    codex_home: &Path,
    user_home: Option<&Path>,
) -> Result<CodexSkillsInventory, String> {
    let skills_root = codex_home.join("skills");
    let mut inventory = list_from_root(codex_home, &skills_root)?;
    if let Some(user_home) = user_home {
        let agents_root = user_home.join(".agents").join("skills");
        if !crate::codex_runtime::same_path(&skills_root, &agents_root) {
            external::append_agents_skills(&agents_root, &mut inventory);
            sort_skills(&mut inventory.items);
        }
    }
    Ok(inventory)
}

fn list_from_root(codex_home: &Path, skills_root: &Path) -> Result<CodexSkillsInventory, String> {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    if skills_root.exists() {
        let entries = fs::read_dir(skills_root)
            .map_err(|err| format!("read Codex skills directory failed: {err}"))?;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!("skipped unreadable skills entry: {err}"));
                    continue;
                }
            };
            let directory_name = entry.file_name().to_string_lossy().to_string();
            if directory_name.starts_with(STAGING_PREFIX)
                || directory_name.starts_with(QUARANTINE_PREFIX)
            {
                continue;
            }
            if directory_name == SYSTEM_DIRECTORY_NAME {
                scan_system_skills(&entry.path(), &mut items, &mut warnings);
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    warnings.push(format!(
                        "skipped unreadable skill directory {directory_name}: {err}"
                    ));
                    continue;
                }
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let deletable = validate_directory_name(&directory_name).is_ok();
            items.push(read_skill_summary(
                &entry.path(),
                directory_name,
                "user",
                deletable,
            ));
        }
    }
    sort_skills(&mut items);
    Ok(CodexSkillsInventory {
        codex_home: codex_home.to_string_lossy().to_string(),
        skills_root: skills_root.to_string_lossy().to_string(),
        items,
        warnings,
    })
}

fn sort_skills(items: &mut [CodexSkillSummary]) {
    items.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then_with(|| left.directory_name.cmp(&right.directory_name))
    });
}

fn scan_system_skills(
    system_root: &Path,
    items: &mut Vec<CodexSkillSummary>,
    warnings: &mut Vec<String>,
) {
    let metadata = match fs::symlink_metadata(system_root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => {
            warnings.push("ignored unsafe .system skills entry".to_string());
            return;
        }
        Err(err) => {
            warnings.push(format!("unable to inspect .system skills: {err}"));
            return;
        }
    };
    let _ = metadata;
    let entries = match fs::read_dir(system_root) {
        Ok(entries) => entries,
        Err(err) => {
            warnings.push(format!("unable to read .system skills: {err}"));
            return;
        }
    };
    for entry in entries.flatten() {
        let directory_name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        items.push(read_skill_summary(
            &entry.path(),
            format!("{SYSTEM_DIRECTORY_NAME}/{directory_name}"),
            "system",
            false,
        ));
    }
}

fn read_skill_summary(
    directory: &Path,
    directory_name: String,
    source: &str,
    deletable: bool,
) -> CodexSkillSummary {
    let fallback_name = directory
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    match read_skill_metadata(&directory.join(SKILL_FILE_NAME)) {
        Ok(metadata) => CodexSkillSummary {
            directory_name,
            name: metadata.name,
            description: metadata.description,
            source: source.to_string(),
            deletable,
            valid: true,
            error: None,
        },
        Err(err) => CodexSkillSummary {
            directory_name,
            name: fallback_name,
            description: String::new(),
            source: source.to_string(),
            deletable,
            valid: false,
            error: Some(err),
        },
    }
}

fn read_skill_metadata(path: &Path) -> Result<SkillMetadata, String> {
    let content = read_text_file_bounded(path, MAX_SKILL_MD_BYTES)?;
    parse_skill_metadata(&content)
}

fn read_text_file_bounded(path: &Path, max_bytes: u64) -> Result<String, String> {
    #[cfg(not(unix))]
    {
        let path_metadata =
            fs::symlink_metadata(path).map_err(|_| format!("missing {SKILL_FILE_NAME}"))?;
        if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
            return Err(format!("{SKILL_FILE_NAME} must be a regular file"));
        }
    }

    let file = open_read_only_no_follow(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            format!("missing {SKILL_FILE_NAME}")
        } else {
            format!("unable to read {SKILL_FILE_NAME}")
        }
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| format!("unable to inspect {SKILL_FILE_NAME}"))?;
    if !is_regular_file_without_reparse(&metadata) {
        return Err(format!("{SKILL_FILE_NAME} must be a regular file"));
    }
    if metadata.len() > max_bytes {
        return Err(format!("{SKILL_FILE_NAME} is too large"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("unable to read {SKILL_FILE_NAME}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("{SKILL_FILE_NAME} is too large"));
    }
    String::from_utf8(bytes).map_err(|_| format!("{SKILL_FILE_NAME} must be UTF-8"))
}

fn open_read_only_no_follow(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

fn is_regular_file_without_reparse(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return false;
        }
    }
    true
}

fn parse_skill_metadata(content: &str) -> Result<SkillMetadata, String> {
    let (name, description) = frontmatter::parse(content)?;
    validate_directory_name(&name)?;
    Ok(SkillMetadata { name, description })
}

fn validate_directory_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > MAX_DIRECTORY_NAME_BYTES
        || name == SYSTEM_DIRECTORY_NAME
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(
            "skill name must use 1-128 ASCII letters, numbers, dots, underscores, or hyphens"
                .to_string(),
        );
    }
    validate_portable_path_component(name.as_ref())?;
    Ok(())
}

fn install_zip_into_root(skills_root: &Path, archive_bytes: &[u8]) -> Result<String, String> {
    if archive_bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("skill archive exceeds compressed size limit".to_string());
    }
    let cursor = Cursor::new(archive_bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|err| format!("invalid ZIP archive: {err}"))?;
    let plans = plan_archive_entries(&mut archive)?;
    let skill_plan = plans
        .iter()
        .find(|entry| entry.relative_path == Path::new(SKILL_FILE_NAME))
        .ok_or_else(|| format!("ZIP archive does not contain a root {SKILL_FILE_NAME}"))?;
    let metadata_content = read_zip_entry_text(&mut archive, skill_plan.index)?;
    let skill_metadata = parse_skill_metadata(&metadata_content)?;
    install_planned_archive(skills_root, &mut archive, &plans, &skill_metadata.name)?;
    Ok(skill_metadata.name)
}

fn plan_archive_entries<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<PlannedArchiveEntry>, String> {
    if archive.len() == 0 {
        return Err("ZIP archive is empty".to_string());
    }
    if archive.len() > MAX_ARCHIVE_ENTRY_COUNT {
        return Err(format!(
            "ZIP archive exceeds the {MAX_ARCHIVE_ENTRY_COUNT} entry limit"
        ));
    }

    let mut raw_entries = Vec::with_capacity(archive.len());
    let mut root_skill_candidates = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|err| format!("unable to inspect ZIP entry: {err}"))?;
        if entry.encrypted() {
            return Err("encrypted ZIP entries are not supported".to_string());
        }
        if entry.is_symlink() || zip_mode_is_unsupported(entry.unix_mode(), entry.is_dir()) {
            return Err("ZIP archive contains a symlink or special file".to_string());
        }
        let path = entry
            .enclosed_name()
            .ok_or_else(|| "ZIP archive contains an unsafe path".to_string())?;
        validate_relative_path(&path, MAX_PATH_DEPTH + 1)?;
        let kind = if entry.is_dir() {
            PlannedEntryKind::Directory
        } else {
            PlannedEntryKind::File
        };
        if kind == PlannedEntryKind::File
            && path.file_name().and_then(|value| value.to_str()) == Some(SKILL_FILE_NAME)
        {
            root_skill_candidates.push(path.parent().unwrap_or(Path::new("")).to_path_buf());
        }
        raw_entries.push((index, path, kind, entry.size(), entry.unix_mode()));
    }

    if root_skill_candidates.len() != 1 {
        return Err(format!(
            "ZIP archive must contain exactly one skill with a root {SKILL_FILE_NAME}"
        ));
    }
    let root_prefix = root_skill_candidates.remove(0);
    if root_prefix.components().count() > 1 {
        return Err(format!(
            "ZIP archive must contain exactly one skill with a root {SKILL_FILE_NAME}"
        ));
    }

    let mut plans = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut file_count = 0usize;
    let mut total_size = 0u64;
    for (index, path, kind, size, unix_mode) in raw_entries {
        let relative_path = if root_prefix.as_os_str().is_empty() {
            path
        } else if path == root_prefix {
            continue;
        } else {
            path.strip_prefix(&root_prefix)
                .map_err(|_| "ZIP archive contains files outside the skill root".to_string())?
                .to_path_buf()
        };
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        validate_relative_path(&relative_path, MAX_PATH_DEPTH)?;
        let collision_key = relative_path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if !seen_paths.insert(collision_key) {
            return Err("ZIP archive contains duplicate or case-conflicting paths".to_string());
        }
        if kind == PlannedEntryKind::File {
            file_count = file_count.saturating_add(1);
            if file_count > MAX_FILE_COUNT {
                return Err(format!(
                    "ZIP archive exceeds the {MAX_FILE_COUNT} file limit"
                ));
            }
            if size > MAX_SINGLE_FILE_BYTES {
                return Err("ZIP archive contains a file that is too large".to_string());
            }
            total_size = total_size
                .checked_add(size)
                .ok_or_else(|| "ZIP archive size overflow".to_string())?;
            if total_size > MAX_TOTAL_UNCOMPRESSED_BYTES {
                return Err("ZIP archive exceeds the uncompressed size limit".to_string());
            }
        }
        plans.push(PlannedArchiveEntry {
            index,
            relative_path,
            kind,
            unix_mode,
        });
    }
    if file_count == 0 {
        return Err("ZIP archive contains no files".to_string());
    }
    Ok(plans)
}

fn zip_mode_is_unsupported(unix_mode: Option<u32>, is_dir: bool) -> bool {
    const TYPE_MASK: u32 = 0o170000;
    const TYPE_FILE: u32 = 0o100000;
    const TYPE_DIR: u32 = 0o040000;
    let Some(mode) = unix_mode else {
        return false;
    };
    let file_type = mode & TYPE_MASK;
    if file_type == 0 {
        return false;
    }
    if is_dir {
        file_type != TYPE_DIR
    } else {
        file_type != TYPE_FILE
    }
}

fn read_zip_entry_text<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
) -> Result<String, String> {
    let mut entry = archive
        .by_index(index)
        .map_err(|err| format!("unable to read {SKILL_FILE_NAME}: {err}"))?;
    if entry.size() > MAX_SKILL_MD_BYTES {
        return Err(format!("{SKILL_FILE_NAME} is too large"));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(MAX_SKILL_MD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("unable to read {SKILL_FILE_NAME}: {err}"))?;
    if bytes.len() as u64 > MAX_SKILL_MD_BYTES {
        return Err(format!("{SKILL_FILE_NAME} is too large"));
    }
    String::from_utf8(bytes).map_err(|_| format!("{SKILL_FILE_NAME} must be UTF-8"))
}

fn install_planned_archive<R: Read + std::io::Seek>(
    skills_root: &Path,
    archive: &mut ZipArchive<R>,
    plans: &[PlannedArchiveEntry],
    directory_name: &str,
) -> Result<(), String> {
    validate_directory_name(directory_name)?;
    create_managed_skills_root(skills_root)?;
    let destination = skills_root.join(directory_name);
    ensure_destination_available(&destination)?;
    let staging = create_staging_directory(skills_root)?;
    let result = (|| {
        let mut total_written = 0u64;
        let mut files_written = 0usize;
        for plan in plans {
            let output_path = staging.join(&plan.relative_path);
            match plan.kind {
                PlannedEntryKind::Directory => {
                    create_skill_subdirectories(&staging, &plan.relative_path)?;
                }
                PlannedEntryKind::File => {
                    if let Some(parent) = plan.relative_path.parent() {
                        if !parent.as_os_str().is_empty() {
                            create_skill_subdirectories(&staging, parent)?;
                        }
                    }
                    let mut input = archive
                        .by_index(plan.index)
                        .map_err(|err| format!("read ZIP entry failed: {err}"))?;
                    let written = copy_reader_bounded(
                        &mut input,
                        &output_path,
                        &mut total_written,
                        &mut files_written,
                    )?;
                    if written != input.size() {
                        return Err("ZIP entry size changed while extracting".to_string());
                    }
                    apply_archive_permissions(&output_path, plan.unix_mode)?;
                }
            }
        }
        for plan in plans {
            if plan.kind == PlannedEntryKind::Directory {
                apply_archive_directory_permissions(
                    &staging.join(&plan.relative_path),
                    plan.unix_mode,
                )?;
            }
        }
        let staged_metadata = read_skill_metadata(&staging.join(SKILL_FILE_NAME))?;
        if staged_metadata.name != directory_name {
            return Err(format!("{SKILL_FILE_NAME} changed while installing"));
        }
        ensure_destination_available(&destination)?;
        fs::rename(&staging, &destination)
            .map_err(|err| format!("activate installed skill failed: {err}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn import_directory_into_root(skills_root: &Path, source_path: &Path) -> Result<String, String> {
    create_managed_skills_root(skills_root)?;
    let source_input_metadata = fs::symlink_metadata(source_path)
        .map_err(|err| format!("inspect source skill directory failed: {err}"))?;
    if source_input_metadata.file_type().is_symlink() || !source_input_metadata.is_dir() {
        return Err("sourcePath must be a regular directory, not a symbolic link".to_string());
    }
    let source = fs::canonicalize(source_path)
        .map_err(|err| format!("resolve source skill directory failed: {err}"))?;
    let source_metadata = fs::symlink_metadata(&source)
        .map_err(|err| format!("inspect source skill directory failed: {err}"))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err("sourcePath must be a regular directory".to_string());
    }
    if !metadata_refers_to_same_object(&source_input_metadata, &source_metadata) {
        return Err("source skill directory changed while resolving its path".to_string());
    }
    let root = fs::canonicalize(skills_root)
        .map_err(|err| format!("resolve Codex skills directory failed: {err}"))?;
    if source.starts_with(&root) || root.starts_with(&source) {
        return Err("source skill must not overlap the managed skills directory".to_string());
    }
    let skill_metadata = read_skill_metadata(&source.join(SKILL_FILE_NAME))?;
    validate_directory_name(&skill_metadata.name)?;
    let destination = skills_root.join(&skill_metadata.name);
    ensure_destination_available(&destination)?;

    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut file_count = 0usize;
    let mut entry_count = 0usize;
    let mut total_size = 0u64;
    collect_source_entries(
        &source,
        &source,
        &mut files,
        &mut directories,
        &mut file_count,
        &mut entry_count,
        &mut total_size,
    )?;
    let staging = create_staging_directory(skills_root)?;
    let result = (|| {
        for relative in &directories {
            create_skill_subdirectories(&staging, relative)?;
        }
        let mut total_written = 0u64;
        let mut files_written = 0usize;
        for entry in &files {
            let output_path = staging.join(&entry.relative_path);
            if let Some(parent) = entry.relative_path.parent() {
                if !parent.as_os_str().is_empty() {
                    create_skill_subdirectories(&staging, parent)?;
                }
            }
            let mut input = open_validated_source_file(entry)?;
            let written = copy_reader_bounded(
                &mut input,
                &output_path,
                &mut total_written,
                &mut files_written,
            )?;
            if written != entry.size {
                return Err("source skill changed while importing".to_string());
            }
            validate_open_source_file(entry, &input)?;
            validate_source_file_path(entry)?;
            apply_imported_permissions(&output_path, &entry.permissions)?;
        }
        let staged_metadata = read_skill_metadata(&staging.join(SKILL_FILE_NAME))?;
        if staged_metadata.name != skill_metadata.name {
            return Err(format!("{SKILL_FILE_NAME} changed while importing"));
        }
        ensure_destination_available(&destination)?;
        fs::rename(&staging, &destination)
            .map_err(|err| format!("activate imported skill failed: {err}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|_| skill_metadata.name)
}

fn collect_source_entries(
    root: &Path,
    current: &Path,
    files: &mut Vec<SourceFileEntry>,
    directories: &mut Vec<PathBuf>,
    file_count: &mut usize,
    entry_count: &mut usize,
    total_size: &mut u64,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|err| format!("read source skill directory failed: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read source skill entry failed: {err}"))?;
        *entry_count = entry_count.saturating_add(1);
        if *entry_count > MAX_SOURCE_ENTRY_COUNT {
            return Err(format!(
                "source skill exceeds the {MAX_SOURCE_ENTRY_COUNT} entry limit"
            ));
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "source skill path escaped its root".to_string())?
            .to_path_buf();
        validate_relative_path(&relative, MAX_PATH_DEPTH)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| format!("inspect source skill entry failed: {err}"))?;
        if metadata.file_type().is_symlink() {
            return Err("source skill contains a symbolic link".to_string());
        }
        let canonical_path = fs::canonicalize(&path)
            .map_err(|err| format!("resolve source skill entry failed: {err}"))?;
        if !canonical_path.starts_with(root) {
            return Err("source skill entry escaped its root".to_string());
        }
        if metadata.is_dir() {
            directories.push(relative);
            collect_source_entries(
                root,
                &path,
                files,
                directories,
                file_count,
                entry_count,
                total_size,
            )?;
        } else if metadata.is_file() {
            *file_count = file_count.saturating_add(1);
            if *file_count > MAX_FILE_COUNT {
                return Err(format!(
                    "source skill exceeds the {MAX_FILE_COUNT} file limit"
                ));
            }
            if metadata.len() > MAX_SINGLE_FILE_BYTES {
                return Err("source skill contains a file that is too large".to_string());
            }
            *total_size = total_size
                .checked_add(metadata.len())
                .ok_or_else(|| "source skill size overflow".to_string())?;
            if *total_size > MAX_TOTAL_UNCOMPRESSED_BYTES {
                return Err("source skill exceeds the total size limit".to_string());
            }
            files.push(SourceFileEntry {
                source_path: path,
                canonical_path,
                relative_path: relative,
                permissions: metadata.permissions(),
                size: metadata.len(),
                modified: metadata.modified().ok(),
                #[cfg(unix)]
                device: {
                    use std::os::unix::fs::MetadataExt;
                    metadata.dev()
                },
                #[cfg(unix)]
                inode: {
                    use std::os::unix::fs::MetadataExt;
                    metadata.ino()
                },
            });
        } else {
            return Err("source skill contains a special file".to_string());
        }
    }
    Ok(())
}

fn validate_relative_path(path: &Path, max_depth: usize) -> Result<(), String> {
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(value) if !value.is_empty() => {
                validate_portable_path_component(value)?;
                depth = depth.saturating_add(1);
            }
            _ => return Err("skill contains an unsafe path".to_string()),
        }
    }
    if depth == 0 || depth > max_depth {
        return Err(format!(
            "skill path depth must be between 1 and {max_depth}"
        ));
    }
    Ok(())
}

fn validate_portable_path_component(value: &std::ffi::OsStr) -> Result<(), String> {
    let value = value
        .to_str()
        .ok_or_else(|| "skill path must be valid UTF-8".to_string())?;
    if value.ends_with(['.', ' '])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        || is_windows_reserved_name(value)
    {
        return Err("skill contains a path that is not portable to Windows".to_string());
    }
    Ok(())
}

fn is_windows_reserved_name(value: &str) -> bool {
    let basename = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(
        basename.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn metadata_refers_to_same_object(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev() && left.ino() == right.ino();
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.modified().ok() == right.modified().ok()
    }
}

fn source_file_metadata_matches(entry: &SourceFileEntry, metadata: &fs::Metadata) -> bool {
    if !is_regular_file_without_reparse(metadata) || metadata.len() != entry.size {
        return false;
    }
    if let Some(expected_modified) = entry.modified {
        if metadata.modified().ok() != Some(expected_modified) {
            return false;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != entry.device || metadata.ino() != entry.inode {
            return false;
        }
    }
    true
}

fn validate_source_file_path(entry: &SourceFileEntry) -> Result<(), String> {
    let canonical_path = fs::canonicalize(&entry.source_path)
        .map_err(|err| format!("resolve imported skill file failed: {err}"))?;
    if canonical_path != entry.canonical_path {
        return Err("source skill entry changed while importing".to_string());
    }
    let metadata = fs::symlink_metadata(&entry.source_path)
        .map_err(|err| format!("inspect imported skill file failed: {err}"))?;
    if metadata.file_type().is_symlink() || !source_file_metadata_matches(entry, &metadata) {
        return Err("source skill entry changed while importing".to_string());
    }
    Ok(())
}

fn validate_open_source_file(entry: &SourceFileEntry, file: &File) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|err| format!("inspect opened skill file failed: {err}"))?;
    if !source_file_metadata_matches(entry, &metadata) {
        return Err("source skill entry changed while importing".to_string());
    }
    Ok(())
}

fn open_validated_source_file(entry: &SourceFileEntry) -> Result<File, String> {
    validate_source_file_path(entry)?;
    let file = open_read_only_no_follow(&entry.source_path)
        .map_err(|err| format!("read imported skill file failed: {err}"))?;
    validate_open_source_file(entry, &file)?;
    Ok(file)
}

fn copy_reader_bounded<R: Read>(
    reader: &mut R,
    output_path: &Path,
    total_written: &mut u64,
    files_written: &mut usize,
) -> Result<u64, String> {
    *files_written = files_written.saturating_add(1);
    if *files_written > MAX_FILE_COUNT {
        return Err(format!("skill exceeds the {MAX_FILE_COUNT} file limit"));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|err| format!("create installed skill file failed: {err}"))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut file_written = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("read skill file failed: {err}"))?;
        if read == 0 {
            break;
        }
        file_written = file_written
            .checked_add(read as u64)
            .ok_or_else(|| "skill file size overflow".to_string())?;
        *total_written = total_written
            .checked_add(read as u64)
            .ok_or_else(|| "skill total size overflow".to_string())?;
        if file_written > MAX_SINGLE_FILE_BYTES {
            return Err("skill contains a file that is too large".to_string());
        }
        if *total_written > MAX_TOTAL_UNCOMPRESSED_BYTES {
            return Err("skill exceeds the total size limit".to_string());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|err| format!("write installed skill file failed: {err}"))?;
    }
    output
        .flush()
        .map_err(|err| format!("flush installed skill file failed: {err}"))?;
    Ok(file_written)
}

#[cfg(unix)]
fn apply_archive_permissions(path: &Path, unix_mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let safe_mode = unix_mode.map(|mode| mode & 0o755).unwrap_or(0o644);
    fs::set_permissions(path, fs::Permissions::from_mode(safe_mode))
        .map_err(|err| format!("set installed skill permissions failed: {err}"))
}

#[cfg(not(unix))]
fn apply_archive_permissions(_path: &Path, _unix_mode: Option<u32>) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn apply_archive_directory_permissions(path: &Path, unix_mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let safe_mode = unix_mode
        .map(|mode| 0o700 | (mode & 0o055))
        .unwrap_or(0o755);
    fs::set_permissions(path, fs::Permissions::from_mode(safe_mode))
        .map_err(|err| format!("set installed skill directory permissions failed: {err}"))
}

#[cfg(not(unix))]
fn apply_archive_directory_permissions(
    _path: &Path,
    _unix_mode: Option<u32>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn apply_imported_permissions(path: &Path, permissions: &fs::Permissions) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let safe_mode = permissions.mode() & 0o755;
    fs::set_permissions(path, fs::Permissions::from_mode(safe_mode))
        .map_err(|err| format!("set imported skill permissions failed: {err}"))
}

#[cfg(not(unix))]
fn apply_imported_permissions(path: &Path, permissions: &fs::Permissions) -> Result<(), String> {
    fs::set_permissions(path, permissions.clone())
        .map_err(|err| format!("set imported skill permissions failed: {err}"))
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o755))
        .map_err(|err| format!("set skill directory permissions failed: {err}"))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

fn create_managed_skills_root(skills_root: &Path) -> Result<(), String> {
    match fs::symlink_metadata(skills_root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("Codex skills path must be a regular directory".to_string());
            }
            harden_existing_directory_permissions(skills_root, &metadata)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(skills_root)
                .map_err(|err| format!("create Codex skills directory failed: {err}"))?;
            ensure_safe_skills_root(skills_root)?;
            set_directory_permissions(skills_root, 0o700)
        }
        Err(err) => Err(format!("inspect Codex skills directory failed: {err}")),
    }
}

#[cfg(unix)]
fn harden_existing_directory_permissions(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let current_mode = metadata.permissions().mode() & 0o7777;
    let hardened_mode = current_mode & !0o022;
    if hardened_mode == current_mode {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(hardened_mode))
        .map_err(|err| format!("set skill directory permissions failed: {err}"))
}

#[cfg(not(unix))]
fn harden_existing_directory_permissions(
    _path: &Path,
    _metadata: &fs::Metadata,
) -> Result<(), String> {
    Ok(())
}

fn create_skill_subdirectories(root: &Path, relative: &Path) -> Result<(), String> {
    if relative.as_os_str().is_empty() {
        return Ok(());
    }
    validate_relative_path(relative, MAX_PATH_DEPTH)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err("skill contains an unsafe directory path".to_string());
        };
        current.push(value);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("skill directory path contains a non-directory".to_string()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|err| format!("create skill directory failed: {err}"))?;
            }
            Err(err) => return Err(format!("inspect skill directory failed: {err}")),
        }
        set_directory_permissions(&current, 0o755)?;
    }
    Ok(())
}

fn ensure_safe_skills_root(skills_root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(skills_root)
        .map_err(|err| format!("inspect Codex skills directory failed: {err}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Codex skills path must be a regular directory".to_string());
    }
    Ok(())
}

fn ensure_destination_available(destination: &Path) -> Result<(), String> {
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err("a skill with the same name is already installed".to_string());
    }
    Ok(())
}

fn create_staging_directory(skills_root: &Path) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..32u32 {
        let candidate = skills_root.join(format!(
            "{STAGING_PREFIX}{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => {
                if let Err(err) = set_directory_permissions(&candidate, 0o700) {
                    let _ = fs::remove_dir(&candidate);
                    return Err(err);
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("create skill staging directory failed: {err}")),
        }
    }
    Err("unable to allocate skill staging directory".to_string())
}

fn allocate_quarantine_path(skills_root: &Path) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..32u32 {
        let candidate = skills_root.join(format!(
            "{QUARANTINE_PREFIX}{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::symlink_metadata(&candidate) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(err) => return Err(format!("inspect skill quarantine path failed: {err}")),
        }
    }
    Err("unable to allocate skill quarantine path".to_string())
}

fn delete_from_root(skills_root: &Path, directory_name: &str) -> Result<(), String> {
    validate_directory_name(directory_name)?;
    if directory_name == SYSTEM_DIRECTORY_NAME
        || directory_name.starts_with(STAGING_PREFIX)
        || directory_name.starts_with(QUARANTINE_PREFIX)
    {
        return Err("this skill directory cannot be deleted".to_string());
    }
    ensure_safe_skills_root(skills_root)?;
    let target = skills_root.join(directory_name);
    if target.parent() != Some(skills_root) {
        return Err(
            "skill deletion is limited to direct children of the skills directory".to_string(),
        );
    }
    let metadata = fs::symlink_metadata(&target).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "skill directory not found".to_string()
        } else {
            format!("inspect skill directory failed: {err}")
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("refusing to delete a symlink or non-directory skill entry".to_string());
    }
    let canonical_root = fs::canonicalize(skills_root)
        .map_err(|err| format!("resolve Codex skills directory failed: {err}"))?;
    let canonical_target = fs::canonicalize(&target)
        .map_err(|err| format!("resolve skill directory failed: {err}"))?;
    if canonical_target.parent() != Some(canonical_root.as_path()) {
        return Err("skill deletion target escaped the skills directory".to_string());
    }
    let quarantine = allocate_quarantine_path(skills_root)?;
    fs::rename(&target, &quarantine).map_err(|err| {
        format!(
            "isolate skill before deletion failed (mounted directories cannot be deleted): {err}"
        )
    })?;
    match fs::remove_dir_all(&quarantine) {
        Ok(()) => Ok(()),
        Err(err) => {
            let rollback = if fs::symlink_metadata(&target).is_err() {
                fs::rename(&quarantine, &target).err()
            } else {
                None
            };
            match rollback {
                Some(rollback_err) => Err(format!(
                    "delete quarantined skill failed: {err}; restore failed: {rollback_err}"
                )),
                None => Err(format!("delete quarantined skill failed: {err}")),
            }
        }
    }
}

#[cfg(test)]
#[path = "codex_skills_tests.rs"]
mod tests;
