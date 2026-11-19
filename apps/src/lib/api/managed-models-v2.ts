import type {
  ManagedModelBatchStateV2Update,
  ManagedModelImportPreviewV2Result,
  ManagedModelImportV2Params,
  ManagedModelListV2Result,
  ManagedModelPriceSyncV2Result,
  ManagedModelStateV2Update,
  ManagedModelV2,
  ManagedModelV2Upsert,
} from "@/types/model-v2";
import type { ModelInfo } from "@/types/model";

import { invoke, withAddr } from "./transport";
export {
  microusdToUsdPerMillion,
  usdPerMillionToMicrousd,
} from "./model-price-v2";

export const managedModelsV2Client = {
  list(
    includeHidden = false,
    addr?: string | null,
  ): Promise<ManagedModelListV2Result> {
    return invoke<ManagedModelListV2Result>(
      "service_managed_model_list_v2",
      withAddr({
        includeHidden,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  get(slug: string, addr?: string | null): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_get_v2",
      withAddr({
        slug,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  upsert(
    input: ManagedModelV2Upsert,
    addr?: string | null,
  ): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_upsert_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  reorder(
    input: { slugs: string[]; expectedSlugs: string[] },
    addr?: string | null,
  ): Promise<ManagedModelV2[]> {
    return invoke<ManagedModelV2[]>(
      "service_managed_model_reorder_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  updateState(
    input: ManagedModelStateV2Update,
    addr?: string | null,
  ): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_update_state_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  updateStates(
    input: ManagedModelBatchStateV2Update,
    addr?: string | null,
  ): Promise<ManagedModelV2[]> {
    return invoke<ManagedModelV2[]>(
      "service_managed_model_batch_update_state_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  delete(slug: string, addr?: string | null): Promise<void> {
    return invoke<void>(
      "service_managed_model_delete_v2",
      withAddr({
        slug,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  previewImport(
    input: ManagedModelImportV2Params,
    addr?: string | null,
  ): Promise<ManagedModelImportPreviewV2Result> {
    return invoke<ManagedModelImportPreviewV2Result>(
      "service_managed_model_import_preview_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  commitImport(
    input: ManagedModelImportV2Params,
    addr?: string | null,
  ): Promise<ManagedModelImportPreviewV2Result> {
    return invoke<ManagedModelImportPreviewV2Result>(
      "service_managed_model_import_commit_v2",
      withAddr({
        payload: input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  syncPrices(
    modelSlugs: string[] = [],
    addr?: string | null,
  ): Promise<ManagedModelPriceSyncV2Result> {
    return invoke<ManagedModelPriceSyncV2Result>(
      "service_managed_model_price_sync_v2",
      withAddr({
        payload: {
          modelSlugs,
        },
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },
};

function capability(model: ManagedModelV2, ...keys: string[]): unknown {
  for (const key of keys) {
    if (key in model.capabilities) {
      return model.capabilities[key];
    }
  }
  return undefined;
}
function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => (typeof item === "string" ? item.trim() : ""))
    .filter(Boolean);
}

function serviceTierName(id: string): string {
  switch (id.toLowerCase()) {
    case "priority":
      return "Fast";
    case "ultrafast":
      return "Ultrafast";
    case "flex":
      return "Flex";
    default:
      return id;
  }
}

function serviceTierDescription(modelSlug: string, id: string): string {
  if (id.toLowerCase() === "priority") {
    const normalizedSlug = modelSlug.toLowerCase();
    if (normalizedSlug === "gpt-6-astra") {
      return "2x speed, increased usage";
    }
    if (
      new Set([
        "gpt-6-sol",
        "gpt-6-luna",
        "gpt-5.4",
        "gpt-5.5",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
      ]).has(normalizedSlug)
    ) {
      return "1.5x speed, increased usage";
    }
    return "";
  }
  if (id.toLowerCase() === "ultrafast") {
    return "The fastest available responses for latency-sensitive work.";
  }
  return "";
}

function booleanCapability(
  model: ManagedModelV2,
  fallback: boolean,
  ...keys: string[]
): boolean {
  const value = capability(model, ...keys);
  return typeof value === "boolean" ? value : fallback;
}

function nullableString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function integerCapability(
  model: ManagedModelV2,
  fallback: number | null,
  ...keys: string[]
): number | null {
  const value = capability(model, ...keys);
  return typeof value === "number" && Number.isSafeInteger(value) ? value : fallback;
}

function truncationPolicy(model: ManagedModelV2): ModelInfo["truncationPolicy"] {
  const mode = nullableString(capability(model, "truncation_mode", "truncationMode"));
  const limit = integerCapability(model, null, "truncation_limit", "truncationLimit");
  return mode && limit !== null ? { mode, limit } : null;
}

export function managedModelV2ToModelInfo(model: ManagedModelV2): ModelInfo {
  const resolved = model.resolvedCapabilities;
  const reasoningEfforts = resolved?.reasoningEfforts ?? stringList(
    capability(model, "reasoning_efforts", "reasoningEfforts"),
  );
  const serviceTiers = stringList(
    capability(model, "service_tiers", "serviceTiers"),
  );
  return {
    slug: model.slug,
    displayName: model.displayName,
    description: model.description,
    defaultReasoningLevel: resolved ? resolved.defaultReasoningEffort : model.defaultReasoningEffort,
    supportedReasoningLevels: reasoningEfforts.map((effort) => ({
      effort,
      description: "",
    })),
    shellType:
      nullableString(capability(model, "shell_type", "shellType")) ||
      "shell_command",
    visibility: model.visibility,
    supportedInApi: model.supportedInApi,
    priority: model.sortOrder,
    additionalSpeedTiers: stringList(
      capability(model, "additional_speed_tiers", "additionalSpeedTiers"),
    ),
    serviceTiers: serviceTiers.map((id) => ({
      id,
      name: serviceTierName(id),
      description: serviceTierDescription(model.slug, id),
    })),
    defaultServiceTier: nullableString(
      capability(model, "default_service_tier", "defaultServiceTier"),
    ),
    availabilityNux: null,
    upgrade: null,
    upgradeInfo: null,
    baseInstructions: null,
    modelMessages: null,
    supportsReasoningSummaries: booleanCapability(
      model,
      false,
      "supports_reasoning_summary_parameter",
      "supports_reasoning_summaries",
      "supportsReasoningSummaries",
    ),
    defaultReasoningSummary: nullableString(
      capability(model, "default_reasoning_summary", "defaultReasoningSummary"),
    ),
    supportVerbosity: booleanCapability(
      model,
      false,
      "supports_verbosity",
      "supportsVerbosity",
    ),
    defaultVerbosity:
      capability(model, "default_verbosity", "defaultVerbosity") ?? null,
    applyPatchToolType: nullableString(
      capability(model, "apply_patch_tool_type", "applyPatchToolType"),
    ),
    webSearchToolType: nullableString(
      capability(model, "web_search_tool_type", "webSearchToolType"),
    ),
    truncationPolicy: truncationPolicy(model),
    supportsParallelToolCalls: booleanCapability(
      model,
      false,
      "supports_parallel_tool_calls",
      "supportsParallelToolCalls",
    ),
    supportsImageDetailOriginal: booleanCapability(
      model,
      false,
      "supports_image_detail_original",
      "supportsImageDetailOriginal",
    ),
    contextWindow: resolved ? resolved.contextWindow : model.contextWindow,
    autoCompactTokenLimit: integerCapability(
      model,
      null,
      "auto_compact_token_limit",
      "autoCompactTokenLimit",
    ),
    effectiveContextWindowPercent: integerCapability(
      model,
      95,
      "effective_context_window_percent",
      "effectiveContextWindowPercent",
    ),
    experimentalSupportedTools: stringList(
      capability(
        model,
        "experimental_supported_tools",
        "experimentalSupportedTools",
      ),
    ),
    inputModalities: resolved?.inputModalities ?? stringList(
      capability(model, "input_modalities", "inputModalities"),
    ),
    outputModalities: stringList(
      capability(model, "output_modalities", "outputModalities"),
    ),
    supportedEndpoints: stringList(
      capability(model, "supported_endpoints", "supportedEndpoints"),
    ),
    supportsTextGeneration: booleanCapability(
      model,
      true,
      "supports_text_generation",
      "supportsTextGeneration",
    ),
    minimalClientVersion:
      capability(model, "minimal_client_version", "minimalClientVersion") ?? null,
    supportsSearchTool: booleanCapability(
      model,
      false,
      "supports_search_tool",
      "supportsSearchTool",
    ),
    availableInPlans: [],
    maxContextWindow: resolved ? resolved.maxContextWindow : model.maxContextWindow,
    compHash: nullableString(capability(model, "comp_hash", "compHash")),
    useResponsesLite: booleanCapability(
      model,
      false,
      "use_responses_lite",
      "useResponsesLite",
    ),
    toolMode: nullableString(capability(model, "tool_mode", "toolMode")),
    multiAgentVersion: nullableString(
      capability(model, "multi_agent_version", "multiAgentVersion"),
    ),
    includeSkillsUsageInstructions: false,
  };
}
