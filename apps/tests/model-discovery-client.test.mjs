import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const sourcePath = path.resolve(import.meta.dirname, "../src/lib/api/model-discovery-client.ts");
const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-model-discovery-client-"));
test.after(() => fs.rm(scratch, { recursive: true, force: true }));
const source = await fs.readFile(sourcePath, "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
}).outputText.replace('from "./transport"', 'from "./transport.mjs"');
await fs.writeFile(path.join(scratch, "client.mjs"), compiled);
await fs.writeFile(path.join(scratch, "transport.mjs"), `
export const calls = [];
export const outcomes = [];
export function withAddr(params) { return { addr: "default-service", ...params }; }
export async function invoke(...args) {
  calls.push(args);
  const result = outcomes.shift();
  if (result instanceof Error) throw result;
  return result;
}
`);
const { modelDiscoveryClient } = await import(pathToFileURL(path.join(scratch, "client.mjs")).href);
const { calls, outcomes } = await import(pathToFileURL(path.join(scratch, "transport.mjs")).href);
test.beforeEach(() => { calls.length = 0; outcomes.length = 0; });

test("model discovery list preserves saved task results and service addressing", async () => {
  const payload = { items: [{ sourceKind: "openai_account", sourceId: "account-1", enabled: false }] };
  outcomes.push(payload);
  assert.equal(await modelDiscoveryClient.list(), payload);
  assert.deepEqual(calls[0], ["service_model_discovery_list", { addr: "default-service" }]);
  await modelDiscoveryClient.list("explicit-service");
  assert.equal(calls[1][1].addr, "explicit-service");
  await modelDiscoveryClient.list(null);
  assert.equal(calls[2][1].addr, null);
});

test("disabling discovery preserves false and omits an unspecified interval", async () => {
  const input = { sourceKind: "openai_account", sourceId: "account-1", enabled: false };
  const payload = { ...input, intervalMinutes: 60, nextRunAt: null };
  outcomes.push(payload);
  assert.equal(await modelDiscoveryClient.update(input), payload);
  assert.deepEqual(calls[0], [
    "service_model_discovery_update",
    { addr: "default-service", ...input },
    { retries: 0 },
  ]);
  assert.equal(Object.hasOwn(calls[0][1], "intervalMinutes"), false);
});

test("aggregate discovery forwards interval and explicit address without replay", async () => {
  const input = { sourceKind: "aggregate_api", sourceId: "api-1", enabled: true, intervalMinutes: 5 };
  await modelDiscoveryClient.update(input, "explicit-service");
  assert.deepEqual(calls[0], ["service_model_discovery_update", { addr: "explicit-service", ...input }, { retries: 0 }]);
  await modelDiscoveryClient.update(input, null);
  assert.equal(calls[1][1].addr, null);
});

test("discovery update failures propagate without local retry", async () => {
  const failure = new Error("permission_denied: modelDiscovery/update");
  outcomes.push(failure);
  await assert.rejects(
    modelDiscoveryClient.update({ sourceKind: "aggregate_api", sourceId: "api-1", enabled: true }),
    (error) => error === failure,
  );
  assert.equal(calls.length, 1);
});
