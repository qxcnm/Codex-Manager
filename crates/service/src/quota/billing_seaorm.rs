use super::*;
use codexmanager_storage_seaorm::{
    ApiKeysRepository as Keys, SeaOrmStorage, UsersRepository as Users,
};
fn failure(e: impl std::fmt::Display) -> String {
    format!("billing rule storage failed: {e}")
}
pub(super) async fn list(s: SeaOrmStorage) -> Result<QuotaBillingRulesResult, String> {
    Ok(QuotaBillingRulesResult {
        items: Users::billing_rules(s.connection())
            .await
            .map_err(failure)?
            .into_iter()
            .map(billing_rule_result)
            .collect(),
    })
}
pub(super) async fn delete(
    s: SeaOrmStorage,
    id: String,
) -> Result<QuotaBillingRulesResult, String> {
    if id.trim().is_empty() {
        return Err("计费规则 ID 不能为空".into());
    }
    Users::delete_billing_rule(s.connection(), id.trim())
        .await
        .map_err(failure)?;
    list(s).await
}
pub(super) async fn upsert(
    s: SeaOrmStorage,
    input: BillingRuleUpsertInput,
) -> Result<QuotaBillingRulesResult, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("计费规则名称不能为空".into());
    }
    if !(0..=100_000).contains(&input.multiplier_millis) {
        return Err("计费倍率必须在 0 到 100 之间".into());
    }
    if matches!((input.starts_at,input.ends_at),(Some(a),Some(b)) if a>=b) {
        return Err("计费规则结束时间必须晚于开始时间".into());
    }
    let uid = normalize_optional_text(input.user_id);
    if let Some(id) = uid.as_deref() {
        if Users::get(s.connection(), id)
            .await
            .map_err(failure)?
            .is_none()
        {
            return Err("计费规则用户不存在".into());
        }
    }
    let kid = normalize_optional_text(input.api_key_id);
    if let Some(id) = kid.as_deref() {
        if Keys::get(s.connection(), id)
            .await
            .map_err(failure)?
            .is_none()
        {
            return Err("计费规则 API Key 不存在".into());
        }
    }
    if normalize_optional_text(input.project_id).is_some() {
        return Err("项目维度计费规则暂未开放".into());
    }
    let now = codexmanager_core::storage::now_ts();
    Users::put_billing_rule(
        s.connection(),
        BillingRule {
            id: normalize_optional_text(input.id).unwrap_or_else(|| generate_id("br", 8)),
            name: name.into(),
            status: normalize_billing_status(input.status)?,
            priority: input.priority.unwrap_or(0),
            multiplier_millis: input.multiplier_millis,
            model_pattern: normalize_optional_text(input.model_pattern),
            service_tier: normalize_optional_text(input.service_tier),
            user_id: uid,
            project_id: None,
            api_key_id: kid,
            starts_at: input.starts_at.filter(|v| *v > 0),
            ends_at: input.ends_at.filter(|v| *v > 0),
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(failure)?;
    list(s).await
}
