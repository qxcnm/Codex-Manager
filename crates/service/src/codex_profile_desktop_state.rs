//! Preserve local project navigation when Codex switches its account-scoped renderer state.
//! Never copy account preferences, remote projects, drafts, or filesystem permissions.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

const GLOBAL_STATE_FILE: &str = ".codex-global-state.json";
const OWNER: &str = "electron-renderer-content-owner";
const LEGACY_STATE: &str = "electron-account-renderer-state-v1";
const WORKSPACE_KEYS: [&str; 3] = [
    "electron-saved-workspace-roots",
    "active-workspace-roots",
    "electron-workspace-root-labels",
];

/// Must run before publishing new credentials. Electron caches the entire state file,
/// so an app-server reload alone cannot safely publish an external state update.
pub(super) fn protect_account_switch(
    profile_dir: &Path,
    auth_json: Option<&str>,
    backup_path: &Path,
) -> Result<(), String> {
    let path = profile_dir.join(GLOBAL_STATE_FILE);
    let Some(original) = super::read_optional(&path)? else {
        return Ok(());
    };
    let mut state: Value = serde_json::from_str(&original).map_err(|err| {
        format!("Cannot preserve Codex desktop projects: invalid global state: {err}")
    })?;
    let root = state.as_object_mut().ok_or_else(|| {
        "Cannot preserve Codex desktop projects: global state is not an object".to_string()
    })?;
    let current = root
        .get(OWNER)
        .or_else(|| {
            root.get(LEGACY_STATE)
                .and_then(|legacy| legacy.get("owner"))
        })
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(current) = current else {
        // A profile without an owner is adopted by Codex on its first authenticated start.
        return Ok(());
    };
    let target = auth_owner(auth_json)?;
    if current == target {
        return Ok(());
    }
    crate::codex_runtime::ensure_codex_desktop_closed(profile_dir)?;
    let inherited = inherit_local_projects(root, &current, &target)?;
    let reindex = invalidate_local_project_index(root, profile_dir)?;
    if !inherited && !reindex {
        return Ok(());
    }
    let updated = serde_json::to_string(&state)
        .map_err(|err| format!("serialize Codex desktop project state failed: {err}"))?;
    // Recheck immediately before publishing: another process may have started or saved
    // state while the profile configuration was being prepared.
    crate::codex_runtime::ensure_codex_desktop_closed(profile_dir)?;
    if super::read_optional(&path)?.as_deref() != Some(original.as_str()) {
        return Err(
            "Codex desktop state changed while switching accounts; close Codex and retry"
                .to_string(),
        );
    }
    super::write_atomic(backup_path, &original)?;
    super::write_atomic(&path, &updated)
}

