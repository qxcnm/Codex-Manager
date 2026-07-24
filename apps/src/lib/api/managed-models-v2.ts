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
      "supports_reasoning_summary_parameter",
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

export function serializeManagedModelV2ForCodexCache(
  model: ManagedModelV2,
): Record<string, unknown> {
  const reasoningEfforts = stringList(
    capability(model, "reasoningEfforts", "reasoning_efforts"),
  );
  const serviceTiers = stringList(
    capability(model, "serviceTiers", "service_tiers"),
  );
  const inputModalities = stringList(
    capability(model, "inputModalities", "input_modalities"),
  );
  const outputModalities = stringList(
    capability(model, "outputModalities", "output_modalities"),
  );
  const supportedEndpoints = stringList(
    capability(model, "supportedEndpoints", "supported_endpoints"),
  );
  const additionalSpeedTiers = stringList(
    capability(model, "additionalSpeedTiers", "additional_speed_tiers"),
  );
  const experimentalSupportedTools = stringList(
    capability(
      model,
      "experimentalSupportedTools",
      "experimental_supported_tools",
    ),
  );
  const truncationMode = capability(
    model,
    "truncationMode",
    "truncation_mode",
  );
  const truncationLimit = capability(
    model,
    "truncationLimit",
    "truncation_limit",
  );

  return {
    slug: model.slug,
    display_name: model.displayName || model.slug,
    description: model.description,
    default_reasoning_level: model.defaultReasoningEffort,
    supported_reasoning_levels: reasoningEfforts.map((effort) => ({
      effort,
      description: "",
    })),
    shell_type:
      nullableString(capability(model, "shellType", "shell_type")) ||
      "shell_command",
    visibility: model.visibility,
    supported_in_api: model.supportedInApi,
    priority: model.sortOrder,
    additional_speed_tiers: additionalSpeedTiers,
    service_tiers: serviceTiers.map((id) => ({ id, name: id, description: "" })),
    default_service_tier: nullableString(
      capability(model, "defaultServiceTier", "default_service_tier"),
    ),
    base_instructions: "",
    include_skills_usage_instructions: false,
    supports_reasoning_summary_parameter: booleanCapability(
      model,
      false,
      "supports_reasoning_summary_parameter",
      "supportsReasoningSummaries",
      "supports_reasoning_summaries",
    ),
    default_reasoning_summary:
      capability(model, "defaultReasoningSummary", "default_reasoning_summary") ??
      "auto",
    support_verbosity: booleanCapability(
      model,
      false,
      "supportsVerbosity",
      "supports_verbosity",
    ),
    default_verbosity:
      capability(model, "defaultVerbosity", "default_verbosity") ?? null,
    apply_patch_tool_type: nullableString(
      capability(model, "applyPatchToolType", "apply_patch_tool_type"),
    ),
    web_search_tool_type:
      capability(model, "webSearchToolType", "web_search_tool_type") ?? "text",
    truncation_policy: {
      mode: typeof truncationMode === "string" ? truncationMode : "tokens",
      limit:
        typeof truncationLimit === "number" && Number.isSafeInteger(truncationLimit)
          ? truncationLimit
          : 10000,
    },
    supports_parallel_tool_calls: booleanCapability(
      model,
      false,
      "supportsParallelToolCalls",
      "supports_parallel_tool_calls",
    ),
    supports_image_detail_original: booleanCapability(
      model,
      false,
      "supportsImageDetailOriginal",
      "supports_image_detail_original",
    ),
    context_window: model.contextWindow,
    max_context_window: model.maxContextWindow,
    auto_compact_token_limit: integerCapability(
      model,
      null,
      "autoCompactTokenLimit",
      "auto_compact_token_limit",
    ),
    comp_hash: nullableString(capability(model, "compHash", "comp_hash")),
    effective_context_window_percent: integerCapability(
      model,
      95,
      "effectiveContextWindowPercent",
      "effective_context_window_percent",
    ),
    experimental_supported_tools: experimentalSupportedTools,
    input_modalities: inputModalities.length > 0 ? inputModalities : ["text", "image"],
    output_modalities: outputModalities,
    supported_endpoints: supportedEndpoints,
    supports_text_generation: booleanCapability(
      model,
      true,
      "supportsTextGeneration",
      "supports_text_generation",
    ),
    supports_search_tool: booleanCapability(
      model,
      false,
      "supportsSearchTool",
      "supports_search_tool",
    ),
    use_responses_lite: booleanCapability(
      model,
      false,
      "useResponsesLite",
      "use_responses_lite",
    ),
    auto_review_model_override: nullableString(
      capability(model, "autoReviewModelOverride", "auto_review_model_override"),
    ),
    tool_mode: nullableString(capability(model, "toolMode", "tool_mode")),
    multi_agent_version: nullableString(
      capability(model, "multiAgentVersion", "multi_agent_version"),
    ),
    prefer_websockets: booleanCapability(
      model,
      false,
      "preferWebsockets",
      "prefer_websockets",
    ),
    minimal_client_version:
      capability(model, "minimalClientVersion", "minimal_client_version") ?? null,
    reasoning_summary_format:
      capability(model, "reasoningSummaryFormat", "reasoning_summary_format") ?? null,
  };
}

export function serializeManagedModelsV2ForCodexCache(
  models: readonly ManagedModelV2[],
): Array<Record<string, unknown>> {
  return [...models]
    .filter(
      (model) =>
        model.enabled &&
        model.supportedInApi &&
        model.visibility === "list" &&
        booleanCapability(
          model,
          true,
          "supportsTextGeneration",
          "supports_text_generation",
        ),
    )
    .sort(
      (left, right) =>
        left.sortOrder - right.sortOrder || left.slug.localeCompare(right.slug),
    )
    .map(serializeManagedModelV2ForCodexCache);
}

export function buildCodexModelsCachePayloadV2(
  models: readonly ManagedModelV2[],
  userAgent: string,
  options?: { etag?: string | null; fetchedAt?: string },
): Record<string, unknown> {
  const clientVersion = String(userAgent || "")
    .match(/codex_cli_rs\/([^\s]+)/)?.[1]
    ?.trim();
  if (!clientVersion) {
    throw new Error("无法从 userAgent 解析 Codex CLI 版本");
  }
  return {
    fetched_at: options?.fetchedAt || new Date().toISOString(),
    etag: options?.etag ?? null,
    client_version: clientVersion,
    models: serializeManagedModelsV2ForCodexCache(models),
  };
}
