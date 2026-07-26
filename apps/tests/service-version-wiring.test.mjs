import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const appsRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

test("service initialize version reaches the shell status", async () => {
  const [runtimeTypes, serviceUtils, bootstrap, header] = await Promise.all([
    fs.readFile(path.join(appsRoot, "src", "types", "runtime.ts"), "utf8"),
    fs.readFile(path.join(appsRoot, "src", "lib", "utils", "service.ts"), "utf8"),
    fs.readFile(
      path.join(appsRoot, "src", "components", "layout", "app-bootstrap.tsx"),
      "utf8",
    ),
    fs.readFile(
      path.join(appsRoot, "src", "components", "layout", "header.tsx"),
      "utf8",
    ),
  ]);

  assert.match(runtimeTypes, /interface ServiceInitializationResult \{\s*version: string;/);
  assert.match(serviceUtils, /const version = typeof source\.version === "string"/);
  assert.match(serviceUtils, /return \{ version, userAgent, codexHome, platformFamily, platformOs \}/);
  assert.match(bootstrap, /initializeResult\.version/);
  assert.match(header, /version: initResult\.version/);
});
