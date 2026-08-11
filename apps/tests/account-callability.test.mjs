import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const sourcePath = path.resolve(
  import.meta.dirname,
  "../src/lib/account-callability.ts",
);

async function loadModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "account-callability-"));
  const tempFile = path.join(tempDir, "account-callability.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const { resolveAccountCallability, accountCallabilityText } = await loadModule();

test("only a verified Responses probe is shown as callable", () => {
  assert.equal(
    resolveAccountCallability({
      status: "active",
      statusReason: "usage_ok",
      credentialState: "healthy",
      gatewayProbeStatus: "available",
      gatewayProbeReason: "codex_responses_verified",
    }),
    "callable",
  );
  assert.equal(
    resolveAccountCallability({
      status: "active",
      statusReason: "usage_ok",
      credentialState: "healthy",
      gatewayProbeStatus: "failed",
      gatewayProbeReason: "codex_responses_unauthorized",
    }),
    "auth_failed",
  );
});

test("revoked tokens are recoverable and are not mislabeled as banned", () => {
  assert.equal(
    resolveAccountCallability({
      status: "unavailable",
      statusReason: "refresh_token_invalid:refresh_token_invalidated",
      credentialState: "refresh_token_revoked",
      credentialAction: "reauthenticate",
      gatewayProbeStatus: "failed",
    }),
    "reauthenticate",
  );
  assert.equal(accountCallabilityText("reauthenticate"), "需要重新登录");
});

test("only explicit account or workspace deactivation is terminal", () => {
  assert.equal(
    resolveAccountCallability({
      status: "banned",
      statusReason: "account_deactivated",
      credentialState: "account_deactivated",
      credentialAction: "stop",
    }),
    "confirmed_deactivated",
  );
  assert.equal(
    resolveAccountCallability({
      status: "unavailable",
      statusReason: "usage_http_401",
      credentialState: "access_token_rejected",
      credentialAction: "refresh",
    }),
    "auth_failed",
  );
});

test("quota and network failures remain recoverable categories", () => {
  assert.equal(
    resolveAccountCallability({
      status: "limited",
      statusReason: "usage_limit_exhausted",
      gatewayProbeStatus: "unavailable",
      gatewayProbeReason: "quota_exhausted",
    }),
    "quota_limited",
  );
  assert.equal(
    resolveAccountCallability({
      status: "active",
      statusReason: "usage_ok",
      credentialState: "healthy",
      gatewayProbeStatus: "failed",
      gatewayProbeReason: "codex_models_probe_failed",
    }),
    "network_unknown",
  );
});
