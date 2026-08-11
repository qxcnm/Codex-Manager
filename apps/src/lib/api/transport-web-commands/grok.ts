import type { WebCommandDescriptor } from "./shared";

export function createGrokWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_grok_credentials_list: { rpcMethod: "grok/credentials/list" },
    service_grok_credential_probe_models: { rpcMethod: "grok/credentials/probeModels" },
    service_grok_credential_set_enabled: { rpcMethod: "grok/credentials/setEnabled" },
    service_grok_credential_delete: { rpcMethod: "grok/credentials/delete" },
    service_grok_import_preview: { rpcMethod: "grok/import/preview" },
    service_grok_import_commit: { rpcMethod: "grok/import/commit" },
  };
}
