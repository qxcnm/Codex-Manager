import type { WebCommandDescriptor } from "./shared";

export function createCodexProfileWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_codex_profile_get: { rpcMethod: "codexProfile/get" },
    service_codex_profile_set_config: { rpcMethod: "codexProfile/setConfig" },
    service_codex_profile_list_candidates: { rpcMethod: "codexProfile/listCandidates" },
    service_codex_profile_apply_direct_account: { rpcMethod: "codexProfile/applyDirectAccount" },
    service_codex_profile_apply_gateway: { rpcMethod: "codexProfile/applyGateway" },
    service_codex_profile_restore: { rpcMethod: "codexProfile/restore" },
    service_codex_profile_repair_history: { rpcMethod: "codexProfile/repairHistory" },
    service_codex_profile_prune_history_backups: { rpcMethod: "codexProfile/pruneHistoryBackups" },
  };
}