import type { WebCommandDescriptor } from "./shared";
import { asRecord, mapKeyIdToId, type InvokeParams } from "./shared";

const API_KEY_UPDATE_PRESENCE_MARKERS = [
  "hasName",
  "hasModelConfig",
  "hasRoutingConfig",
  "hasAccountGroupFilter",
  "hasQuotaLimitTokens",
] as const;

export function mapApiKeyUpdateParams(params?: InvokeParams): InvokeParams {
  const mapped = { ...mapKeyIdToId(params) };
  for (const marker of API_KEY_UPDATE_PRESENCE_MARKERS) {
    delete mapped[marker];
  }
  return mapped;
}

export function createApiKeyWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_apikey_list: { rpcMethod: "apikey/list" },
    service_apikey_create: { rpcMethod: "apikey/create" },
    service_apikey_usage_stats: { rpcMethod: "apikey/usageStats" },
    service_apikey_delete: { rpcMethod: "apikey/delete", mapParams: mapKeyIdToId },
    service_apikey_update_model: {
      rpcMethod: "apikey/updateModel",
      mapParams: mapApiKeyUpdateParams,
    },
    service_apikey_disable: { rpcMethod: "apikey/disable", mapParams: mapKeyIdToId },
    service_apikey_enable: { rpcMethod: "apikey/enable", mapParams: mapKeyIdToId },
    service_managed_model_list_v2: { rpcMethod: "apikey/managedModelListV2" },
    service_managed_model_get_v2: { rpcMethod: "apikey/managedModelGetV2" },
    service_managed_model_upsert_v2: { rpcMethod: "apikey/managedModelUpsertV2", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_managed_model_update_state_v2: { rpcMethod: "apikey/managedModelUpdateStateV2", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_managed_model_batch_update_state_v2: { rpcMethod: "apikey/managedModelBatchUpdateStateV2", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_managed_model_delete_v2: { rpcMethod: "apikey/managedModelDeleteV2" },
    service_managed_model_import_preview_v2: { rpcMethod: "apikey/managedModelImportPreviewV2", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_managed_model_import_commit_v2: { rpcMethod: "apikey/managedModelImportCommitV2", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_managed_model_price_sync_v2: {
      rpcMethod: "apikey/managedModelPriceSyncV2",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_apikey_read_secret: { rpcMethod: "apikey/readSecret", mapParams: mapKeyIdToId },
  };
}
