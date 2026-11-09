import assert from "node:assert/strict";
import fs from "node:fs/promises";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

const source = await fs.readFile(new URL("../src/lib/account-bps.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
}).outputText;
const { normalizeBpsSummary, resolveBpsBatch, isBpsAuthorizationUrl } = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`
);

test("old service responses default BPS forwarding and authorization to off", () => {
  assert.deepEqual(normalizeBpsSummary({}), { bpsEnabled: false, bpsAuthorized: false, bpsExpiresAt: null });
  assert.deepEqual(normalizeBpsSummary({ bps_enabled: true, bps_authorized: true, bps_expires_at: 2000 }), {
    bpsEnabled: true, bpsAuthorized: true, bpsExpiresAt: 2000,
  });
  assert.equal(normalizeBpsSummary({ bpsEnabled: "false", bpsAuthorized: "true" }).bpsEnabled, false);
  assert.equal(normalizeBpsSummary({ bpsAuthorized: "true" }).bpsAuthorized, false);
});

test("bulk enable reports every unauthorized selection instead of filtering it out", () => {
  const accounts = [{ id: "one", bpsAuthorized: true }, { id: "two", bpsAuthorized: false }];
  assert.deepEqual(resolveBpsBatch(accounts, ["one", "two", "one"], true), {
    accountIds: ["one", "two"], missingIds: [], unauthorizedIds: ["two"],
  });
  assert.deepEqual(resolveBpsBatch(accounts, ["one", "unknown"], true), {
    accountIds: ["one", "unknown"], missingIds: ["unknown"], unauthorizedIds: [],
  });
});

test("bulk disable stays available without authorization and never implies all accounts", () => {
  const accounts = [{ id: "one", bpsAuthorized: false }];
  assert.deepEqual(resolveBpsBatch(accounts, [], false).accountIds, []);
  assert.deepEqual(resolveBpsBatch(accounts, ["one"], false).unauthorizedIds, []);
});

test("BPS authorization links only open the expected HTTPS identity service", () => {
  assert.equal(isBpsAuthorizationUrl("https://auth.openai.com/api/accounts/deviceauth/authorize?client_id=example"), true);
  for (const value of ["javascript:alert(1)", "http://auth.openai.com/", "https://auth.openai.com.evil.test/", "https://user:pass@auth.openai.com/"]) {
    assert.equal(isBpsAuthorizationUrl(value), false);
  }
});
