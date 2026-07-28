use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CACHE_DIRECTORY_NAME: &str = "codex-skill-repositories";
const DEFAULT_REF_NAME: &str = "main";
const MAX_REPOSITORY_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_REGISTRY_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_GITHUB_METADATA_BYTES: usize = 1024 * 1024;
const MAX_SKILL_MD_BYTES: u64 = 512 * 1024;
const MAX_REPOSITORY_ENTRIES: usize = 40_000;
const MAX_REPOSITORY_SKILLS: usize = 2_000;
const MAX_SKILL_DOCUMENT_CANDIDATES: usize = 4_096;
const MAX_SKILL_DOCUMENT_BYTES_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_SKILL_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_SKILL_FILES: usize = 512;
const MAX_SKILL_SINGLE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SKILL_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SKILL_PATH_DEPTH: usize = 16;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillRepositoryInventory {
    pub repositories: Vec<CodexSkillRepositorySummary>,
    pub items: Vec<CodexSkillCatalogItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillRepositorySummary {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub repository: String,
    pub source_url: String,
    pub ref_name: String,
    pub builtin: bool,
    pub enabled: bool,
    pub skill_count: usize,
    pub last_scanned_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillCatalogItem {
    pub skill_id: String,
    pub repository_id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub path: String,
    pub repository_name: String,
    pub repository_owner: String,
    pub repository_ref: String,
    pub source_url: String,
    pub installed: bool,
    pub installed_directory_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSkillRegistrySearchResult {
    pub items: Vec<CodexSkillCatalogItem>,
    pub total: usize,
    pub query: String,
    pub limit: usize,
    pub offset: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitHubRepositorySource {
    owner: String,
    repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScannedSkill {
    skill_id: String,
    name: String,
    description: String,
    path: String,
    source_url: String,
}

#[derive(Debug, Deserialize)]
struct SkillsShResponse {
    #[serde(default)]
    skills: Vec<SkillsShItem>,
    #[serde(default)]
    count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsShItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    skill_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    installs: u64,
    #[serde(default)]
    source: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRepositoryMetadata {
    default_branch: String,
}

fn mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn http_client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(NETWORK_TIMEOUT)
                .redirect(Policy::none())
                .user_agent(concat!("CodexManager/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|err| format!("build Skills repository HTTP client failed: {err}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn list(codex_home: Option<&str>) -> Result<CodexSkillRepositoryInventory, String> {
    build_inventory(codex_home, Vec::new())
}

pub(crate) fn add(
    source: Option<&str>,
    ref_name: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillRepositoryInventory, String> {
    let source = source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing source".to_string())?;
    let source = normalize_github_source(source)?;
    let ref_name = match normalize_ref_name(ref_name)? {
        Some(ref_name) => ref_name,
        None => fetch_default_branch(&source).unwrap_or_else(|_| DEFAULT_REF_NAME.to_string()),
    };
    let repository_id = stable_repository_id(&source.owner, &source.repository, &ref_name);
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "Skills repository mutation lock poisoned".to_string())?;
    let now = codexmanager_core::storage::now_ts();
    let storage = open_storage()?;
    storage
        .upsert_codex_skill_repository(&codexmanager_core::storage::CodexSkillRepositoryUpsert {
            id: repository_id.clone(),
            owner: source.owner,
            repository: source.repository,
            ref_name,
            is_builtin: false,
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .map_err(|err| format!("save Skills repository failed: {err}"))?;
    drop(storage);

    let mut warnings = Vec::new();
    if let Err(err) = refresh_one_locked(&repository_id) {
        warnings.push(err);
    }
    drop(_guard);
    build_inventory(codex_home, warnings)
}

pub(crate) fn delete(
    repository_id: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillRepositoryInventory, String> {
    let repository_id = required_opaque_id(repository_id, "repositoryId")?;
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "Skills repository mutation lock poisoned".to_string())?;
    let storage = open_storage()?;
    let deleted = storage
        .delete_codex_skill_repository(repository_id)
        .map_err(|err| format!("delete Skills repository failed: {err}"))?;
    if !deleted {
        return Err("Skills repository not found".to_string());
    }
    drop(storage);
    cleanup_repository_cache(repository_id, None);
    drop(_guard);
    build_inventory(codex_home, Vec::new())
}

pub(crate) fn refresh(
    repository_id: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillRepositoryInventory, String> {
    let repository_id = repository_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| required_opaque_id(Some(value), "repositoryId"))
        .transpose()?;
    let repository_ids = if let Some(repository_id) = repository_id {
        vec![repository_id.to_string()]
    } else {
        open_storage()?
            .list_codex_skill_repositories()
            .map_err(|err| format!("list Skills repositories failed: {err}"))?
            .into_iter()
            .filter(|repository| repository.enabled)
            .map(|repository| repository.id)
            .collect()
    };
    let mut warnings = Vec::new();
    for repository_id in repository_ids {
        if let Err(err) = refresh_one(&repository_id) {
            warnings.push(err);
        }
    }
    build_inventory(codex_home, warnings)
}

pub(crate) fn install(
    repository_id: Option<&str>,
    skill_id: Option<&str>,
    codex_home: Option<&str>,
) -> Result<CodexSkillRepositoryInventory, String> {
    let repository_id = required_opaque_id(repository_id, "repositoryId")?;
    let skill_id = required_opaque_id(skill_id, "skillId")?;
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "Skills repository mutation lock poisoned".to_string())?;
    let (mut repository, mut skill) = load_repository_and_skill(repository_id, skill_id)?;
    let mut cache_path = repository
        .revision
        .as_deref()
        .map(|revision| repository_cache_path(repository_id, revision))
        .transpose()?
        .unwrap_or_default();
    if cache_path.as_os_str().is_empty() || !cache_path.exists() {
        refresh_one_locked(repository_id)?;
        (repository, skill) = load_repository_and_skill(repository_id, skill_id)?;
        let revision = repository
            .revision
            .as_deref()
            .ok_or_else(|| "refreshed Skills repository has no revision".to_string())?;
        cache_path = repository_cache_path(repository_id, revision)?;
    }
    let archive = read_file_bounded(&cache_path, MAX_REPOSITORY_ARCHIVE_BYTES)?;
    let expected_revision = repository
        .revision
        .as_deref()
        .ok_or_else(|| "Skills repository snapshot has no revision".to_string())?;
    if skill.revision.as_deref() != Some(expected_revision) {
        return Err("repository Skill revision does not match its snapshot".to_string());
    }
    if archive_revision(&archive) != expected_revision {
        return Err("cached Skills repository revision does not match its snapshot".to_string());
    }
    let selected = build_selected_skill_archive(&archive, &skill.path)?;
    crate::codex_skills::install_archive_bytes(&selected, codex_home)?;
    drop(repository);
    drop(_guard);
    build_inventory(codex_home, Vec::new())
}

pub(crate) fn registry_search(
    query: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
    codex_home: Option<&str>,
) -> Result<CodexSkillRegistrySearchResult, String> {
    let query = query
        .map(str::trim)
        .filter(|value| value.chars().count() >= 2)
        .ok_or_else(|| "skills.sh search requires at least 2 characters".to_string())?;
    if query.len() > 128 {
        return Err("skills.sh search query is too long".to_string());
    }
    let limit = usize::try_from(limit.unwrap_or(24).clamp(1, 50)).unwrap_or(24);
    let offset = usize::try_from(offset.unwrap_or(0).clamp(0, 10_000)).unwrap_or(0);
    let response = fetch_skills_sh_search(query, limit, offset)?;
    let installed = installed_skill_names(codex_home)?;
    let mut warnings = Vec::new();
    let mut items = Vec::new();
    for item in response.skills {
        let source = match normalize_github_source(&item.source) {
            Ok(source) => source,
            Err(_) => {
                warnings.push(format!(
                    "ignored skills.sh result with unsupported source: {}",
                    item.source
                ));
                continue;
            }
        };
        let skill_name = if item.name.trim().is_empty() {
            item.skill_id.trim().to_string()
        } else {
            item.name.trim().to_string()
        };
        if skill_name.is_empty() || item.skill_id.trim().is_empty() {
            continue;
        }
        let installed_directory_name = installed_directory(&installed, &skill_name);
        let is_installed = installed_contains(&installed, &skill_name);
        items.push(CodexSkillCatalogItem {
            skill_id: item.skill_id,
            repository_id: String::new(),
            name: skill_name,
            description: String::new(),
            author: source.owner.clone(),
            category: String::new(),
            path: item.id,
            repository_name: format!("{}/{}", source.owner, source.repository),
            repository_owner: source.owner.clone(),
            repository_ref: String::new(),
            source_url: format!("https://github.com/{}/{}", source.owner, source.repository),
            installed: is_installed,
            installed_directory_name,
            installs: Some(item.installs),
        });
    }
    Ok(CodexSkillRegistrySearchResult {
        total: response.count.max(items.len()),
        items,
        query: query.to_string(),
        limit,
        offset,
        warnings,
    })
}

pub(crate) fn registry_install(
    source: Option<&str>,
    skill_id: Option<&str>,
    codex_home: Option<&str>,
) -> Result<crate::codex_skills::CodexSkillsInventory, String> {
    let source = source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing source".to_string())?;
    let source = normalize_github_source(source)?;
    let skill_id = skill_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing skillId".to_string())?;
    if skill_id.len() > 256 || skill_id.chars().any(char::is_control) {
        return Err("invalid skillId".to_string());
    }
    verify_skills_sh_entry(&source, skill_id)?;
    let preferred_ref =
        fetch_default_branch(&source).unwrap_or_else(|_| DEFAULT_REF_NAME.to_string());
    let (ref_name, archive) = match download_repository_archive(&source, &preferred_ref) {
        Ok(archive) => (preferred_ref, archive),
        Err(first_error) if preferred_ref != "master" => (
            "master".to_string(),
            download_repository_archive(&source, "master").map_err(|_| first_error)?,
        ),
        Err(error) => return Err(error),
    };
    let transient_id = stable_repository_id(&source.owner, &source.repository, &ref_name);
    let skills = scan_repository_archive(&archive, &transient_id, &source, &ref_name)?;
    let normalized_skill_id = skill_id.trim_matches('/');
    let candidates = skills
        .iter()
        .filter(|skill| {
            skill.name.eq_ignore_ascii_case(normalized_skill_id)
                || skill
                    .path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(normalized_skill_id))
                || skill.path.eq_ignore_ascii_case(normalized_skill_id)
        })
        .collect::<Vec<_>>();
    let skill = match candidates.as_slice() {
        [skill] => *skill,
        [] => return Err("skill was not found in the GitHub repository".to_string()),
        _ => return Err("skillId matches more than one skill in the repository".to_string()),
    };
    let selected = build_selected_skill_archive(&archive, &skill.path)?;
    crate::codex_skills::install_archive_bytes(&selected, codex_home)
}

fn open_storage() -> Result<crate::storage_helpers::StorageHandle, String> {
    crate::initialize_storage_if_needed()?;
    crate::storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())
}

fn build_inventory(
    codex_home: Option<&str>,
    mut warnings: Vec<String>,
) -> Result<CodexSkillRepositoryInventory, String> {
    let storage = open_storage()?;
    let snapshot = storage
        .codex_skill_repository_catalog_snapshot()
        .map_err(|err| format!("list Skills repository catalog failed: {err}"))?;
    let records = snapshot.repositories;
    let skills = snapshot.skills;
    drop(storage);
    let installed = installed_skill_names(codex_home)?;
    let repository_by_id = records
        .iter()
        .cloned()
        .map(|record| (record.id.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut counts = HashMap::<String, usize>::new();
    let mut items = Vec::new();
    for skill in skills {
        let Some(repository) = repository_by_id.get(&skill.repository_id) else {
            warnings.push(format!(
                "ignored orphaned repository Skill snapshot: {}",
                skill.skill_id
            ));
            continue;
        };
        *counts.entry(repository.id.clone()).or_default() += 1;
        let installed_directory_name = installed_directory(&installed, &skill.name);
        let is_installed = installed_contains(&installed, &skill.name);
        items.push(CodexSkillCatalogItem {
            skill_id: skill.skill_id,
            repository_id: repository.id.clone(),
            name: skill.name,
            description: skill.description,
            author: repository.owner.clone(),
            category: String::new(),
            path: skill.path,
            repository_name: format!("{}/{}", repository.owner, repository.repository),
            repository_owner: repository.owner.clone(),
            repository_ref: repository.ref_name.clone(),
            source_url: skill.source_url,
            installed: is_installed,
            installed_directory_name,
            installs: None,
        });
    }
    items.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.repository_name.cmp(&right.repository_name))
            .then_with(|| left.path.cmp(&right.path))
    });
    let repositories = records
        .into_iter()
        .map(|record| CodexSkillRepositorySummary {
            skill_count: counts.get(&record.id).copied().unwrap_or_default(),
            name: format!("{}/{}", record.owner, record.repository),
            source_url: format!("https://github.com/{}/{}", record.owner, record.repository),
            id: record.id,
            owner: record.owner,
            repository: record.repository,
            ref_name: record.ref_name,
            builtin: record.is_builtin,
            enabled: record.enabled,
            last_scanned_at: record.last_scanned_at,
            last_error: record.last_error,
        })
        .collect();
    Ok(CodexSkillRepositoryInventory {
        repositories,
        items,
        warnings,
    })
}

fn load_repository_and_skill(
    repository_id: &str,
    skill_id: &str,
) -> Result<
    (
        codexmanager_core::storage::CodexSkillRepositoryRecord,
        codexmanager_core::storage::CodexSkillRepositorySkillRecord,
    ),
    String,
> {
    let storage = open_storage()?;
    let repository = storage
        .get_codex_skill_repository(repository_id)
        .map_err(|err| format!("read Skills repository failed: {err}"))?
        .ok_or_else(|| "Skills repository not found".to_string())?;
    let skill = storage
        .get_codex_skill_repository_skill(repository_id, skill_id)
        .map_err(|err| format!("read repository Skill failed: {err}"))?
        .ok_or_else(|| "repository Skill not found".to_string())?;
    Ok((repository, skill))
}

fn refresh_one(repository_id: &str) -> Result<(), String> {
    let _guard = mutation_lock()
        .lock()
        .map_err(|_| "Skills repository mutation lock poisoned".to_string())?;
    refresh_one_locked(repository_id)
}

fn refresh_one_locked(repository_id: &str) -> Result<(), String> {
    let storage = open_storage()?;
    let repository = storage
        .get_codex_skill_repository(repository_id)
        .map_err(|err| format!("read Skills repository failed: {err}"))?
        .ok_or_else(|| "Skills repository not found".to_string())?;
    drop(storage);
    let source = GitHubRepositorySource {
        owner: repository.owner.clone(),
        repository: repository.repository.clone(),
    };
    let result: Result<(), String> = (|| {
        let archive = download_repository_archive(&source, &repository.ref_name)?;
        let scanned =
            scan_repository_archive(&archive, repository_id, &source, &repository.ref_name)?;
        let revision = archive_revision(&archive);
        write_cache_archive(repository_id, &revision, &archive)?;
        let scanned_at = codexmanager_core::storage::now_ts();
        let records = scanned
            .into_iter()
            .map(
                |skill| codexmanager_core::storage::CodexSkillRepositorySkillRecord {
                    repository_id: repository_id.to_string(),
                    skill_id: skill.skill_id,
                    name: skill.name,
                    description: skill.description,
                    path: skill.path,
                    source_url: skill.source_url,
                    revision: Some(revision.clone()),
                },
            )
            .collect::<Vec<_>>();
        open_storage()?
            .replace_codex_skill_repository_snapshot(repository_id, &records, scanned_at)
            .map_err(|err| format!("save Skills repository snapshot failed: {err}"))?;
        cleanup_repository_cache(repository_id, Some(&revision));
        Ok(())
    })();
    if let Err(err) = &result {
        if let Ok(storage) = open_storage() {
            if let Err(record_err) = storage.record_codex_skill_repository_error(repository_id, err)
            {
                log::warn!(
                    "event=skill_repository_error_record_failed repository_id={} error={}",
                    repository_id,
                    record_err
                );
            }
        }
    }
    result.map_err(|err| {
        format!(
            "refresh {}/{} failed: {err}",
            source.owner, source.repository
        )
    })
}

fn installed_skill_names(
    codex_home: Option<&str>,
) -> Result<HashMap<String, Option<String>>, String> {
    let inventory = crate::codex_skills::list(codex_home)?;
    let mut installed: HashMap<String, Option<String>> = HashMap::new();
    for item in inventory.items {
        if item.valid {
            let key = item.name.to_ascii_lowercase();
            let directory = item.deletable.then_some(item.directory_name);
            installed
                .entry(key)
                .and_modify(|existing| {
                    if existing.is_none() && directory.is_some() {
                        *existing = directory.clone();
                    }
                })
                .or_insert(directory);
        }
    }
    Ok(installed)
}

fn installed_contains(installed: &HashMap<String, Option<String>>, name: &str) -> bool {
    installed.contains_key(&name.to_ascii_lowercase())
}

fn installed_directory(installed: &HashMap<String, Option<String>>, name: &str) -> Option<String> {
    installed
        .get(&name.to_ascii_lowercase())
        .and_then(Clone::clone)
}

fn normalize_github_source(source: &str) -> Result<GitHubRepositorySource, String> {
    let (owner, repository) = if source.contains("://") {
        let parsed = url::Url::parse(source)
            .map_err(|_| "source must be a GitHub owner/repo or HTTPS URL".to_string())?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("source must be a public GitHub HTTPS repository".to_string());
        }
        let parts = parsed
            .path()
            .trim_matches('/')
            .split('/')
            .collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("GitHub source must identify exactly one owner and repository".to_string());
        }
        (parts[0].to_string(), parts[1].to_string())
    } else {
        let parts = source.trim_matches('/').split('/').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("GitHub source must use owner/repo".to_string());
        }
        (parts[0].to_string(), parts[1].to_string())
    };
    let repository = repository
        .strip_suffix(".git")
        .unwrap_or(&repository)
        .to_string();
    if !valid_github_owner(&owner) || !valid_github_repository(&repository) {
        return Err("GitHub owner or repository name is invalid".to_string());
    }
    Ok(GitHubRepositorySource { owner, repository })
}

fn valid_github_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_github_repository(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with(['.', '-'])
        && !value.ends_with(['.', '-'])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn normalize_ref_name(ref_name: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = ref_name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 255
        || value.starts_with(['.', '/', '-'])
        || value.ends_with(['.', '/'])
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b'_' | b'.' | b'/'))
        || value.split('/').any(|component| {
            component.is_empty()
                || component.starts_with('.')
                || component.ends_with(".lock")
                || component.ends_with('.')
        })
    {
        return Err("invalid refName".to_string());
    }
    Ok(Some(value.to_string()))
}

fn required_opaque_id<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {name}"))?;
    if value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("invalid {name}"));
    }
    Ok(value)
}

