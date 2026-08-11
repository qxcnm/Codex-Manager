use codexmanager_core::storage::{
    KiroCredentialSecret, KiroCredentialUpsert, KiroVaultError, Storage,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroImportPreviewItem {
    pub source_index: usize,
    pub auth_method: String,
    pub email: Option<String>,
    pub region: Option<String>,
    pub subscription: Option<String>,
    pub confidence: f32,
    pub duplicate_hint: String,
    pub is_update: bool,
    pub mapped_fields: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroImportIssue {
    pub source_index: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroImportPreview {
    pub items: Vec<KiroImportPreviewItem>,
    pub issues: Vec<KiroImportIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KiroImportResult {
    pub imported: usize,
    pub failed: usize,
    pub issues: Vec<KiroImportIssue>,
}

/// User supplied field paths for credential formats that are not recognized
/// by the built-in aliases. Paths use dot notation (for example
/// `auth.refresh_token`) and are evaluated case-sensitively first, then with a
/// case-insensitive object-key fallback.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KiroImportMapping {
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub auth_method: Option<String>,
    pub email: Option<String>,
    pub region: Option<String>,
    pub auth_region: Option<String>,
    pub api_region: Option<String>,
    pub subscription: Option<String>,
    pub expires_at: Option<String>,
    pub proxy_url: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub credit_limit: Option<String>,
    pub credit_used: Option<String>,
    pub machine_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedCredential {
    preview: KiroImportPreviewItem,
    secret: KiroCredentialSecret,
    expires_at: Option<i64>,
    auth_region: Option<String>,
    api_region: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    credit_limit: Option<f64>,
    credit_used: Option<f64>,
    identity_hint: String,
}

/// Parses Kiro Social/IdC JSON without returning any token or client secret.
#[cfg(test)]
pub(crate) fn preview_json(input: &str) -> Result<KiroImportPreview, String> {
    preview_json_with_mapping(input, None)
}

#[cfg(test)]
pub(crate) fn preview_json_with_mapping(
    input: &str,
    mapping: Option<&KiroImportMapping>,
) -> Result<KiroImportPreview, String> {
    preview_json_for_import(None, input, mapping)
}

pub(crate) fn preview_json_with_storage(
    storage: &Storage,
    input: &str,
    mapping: Option<&KiroImportMapping>,
) -> Result<KiroImportPreview, String> {
    preview_json_for_import(Some(storage), input, mapping)
}

fn preview_json_for_import(
    storage: Option<&Storage>,
    input: &str,
    mapping: Option<&KiroImportMapping>,
) -> Result<KiroImportPreview, String> {
    let value = parse_and_map(input, mapping)?;
    let mut objects = Vec::new();
    collect_candidate_objects(&value, &mut objects);
    let mut preview = KiroImportPreview::default();
    for (index, object) in objects.into_iter().enumerate() {
        match parse_object(index, object) {
            Ok(mut parsed) => {
                if let Some(storage) = storage {
                    parsed.preview.is_update = storage
                        .kiro_credential_exists(
                            parsed.preview.auth_method.as_str(),
                            parsed.identity_hint.as_str(),
                        )
                        .map_err(|error| safe_vault_error(&error))?;
                }
                preview.items.push(parsed.preview)
            }
            Err(message) => preview.issues.push(KiroImportIssue {
                source_index: index,
                message,
            }),
        }
    }
    if preview.items.is_empty() && preview.issues.is_empty() {
        preview.issues.push(KiroImportIssue {
            source_index: 0,
            message: "no credential objects found".into(),
        });
    }
    Ok(preview)
}

/// Imports valid entries independently. A malformed entry never rolls back other entries.
#[cfg(test)]
pub(crate) fn import_json(storage: &Storage, input: &str) -> Result<KiroImportResult, String> {
    import_json_with_mapping(storage, input, None)
}

pub(crate) fn import_json_with_mapping(
    storage: &Storage,
    input: &str,
    mapping: Option<&KiroImportMapping>,
) -> Result<KiroImportResult, String> {
    let value = parse_and_map(input, mapping)?;
    let mut objects = Vec::new();
    collect_candidate_objects(&value, &mut objects);
    let mut result = KiroImportResult::default();
    for (index, object) in objects.into_iter().enumerate() {
        let parsed = match parse_object(index, object) {
            Ok(parsed) => parsed,
            Err(message) => {
                result.failed += 1;
                result.issues.push(KiroImportIssue {
                    source_index: index,
                    message,
                });
                continue;
            }
        };
        let metadata_json = serde_json::to_string(&parsed.preview.metadata)
            .map_err(|error| format!("serialize metadata failed: {error}"))?;
        let upsert = KiroCredentialUpsert {
            id: new_credential_id(),
            auth_method: parsed.preview.auth_method.clone(),
            identity_hint: parsed.identity_hint,
            email: parsed.preview.email,
            auth_region: parsed.auth_region,
            api_region: parsed.api_region,
            subscription: parsed.preview.subscription,
            status: "active".into(),
            priority: 0,
            weight: 1.0,
            proxy_url: parsed.proxy_url,
            proxy_username: parsed.proxy_username,
            metadata_json,
            credit_limit: parsed.credit_limit,
            credit_used: parsed.credit_used,
            expires_at: parsed.expires_at,
            secret: parsed.secret,
        };
        match storage.upsert_kiro_credential(&upsert) {
            Ok(()) => result.imported += 1,
            Err(error) => {
                result.failed += 1;
                result.issues.push(KiroImportIssue {
                    source_index: index,
                    message: safe_vault_error(&error),
                });
            }
        }
    }
    Ok(result)
}

fn parse_and_map(input: &str, mapping: Option<&KiroImportMapping>) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON: {error}"))?;
    let Some(mapping) = mapping else {
        return Ok(value);
    };
    if mapping.refresh_token.trim().is_empty() {
        return Err("manual mapping requires refreshToken path".into());
    }
    let mut candidates = Vec::new();
    collect_mapped_objects(&value, mapping, &mut candidates);
    if candidates.is_empty() {
        return Err(format!(
            "manual mapping path '{}' did not match any credential",
            mapping.refresh_token
        ));
    }
    Ok(Value::Array(
        candidates.into_iter().map(Value::Object).collect(),
    ))
}

fn collect_mapped_objects(
    value: &Value,
    mapping: &KiroImportMapping,
    out: &mut Vec<Map<String, Value>>,
) {
    match value {
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_mapped_objects(value, mapping, out)),
        Value::Object(object) => {
            if value_at_path(object, &mapping.refresh_token).is_some() {
                out.push(apply_mapping(object, mapping));
                return;
            }
            object
                .values()
                .for_each(|value| collect_mapped_objects(value, mapping, out));
        }
        _ => {}
    }
}

fn apply_mapping(object: &Map<String, Value>, mapping: &KiroImportMapping) -> Map<String, Value> {
    let mut mapped = Map::new();
    let fields = [
        ("refreshToken", Some(mapping.refresh_token.as_str())),
        ("accessToken", mapping.access_token.as_deref()),
        ("clientId", mapping.client_id.as_deref()),
        ("clientSecret", mapping.client_secret.as_deref()),
        ("authMethod", mapping.auth_method.as_deref()),
        ("email", mapping.email.as_deref()),
        ("region", mapping.region.as_deref()),
        ("authRegion", mapping.auth_region.as_deref()),
        ("apiRegion", mapping.api_region.as_deref()),
        ("subscription", mapping.subscription.as_deref()),
        ("expiresAt", mapping.expires_at.as_deref()),
        ("proxyUrl", mapping.proxy_url.as_deref()),
        ("proxyUsername", mapping.proxy_username.as_deref()),
        ("proxyPassword", mapping.proxy_password.as_deref()),
        ("creditLimit", mapping.credit_limit.as_deref()),
        ("creditUsed", mapping.credit_used.as_deref()),
        ("machineId", mapping.machine_id.as_deref()),
    ];
    for (target, source) in fields {
        if let Some(value) = source.and_then(|path| value_at_path(object, path)) {
            mapped.insert(target.into(), value.clone());
        }
    }
    mapped
}

fn value_at_path<'a>(object: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut segments = path
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty());
    let first = segments.next()?;
    let mut value = object.get(first).or_else(|| get_ci(object, first))?;
    for segment in segments {
        value = match value {
            Value::Object(current) => current.get(segment).or_else(|| get_ci(current, segment))?,
            Value::Array(current) => current.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

fn collect_candidate_objects<'a>(value: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match value {
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_candidate_objects(value, out)),
        Value::Object(object) => {
            if looks_like_credential(object) {
                out.push(object);
                return;
            }
            for key in ["credentials", "accounts", "items", "data", "kiro"] {
                if let Some(value) = get_ci(object, key) {
                    collect_candidate_objects(value, out);
                }
            }
        }
        _ => {}
    }
}

