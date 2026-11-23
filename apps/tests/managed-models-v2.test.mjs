import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

const sourcePath = path.resolve(
  import.meta.dirname,
  "../src/lib/api/managed-models-v2.ts",
);
const source = await fs.readFile(sourcePath, "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2022,
    target: ts.ScriptTarget.ES2022,
  },
  fileName: sourcePath,
});
const runnable = compiled.outputText
  .replace(
    'import { invoke, withAddr } from "./transport";',
    "const invoke = () => { throw new Error('not used'); }; const withAddr = (value) => value;",
  )
  .replace(
    'export { microusdToUsdPerMillion, usdPerMillionToMicrousd, } from "./model-price-v2";',
    "",
  );
const moduleUrl = `data:text/javascript;base64,${Buffer.from(runnable).toString("base64")}`;
const { managedModelV2ToModelInfo } = await import(moduleUrl);

function model(overrides = {}) {
  return {
    id: "builtin:gpt-5.6-sol",
    slug: "gpt-5.6-sol",
    displayName: "GPT-5.6-Sol",
    description: "Latest frontier agentic coding model.",
    provider: null,
    family: null,
    category: null,
    tags: [],
    origin: "builtin",
    enabled: true,
    supportedInApi: true,
    visibility: "list",
    sortOrder: 1,
    contextWindow: 372000,
    maxContextWindow: 372000,
    defaultReasoningEffort: "low",
    capabilities: {
      reasoning_efforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
      service_tiers: ["priority", "ultrafast"],
      additional_speed_tiers: ["fast"],
      input_modalities: ["text", "image"],
      output_modalities: ["text"],
      supported_endpoints: ["/v1/responses", "/v1/chat/completions"],
      supports_text_generation: true,
      prefer_websockets: true,
      supports_parallel_tool_calls: true,
      supports_reasoning_summaries: true,
      default_reasoning_summary: "none",
      reasoning_summary_format: "experimental",
      supports_verbosity: true,
      default_verbosity: "low",
      supports_image_detail_original: true,
      supports_search_tool: true,
      apply_patch_tool_type: "freeform",
      web_search_tool_type: "text_and_image",
      shell_type: "shell_command",
      truncation_mode: "tokens",
      truncation_limit: 10000,
      experimental_supported_tools: [],
      effective_context_window_percent: 95,
      minimal_client_version: "0.144.0",
      comp_hash: "3000",
      tool_mode: "code_mode_only",
      multi_agent_version: "v2",
      use_responses_lite: true,
      include_skills_usage_instructions: false,
    },
    instructionsMode: "passthrough",
    instructionsText: null,
    builtinRevision: 4,
    userEdited: false,
    price: {
      priceStatus: "official",
      priceSource: "https://developers.openai.com/api/docs/models/compare",
      inputMicrousdPer1m: 5_000_000,
      cachedInputMicrousdPer1m: 500_000,
      outputMicrousdPer1m: 30_000_000,
    },
    priceTiers: [
      {
        minInputTokens: 0,
        inputMicrousdPer1m: 5_000_000,
        cachedInputMicrousdPer1m: 500_000,
        outputMicrousdPer1m: 30_000_000,
      },
      {
        minInputTokens: 272_000,
        inputMicrousdPer1m: 10_000_000,
        cachedInputMicrousdPer1m: 1_000_000,
        outputMicrousdPer1m: 45_000_000,
      },
    ],
    routes: [],
    permissionGroupIds: [],
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

test("managed model adapter carries non-prompt Codex runtime metadata", () => {
  const info = managedModelV2ToModelInfo(model());

  assert.equal(info.maxContextWindow, 372000);
  assert.equal(info.multiAgentVersion, "v2");
  assert.equal(info.toolMode, "code_mode_only");
  assert.equal(info.useResponsesLite, true);
  assert.equal(info.compHash, "3000");
  assert.deepEqual(info.additionalSpeedTiers, ["fast"]);
  assert.deepEqual(info.serviceTiers, [
    {
      id: "priority",
      name: "Fast",
      description: "1.5x speed, increased usage",
    },
    {
      id: "ultrafast",
      name: "Ultrafast",
      description: "The fastest available responses for latency-sensitive work.",
    },
  ]);
  assert.deepEqual(info.outputModalities, ["text"]);
  assert.deepEqual(info.supportedEndpoints, [
    "/v1/responses",
    "/v1/chat/completions",
  ]);
  assert.equal(info.supportsTextGeneration, true);
  assert.deepEqual(info.truncationPolicy, { mode: "tokens", limit: 10000 });
  assert.equal(info.baseInstructions, null);
  assert.equal(info.modelMessages, null);
});

test("managed model adapter labels GPT-6 Sol and Luna Fast tiers", () => {
  for (const slug of ["gpt-6-sol", "gpt-6-luna"]) {
    const info = managedModelV2ToModelInfo(
      model({
        id: `builtin:${slug}`,
        slug,
        displayName: slug.toUpperCase(),
        capabilities: {
          service_tiers: ["priority"],
          output_modalities: ["text"],
          supports_text_generation: true,
        },
      }),
    );

    assert.deepEqual(info.serviceTiers, [
      {
        id: "priority",
        name: "Fast",
        description: "1.5x speed, increased usage",
      },
    ]);
  }
});

test("managed model adapter preserves the Astra 2x Fast tier description", () => {
  const info = managedModelV2ToModelInfo(
    model({
      id: "builtin:gpt-6-astra",
      slug: "gpt-6-astra",
      displayName: "GPT-6-Astra",
      capabilities: {
        service_tiers: ["priority"],
        output_modalities: ["text"],
        supports_text_generation: true,
      },
    }),
  );

  assert.deepEqual(info.serviceTiers, [
    {
      id: "priority",
      name: "Fast",
      description: "2x speed, increased usage",
    },
  ]);
});

test("managed model adapter accepts camelCase capability names and legacy defaults", () => {
  const imageInfo = managedModelV2ToModelInfo(
    model({
      capabilities: {
        outputModalities: ["image"],
        supportedEndpoints: ["/v1/images/generations", "/v1/images/edits"],
        supportsTextGeneration: false,
      },
    }),
  );
  const legacyInfo = managedModelV2ToModelInfo(model({ capabilities: {} }));

  assert.deepEqual(imageInfo.outputModalities, ["image"]);
  assert.deepEqual(imageInfo.supportedEndpoints, [
    "/v1/images/generations",
    "/v1/images/edits",
  ]);
  assert.equal(imageInfo.supportsTextGeneration, false);
  assert.equal(legacyInfo.supportsTextGeneration, true);
});

test("managed model adapter preserves GPT Image 2.5 capability metadata", () => {
  const imageInfo = managedModelV2ToModelInfo(
    model({
      id: "builtin:gpt-image-2.5-sunburst",
      slug: "gpt-image-2.5-sunburst",
      displayName: "GPT Image 2.5 Sunburst",
      capabilities: {
        input_modalities: ["text", "image"],
        output_modalities: ["image"],
        supported_endpoints: ["/v1/images/generations", "/v1/images/edits"],
        snapshot: "gpt-image-2.5-sunburst-2026-09-08",
        supports_text_generation: false,
        supports_image_generation: true,
        supports_image_editing: true,
      },
    }),
  );

  assert.deepEqual(imageInfo.inputModalities, ["text", "image"]);
  assert.deepEqual(imageInfo.outputModalities, ["image"]);
  assert.deepEqual(imageInfo.supportedEndpoints, [
    "/v1/images/generations",
    "/v1/images/edits",
  ]);
  assert.equal(imageInfo.supportsTextGeneration, false);
});

test("managed model adapter does not invent Fast speed copy for custom models", () => {
  const info = managedModelV2ToModelInfo(
    model({
      slug: "custom-fast-model",
      origin: "custom",
      capabilities: { service_tiers: ["priority"] },
    }),
  );

  assert.deepEqual(info.serviceTiers, [
    { id: "priority", name: "Fast", description: "" },
  ]);
});

test("managed model adapter uses resolved capabilities without rewriting stored overrides", () => {
  const stored = model({
    contextWindow: null,
    maxContextWindow: null,
    defaultReasoningEffort: null,
    capabilities: { supports_text_generation: true },
    resolvedCapabilities: {
      source: "official_cache", matchedSlug: "gpt-6-sol",
      contextWindow: 272000, maxContextWindow: 872000,
      defaultReasoningEffort: "low", reasoningEfforts: ["low", "medium", "high"],
      inputModalities: ["text", "image"],
      reasoningSource: "official_cache", contextSource: "official_cache", inputSource: "official_cache",
    },
  });
  const before = structuredClone(stored);
  const info = managedModelV2ToModelInfo(stored);
  assert.equal(info.defaultReasoningLevel, "low");
  assert.deepEqual(info.supportedReasoningLevels.map(({ effort }) => effort), ["low", "medium", "high"]);
  assert.equal(info.contextWindow, 272000);
  assert.equal(info.maxContextWindow, 872000);
  assert.deepEqual(info.inputModalities, ["text", "image"]);
  assert.deepEqual(stored, before);
});

test("managed model adapter preserves resolved empty reasoning and text-only inputs", () => {
  const info = managedModelV2ToModelInfo(model({
    defaultReasoningEffort: null,
    capabilities: { reasoning_efforts: [], input_modalities: ["text"] },
    resolvedCapabilities: {
      source: "stored", matchedSlug: "gpt-6-sol",
      contextWindow: 12345, maxContextWindow: 54321,
      defaultReasoningEffort: null, reasoningEfforts: [], inputModalities: ["text"],
      reasoningSource: "stored", contextSource: "stored", inputSource: "stored",
    },
  }));
  assert.equal(info.defaultReasoningLevel, null);
  assert.deepEqual(info.supportedReasoningLevels, []);
  assert.deepEqual(info.inputModalities, ["text"]);
  assert.equal(info.contextWindow, 12345);
});

test("managed model adapter gives snake_case restrictions precedence over aliases", () => {
  const info = managedModelV2ToModelInfo(model({
    capabilities: {
      reasoning_efforts: [], reasoningEfforts: ["high"],
      input_modalities: ["text"], inputModalities: ["text", "image"],
    },
  }));
  assert.deepEqual(info.supportedReasoningLevels, []);
  assert.deepEqual(info.inputModalities, ["text"]);
});

test("managed model adapter resolves mixed scalar and collection aliases with snake_case precedence", () => {
  const info = managedModelV2ToModelInfo(model({
    capabilities: {
      supports_text_generation: false, supportsTextGeneration: true,
      supports_reasoning_summaries: false, supportsReasoningSummaries: true,
      supports_verbosity: false, supportsVerbosity: true,
      supports_parallel_tool_calls: false, supportsParallelToolCalls: true,
      supports_image_detail_original: false, supportsImageDetailOriginal: true,
      supports_search_tool: false, supportsSearchTool: true,
      use_responses_lite: false, useResponsesLite: true,
      auto_compact_token_limit: 240000, autoCompactTokenLimit: 1,
      effective_context_window_percent: 95, effectiveContextWindowPercent: 1,
      truncation_mode: "tokens", truncationMode: "bytes",
      truncation_limit: 10000, truncationLimit: 1,
      shell_type: "disabled", shellType: "unified_exec",
      default_reasoning_summary: "none", defaultReasoningSummary: "auto",
      default_verbosity: "low", defaultVerbosity: "high",
      apply_patch_tool_type: "freeform", applyPatchToolType: "alias",
      web_search_tool_type: "text_and_image", webSearchToolType: "text",
      default_service_tier: "flex", defaultServiceTier: "priority",
      comp_hash: "canonical", compHash: "alias",
      tool_mode: "canonical", toolMode: "alias",
      multi_agent_version: "v2", multiAgentVersion: "v1",
      minimal_client_version: "0.169.0", minimalClientVersion: "0.1.0",
      service_tiers: [], serviceTiers: ["priority"],
      additional_speed_tiers: [], additionalSpeedTiers: ["fast"],
      output_modalities: ["text"], outputModalities: ["image"],
      supported_endpoints: ["/v1/responses"], supportedEndpoints: ["/v1/images/generations"],
      experimental_supported_tools: [], experimentalSupportedTools: ["alias_tool"],
    },
  }));
  assert.equal(info.supportsTextGeneration, false);
  assert.equal(info.supportsReasoningSummaries, false);
  assert.equal(info.supportVerbosity, false);
  assert.equal(info.supportsParallelToolCalls, false);
  assert.equal(info.supportsImageDetailOriginal, false);
  assert.equal(info.supportsSearchTool, false);
  assert.equal(info.useResponsesLite, false);
  assert.equal(info.autoCompactTokenLimit, 240000);
  assert.equal(info.effectiveContextWindowPercent, 95);
  assert.deepEqual(info.truncationPolicy, { mode: "tokens", limit: 10000 });
  assert.equal(info.shellType, "disabled");
  assert.equal(info.defaultReasoningSummary, "none");
  assert.equal(info.defaultVerbosity, "low");
  assert.equal(info.applyPatchToolType, "freeform");
  assert.equal(info.webSearchToolType, "text_and_image");
  assert.equal(info.defaultServiceTier, "flex");
  assert.equal(info.compHash, "canonical");
  assert.equal(info.toolMode, "canonical");
  assert.equal(info.multiAgentVersion, "v2");
  assert.equal(info.minimalClientVersion, "0.169.0");
  assert.deepEqual(info.serviceTiers, []);
  assert.deepEqual(info.additionalSpeedTiers, []);
  assert.deepEqual(info.outputModalities, ["text"]);
  assert.deepEqual(info.supportedEndpoints, ["/v1/responses"]);
  assert.deepEqual(info.experimentalSupportedTools, []);
});
