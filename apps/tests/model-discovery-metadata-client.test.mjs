import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import vm from "node:vm";
import ts from "../node_modules/typescript/lib/typescript.js";

const apiRoot = path.resolve(import.meta.dirname, "../src/lib/api");
const calls = [];
let outcome;

async function loadModule(name, dependencies = {}) {
  const sourcePath = path.join(apiRoot, name);
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const loaded = { exports: {} };
  vm.runInNewContext(compiled.outputText, {
    exports: loaded.exports,
    module: loaded,
    require: (name) => dependencies[name] ?? {},
  });
  return loaded.exports;
}

const normalize = await loadModule("normalize.ts", {
  "@/lib/utils/usage": {
    toNullableNumber(value) {
      if (typeof value === "number" && Number.isFinite(value)) return value;
      if (typeof value !== "string" || value.trim() === "") return null;
      const parsed = Number(value);
      return Number.isFinite(parsed) ? parsed : null;
    },
  },
});
const { accountClient } = await loadModule("account-client.ts", {
  "./normalize": normalize,
  "./transport": {
    withAddr: (params) => ({ addr: "default-service", ...params }),
    invoke: async (command, params) => {
      calls.push({ command, params });
      return outcome;
    },
  },
});
const plain = (value) => JSON.parse(JSON.stringify(value));

test.beforeEach(() => { calls.length = 0; outcome = {}; });

for (const [source, fetchMethod, associateMethod, command] of [
  ["account", "fetchAccountModels", "associateAccountModels", "service_account_associate_models"],
  ["aggregate", "fetchAggregateApiModels", "associateAggregateApiModels", "service_aggregate_api_associate_models"],
]) {
  test(`${source} discovery preserves model capabilities from fetch to association`, async () => {
    const metadata = {
      context_window: 272000,
      max_context_window: 872000,
      input_modalities: ["text", "image"],
      supports_image_detail_original: false,
      supports_parallel_tool_calls: true,
      supported_reasoning_levels: [{ effort: "high", description: "High" }],
    };
    outcome = { items: [{ upstream_model: "future-model", display_name: "Future Model", metadata }] };
    const fetched = await accountClient[fetchMethod]("source-1", "service-1");
    assert.deepEqual(plain(fetched.items[0].metadata), metadata);
    assert.equal(calls[0].params.addr, "service-1");
    outcome = { createdModels: ["future-model"], addedRoutes: ["future-model"] };
    const result = await accountClient[associateMethod](
      "source-1", ["future-model"], { "future-model": "Future Model" }, "service-1",
      { "future-model": fetched.items[0].metadata },
    );
    assert.equal(calls[1].command, command);
    assert.equal(calls[1].params.addr, "service-1");
    assert.deepEqual(plain(calls[1].params.modelMetadata), { "future-model": metadata });
    assert.deepEqual(plain(result.createdModels), ["future-model"]);
  });

  test(`${source} legacy discovery and association calls remain valid without metadata`, async () => {
    outcome = { items: [{ upstreamModel: "legacy-model", displayName: "Legacy Model" }] };
    const fetched = await accountClient[fetchMethod]("source-1");
    assert.equal(fetched.items[0].metadata, null);
    assert.equal(calls[0].params.addr, "default-service");
    outcome = {};
    await accountClient[associateMethod]("source-1", ["legacy-model"], undefined, null);
    assert.equal(calls[1].params.addr, null);
    assert.deepEqual(plain(calls[1].params.modelMetadata), {});
  });
}

test("fetched metadata ignores non-object payloads for both model sources", () => {
  for (const normalizeResult of [normalize.normalizeAccountFetchModelsResult, normalize.normalizeAggregateApiFetchModelsResult]) {
    for (const metadata of [null, [], "invalid", 123, {}]) {
      const result = normalizeResult({ items: [{ upstreamModel: "test", metadata }] });
      assert.equal(result.items[0].metadata, null);
    }
  }
});