fn stable_repository_id(owner: &str, repository: &str, ref_name: &str) -> String {
    let digest = Sha256::digest(format!("{owner}/{repository}\n{ref_name}").as_bytes());
    format!("repo_{}", hex_prefix(&digest, 20))
}

fn stable_skill_id(repository_id: &str, path: &str) -> String {
    let digest = Sha256::digest(format!("{repository_id}\n{path}").as_bytes());
    format!("skill_{}", hex_prefix(&digest, 24))
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(count);
    for byte in bytes {
        if output.len() >= count {
            break;
        }
        output.push(HEX[(byte >> 4) as usize] as char);
        if output.len() < count {
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    output
}

fn download_repository_archive(
    source: &GitHubRepositorySource,
    ref_name: &str,
) -> Result<Vec<u8>, String> {
    let ref_name =
        normalize_ref_name(Some(ref_name))?.ok_or_else(|| "missing repository ref".to_string())?;
    let url = format!(
        "https://codeload.github.com/{}/{}/zip/{}",
        source.owner,
        source.repository,
        urlencoding::encode(&ref_name)
    );
    get_bounded(
        &url,
        MAX_REPOSITORY_ARCHIVE_BYTES,
        "download GitHub repository archive",
    )
}

fn fetch_default_branch(source: &GitHubRepositorySource) -> Result<String, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}",
        source.owner, source.repository
    );
    let bytes = get_bounded(&url, MAX_GITHUB_METADATA_BYTES, "read GitHub repository")?;
    let metadata: GitHubRepositoryMetadata = serde_json::from_slice(&bytes)
        .map_err(|err| format!("parse GitHub repository metadata failed: {err}"))?;
    normalize_ref_name(Some(&metadata.default_branch))?
        .ok_or_else(|| "GitHub repository has no default branch".to_string())
}

