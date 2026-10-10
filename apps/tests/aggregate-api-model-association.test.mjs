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
  "aggregate-api-model-association.ts",
);

async function loadAssociationModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-aggregate-association-"),
  );
  const tempFile = path.join(tempDir, "aggregate-api-model-association.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const associationModule = await loadAssociationModule();

const items = [
  { upstreamModel: "new-a", existingModelSlug: null },
  { upstreamModel: "existing-a", existingModelSlug: "local-a" },
  { upstreamModel: "new-b", existingModelSlug: null },
  { upstreamModel: "existing-b", existingModelSlug: "local-b" },
];

test("existing models are listed first without changing group order", () => {
  assert.deepEqual(
    associationModule
      .sortAggregateApiAssociationItems(items)
      .map((item) => item.upstreamModel),
    ["existing-a", "existing-b", "new-a", "new-b"],
  );
});

test("existing model selection excludes new upstream-only models", () => {
  assert.deepEqual(associationModule.existingAggregateApiModelIds(items), [
    "existing-a",
    "existing-b",
  ]);
});
