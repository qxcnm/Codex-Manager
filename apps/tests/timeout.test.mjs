import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "utils", "timeout.ts");

async function loadTimeoutModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-timeout-"));
  const tempFile = path.join(tempDir, "timeout.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const timeout = await loadTimeoutModule();

test("withTimeout returns the original resolved value", async () => {
  const result = await timeout.withTimeout(Promise.resolve("ready"), 50);

  assert.equal(result, "ready");
});

test("withTimeout rejects with TimeoutError when a task hangs", async () => {
  await assert.rejects(
    timeout.withTimeout(new Promise(() => {}), 5, "startup stalled"),
    {
      name: "TimeoutError",
      message: "startup stalled",
    },
  );
});