fn fetch_skills_sh_search(
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<SkillsShResponse, String> {
    let url = format!(
        "https://skills.sh/api/search?q={}&limit={limit}&offset={offset}",
        urlencoding::encode(query)
    );
    let bytes = get_bounded(&url, MAX_REGISTRY_RESPONSE_BYTES, "skills.sh search")?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("parse skills.sh search response failed: {err}"))
}

fn verify_skills_sh_entry(source: &GitHubRepositorySource, skill_id: &str) -> Result<(), String> {
    let response = fetch_skills_sh_search(skill_id, 50, 0)?;
    let expected_source = format!("{}/{}", source.owner, source.repository);
    let expected_id = format!("{expected_source}/{skill_id}");
    let registered = response.skills.into_iter().any(|item| {
        item.skill_id == skill_id
            && item.source.eq_ignore_ascii_case(&expected_source)
            && item.id.eq_ignore_ascii_case(&expected_id)
    });
    if !registered {
        return Err(
            "skill is no longer registered by skills.sh for this GitHub source".to_string(),
        );
    }
    Ok(())
}

fn get_bounded(url: &str, max_bytes: usize, action: &str) -> Result<Vec<u8>, String> {
    let mut response = http_client()?
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .map_err(|err| format!("{action} failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("{action} returned HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!("{action} exceeded the size limit"));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(max_bytes as u64) as usize,
    );
    response
        .by_ref()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("{action} body failed: {err}"))?;
    if bytes.len() > max_bytes {
        return Err(format!("{action} exceeded the size limit"));
    }
    Ok(bytes)
}

