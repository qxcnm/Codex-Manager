import type { WebCommandDescriptor } from "./shared";

export function createQuotaWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_quota_overview: { rpcMethod: "quota/overview" },
    service_quota_model_usage: { rpcMethod: "quota/modelUsage" },
    service_quota_api_key_usage: { rpcMethod: "quota/apiKeyUsage" },
    service_quota_source_list: { rpcMethod: "quota/sourceList" },
    service_quota_model_pools: { rpcMethod: "quota/modelPools" },
    service_quota_system_pool: { rpcMethod: "quota/systemPool" },
    service_quota_capacity_config: { rpcMethod: "quota/capacityConfig" },
    service_quota_billing_rules: { rpcMethod: "quota/billingRules" },
    service_quota_billing_rule_upsert: {
      rpcMethod: "quota/billingRule/upsert",
      mapParams: (params) => ({
        id: typeof params?.id === "string" ? params.id : null,
        name: typeof params?.name === "string" ? params.name : "",
        status: typeof params?.status === "string" ? params.status : null,
        priority: typeof params?.priority === "number" ? params.priority : null,
        multiplierMillis: typeof params?.multiplierMillis === "number" ? params.multiplierMillis : typeof params?.multiplier_millis === "number" ? params.multiplier_millis : 1000,
        modelPattern: typeof params?.modelPattern === "string" ? params.modelPattern : typeof params?.model_pattern === "string" ? params.model_pattern : null,
        serviceTier: typeof params?.serviceTier === "string" ? params.serviceTier : typeof params?.service_tier === "string" ? params.service_tier : null,
        userId: typeof params?.userId === "string" ? params.userId : typeof params?.user_id === "string" ? params.user_id : null,
        apiKeyId: typeof params?.apiKeyId === "string" ? params.apiKeyId : typeof params?.api_key_id === "string" ? params.api_key_id : null,
        startsAt: typeof params?.startsAt === "number" ? params.startsAt : typeof params?.starts_at === "number" ? params.starts_at : null,
        endsAt: typeof params?.endsAt === "number" ? params.endsAt : typeof params?.ends_at === "number" ? params.ends_at : null,
      }),
    },
    service_quota_billing_rule_delete: { rpcMethod: "quota/billingRule/delete", mapParams: (params) => ({ id: typeof params?.id === "string" ? params.id : "" }) },
    service_quota_source_models_set: {
      rpcMethod: "quota/sourceModels/set",
      mapParams: (params) => ({
        sourceKind: typeof params?.sourceKind === "string" ? params.sourceKind : typeof params?.source_kind === "string" ? params.source_kind : "",
        sourceId: typeof params?.sourceId === "string" ? params.sourceId : typeof params?.source_id === "string" ? params.source_id : "",
        modelSlugs: Array.isArray(params?.modelSlugs) ? params.modelSlugs : Array.isArray(params?.model_slugs) ? params.model_slugs : [],
      }),
    },
    service_quota_capacity_template_update: { rpcMethod: "quota/capacityTemplate/update" },
    service_quota_account_capacity_override_update: { rpcMethod: "quota/accountCapacityOverride/update" },
    service_quota_refresh_sources: { rpcMethod: "quota/refreshSources", mapParams: (params) => ({ kinds: Array.isArray(params?.kinds) ? params.kinds : [], sourceIds: Array.isArray(params?.sourceIds) ? params.sourceIds : Array.isArray(params?.source_ids) ? params.source_ids : [] }) },
  };
}
