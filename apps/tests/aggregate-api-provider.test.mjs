import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "aggregate-api-provider.ts");

async function loadProviderModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-aggregate-provider-"),
  );
  const tempFile = path.join(tempDir, "aggregate-api-provider.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const providerModule = await loadProviderModule();

test("compatible aggregate APIs match Codex and Claude filters", () => {
  const matches = providerModule.aggregateApiProviderMatchesFilter;

  assert.equal(matches("compatible", "codex"), true);
  assert.equal(matches("compatible", "claude"), true);
  assert.equal(matches("compatible", "gemini"), false);
  assert.equal(matches("compatible", "compatible"), true);
  assert.equal(matches("claude", "codex"), false);
  assert.equal(matches("codex", "all"), true);
});

test("only compatible aggregate APIs require the incoming request path", () => {
  const usesIncomingPath = providerModule.aggregateApiUsesIncomingPath;

  assert.equal(usesIncomingPath("compatible"), true);
  assert.equal(usesIncomingPath(" Compatible "), true);
  assert.equal(usesIncomingPath("codex"), false);
  assert.equal(usesIncomingPath("claude"), false);
});

test("aggregate API status filters match active and disabled connections", () => {
  const matches = providerModule.aggregateApiStatusMatchesFilter;

  assert.equal(matches("active", "all"), true);
  assert.equal(matches("active", "active"), true);
  assert.equal(matches("disabled", "active"), false);
  assert.equal(matches(" DISABLED ", "disabled"), true);
});