fn scan_repository_archive(
    archive_bytes: &[u8],
    repository_id: &str,
    source: &GitHubRepositorySource,
    ref_name: &str,
) -> Result<Vec<ScannedSkill>, String> {
    if archive_bytes.len() > MAX_REPOSITORY_ARCHIVE_BYTES {
        return Err("repository archive exceeds the compressed size limit".to_string());
    }
    let mut archive = ZipArchive::new(Cursor::new(archive_bytes))
        .map_err(|err| format!("invalid GitHub repository ZIP: {err}"))?;
    if archive.len() == 0 || archive.len() > MAX_REPOSITORY_ENTRIES {
        return Err(format!(
            "repository archive must contain 1-{MAX_REPOSITORY_ENTRIES} entries"
        ));
    }
    let mut skills = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut skill_document_candidates = 0usize;
    let mut skill_document_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("inspect repository ZIP entry failed: {err}"))?;
        if entry.is_dir()
            || entry.encrypted()
            || entry.is_symlink()
            || zip_mode_is_special(entry.unix_mode(), false)
        {
            continue;
        }
        let Some(relative_path) = repository_relative_path(&entry) else {
            continue;
        };
        if relative_path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") {
            continue;
        }
        if relative_path
            .components()
            .any(|component| is_ignored_repository_component(component.as_os_str()))
        {
            continue;
        }
        if entry.size() > MAX_SKILL_MD_BYTES {
            continue;
        }
        consume_skill_document_budget(
            &mut skill_document_candidates,
            &mut skill_document_bytes,
            entry.size(),
        )?;
        let directory = relative_path.parent().unwrap_or(Path::new(""));
        if directory.components().count() > MAX_SKILL_PATH_DEPTH {
            continue;
        }
        if !directory.as_os_str().is_empty() && validate_skill_relative_path(directory).is_err() {
            continue;
        }
        let path = if directory.as_os_str().is_empty() {
            ".".to_string()
        } else {
            path_to_slash_string(directory)?
        };
        if !seen_paths.insert(path.to_ascii_lowercase()) {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_SKILL_MD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| format!("read repository SKILL.md failed: {err}"))?;
        if bytes.len() as u64 > MAX_SKILL_MD_BYTES {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let Ok((name, description)) = crate::codex_skills::parse_skill_metadata_document(&content)
        else {
            continue;
        };
        let description = truncate_utf8_bytes(description, MAX_SKILL_DESCRIPTION_BYTES);
        let source_url = github_tree_url(source, ref_name, &path)?;
        skills.push(ScannedSkill {
            skill_id: stable_skill_id(repository_id, &path),
            name,
            description,
            path,
            source_url,
        });
        if skills.len() > MAX_REPOSITORY_SKILLS {
            return Err(format!(
                "repository contains more than {MAX_REPOSITORY_SKILLS} Skills"
            ));
        }
    }
    skills.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    if skills.is_empty() {
        return Err("repository contains no valid Skills".to_string());
    }
    Ok(skills)
}

