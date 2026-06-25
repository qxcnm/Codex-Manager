import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "components",
  "layout",
  "app-bootstrap.tsx",
);

const source = await fs.readFile(sourcePath, "utf8");

test("AppBootstrap gives Docker/Web gateway a longer settings cold-start window", () => {
  assert.match(source, /const STARTUP_STEP_TIMEOUT_MS = 15_000;/);
  assert.match(source, /const WEB_GATEWAY_SETTINGS_TIMEOUT_MS = 60_000;/);
  assert.match(
    source,
    /startupAppSettingsTimeoutMs\(runtimeMode\?: string \| null\): number \{[\s\S]*runtimeMode === "web-gateway"[\s\S]*WEB_GATEWAY_SETTINGS_TIMEOUT_MS[\s\S]*STARTUP_STEP_TIMEOUT_MS[\s\S]*\}/,
  );
  assert.match(
    source,
    /const appSettingsTimeoutMs = startupAppSettingsTimeoutMs\([\s\S]*detectedRuntimeCapabilities\.mode[\s\S]*\);[\s\S]*appClient\.getSettings\(\),[\s\S]*appSettingsTimeoutMs,[\s\S]*Loading app settings timed out after \$\{appSettingsTimeoutMs \/ 1000\}s/,
  );
});