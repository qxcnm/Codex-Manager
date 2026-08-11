use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;

use crate::{account_status, storage_helpers::open_storage};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CredentialRepairReportResult {
    account_id: String,
    outcome: String,
    status: String,
    status_reason: String,
    terminal: bool,
}

/// Accept a result from an interactive/mail-assisted OAuth repair worker.
/// The outcome vocabulary is deliberately closed so arbitrary browser text can
/// never turn an account into `banned`.
pub(crate) fn report_credential_repair(
    account_id: &str,
    outcome: &str,
    detail: Option<&str>,
) -> Result<CredentialRepairReportResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    if storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("find account failed: {err}"))?
        .is_none()
    {
        return Err("account not found".to_string());
    }

    let normalized = outcome.trim().to_ascii_lowercase();
    let (status, reason, terminal) = match normalized.as_str() {
        "repaired" => ("active", "credential_repaired", false),
        "reauth_in_progress" => ("unavailable", "credential_reauth_in_progress", false),
        "reauth_required" => ("unavailable", "credential_reauth_required", false),
        "refresh_token_expired" => (
            "unavailable",
            "refresh_token_invalid:refresh_token_expired",
            false,
        ),
        "refresh_token_revoked" => (
            "unavailable",
            "refresh_token_invalid:refresh_token_invalidated",
            false,
        ),
        "network_unknown" | "cloudflare_challenge" => {
            ("unavailable", "credential_repair_network_unknown", false)
        }
        "account_deactivated" => ("banned", "account_deactivated", true),
        "workspace_deactivated" => ("banned", "workspace_deactivated", true),
        _ => {
            return Err(format!(
                "unsupported credential repair outcome: {normalized}"
            ))
        }
    };

    account_status::set_account_status(&storage, account_id, status, reason);
    let safe_detail = sanitize_detail(detail.unwrap_or_default());
    let message = if safe_detail.is_empty() {
        format!("outcome={normalized} reason={reason}")
    } else {
        format!("outcome={normalized} reason={reason} detail={safe_detail}")
    };
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "credential_repair_report".to_string(),
        message,
        created_at: now_ts(),
    });

    Ok(CredentialRepairReportResult {
        account_id: account_id.to_string(),
        outcome: normalized,
        status: status.to_string(),
        status_reason: reason.to_string(),
        terminal,
    })
}

fn sanitize_detail(detail: &str) -> String {
    detail
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect::<String>()
        .replace(['=', '&'], "_")
}

#[cfg(test)]
mod tests {
    use super::sanitize_detail;

    #[test]
    fn repair_detail_is_bounded_and_control_free() {
        let detail = format!("oauth_page=account\n{}", "x".repeat(300));
        let safe = sanitize_detail(&detail);
        assert!(safe.len() <= 160);
        assert!(!safe.contains('\n'));
        assert!(!safe.contains('='));
    }
}