fn consume_skill_document_budget(
    candidates: &mut usize,
    total_bytes: &mut u64,
    document_bytes: u64,
) -> Result<(), String> {
    *candidates = candidates.saturating_add(1);
    if *candidates > MAX_SKILL_DOCUMENT_CANDIDATES {
        return Err(format!(
            "repository contains more than {MAX_SKILL_DOCUMENT_CANDIDATES} SKILL.md candidates"
        ));
    }
    *total_bytes = total_bytes
        .checked_add(document_bytes)
        .ok_or_else(|| "repository SKILL.md size overflow".to_string())?;
    if *total_bytes > MAX_SKILL_DOCUMENT_BYTES_TOTAL {
        return Err("repository SKILL.md documents exceed the scan size limit".to_string());
    }
    Ok(())
}

fn truncate_utf8_bytes(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

fn build_selected_skill_archive(
    repository_archive: &[u8],
    selected_path: &str,
) -> Result<Vec<u8>, String> {
    let selected_path = validate_snapshot_path(selected_path)?;
    let mut archive = ZipArchive::new(Cursor::new(repository_archive))
        .map_err(|err| format!("invalid cached repository ZIP: {err}"))?;
    if archive.len() == 0 || archive.len() > MAX_REPOSITORY_ENTRIES {
        return Err("cached repository ZIP has an invalid entry count".to_string());
    }
    let nested_skill_roots = collect_skill_roots(&mut archive)?;
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let mut files = 0usize;
    let mut total_bytes = 0u64;
    let mut saw_skill_md = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("read cached repository ZIP entry failed: {err}"))?;
        let Some(path) = repository_relative_path(&entry) else {
            continue;
        };
        if !path.starts_with(&selected_path) || path == selected_path || entry.is_dir() {
            continue;
        }
        if nested_skill_roots.iter().any(|root| {
            root != &selected_path && root.starts_with(&selected_path) && path.starts_with(root)
        }) {
            continue;
        }
        if entry.encrypted() || entry.is_symlink() || zip_mode_is_special(entry.unix_mode(), false)
        {
            return Err(
                "selected skill contains a symlink, encrypted entry, or special file".to_string(),
            );
        }
        let relative = path
            .strip_prefix(&selected_path)
            .map_err(|_| "selected skill path escaped its root".to_string())?;
        validate_skill_relative_path(relative)?;
        let relative_name = path_to_slash_string(relative)?;
        if relative_name == "SKILL.md" {
            saw_skill_md = true;
        }
        files = files.saturating_add(1);
        if files > MAX_SKILL_FILES {
            return Err(format!(
                "selected skill exceeds the {MAX_SKILL_FILES} file limit"
            ));
        }
        if entry.size() > MAX_SKILL_SINGLE_FILE_BYTES {
            return Err("selected skill contains a file that is too large".to_string());
        }
        total_bytes = total_bytes
            .checked_add(entry.size())
            .ok_or_else(|| "selected skill size overflow".to_string())?;
        if total_bytes > MAX_SKILL_TOTAL_BYTES {
            return Err("selected skill exceeds the total size limit".to_string());
        }
        let mode = if entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0) {
            0o755
        } else {
            0o644
        };
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(mode);
        writer
            .start_file(relative_name, options)
            .map_err(|err| format!("create selected skill ZIP entry failed: {err}"))?;
        let copied = std::io::copy(
            &mut entry.by_ref().take(MAX_SKILL_SINGLE_FILE_BYTES + 1),
            &mut writer,
        )
        .map_err(|err| format!("copy selected skill ZIP entry failed: {err}"))?;
        if copied != entry.size() || copied > MAX_SKILL_SINGLE_FILE_BYTES {
            return Err("selected skill ZIP entry size changed while reading".to_string());
        }
    }
    if !saw_skill_md || files == 0 {
        return Err("selected skill no longer contains a root SKILL.md".to_string());
    }
    let output = writer
        .finish()
        .map_err(|err| format!("finish selected skill ZIP failed: {err}"))?
        .into_inner();
    if output.len() > crate::codex_skills::MAX_ARCHIVE_BYTES {
        return Err("selected skill exceeds the compressed install size limit".to_string());
    }
    Ok(output)
}

