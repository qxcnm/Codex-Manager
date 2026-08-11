use codexmanager_core::storage::{
    GrokCredentialSecret, GrokCredentialUpsert, KiroVaultError, Storage,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokImportPreviewItem {
    pub source_index: usize,
    pub account_masked: String,
    pub confidence: f32,
    pub is_update: bool,
    pub mapped_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokImportIssue {
    pub source_index: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokImportPreview {
    pub items: Vec<GrokImportPreviewItem>,
    pub issues: Vec<GrokImportIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokImportResult {
    pub imported: usize,
    pub failed: usize,
    pub issues: Vec<GrokImportIssue>,
}

#[derive(Debug, Clone)]
struct ParsedCredential {
    source_index: usize,
    account: String,
    password: String,
    sso_token: String,
}

/// Previews Grok Web SSO card text without ever returning a password or SSO token.
pub(crate) fn preview_text_with_storage(
    storage: &Storage,
    input: &str,
) -> Result<GrokImportPreview, String> {
    let (credentials, issues) = parse_text(input);
    let mut preview = GrokImportPreview {
        items: Vec::new(),
        issues,
    };
    for credential in credentials {
        let is_update = storage
            .grok_credential_exists(&credential.account)
            .map_err(|error| safe_vault_error(&error))?;
        preview.items.push(GrokImportPreviewItem {
            source_index: credential.source_index,
            account_masked: mask_account(&credential.account),
            confidence: 0.98,
            is_update,
            mapped_fields: vec!["account".into(), "password".into(), "ssoToken".into()],
        });
    }
    if preview.items.is_empty() && preview.issues.is_empty() {
        preview.issues.push(GrokImportIssue {
            source_index: 0,
            message: "no Grok credential lines found".into(),
        });
    }
    Ok(preview)
}

/// Imports every valid line independently. Invalid lines never roll back valid entries.
pub(crate) fn import_text(storage: &Storage, input: &str) -> Result<GrokImportResult, String> {
    let (credentials, issues) = parse_text(input);
    let mut result = GrokImportResult {
        imported: 0,
        failed: issues.len(),
        issues,
    };
    for credential in credentials {
        let upsert = GrokCredentialUpsert {
            id: new_credential_id(),
            account: credential.account.clone(),
            status: "active".into(),
            priority: 0,
            weight: 1.0,
            proxy_url: None,
            metadata_json: "{}".into(),
            expires_at: None,
            secret: GrokCredentialSecret {
                account: credential.account,
                password: credential.password,
                sso_token: credential.sso_token,
            },
        };
        match storage.upsert_grok_credential(&upsert) {
            Ok(()) => result.imported += 1,
            Err(error) => {
                result.failed += 1;
                result.issues.push(GrokImportIssue {
                    source_index: credential.source_index,
                    message: safe_vault_error(&error),
                });
            }
        }
    }
    Ok(result)
}

fn parse_text(input: &str) -> (Vec<ParsedCredential>, Vec<GrokImportIssue>) {
    let mut credentials = Vec::new();
    let mut issues = Vec::new();
    for (line_index, raw_line) in input.lines().enumerate() {
        let source_index = line_index + 1;
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if should_skip_line(line) {
            continue;
        }
        let Some((account_and_password, sso_token)) = line.rsplit_once("----") else {
            // Human-readable headings and instructions are ignored. Lines that look like
            // credential data are reported without echoing any supplied value.
            if line.contains('@') || line.contains("eyJ") {
                issues.push(invalid_line(source_index));
            }
            continue;
        };
        let Some((account, password)) = account_and_password.split_once("----") else {
            issues.push(invalid_line(source_index));
            continue;
        };
        let account = account.trim();
        let password = password.trim();
        let sso_token = sso_token.trim();
        if !valid_account(account)
            || password.is_empty()
            || !looks_like_jwt(sso_token)
            || password.contains("----")
        {
            issues.push(invalid_line(source_index));
            continue;
        }
        credentials.push(ParsedCredential {
            source_index,
            account: account.to_string(),
            password: password.to_string(),
            sso_token: sso_token.to_string(),
        });
    }
    (credentials, issues)
}

fn should_skip_line(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("===")
        || line.starts_with('#')
        || line.contains("账号----grok密码----SSO")
        || line.contains("account----password----SSO")
}

fn valid_account(account: &str) -> bool {
    let Some((local, domain)) = account.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !account.chars().any(char::is_whitespace)
}

fn looks_like_jwt(value: &str) -> bool {
    let mut segments = value.split('.');
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    };
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some(a), Some(b), Some(c), None)
            if valid_segment(a) && valid_segment(b) && valid_segment(c)
    )
}

fn invalid_line(source_index: usize) -> GrokImportIssue {
    GrokImportIssue {
        source_index,
        message: "invalid Grok credential line".into(),
    }
}

fn mask_account(account: &str) -> String {
    let normalized = account.trim().to_ascii_lowercase();
    let Some((local, domain)) = normalized.split_once('@') else {
        return "***".into();
    };
    format!("{}***@{domain}", local.chars().take(2).collect::<String>())
}

fn new_credential_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "grok-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn safe_vault_error(_error: &KiroVaultError) -> String {
    "credential storage operation failed".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSO: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzZXNzaW9uX2lkIjoidGVzdCJ9.signature";

    fn storage() -> Storage {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
    }

    #[test]
    fn previews_card_delivery_text_without_secrets() {
        let storage = storage();
        let input = format!(
            "=== 使用说明 ===\n账号----grok密码----SSO\nuser@example.test----secret-password----{SSO}"
        );
        let preview = preview_text_with_storage(&storage, &input).unwrap();
        assert_eq!(preview.items.len(), 1);
        assert_eq!(preview.items[0].account_masked, "us***@example.test");
        let serialized = serde_json::to_string(&preview).unwrap();
        assert!(!serialized.contains("secret-password"));
        assert!(!serialized.contains(SSO));
    }

    #[test]
    fn malformed_line_does_not_block_valid_import() {
        let storage = storage();
        let input = format!("bad@example.test----missing-token\nok@example.test----pass----{SSO}");
        let result = import_text(&storage, &input).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(storage.list_grok_credentials().unwrap().len(), 1);
    }

    #[test]
    fn imports_one_thousand_credentials_and_updates_duplicates() {
        let storage = storage();
        let input = (0..1_000)
            .map(|index| format!("user{index}@example.test----pass-{index}----{SSO}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = import_text(&storage, &input).unwrap();
        assert_eq!(result.imported, 1_000);
        assert_eq!(result.failed, 0);
        assert_eq!(storage.list_grok_credentials().unwrap().len(), 1_000);

        let duplicate = format!("user0@example.test----new-password----{SSO}");
        let preview = preview_text_with_storage(&storage, &duplicate).unwrap();
        assert!(preview.items[0].is_update);
        assert_eq!(import_text(&storage, &duplicate).unwrap().imported, 1);
        assert_eq!(storage.list_grok_credentials().unwrap().len(), 1_000);
    }
}
