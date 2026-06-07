import type { WebCommandDescriptor } from "./shared";
import { asRecord, mapKeyIdToId } from "./shared";

export function createApiKeyWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_apikey_list: { rpcMethod: "apikey/list" },
    service_apikey_create: { rpcMethod: "apikey/create" },
    service_apikey_usage_stats: { rpcMethod: "apikey/usageStats" },
    service_apikey_delete: { rpcMethod: "apikey/delete", mapParams: mapKeyIdToId },
    service_apikey_update_model: { rpcMethod: "apikey/updateModel", mapParams: mapKeyIdToId },
    service_apikey_disable: { rpcMethod: "apikey/disable", mapParams: mapKeyIdToId },
    service_apikey_enable: { rpcMethod: "apikey/enable", mapParams: mapKeyIdToId },
    service_apikey_models: { rpcMethod: "apikey/models" },
    service_model_catalog_list: { rpcMethod: "apikey/modelCatalogList" },
    service_model_catalog_save: { rpcMethod: "apikey/modelCatalogSave", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_model_catalog_delete: { rpcMethod: "apikey/modelCatalogDelete" },
    service_model_catalog_prune_stale_remote: { rpcMethod: "apikey/modelCatalogPruneStaleRemote" },
    service_model_routing: { rpcMethod: "apikey/modelRouting" },
    service_model_source_sync: { rpcMethod: "apikey/modelSourceSync", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_model_source_model_save: { rpcMethod: "apikey/modelSourceModelSave", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_model_source_mapping_save: { rpcMethod: "apikey/modelSourceMappingSave", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_model_source_mapping_delete: { rpcMethod: "apikey/modelSourceMappingDelete", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_model_price_rules_list: { rpcMethod: "quota/modelPriceRules/list" },
    service_model_price_rule_read: { rpcMethod: "quota/modelPriceRule/read" },
    service_model_price_rule_upsert: { rpcMethod: "quota/modelPriceRule/upsert", mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {} },
    service_apikey_read_secret: { rpcMethod: "apikey/readSecret", mapParams: mapKeyIdToId },
  };
}