fn collect_skill_roots<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|err| format!("inspect cached repository ZIP entry failed: {err}"))?;
        let Some(path) = repository_relative_path(&entry) else {
            continue;
        };
        if !entry.is_dir() && path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md")
        {
            if let Some(parent) = path.parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    Ok(roots)
}

fn repository_relative_path(entry: &zip::read::ZipFile<'_>) -> Option<PathBuf> {
    if entry.name().contains('\\') {
        return None;
    }
    let enclosed = entry.enclosed_name()?;
    let mut components = enclosed.components();
    match components.next()? {
        Component::Normal(_) => {}
        _ => return None,
    }
    let relative = components.as_path();
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_path_buf())
}

fn is_ignored_repository_component(value: &std::ffi::OsStr) -> bool {
    value.to_str().is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            ".git" | "node_modules" | "target" | "vendor"
        )
    })
}

fn zip_mode_is_special(unix_mode: Option<u32>, is_dir: bool) -> bool {
    const TYPE_MASK: u32 = 0o170000;
    const TYPE_FILE: u32 = 0o100000;
    const TYPE_DIR: u32 = 0o040000;
    let Some(mode) = unix_mode else {
        return false;
    };
    let file_type = mode & TYPE_MASK;
    file_type != 0
        && if is_dir {
            file_type != TYPE_DIR
        } else {
            file_type != TYPE_FILE
        }
}

