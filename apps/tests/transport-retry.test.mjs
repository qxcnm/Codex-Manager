import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const libRoot = path.resolve(import.meta.dirname, "../src/lib");
const scratch = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-transport-retry-"));
test.after(() => fs.rm(scratch, { recursive: true, force: true }));

async function write(relative, source) {
  const output = path.join(scratch, relative);
  await fs.mkdir(path.dirname(output), { recursive: true });
  await fs.writeFile(output, source);
}

const modules = ["api/transport", "api/rpc-http", "api/rpc-retry-policy", "api/transport-errors", "api/transport-web-commands", "utils/request"];
for (const file of await fs.readdir(path.join(libRoot, "api/transport-web-commands"))) {
  if (file.endsWith(".ts")) modules.push(`api/transport-web-commands/${file.slice(0, -3)}`);
}
for (const moduleName of modules) {
  const source = await fs.readFile(path.join(libRoot, `${moduleName}.ts`), "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
  }).outputText
    .replaceAll('"@tauri-apps/api/core"', '"../tauri"')
    .replaceAll('"./rpc-retry-policy.json"', '"./policy-data"')
    .replace(/from "(\.[^"]+)"/g, 'from "$1.js"');
  await write(`${moduleName}.js`, compiled);
}
await write("package.json", '{"type":"module"}');
await write("api/policy-data.js", `export default ${await fs.readFile(path.join(libRoot, "api/rpc-retry-policy.json"), "utf8")};`);
await write("store/useAppStore.js", 'export const useAppStore = { getState: () => ({ serviceStatus: { addr: "mock-service" } }) };');
await write("runtime/runtime-capabilities.js", 'export const DEFAULT_UNSUPPORTED_WEB_REASON = "unsupported";');
await write("api/transport-runtime.js", `
export let desktop = true;
export function setDesktop(value) { desktop = value; }
export function isTauriRuntime() { return desktop; }
export function getCachedRuntimeCapabilities() { return null; }
export async function loadRuntimeCapabilities() { return { mode: "web-gateway", rpcBaseUrl: "/api/rpc" }; }
`);
await write("tauri.js", `
export const calls = [];
let handler;
export function setHandler(value) { calls.length = 0; handler = value; }
export async function invoke(...args) { calls.push(args); return handler(...args); }
`);
const transport = await import(pathToFileURL(path.join(scratch, "api/transport.js")).href);
const native = await import(pathToFileURL(path.join(scratch, "tauri.js")).href);
const runtime = await import(pathToFileURL(path.join(scratch, "api/transport-runtime.js")).href);
const retry = { retries: 3, retryDelayMs: 0, shouldRetry: () => true, shouldRetryStatus: () => true };

test("outer Tauri invoke cannot override non-idempotent or unknown command protection", async () => {
  for (const command of ["service_apikey_create", "service_usage_reset_credit_consume", "service_model_discovery_update", "app_future_write"]) {
    native.setHandler(() => { throw new Error("rpc_result_unknown: response lost, not found"); });
    await assert.rejects(transport.invoke(command, {}, retry), /rpc_result_unknown:/);
    assert.equal(native.calls.length, 1, command);
  }
});

test("outer Tauri invoke retains caller retries for read-only and idempotent RPCs", async () => {
  for (const command of ["service_apikey_list", "service_apikey_disable", "service_model_discovery_list"]) {
    native.setHandler(() => {
      if (native.calls.length === 1) throw new Error("temporary connection failure");
      return { result: { ok: true } };
    });
    assert.deepEqual(await transport.invoke(command, {}, retry), { ok: true });
    assert.equal(native.calls.length, 2);
  }
});

test("invokeFirst never tries an alias after an unknown write result", async () => {
  native.setHandler(() => { throw new Error("rpc_result_unknown: HTTP 404 not found"); });
  await assert.rejects(transport.invokeFirst(["service_apikey_create", "legacy_create"], {}, retry), /rpc_result_unknown:/);
  assert.equal(native.calls.length, 1);
});

test("invokeFirst still falls back when a command is missing before execution", async () => {
  native.setHandler((command) => {
    if (command === "missing_create") throw new Error("unknown command missing_create");
    return { result: { id: "created-once" } };
  });
  assert.deepEqual(await transport.invokeFirst(["missing_create", "service_apikey_create"]), { id: "created-once" });
  assert.equal(native.calls.length, 2);
});

test("cancelling an already-started write reports unknown without replay", async () => {
  const controller = new AbortController();
  let started;
  let complete;
  const called = new Promise((resolve) => { started = resolve; });
  const lateReply = new Promise((resolve) => { complete = resolve; });
  let cache;
  native.setHandler(() => { started(); return lateReply; });
  const pending = transport.invoke("service_apikey_create", {}, { ...retry, signal: controller.signal });
  pending.then((value) => { cache = value; }, () => {});
  const rejected = assert.rejects(pending, (error) => {
    assert.equal(error.name, "AbortError");
    assert.equal(error.cause.name, "AbortError");
    assert.match(error.message, /rpc_result_unknown:.*刷新核实/);
    return true;
  });
  await called;
  controller.abort();
  await rejected;
  complete({ result: { id: "late-key" } });
  await lateReply;
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(cache, undefined);
  assert.equal(native.calls.length, 1);
});

test("cancellation before the scheduled IPC starts stays definitive", async () => {
  const controller = new AbortController();
  native.setHandler(() => assert.fail("IPC must not start"));
  const pending = transport.invoke("service_apikey_create", {}, { ...retry, signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, (error) => {
    assert.equal(error.name, "AbortError");
    assert.doesNotMatch(error.message, /rpc_result_unknown:/);
    return true;
  });
  assert.equal(native.calls.length, 0);
});

test("pre-cancelled native writes never invoke IPC", async () => {
  const controller = new AbortController();
  controller.abort();
  native.setHandler(() => assert.fail("IPC must not start"));
  await assert.rejects(transport.invoke("service_apikey_create", {}, { signal: controller.signal }), { name: "AbortError" });
  assert.equal(native.calls.length, 0);
});

test("Web command mapping and real retry utility protect key creation centrally", async () => {
  const originalFetch = globalThis.fetch;
  let commits = 0;
  runtime.setDesktop(false);
  globalThis.fetch = async (_url, init) => {
    assert.equal(JSON.parse(init.body).method, "apikey/create");
    commits += 1;
    throw new TypeError("Failed to fetch");
  };
  try {
    await assert.rejects(transport.invoke("service_apikey_create", { name: "test" }, retry), /rpc_result_unknown:/);
    assert.equal(commits, 1);
  } finally {
    globalThis.fetch = originalFetch;
    runtime.setDesktop(true);
  }
});

test("Web discovery updates are never replayed after a lost response", async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  runtime.setDesktop(false);
  globalThis.fetch = async (_url, init) => {
    calls.push(JSON.parse(init.body));
    throw new TypeError("Failed to fetch");
  };
  const input = { sourceKind: "aggregate_api", sourceId: "api-1", enabled: false, intervalMinutes: 5 };
  try {
    await assert.rejects(transport.invoke("service_model_discovery_update", input, retry), /rpc_result_unknown:/);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].method, "modelDiscovery/update");
    assert.deepEqual(calls[0].params, input);
  } finally {
    globalThis.fetch = originalFetch;
    runtime.setDesktop(true);
  }
});
