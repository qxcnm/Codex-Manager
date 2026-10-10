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
  "reset-credit-client.ts",
);

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function loadResetCreditClient({
  storage,
  serviceAddr = "127.0.0.1:48760",
} = {}) {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-reset-credit-client-"),
  );
  const modulePath = path.join(tempDir, "reset-credit-client.mjs");
  const transportPath = path.join(tempDir, "transport.mjs");

  const previousStorage = globalThis.localStorage;
  if (storage) globalThis.localStorage = storage;

  await fs.writeFile(
    transportPath,
    `export const calls = [];
export const outcomes = [];
export function withAddr(params) { return { addr: ${JSON.stringify(serviceAddr)}, ...params }; }
export async function invoke(...args) {
  calls.push(args);
  const outcome = outcomes.shift();
  if (outcome instanceof Error) throw outcome;
  if (outcome) return outcome;
  return { consumed: true, usageRefreshed: true, snapshot: null, warning: null };
}
`,
    "utf8",
  );
  await fs.writeFile(
    modulePath,
    compiled.outputText.replace(
      /from "\.\/transport"/g,
      'from "./transport.mjs"',
    ),
    "utf8",
  );

  const modules = await Promise.all([
    import(pathToFileURL(modulePath).href),
    import(pathToFileURL(transportPath).href),
  ]);
  if (storage) {
    // Keep the fake storage installed for the caller's replay assertions.
    modules.push({
      restoreStorage: () => {
        if (previousStorage === undefined) delete globalThis.localStorage;
        else globalThis.localStorage = previousStorage;
      },
    });
  }
  return modules;
}

test("reset credit consumption disables transport replay and allows the service transaction to finish", async () => {
  const [clientModule, transportModule] = await loadResetCreditClient();

  const result = await clientModule.resetCreditClient.consume("account-1");

  assert.equal(result.consumed, true);
  assert.equal(transportModule.calls.length, 1);
  const [command, params, options] = transportModule.calls[0];
  assert.equal(command, "service_usage_reset_credit_consume");
  assert.deepEqual(
    { addr: params.addr, accountId: params.accountId },
    { addr: "127.0.0.1:48760", accountId: "account-1" },
  );
  assert.match(
    params.operationId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  assert.deepEqual(options, { retries: 0, timeoutMs: 600_000 });
});

test("unknown reset-credit outcomes reuse the persisted operation id", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });

  transportModule.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-retry"),
  );
  const firstOperationId = transportModule.calls[0][1].operationId;

  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-retry");
  assert.equal(transportModule.calls[1][1].operationId, firstOperationId);
  assert.equal(values.size, 0);
  testHooks.restoreStorage();
});

test("pending operation ids are isolated by service address", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [firstClient, firstTransport, firstHooks] = await loadResetCreditClient(
    {
      storage,
      serviceAddr: "LOCALHOST:48760",
    },
  );
  firstTransport.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    firstClient.resetCreditClient.consume("account-isolated"),
  );
  const firstOperationId = firstTransport.calls[0][1].operationId;

  const [secondClient, secondTransport, secondHooks] =
    await loadResetCreditClient({
      storage,
      serviceAddr: "localhost:48761",
    });
  await secondClient.resetCreditClient.consume("account-isolated");
  const secondOperationId = secondTransport.calls[0][1].operationId;

  assert.notEqual(secondOperationId, firstOperationId);
  assert.equal(
    values.size,
    1,
    "the first service's pending operation must remain stored",
  );
  secondHooks.restoreStorage();
  firstHooks.restoreStorage();
});

test("unknown reset-credit outcomes reuse the operation id when localStorage writes fail", async () => {
  const storage = {
    getItem() {
      throw new Error("storage unavailable");
    },
    setItem() {
      throw new Error("storage unavailable");
    },
    removeItem() {
      throw new Error("storage unavailable");
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });

  transportModule.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-memory"),
  );
  const firstOperationId = transportModule.calls[0][1].operationId;

  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-memory");
  assert.equal(transportModule.calls[1][1].operationId, firstOperationId);
  testHooks.restoreStorage();
});

