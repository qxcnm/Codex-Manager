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
      service_tiers: ["priority"],
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
