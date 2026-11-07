use super::{store, types::*};
use crate::RpcActor;
use codexmanager_core::storage::ServiceHealthRecord;

pub fn owner(actor: &RpcActor) -> Result<String, String> {
    if let Some(id) = actor.user_id.as_deref().filter(|v| !v.is_empty()) {
        return Ok(format!("user:{id}"));
    }
    if actor.is_admin() {
        return Ok("desktop:administrator".into());
    }
    Err("permission_denied: health report requires user session".into())
}
pub fn active(now: i64) -> Result<Vec<(String, Report)>, String> {
    store::list("report", now, 10001)?
        .into_iter()
        .map(|row| {
            let report = serde_json::from_str::<Report>(&row.payload)
                .map_err(|_| "health_record_invalid".to_owned())?;
            Ok((row.owner, report))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|rows| {
            rows.into_iter()
                .filter(|(_, r)| r.expires_at > now)
                .collect()
        })
}
pub fn mine(actor: &RpcActor, now: i64) -> Result<Vec<Report>, String> {
    let owner = owner(actor)?;
    Ok(active(now)?
        .into_iter()
        .filter(|(id, _)| id == &owner)
        .map(|(_, r)| r)
        .collect())
}
pub fn report(actor: &RpcActor, input: ReportInput, now: i64) -> Result<Report, String> {
    let owner = owner(actor)?;
    let scope = serde_json::to_string(&(input.provider, input.region))
        .map_err(|_| "health_record_invalid")?;
    let id = format!("report:{}", super::digest(&format!("{owner}:{scope}")));
    let previous = store::get(&id)?;
    if let Some(row) = previous.as_ref() {
        let old: Report =
            serde_json::from_str(&row.payload).map_err(|_| "health_record_invalid")?;
        if now - old.updated_at < 600 {
            if old.expires_at > now && old.symptom == input.symptom {
                return Ok(old);
            }
            return Err("health_report_rate_limited".into());
        }
    } else if store::list("report", now, 10001)?.len() >= 10000 {
        return Err("health_report_capacity_reached".into());
    }
    let report = Report {
        id: id.clone(),
        provider: input.provider,
        symptom: input.symptom,
        region: input.region,
        created_at: previous.as_ref().map(|r| r.created_at).unwrap_or(now),
        updated_at: now,
        expires_at: now + REPORT_TTL,
    };
    let row = ServiceHealthRecord {
        id,
        kind: "report".into(),
        owner,
        payload: serde_json::to_string(&report).map_err(|_| "health_record_invalid")?,
        created_at: report.created_at,
        updated_at: previous
            .as_ref()
            .map(|r| now.max(r.updated_at + 1))
            .unwrap_or(now),
        expires_at: now + WINDOW_SECS,
    };
    if !store::put(row, previous.map(|r| r.updated_at))? {
        return Err("health_report_rate_limited".into());
    }
    Ok(report)
}
pub fn resolve(actor: &RpcActor, id: &str, now: i64) -> Result<(), String> {
    let owner = owner(actor)?;
    let mut row = store::get(id)?
        .filter(|r| r.kind == "report" && r.owner == owner)
        .ok_or("health_report_not_found")?;
    let mut report: Report =
        serde_json::from_str(&row.payload).map_err(|_| "health_record_invalid")?;
    let expected = row.updated_at;
    report.expires_at = now; // Keep the submission time so resolving cannot reset cooldown.
    row.payload = serde_json::to_string(&report).map_err(|_| "health_record_invalid")?;
    row.updated_at = now.max(expected + 1);
    if !store::put(row, Some(expected))? {
        return Err("health_report_conflict".into());
    }
    Ok(())
}
