//! Explicit gateway authentication changes, with a recoverable account snapshot.
use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct CommitWrites {
    files: HashMap<PathBuf, Option<String>>,
    settings: HashMap<String, Option<String>>,
}

thread_local! {
    static COMMIT_WRITES: RefCell<Option<CommitWrites>> = const { RefCell::new(None) };
}

pub(super) fn record_file_write(path: &Path, content: Option<&str>) {
    COMMIT_WRITES.with_borrow_mut(|writes| {
        if let Some(writes) = writes {
            writes
                .files
                .insert(path.to_path_buf(), content.map(str::to_owned));
        }
    });
}

pub(super) fn record_setting_write(key: &str, content: Option<&str>) {
    COMMIT_WRITES.with_borrow_mut(|writes| {
        if let Some(writes) = writes {
            writes
                .settings
                .insert(key.to_owned(), content.map(str::to_owned));
        }
    });
}

const AUTH_SNAPSHOT_FILE: &str = "gateway-auth-before-apikey.json";
const AUTH_CONFIG_KEYS: [&str; 3] = [
    "cli_auth_credentials_store",
    "forced_login_method",
    "forced_chatgpt_workspace_id",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GatewayAuthMode {
    #[default]
    Preserve,
    Apikey,
}

impl GatewayAuthMode {
    pub(super) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("preserve") => Ok(Self::Preserve),
            Some("apikey") => Ok(Self::Apikey),
            Some(_) => Err("invalid authMode: expected preserve or apikey".to_string()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthSnapshot {
    original_auth: Option<String>,
    managed_auth_sha256: String,
    config_fields: HashMap<String, Option<String>>,
}

pub(super) struct AuthChange {
    original_auth: Option<String>,
    target_auth: Option<String>,
    snapshot: Option<String>,
    pub(super) config_toml: String,
}

fn fingerprint(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn read_snapshot(paths: &ManagedProfilePaths) -> Result<Option<AuthSnapshot>, String> {
    read_optional(&paths.root.join(AUTH_SNAPSHOT_FILE))?
        .map(|content| {
            // Authentication contents must never be included in a parse error.
            serde_json::from_str(&content)
                .map_err(|_| "invalid saved gateway authentication snapshot".to_string())
        })
        .transpose()
}

pub(super) fn prepare(
    profile_dir: &Path,
    paths: &ManagedProfilePaths,
    mode: GatewayAuthMode,
    secret: &str,
    explicit_switch: bool,
    config_toml: &str,
) -> Result<AuthChange, String> {
    let original_auth = read_optional(&profile_dir.join(AUTH_FILE))?;
    let snapshot = read_snapshot(paths)?;
    let still_managed = snapshot.as_ref().is_some_and(|snapshot| {
        original_auth
            .as_deref()
            .is_some_and(|auth| fingerprint(auth) == snapshot.managed_auth_sha256)
    });
    match mode {
        GatewayAuthMode::Preserve => {
            let config_toml = restore_auth_config(config_toml, snapshot.as_ref())?;
            Ok(AuthChange {
                target_auth: if still_managed {
                    snapshot.and_then(|snapshot| snapshot.original_auth)
                } else {
                    original_auth.clone()
                },
                original_auth,
                snapshot: None,
                config_toml,
            })
        }
        GatewayAuthMode::Apikey => {
            if !explicit_switch && !still_managed {
                return Err("Codex auth.json changed outside CodexManager; reapply the gateway authentication mode before syncing".to_string());
            }
            let secret = secret.trim();
            if secret.is_empty() {
                return Err("platform key secret is empty".to_string());
            }
            let target_auth = serde_json::to_string_pretty(&serde_json::json!({
                "auth_mode": "apikey",
                "OPENAI_API_KEY": secret,
            }))
            .map_err(|_| "serialize gateway authentication failed".to_string())?;
            let mut config = parse_config(config_toml)?;
            let config_fields = if still_managed {
                snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.config_fields.clone())
                    .unwrap_or_default()
            } else {
                AUTH_CONFIG_KEYS
                    .into_iter()
                    .map(|key| (key.to_string(), config.get(key).map(ToString::to_string)))
                    .collect()
            };
            config["cli_auth_credentials_store"] = toml_value("file");
            config.remove("forced_login_method");
            config.remove("forced_chatgpt_workspace_id");
            let original_snapshot = if still_managed {
                snapshot.and_then(|snapshot| snapshot.original_auth)
            } else {
                original_auth.clone()
            };
            let snapshot = serde_json::to_string(&AuthSnapshot {
                original_auth: original_snapshot,
                managed_auth_sha256: fingerprint(&target_auth),
                config_fields,
            })
            .map_err(|_| "serialize gateway authentication snapshot failed".to_string())?;
            Ok(AuthChange {
                original_auth,
                target_auth: Some(target_auth),
                snapshot: Some(snapshot),
                config_toml: config.to_string(),
            })
        }
    }
}

fn restore_auth_config(
    config_toml: &str,
    snapshot: Option<&AuthSnapshot>,
) -> Result<String, String> {
    let Some(snapshot) = snapshot else {
        return Ok(config_toml.to_string());
    };
    let mut config = parse_config(config_toml)?;
    for key in AUTH_CONFIG_KEYS {
        let unchanged = if key == "cli_auth_credentials_store" {
            config.get(key).and_then(Item::as_str) == Some("file")
        } else {
            config.get(key).is_none()
        };
        if !unchanged {
            continue;
        }
        match snapshot.config_fields.get(key) {
            Some(Some(original)) => {
                let original = parse_config(&format!("{key} = {original}"))?;
                if let Some(value) = original.get(key) {
                    config.insert(key, value.clone());
                }
            }
            Some(None) => {
                config.remove(key);
            }
            None => {}
        }
    }
    Ok(config.to_string())
}

impl AuthChange {
    pub(super) fn apply(
        self,
        profile_dir: &Path,
        paths: &ManagedProfilePaths,
    ) -> Result<(), String> {
        if read_optional(&profile_dir.join(AUTH_FILE))? != self.original_auth {
            return Err(
                "Codex auth.json changed while applying the gateway profile; retry".to_string(),
            );
        }
        let changed = self.original_auth != self.target_auth;
        if changed {
            desktop_state::protect_account_switch(
                profile_dir,
                self.target_auth.as_deref(),
                &paths.root.join("desktop-state-before-switch.json"),
            )?;
        }
        // Publish the recovery data before replacing credentials.
        if let Some(snapshot) = self.snapshot.as_deref() {
            write_atomic(&paths.root.join(AUTH_SNAPSHOT_FILE), snapshot)?;
        }
        if changed {
            restore_optional_file(&profile_dir.join(AUTH_FILE), self.target_auth.as_deref())?;
        }
        if self.snapshot.is_none() {
            clear_snapshot(paths)?;
        }
        Ok(())
    }
}

pub(super) fn clear_snapshot(paths: &ManagedProfilePaths) -> Result<(), String> {
    remove_file_if_exists(&paths.root.join(AUTH_SNAPSHOT_FILE))
}

/// Keep credentials, their recovery snapshot, config, and profile metadata together.
pub(super) fn commit<T>(
    profile_dir: &Path,
    paths: &ManagedProfilePaths,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let snapshot = capture_apply_models_commit_snapshot(profile_dir, paths)?;
    let mut files = [
        profile_dir.join(AUTH_FILE),
        paths.root.join(AUTH_SNAPSHOT_FILE),
        profile_dir.join(".codex-global-state.json"),
        paths.root.join("desktop-state-before-switch.json"),
    ]
    .into_iter()
    .map(|path| read_optional(&path).map(|content| (path, content)))
    .collect::<Result<Vec<_>, _>>()?;
    files.extend([
        (
            paths.gateway_model_catalog_path.clone(),
            snapshot.gateway_model_catalog,
        ),
        (profile_dir.join(CONFIG_FILE), snapshot.config_toml),
        (paths.marker_path.clone(), snapshot.marker),
        (paths.legacy_marker_path.clone(), snapshot.legacy_marker),
    ]);
    let settings = [
        (APP_SETTING_CODEX_HOME_KEY, snapshot.codex_home_setting),
        (APP_SETTING_STATE_KEY, snapshot.state_setting),
        (APP_SETTING_BACKUPS_KEY, snapshot.backups_setting),
    ];
    COMMIT_WRITES.with_borrow_mut(|writes| *writes = Some(CommitWrites::default()));
    let result = work();
    let writes = COMMIT_WRITES
        .with_borrow_mut(Option::take)
        .unwrap_or_default();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let mut errors = Vec::new();
            for (path, content) in files {
                let Some(written) = writes.files.get(&path) else {
                    continue;
                };
                match read_optional(&path) {
                    Ok(current) if &current == written => collect_apply_models_rollback_error(
                        &mut errors,
                        restore_optional_file(&path, content.as_deref()),
                    ),
                    Ok(_) => {
                        errors.push(format!("externally changed file kept ({})", path.display()))
                    }
                    Err(error) => errors.push(error),
                }
            }
            for (key, content) in settings {
                let Some(written) = writes.settings.get(key) else {
                    continue;
                };
                if &crate::app_settings::get_persisted_app_setting(key) == written {
                    collect_apply_models_rollback_error(
                        &mut errors,
                        crate::app_settings::save_persisted_app_setting(key, content.as_deref()),
                    );
                } else {
                    errors.push(format!("externally changed setting kept ({key})"));
                }
            }
            if !snapshot.managed_root_existed {
                let _ = fs::remove_dir(&paths.root);
            }
            if errors.is_empty() {
                Err(error)
            } else {
                Err(format!(
                    "{error}; gateway authentication rollback failed: {}",
                    errors.join("; ")
                ))
            }
        }
    }
}
