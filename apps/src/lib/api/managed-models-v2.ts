import type {
  ManagedModelBatchStateV2Update,
  ManagedModelImportPreviewV2Result,
  ManagedModelImportV2Params,
  ManagedModelListV2Result,
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
  list(includeHidden = false): Promise<ManagedModelListV2Result> {
    return invoke<ManagedModelListV2Result>(
      "service_managed_model_list_v2",
      withAddr({ includeHidden }),
    );
  },

  get(slug: string): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_get_v2",
      withAddr({ slug }),
    );
  },

  upsert(input: ManagedModelV2Upsert): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_upsert_v2",
      withAddr({ payload: input }),
    );
  },

  updateState(input: ManagedModelStateV2Update): Promise<ManagedModelV2> {
    return invoke<ManagedModelV2>(
      "service_managed_model_update_state_v2",
      withAddr({ payload: input }),
    );
  },

  updateStates(
    input: ManagedModelBatchStateV2Update,
  ): Promise<ManagedModelV2[]> {
    return invoke<ManagedModelV2[]>(
      "service_managed_model_batch_update_state_v2",
      withAddr({ payload: input }),
    );
  },

  delete(slug: string): Promise<void> {
    return invoke<void>(
      "service_managed_model_delete_v2",
      withAddr({ slug }),
    );
  },

  previewImport(
    input: ManagedModelImportV2Params,
  ): Promise<ManagedModelImportPreviewV2Result> {
    return invoke<ManagedModelImportPreviewV2Result>(
      "service_managed_model_import_preview_v2",
      withAddr({ payload: input }),
    );
  },

  commitImport(
    input: ManagedModelImportV2Params,
  ): Promise<ManagedModelImportPreviewV2Result> {
    return invoke<ManagedModelImportPreviewV2Result>(
      "service_managed_model_import_commit_v2",
      withAddr({ payload: input }),
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
  const mode = nullableString(capability(model, "truncationMode", "truncation_mode"));
  const limit = integerCapability(model, null, "truncationLimit", "truncation_limit");
  return mode && limit !== null ? { mode, limit } : null;
}

export function managedModelV2ToModelInfo(model: ManagedModelV2): ModelInfo {
  const reasoningEfforts = stringList(
    capability(model, "reasoningEfforts", "reasoning_efforts"),
  );
  const serviceTiers = stringList(
    capability(model, "serviceTiers", "service_tiers"),
  );
  return {
    slug: model.slug,
    displayName: model.displayName,
    description: model.description,
    defaultReasoningLevel: model.defaultReasoningEffort,
    supportedReasoningLevels: reasoningEfforts.map((effort) => ({
      effort,
      description: "",
    })),
    shellType:
      nullableString(capability(model, "shellType", "shell_type")) ||
      "shell_command",
    visibility: model.visibility,
    supportedInApi: model.supportedInApi,
    priority: model.sortOrder,
    additionalSpeedTiers: stringList(
      capability(model, "additionalSpeedTiers", "additional_speed_tiers"),
    ),
    serviceTiers: serviceTiers.map((id) => ({ id, name: id, description: "" })),
    defaultServiceTier: nullableString(
      capability(model, "defaultServiceTier", "default_service_tier"),
    ),
    availabilityNux: null,
    upgrade: null,
    upgradeInfo: null,
    baseInstructions: null,
    modelMessages: null,
    supportsReasoningSummaries: booleanCapability(
      model,
      false,
      "supportsReasoningSummaries",
      "supports_reasoning_summaries",
    ),
    defaultReasoningSummary: nullableString(
      capability(model, "defaultReasoningSummary", "default_reasoning_summary"),
    ),
    supportVerbosity: booleanCapability(
      model,
      false,
      "supportsVerbosity",
      "supports_verbosity",
    ),
    defaultVerbosity:
      capability(model, "defaultVerbosity", "default_verbosity") ?? null,
    applyPatchToolType: nullableString(
      capability(model, "applyPatchToolType", "apply_patch_tool_type"),
    ),
    webSearchToolType: nullableString(
      capability(model, "webSearchToolType", "web_search_tool_type"),
    ),
    truncationPolicy: truncationPolicy(model),
    supportsParallelToolCalls: booleanCapability(
      model,
      false,
      "supportsParallelToolCalls",
      "supports_parallel_tool_calls",
    ),
    supportsImageDetailOriginal: booleanCapability(
      model,
      false,
      "supportsImageDetailOriginal",
      "supports_image_detail_original",
    ),
    contextWindow: model.contextWindow,
    autoCompactTokenLimit: integerCapability(
      model,
      null,
      "autoCompactTokenLimit",
      "auto_compact_token_limit",
    ),
    effectiveContextWindowPercent: integerCapability(
      model,
      95,
      "effectiveContextWindowPercent",
      "effective_context_window_percent",
    ),
    experimentalSupportedTools: stringList(
      capability(
        model,
        "experimentalSupportedTools",
        "experimental_supported_tools",
      ),
    ),
    inputModalities: stringList(
      capability(model, "inputModalities", "input_modalities"),
    ),
    outputModalities: stringList(
      capability(model, "outputModalities", "output_modalities"),
    ),
    supportedEndpoints: stringList(
      capability(model, "supportedEndpoints", "supported_endpoints"),
    ),
    supportsTextGeneration: booleanCapability(
      model,
      true,
      "supportsTextGeneration",
      "supports_text_generation",
    ),
    minimalClientVersion:
      capability(model, "minimalClientVersion", "minimal_client_version") ?? null,
    supportsSearchTool: booleanCapability(
      model,
      false,
      "supportsSearchTool",
      "supports_search_tool",
    ),
    availableInPlans: [],
    maxContextWindow: model.maxContextWindow,
    compHash: nullableString(capability(model, "compHash", "comp_hash")),
    useResponsesLite: booleanCapability(
      model,
      false,
      "useResponsesLite",
      "use_responses_lite",
    ),
    toolMode: nullableString(capability(model, "toolMode", "tool_mode")),
    multiAgentVersion: nullableString(
      capability(model, "multiAgentVersion", "multi_agent_version"),
    ),
    includeSkillsUsageInstructions: false,
  };
}