fn validate_snapshot_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 2048 || value.contains('\\') {
        return Err("repository Skill snapshot path is invalid".to_string());
    }
    if value == "." {
        return Ok(PathBuf::new());
    }
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path.components().count() == 0
        || path.components().count() > MAX_SKILL_PATH_DEPTH
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("repository Skill snapshot path is invalid".to_string());
    }
    Ok(path)
}

fn validate_skill_relative_path(path: &Path) -> Result<(), String> {
    let depth = path.components().count();
    if depth == 0
        || depth > MAX_SKILL_PATH_DEPTH
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("selected skill contains an unsafe path".to_string());
    }
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err("selected skill contains an unsafe path".to_string());
        };
        let value = value
            .to_str()
            .ok_or_else(|| "selected skill path must be UTF-8".to_string())?;
        if value.ends_with(['.', ' '])
            || value.chars().any(|character| {
                character.is_control()
                    || matches!(
                        character,
                        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                    )
            })
        {
            return Err("selected skill contains a non-portable path".to_string());
        }
        if is_windows_reserved_component(value) {
            return Err("selected skill contains a Windows-reserved path".to_string());
        }
    }
    Ok(())
}

fn is_windows_reserved_component(value: &str) -> bool {
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

fn path_to_slash_string(path: &Path) -> Result<String, String> {
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_string)
                .ok_or_else(|| "repository path must be UTF-8".to_string()),
            _ => Err("repository path is unsafe".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

fn github_tree_url(
    source: &GitHubRepositorySource,
    ref_name: &str,
    path: &str,
) -> Result<String, String> {
    let mut url = url::Url::parse(&format!(
        "https://github.com/{}/{}/tree/",
        source.owner, source.repository
    ))
    .map_err(|err| format!("build GitHub Skill URL failed: {err}"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "build GitHub Skill URL failed".to_string())?;
        for component in ref_name.split('/') {
            segments.push(component);
        }
        if path != "." {
            for component in path.split('/') {
                segments.push(component);
            }
        }
    }
    Ok(url.to_string())
}

fn repository_cache_root() -> Result<PathBuf, String> {
    let root = crate::process_env::db_dir().join(CACHE_DIRECTORY_NAME);
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata_is_safe_directory(&metadata) => {}
        Ok(_) => return Err("Skills repository cache path must be a regular directory".to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&root)
                .map_err(|err| format!("create Skills repository cache failed: {err}"))?;
        }
        Err(err) => return Err(format!("inspect Skills repository cache failed: {err}")),
    }
    let metadata = fs::symlink_metadata(&root)
        .map_err(|err| format!("verify Skills repository cache failed: {err}"))?;
    if !metadata_is_safe_directory(&metadata) {
        return Err("Skills repository cache path must be a regular directory".to_string());
    }
    set_private_directory_permissions(&root)?;
    Ok(root)
}

fn repository_cache_path(repository_id: &str, revision: &str) -> Result<PathBuf, String> {
    required_opaque_id(Some(repository_id), "repositoryId")?;
    if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid Skills repository revision".to_string());
    }
    Ok(repository_cache_root()?.join(format!("{repository_id}-{revision}.zip")))
}