test("an existing pending operation survives a later service connection failure", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });

  transportModule.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-disconnected"),
  );
  const pendingOperationId = transportModule.calls[0][1].operationId;

  transportModule.outcomes.push(new Error("Failed to connect to service"));
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-disconnected"),
  );
  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-disconnected");

  assert.equal(transportModule.calls[1][1].operationId, pendingOperationId);
  assert.equal(transportModule.calls[2][1].operationId, pendingOperationId);
  assert.equal(values.size, 0);
  testHooks.restoreStorage();
});

test("a backend pending-operation marker recovers an operation id lost by the client", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });
  const recoveredOperationId = "01234567-89ab-4def-8abc-0123456789ab";

  transportModule.outcomes.push(
    new Error(
      `reset_credit_pending_operation:${recoveredOperationId}; reset credit operation result is unknown`,
    ),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-recovered"),
  );
  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-recovered");

  assert.equal(
    transportModule.calls[1][1].operationId,
    recoveredOperationId,
  );
  assert.equal(values.size, 0);
  testHooks.restoreStorage();
});

test("a backend terminal marker clears the pending operation id", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });

  transportModule.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-terminal"),
  );
  const failedOperationId = transportModule.calls[0][1].operationId;

  transportModule.outcomes.push(
    new Error("reset_credit_terminal_failure:provider rejected"),
  );
  await assert.rejects(
    () => clientModule.resetCreditClient.consume("account-terminal"),
    /provider rejected/,
  );
  assert.equal(values.size, 0);

  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-terminal");
  assert.notEqual(transportModule.calls[2][1].operationId, failedOperationId);
  testHooks.restoreStorage();
});

test("a terminal failure containing a pending marker stays terminal", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });
  const embeddedOperationId = "01234567-89ab-4def-8abc-0123456789ab";

  transportModule.outcomes.push(
    new Error("reset credit operation result is unknown"),
  );
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-terminal-marker"),
  );

  transportModule.outcomes.push(
    new Error(
      `reset_credit_terminal_failure:provider rejected reset_credit_pending_operation:${embeddedOperationId}`,
    ),
  );
  await assert.rejects(
    () => clientModule.resetCreditClient.consume("account-terminal-marker"),
    /provider rejected/,
  );

  assert.equal(values.size, 0);
  testHooks.restoreStorage();
});

test("a fresh operation keeps its id after an ambiguous Web gateway 502", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });

  transportModule.outcomes.push(new Error("502 Bad Gateway"));
  await assert.rejects(() =>
    clientModule.resetCreditClient.consume("account-web-502"),
  );
  const operationId = transportModule.calls[0][1].operationId;
  transportModule.outcomes.push({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await clientModule.resetCreditClient.consume("account-web-502");

  assert.equal(transportModule.calls[1][1].operationId, operationId);
  assert.equal(values.size, 0);
  testHooks.restoreStorage();
});

test("a late success does not clear a newer operation id", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });
  const firstOutcome = deferred();
  transportModule.outcomes.push(firstOutcome.promise);
  const firstRequest = clientModule.resetCreditClient.consume("account-late");
  await new Promise((resolve) => setImmediate(resolve));

  const storageKey = [...values.keys()][0];
  const newerOperationId = "11111111-2222-4333-8444-555555555555";
  values.set(storageKey, newerOperationId);
  firstOutcome.resolve({
    consumed: true,
    usageRefreshed: true,
    snapshot: null,
    warning: null,
  });
  await firstRequest;

  assert.equal(values.get(storageKey), newerOperationId);
  testHooks.restoreStorage();
});

test("a late recovery marker does not overwrite a newer operation id", async () => {
  const values = new Map();
  const storage = {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    removeItem(key) {
      values.delete(key);
    },
  };
  const [clientModule, transportModule, testHooks] =
    await loadResetCreditClient({ storage });
  const firstOutcome = deferred();
  transportModule.outcomes.push(firstOutcome.promise);
  const firstRequest = clientModule.resetCreditClient.consume("account-late-recovery");
  await new Promise((resolve) => setImmediate(resolve));

  const storageKey = [...values.keys()][0];
  const newerOperationId = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
  const recoveredOperationId = "01234567-89ab-4def-8abc-0123456789ab";
  values.set(storageKey, newerOperationId);
  firstOutcome.reject(
    new Error(
      `reset_credit_pending_operation:${recoveredOperationId}; reset credit operation result is unknown`,
    ),
  );
  await assert.rejects(() => firstRequest);

  assert.equal(values.get(storageKey), newerOperationId);
  testHooks.restoreStorage();
});