fn looks_like_credential(object: &Map<String, Value>) -> bool {
    string_ci(object, "refreshToken").is_some() || string_ci(object, "refresh_token").is_some()
}

fn parse_object(index: usize, object: &Map<String, Value>) -> Result<ParsedCredential, String> {
    let refresh_token = required_string(object, &["refreshToken", "refresh_token"])?;
    let client_id = optional_string(object, &["clientId", "client_id"]);
    let client_secret = optional_string(object, &["clientSecret", "client_secret"]);
    let declared_auth = optional_string(object, &["authMethod", "auth_method", "provider"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    let auth_method =
        if client_id.is_some() || client_secret.is_some() || declared_auth.contains("idc") {
            if client_id.is_none() || client_secret.is_none() {
                return Err("IdC credential requires clientId and clientSecret".into());
            }
            "idc"
        } else {
            "social"
        };
    let email = optional_string(object, &["email", "accountEmail"]);
    let region = optional_string(object, &["region"]);
    let auth_region =
        optional_string(object, &["authRegion", "auth_region"]).or_else(|| region.clone());
    let api_region =
        optional_string(object, &["apiRegion", "api_region"]).or_else(|| region.clone());
    let preview_region = api_region.clone().or_else(|| auth_region.clone());
    let subscription = optional_string(object, &["subscription", "plan", "planType"]);
    let expires_at = optional_i64(object, &["expiresAt", "expires_at"]);
    let access_token = optional_string(object, &["accessToken", "access_token"]);
    let raw_proxy_url = optional_string(object, &["proxyUrl", "proxy_url"]);
    let explicit_proxy_username = optional_string(object, &["proxyUsername", "proxy_username"]);
    let explicit_proxy_password = optional_string(object, &["proxyPassword", "proxy_password"]);
    let (proxy_url, embedded_proxy_username, embedded_proxy_password) =
        split_proxy_credentials(raw_proxy_url)?;
    let proxy_username = explicit_proxy_username.or(embedded_proxy_username);
    let proxy_password = explicit_proxy_password.or(embedded_proxy_password);
    let credit_limit = optional_f64(object, &["creditLimit", "credit_limit"]);
    let credit_used = optional_f64(object, &["creditUsed", "credit_used"]);
    let identity_hint = email
        .clone()
        .or_else(|| client_id.clone())
        .unwrap_or_else(|| {
            let digest = Sha256::digest(refresh_token.as_bytes());
            format!("token:{:x}", digest)
        });
    let mut mapped_fields = vec!["refreshToken".into()];
    if client_id.is_some() {
        mapped_fields.push("clientId".into());
    }
    if client_secret.is_some() {
        mapped_fields.push("clientSecret".into());
    }
    if email.is_some() {
        mapped_fields.push("email".into());
    }
    if region.is_some() {
        mapped_fields.push("region (auth + api)".into());
    } else {
        if auth_region.is_some() {
            mapped_fields.push("authRegion".into());
        }
        if api_region.is_some() {
            mapped_fields.push("apiRegion".into());
        }
    }
    if subscription.is_some() {
        mapped_fields.push("subscription".into());
    }
    if credit_limit.is_some() {
        mapped_fields.push("creditLimit".into());
    }
    if credit_used.is_some() {
        mapped_fields.push("creditUsed".into());
    }
    if proxy_url.is_some() {
        mapped_fields.push("proxyUrl".into());
    }
    if proxy_username.is_some() {
        mapped_fields.push("proxyUsername".into());
    }
    if proxy_password.is_some() {
        mapped_fields.push("proxyPassword".into());
    }
    let mut metadata = BTreeMap::new();
    for key in [
        "provider",
        "time",
        "machineId",
        "machine_id",
        "systemVersion",
        "system_version",
        "nodeVersion",
        "node_version",
        "kiroVersion",
        "kiro_version",
    ] {
        if let Some(value) = get_ci(object, key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    let duplicate_hint = short_hash(&identity_hint);
    Ok(ParsedCredential {
        preview: KiroImportPreviewItem {
            source_index: index,
            auth_method: auth_method.into(),
            email,
            region: preview_region,
            subscription,
            confidence: if auth_method == "idc" { 0.99 } else { 0.95 },
            duplicate_hint,
            is_update: false,
            mapped_fields,
            metadata,
        },
        secret: KiroCredentialSecret {
            refresh_token,
            access_token,
            client_id,
            client_secret,
            proxy_password,
        },
        expires_at,
        auth_region,
        api_region,
        proxy_url,
        proxy_username,
        credit_limit,
        credit_used,
        identity_hint,
    })
}

fn split_proxy_credentials(
    proxy_url: Option<String>,
) -> Result<(Option<String>, Option<String>, Option<String>), String> {
    let Some(raw) = proxy_url else {
        return Ok((None, None, None));
    };
    if raw.eq_ignore_ascii_case("direct") {
        return Ok((Some(raw), None, None));
    }
    let mut parsed = url::Url::parse(&raw)
        .or_else(|_| url::Url::parse(format!("http://{raw}").as_str()))
        .map_err(|_| "invalid proxyUrl".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err("unsupported proxyUrl scheme".into());
    }
    let username = (!parsed.username().is_empty()).then(|| {
        urlencoding::decode(parsed.username())
            .map(|value| value.into_owned())
            .unwrap_or_else(|_| parsed.username().to_string())
    });
    let password = parsed.password().map(|value| {
        urlencoding::decode(value)
            .map(|value| value.into_owned())
            .unwrap_or_else(|_| value.to_string())
    });
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    Ok((Some(parsed.to_string()), username, password))
}

pub(crate) fn sanitize_proxy_settings(
    proxy_url: Option<String>,
) -> Result<(Option<String>, Option<String>, Option<String>), String> {
    split_proxy_credentials(proxy_url)
}

fn required_string(object: &Map<String, Value>, keys: &[&str]) -> Result<String, String> {
    optional_string(object, keys)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing {}", keys[0]))
}
fn optional_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| string_ci(object, key))
        .map(ToOwned::to_owned)
}
fn optional_i64(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| get_ci(object, key))
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
}
fn optional_f64(object: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| get_ci(object, key))
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
}
fn string_ci<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    get_ci(object, key)?.as_str()
}
fn get_ci<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}
fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..12].to_string()
}
fn new_credential_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "kiro-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}
fn safe_vault_error(error: &KiroVaultError) -> String {
    // Vault errors are deliberately structural and never include plaintext secrets.
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_detects_social_and_idc_and_redacts_secrets() {
        let json = r#"[{"refreshToken":"social-secret","email":"social@example.test"},{"refreshToken":"idc-secret","clientId":"client","clientSecret":"client-secret","provider":"IdC","region":"us-east-1","subscription":"pro","creditLimit":100,"creditUsed":2}]"#;
        let preview = preview_json(json).unwrap();
        assert_eq!(preview.items.len(), 2);
        assert_eq!(preview.items[0].auth_method, "social");
        assert_eq!(preview.items[1].auth_method, "idc");
        let serialized = serde_json::to_string(&preview).unwrap();
        assert!(!serialized.contains("social-secret"));
        assert!(!serialized.contains("client-secret"));
    }

    #[test]
    fn bad_record_does_not_hide_valid_preview() {
        let json = r#"{"credentials":[{"refreshToken":"ok"},{"refreshToken":"bad","clientId":"missing-secret"}]}"#;
        let preview = preview_json(json).unwrap();
        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.issues.len(), 1);
    }

    #[test]
    fn manual_mapping_supports_nested_fields_and_wrapped_arrays() {
        let json = r#"{"payload":{"users":[{"identity":{"mail":"mapped@example.test"},"tokens":{"refresh":"mapped-secret"},"aws":{"auth":"us-west-2","api":"eu-west-1"},"limits":{"total":"50","used":4}}]}}"#;
        let mapping = KiroImportMapping {
            refresh_token: "tokens.refresh".into(),
            email: Some("identity.mail".into()),
            auth_region: Some("aws.auth".into()),
            api_region: Some("aws.api".into()),
            credit_limit: Some("limits.total".into()),
            credit_used: Some("limits.used".into()),
            ..Default::default()
        };

        let preview = preview_json_with_mapping(json, Some(&mapping)).unwrap();

        assert_eq!(preview.items.len(), 1);
        assert_eq!(
            preview.items[0].email.as_deref(),
            Some("mapped@example.test")
        );
        assert_eq!(preview.items[0].region.as_deref(), Some("eu-west-1"));
        let serialized = serde_json::to_string(&preview).unwrap();
        assert!(!serialized.contains("mapped-secret"));
    }

    #[test]
    fn manual_mapping_rejects_missing_refresh_path_without_leaking_input() {
        let mapping = KiroImportMapping {
            refresh_token: "missing.token".into(),
            ..Default::default()
        };
        let error =
            preview_json_with_mapping(r#"{"secret":"must-not-appear-in-error"}"#, Some(&mapping))
                .unwrap_err();
        assert!(error.contains("missing.token"));
        assert!(!error.contains("must-not-appear-in-error"));
    }

    #[test]
    fn embedded_proxy_credentials_are_removed_from_plaintext_url() {
        let (url, username, password) =
            split_proxy_credentials(Some("http://proxy-user:proxy-secret@127.0.0.1:7897".into()))
                .unwrap();
        assert_eq!(url.as_deref(), Some("http://127.0.0.1:7897/"));
        assert_eq!(username.as_deref(), Some("proxy-user"));
        assert_eq!(password.as_deref(), Some("proxy-secret"));
    }

    #[cfg(windows)]
    #[test]
    fn import_encrypts_proxy_password_and_stores_sanitized_url() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let json = r#"{"refreshToken":"refresh","email":"proxy@example.test","proxyUrl":"http://proxy-user:proxy-secret@127.0.0.1:7897"}"#;

        let result = import_json(&storage, json).unwrap();
        assert_eq!(result.imported, 1);
        let record = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(record.proxy_url.as_deref(), Some("http://127.0.0.1:7897/"));
        assert_eq!(record.proxy_username.as_deref(), Some("proxy-user"));
        let secret = storage
            .read_kiro_credential_secret(&record.id)
            .unwrap()
            .unwrap();
        assert_eq!(secret.proxy_password.as_deref(), Some("proxy-secret"));
    }

    #[cfg(windows)]
    #[test]
    fn import_preserves_independent_auth_and_api_regions() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let json = r#"{
            "refreshToken":"refresh",
            "email":"regions@example.test",
            "authRegion":"us-east-1",
            "apiRegion":"eu-central-1"
        }"#;

        let result = import_json(&storage, json).unwrap();
        assert_eq!(result.imported, 1);
        let record = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(record.auth_region.as_deref(), Some("us-east-1"));
        assert_eq!(record.api_region.as_deref(), Some("eu-central-1"));
    }

    #[cfg(windows)]
    #[test]
    fn preview_marks_existing_identity_as_token_update() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let first = r#"{"refreshToken":"old-refresh","email":"existing@example.test"}"#;
        assert_eq!(import_json(&storage, first).unwrap().imported, 1);

        let next = r#"{"refreshToken":"new-refresh","email":"existing@example.test"}"#;
        let preview = preview_json_with_storage(&storage, next, None).unwrap();

        assert_eq!(preview.items.len(), 1);
        assert!(preview.items[0].is_update);
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("new-refresh"));
    }

    #[cfg(windows)]
    #[test]
    fn imports_one_thousand_records_without_bad_record_rollback() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let mut records = (0..1_000)
            .map(|index| {
                serde_json::json!({
                    "refreshToken": format!("refresh-{index}"),
                    "email": format!("user-{index}@example.test"),
                    "region": "us-east-1"
                })
            })
            .collect::<Vec<_>>();
        records.push(serde_json::json!({
            "refreshToken": "bad-idc",
            "clientId": "missing-client-secret"
        }));
        let json = serde_json::to_string(&records).unwrap();

        let result = import_json(&storage, &json).unwrap();

        assert_eq!(result.imported, 1_000);
        assert_eq!(result.failed, 1);
        assert_eq!(storage.list_kiro_credentials().unwrap().len(), 1_000);
    }

    #[cfg(windows)]
    #[test]
    fn manual_mapping_imports_and_encrypts_custom_format() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let mapping = KiroImportMapping {
            refresh_token: "secrets.refresh".into(),
            client_id: Some("oauth.id".into()),
            client_secret: Some("oauth.secret".into()),
            email: Some("profile.email".into()),
            ..Default::default()
        };
        let json = r#"{"secrets":{"refresh":"custom-refresh"},"oauth":{"id":"custom-id","secret":"custom-secret"},"profile":{"email":"custom@example.test"}}"#;

        let result = import_json_with_mapping(&storage, json, Some(&mapping)).unwrap();

        assert_eq!(result.imported, 1);
        let record = storage.list_kiro_credentials().unwrap().remove(0);
        assert_eq!(record.auth_method, "idc");
        assert_eq!(record.email.as_deref(), Some("custom@example.test"));
        let secret = storage
            .read_kiro_credential_secret(&record.id)
            .unwrap()
            .unwrap();
        assert_eq!(secret.refresh_token, "custom-refresh");
        assert_eq!(secret.client_secret.as_deref(), Some("custom-secret"));
    }
}