fn write_cache_archive(repository_id: &str, revision: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_REPOSITORY_ARCHIVE_BYTES {
        return Err("repository archive exceeds the cache size limit".to_string());
    }
    let destination = repository_cache_path(repository_id, revision)?;
    let root = destination
        .parent()
        .ok_or_else(|| "Skills repository cache has no parent".to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = root.join(format!(
        ".{repository_id}.{}.{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|err| format!("create Skills repository cache staging file failed: {err}"))?;
        set_private_file_permissions(&file)?;
        file.write_all(bytes)
            .map_err(|err| format!("write Skills repository cache failed: {err}"))?;
        file.sync_all()
            .map_err(|err| format!("sync Skills repository cache failed: {err}"))?;
        drop(file);
        if destination.exists() {
            if read_file_bounded(&destination, MAX_REPOSITORY_ARCHIVE_BYTES)
                .is_ok_and(|existing| archive_revision(&existing) == revision)
            {
                fs::remove_file(&staging).map_err(|err| {
                    format!("discard duplicate Skills repository cache failed: {err}")
                })?;
                return Ok(());
            }
            let metadata = fs::symlink_metadata(&destination).map_err(|err| {
                format!("inspect replaceable Skills repository cache failed: {err}")
            })?;
            if !metadata_is_safe_file(&metadata) {
                return Err("existing Skills repository cache is not a regular file".to_string());
            }
        }
        match fs::rename(&staging, &destination) {
            Ok(()) => Ok(()),
            Err(err) if destination.exists() => {
                let quarantine = root.join(format!(
                    ".{repository_id}.{}.{nonce}.old",
                    std::process::id()
                ));
                fs::rename(&destination, &quarantine).map_err(|move_err| {
                    format!("isolate stale Skills repository cache failed: {err}; {move_err}")
                })?;
                match fs::rename(&staging, &destination) {
                    Ok(()) => {
                        if let Err(remove_err) = fs::remove_file(&quarantine) {
                            log::warn!(
                                "remove replaced Skills repository cache failed ({}): {}",
                                quarantine.display(),
                                remove_err
                            );
                        }
                        Ok(())
                    }
                    Err(activate_err) => {
                        let rollback_error = fs::rename(&quarantine, &destination).err();
                        Err(match rollback_error {
                            Some(rollback_error) => format!(
                                "activate Skills repository cache failed: {activate_err}; rollback failed: {rollback_error}"
                            ),
                            None => format!(
                                "activate Skills repository cache failed: {activate_err}"
                            ),
                        })
                    }
                }
            }
            Err(err) => Err(format!("activate Skills repository cache failed: {err}")),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}

fn cleanup_repository_cache(repository_id: &str, keep_revision: Option<&str>) {
    if required_opaque_id(Some(repository_id), "repositoryId").is_err() {
        return;
    }
    let Ok(root) = repository_cache_root() else {
        return;
    };
    let keep_name = keep_revision
        .and_then(|revision| repository_cache_path(repository_id, revision).ok())
        .and_then(|path| path.file_name().map(|name| name.to_os_string()));
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let prefix = format!("{repository_id}-");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let Some(revision) = name_text
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.strip_suffix(".zip"))
        else {
            continue;
        };
        if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        if keep_name.as_ref().is_some_and(|keep| keep == &name) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata_is_safe_file(&metadata) {
            if let Err(err) = fs::remove_file(&path) {
                log::warn!(
                    "remove stale Skills repository cache failed ({}): {}",
                    path.display(),
                    err
                );
            }
        }
    }
}

fn archive_revision(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_prefix(&digest, 64)
}

fn read_file_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("inspect cached Skills repository failed: {err}"))?;
    if !metadata_is_safe_file(&metadata) || metadata.len() > max_bytes as u64 {
        return Err("cached Skills repository is not a safe bounded file".to_string());
    }
    let mut file = open_read_only_no_follow(path)
        .map_err(|err| format!("open cached Skills repository failed: {err}"))?;
    let opened_metadata = file
        .metadata()
        .map_err(|err| format!("inspect opened Skills repository cache failed: {err}"))?;
    if !metadata_is_safe_file(&opened_metadata)
        || opened_metadata.len() > max_bytes as u64
        || !metadata_refers_to_same_object(&metadata, &opened_metadata)
    {
        return Err("cached Skills repository changed while opening".to_string());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("read cached Skills repository failed: {err}"))?;
    if bytes.len() > max_bytes {
        return Err("cached Skills repository exceeded the size limit".to_string());
    }
    let finished_metadata = file
        .metadata()
        .map_err(|err| format!("verify opened Skills repository cache failed: {err}"))?;
    if !metadata_refers_to_same_object(&opened_metadata, &finished_metadata)
        || finished_metadata.len() != bytes.len() as u64
    {
        return Err("cached Skills repository changed while reading".to_string());
    }
    Ok(bytes)
}

fn metadata_is_safe_directory(metadata: &fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
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

fn metadata_is_safe_file(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
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

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("secure Skills repository cache failed: {err}"))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &File) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("secure Skills repository cache file failed: {err}"))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &File) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
#[path = "codex_skill_repositories_tests.rs"]
mod tests;
