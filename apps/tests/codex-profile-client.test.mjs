import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "codex-profile-client.ts",
);

async function loadClientModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-codex-profile-client-"),
  );
  const tempFile = path.join(tempDir, "codex-profile-client.mjs");
  await fs.writeFile(
    path.join(tempDir, "transport.mjs"),
    "export async function invoke(command, params) { globalThis.__codexProfileInvokeCalls ??= []; globalThis.__codexProfileInvokeCalls.push({ command, params }); return globalThis.__codexProfileInvokeResult ?? {}; }\nexport function withAddr(value = {}) { return value; }\n",
    "utf8",
  );
  await fs.writeFile(
    tempFile,
    compiled.outputText.replace("./transport", "./transport.mjs"),
    "utf8",
  );
  return import(pathToFileURL(tempFile).href);
}

const client = await loadClientModule();

test("normalizeCodexProfileStatus reads the managed catalog state", () => {
  assert.equal(
    client.normalizeCodexProfileStatus({
      mode: "gateway",
      profile_writable: true,
      managed_catalog_active: true,
    }).managedCatalogActive,
    true,
  );
  assert.equal(
    client.normalizeCodexProfileStatus({
      mode: "gateway",
      profileWritable: true,
      managedCatalogActive: false,
    }).managedCatalogActive,
    false,
  );
});

test("normalizeCodexProfileCandidates reads direct aggregate candidates", () => {
  const candidates = client.normalizeCodexProfileCandidates({
    aggregate_apis: [
      {
        id: "agg-1",
        label: "Primary aggregate",
        supplier_name: "Primary aggregate",
        provider_type: "codex",
        base_url: "https://aggregate.example/v1",
        sort: -1,
        model_override: "gpt-5.6-sol",
        user_agent: "Aggregate/1.0",
      },
    ],
  });

  assert.deepEqual(candidates.aggregateApis, [
    {
      id: "agg-1",
      label: "Primary aggregate",
      supplierName: "Primary aggregate",
      providerType: "codex",
      baseUrl: "https://aggregate.example/v1",
      sort: -1,
      modelOverride: "gpt-5.6-sol",
      userAgent: "Aggregate/1.0",
    },
  ]);
});

test("applyDirectAggregate writes the selected aggregate profile", async () => {
  globalThis.__codexProfileInvokeCalls = [];
  globalThis.__codexProfileInvokeResult = {
    codex_home: "/srv/codex",
    mode: "direct_aggregate",
    selected_aggregate_api_id: "agg-1",
    aggregate_api_base_url: "https://aggregate.example/v1",
  };

  const status = await client.codexProfileClient.applyDirectAggregate({
    aggregateApiId: "agg-1",
    codexHome: "/srv/codex",
    reloadAfterSwitch: true,
  });

  assert.deepEqual(globalThis.__codexProfileInvokeCalls, [
    {
      command: "service_codex_profile_apply_direct_aggregate",
      params: {
        aggregateApiId: "agg-1",
        codexHome: "/srv/codex",
        reloadAfterSwitch: true,
      },
    },
  ]);
  assert.equal(status.mode, "direct_aggregate");
  assert.equal(status.selectedAggregateApiId, "agg-1");
  assert.equal(status.aggregateApiBaseUrl, "https://aggregate.example/v1");
});

test("applyModels uses the standalone command without gateway credentials", async () => {
  globalThis.__codexProfileInvokeCalls = [];
  globalThis.__codexProfileInvokeResult = {
    codex_home: "/srv/codex",
    mode: "gateway",
    profile_writable: true,
    managed_catalog_active: true,
  };

  const status = await client.codexProfileClient.applyModels({
    codexHome: "/srv/codex",
    modelSlugs: ["gpt-5.6-sol", "gpt-image-2"],
  });

  assert.deepEqual(globalThis.__codexProfileInvokeCalls, [
    {
      command: "service_codex_profile_apply_models",
      params: {
        codexHome: "/srv/codex",
        modelSlugs: ["gpt-5.6-sol", "gpt-image-2"],
        reloadAfterSwitch: false,
      },
    },
  ]);
  assert.equal(status.managedCatalogActive, true);
  assert.equal(status.profileWritable, true);
});