fn invalidate_local_project_index(
    root: &mut Map<String, Value>,
    profile_dir: &Path,
) -> Result<bool, String> {
    // A cached core ID suppresses project/import even if project/list no longer
    // contains that project. Let Codex rebuild this derived index using its
    // existing local-ID idempotency keys. Core projects and deletion tombstones
    // stay intact; no thread IDs or project IDs are rewritten here.
    let local_ids = root
        .get("local-projects")
        .and_then(Value::as_object)
        .map(|projects| {
            projects
                .iter()
                .filter_map(|(id, project)| {
                    (project.get("id").and_then(Value::as_str) == Some(id.as_str())
                        && project.get("name").is_some_and(Value::is_string)
                        && project.get("createdAt").is_some_and(Value::is_number)
                        && project.get("updatedAt").is_some_and(Value::is_number)
                        && project
                            .get("rootPaths")
                            .and_then(Value::as_array)
                            .is_some_and(|paths| {
                                !paths.is_empty()
                                    && paths.iter().all(|path| {
                                        path.as_str().is_some_and(|path| !path.is_empty())
                                    })
                            }))
                    .then(|| id.clone())
                })
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    if local_ids.is_empty() {
        return Ok(false);
    }
    let Some(index) = root.get_mut("app-server-project-id-by-legacy-project-id-by-host") else {
        return Ok(false);
    };
    let index = index
        .as_object_mut()
        .ok_or("Invalid Codex local project index")?;
    let mut changed = false;
    for (identity, mapping) in index {
        let Some(home) = identity.strip_prefix("local:") else {
            continue;
        };
        if !crate::codex_runtime::same_path(Path::new(home), profile_dir) {
            continue;
        }
        let mapping = mapping
            .as_object_mut()
            .ok_or("Invalid Codex local project ID mapping")?;
        let before = mapping.len();
        mapping.retain(|id, _| !local_ids.contains(id));
        changed |= before != mapping.len();
    }
    Ok(changed)
}

fn auth_owner(auth_json: Option<&str>) -> Result<String, String> {
    let Some(auth_json) = auth_json else {
        return Ok("signed-out".to_string());
    };
    let auth: Value = serde_json::from_str(auth_json)
        .map_err(|err| format!("read Codex account identity failed: {err}"))?;
    if auth.get("auth_mode").and_then(Value::as_str) == Some("apikey") {
        return Ok("signed-out".to_string());
    }
    let Some(tokens) = auth.get("tokens").filter(|tokens| !tokens.is_null()) else {
        return Ok("signed-out".to_string());
    };
    let principal = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .and_then(|token| token.split('.').nth(1))
        .and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
        .and_then(|payload| serde_json::from_slice::<Value>(&payload).ok());
    let identity = principal
        .as_ref()
        .filter(|payload| {
            payload
                .get("exp")
                .and_then(Value::as_u64)
                .is_some_and(|exp| exp > 0)
        })
        .and_then(|payload| payload.get("https://api.openai.com/auth"));
    let claim = |preferred: &str, fallback: &str| {
        identity
            .and_then(|identity| identity.get(preferred).or_else(|| identity.get(fallback)))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
    };
    let user = claim("user_id", "chatgpt_user_id");
    let account = claim("chatgpt_account_id", "account_id");
    let (Some(user), Some(account)) = (user, account) else {
        return Err("Cannot preserve Codex desktop projects: access token has no supported account identity; sign in again before switching".to_string());
    };
    // Match Codex's SHA256(JSON.stringify([userId, accountId])) exactly.
    let principal = serde_json::to_string(&[user, account]).map_err(|err| err.to_string())?;
    Ok(format!("{:x}", Sha256::digest(principal.as_bytes())))
}

fn inherit_local_projects(
    root: &mut Map<String, Value>,
    current: &str,
    target: &str,
) -> Result<bool, String> {
    let before = root.clone();
    let source = local_project_fields(root, current);
    if source.is_empty() {
        return Ok(false);
    }
    if let Some(legacy) = root.get_mut(LEGACY_STATE) {
        let legacy = legacy
            .as_object_mut()
            .ok_or("Invalid Codex account renderer state")?;
        let inactive = legacy
            .entry("inactive")
            .or_insert_with(|| Value::Object(Map::new()));
        let inactive = inactive
            .as_object_mut()
            .ok_or("Invalid Codex inactive renderer states")?;
        let target_state = inactive
            .entry(target)
            .or_insert_with(|| serde_json::json!({"atoms": {}, "globals": {}}));
        let target_state = target_state
            .as_object_mut()
            .ok_or("Invalid Codex target renderer state")?;
        let globals = target_state
            .entry("globals")
            .or_insert_with(|| Value::Object(Map::new()));
        let globals = globals
            .as_object_mut()
            .ok_or("Invalid Codex target project state")?;
        for (key, value) in &source {
            merge_project_field(globals, key, value);
        }
    }
    // Current Codex keeps local-projects shared, but legacy workspace roots and
    // selected-project are still owner-scoped. Keep target details when adding roots.
    for key in WORKSPACE_KEYS.into_iter().chain(["selected-project"]) {
        if let Some(value) = source.get(key) {
            merge_project_field(root, &format!("electron-owner-value:{key}:{target}"), value);
        }
    }
    Ok(*root != before)
}

fn merge_project_field(target: &mut Map<String, Value>, key: &str, source: &Value) {
    let Some(existing) = target.get_mut(key) else {
        target.insert(key.to_string(), source.clone());
        return;
    };
    match (existing, source) {
        (Value::Object(target), Value::Object(source)) if !key.contains("selected-project") => {
            for (id, value) in source {
                target.entry(id.clone()).or_insert_with(|| value.clone());
            }
        }
        (Value::Array(target), Value::Array(source)) => {
            for value in source {
                if !target.contains(value) {
                    target.push(value.clone());
                }
            }
        }
        _ => {}
    }
}

fn local_project_fields(root: &Map<String, Value>, owner: &str) -> Map<String, Value> {
    let get = |key: &str| {
        root.get(key)
            .or_else(|| root.get(&format!("electron-owner-value:{key}:{owner}")))
            .or_else(|| {
                root.get(LEGACY_STATE)?
                    .get("inactive")?
                    .get(owner)?
                    .get("globals")?
                    .get(key)
            })
    };
    let mut result = Map::new();
    let projects = get("local-projects").and_then(Value::as_object);
    let ids = projects
        .map(|projects| projects.keys().map(String::as_str).collect::<HashSet<_>>())
        .unwrap_or_default();
    if let Some(projects) = projects.filter(|projects| !projects.is_empty()) {
        result.insert(
            "local-projects".to_string(),
            Value::Object(projects.clone()),
        );
    }
    for key in ["project-order", "pinned-project-ids"] {
        if let Some(values) = get(key).and_then(Value::as_array) {
            let values = values
                .iter()
                .filter(|id| id.as_str().is_some_and(|id| ids.contains(id)))
                .cloned()
                .collect::<Vec<_>>();
            if !values.is_empty() {
                result.insert(key.to_string(), Value::Array(values));
            }
        }
    }
    for key in ["project-appearances", "sidebar-project-thread-orders"] {
        if let Some(values) = get(key).and_then(Value::as_object) {
            let values = values
                .iter()
                .filter(|(id, _)| ids.contains(id.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>();
            if !values.is_empty() {
                result.insert(key.to_string(), Value::Object(values));
            }
        }
    }
    if let Some(selected) = get("selected-project").filter(|selected| {
        selected.get("type").and_then(Value::as_str) == Some("local")
            && selected
                .get("projectId")
                .and_then(Value::as_str)
                .is_some_and(|id| ids.contains(id))
    }) {
        result.insert("selected-project".to_string(), selected.clone());
    }
    if let Some(assignments) = get("thread-project-assignments").and_then(Value::as_object) {
        let assignments = assignments
            .iter()
            .filter(|(_, assignment)| {
                assignment.get("projectKind").and_then(Value::as_str) == Some("local")
                    && assignment
                        .get("projectId")
                        .and_then(Value::as_str)
                        .is_some_and(|id| ids.contains(id))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>();
        if !assignments.is_empty() {
            result.insert(
                "thread-project-assignments".to_string(),
                Value::Object(assignments),
            );
        }
    }
    // Older Codex releases identify local projects by directory. Exclude known
    // remote cwd values; do not transfer project-writable-roots permissions.
    let remote_paths = root
        .get(&format!(
            "electron-account-remote-preference:remote-cwds-by-host-and-workspace:{owner}"
        ))
        .or_else(|| get("remote-cwds-by-host-and-workspace"))
        .and_then(Value::as_object)
        .map(|hosts| {
            hosts
                .values()
                .filter_map(Value::as_object)
                .flat_map(|workspaces| workspaces.iter())
                .filter_map(|(workspace, cwd)| cwd.as_str().filter(|cwd| workspace == *cwd))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut paths = HashSet::new();
    for key in ["electron-saved-workspace-roots", "active-workspace-roots"] {
        if let Some(values) = get(key).and_then(Value::as_array) {
            let values = values
                .iter()
                .filter(|value| {
                    value
                        .as_str()
                        .is_some_and(|path| !path.is_empty() && !remote_paths.contains(path))
                })
                .cloned()
                .collect::<Vec<_>>();
            paths.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
            if !values.is_empty() {
                result.insert(key.to_string(), Value::Array(values));
            }
        }
    }
    if let Some(labels) = get("electron-workspace-root-labels").and_then(Value::as_object) {
        let labels = labels
            .iter()
            .filter(|(path, label)| paths.contains(path.as_str()) && label.is_string())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>();
        if !labels.is_empty() {
            result.insert(
                "electron-workspace-root-labels".to_string(),
                Value::Object(labels),
            );
        }
    }
    result
}

#[cfg(test)]
#[path = "codex_profile_desktop_state_tests.rs"]
mod tests;
