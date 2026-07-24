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
  "codex-cli-onboarding-dialog.tsx",
);
const source = await fs.readFile(sourcePath, "utf8");
const platformModeState = await fs.readFile(
  path.join(appsRoot, "src", "app", "platform-mode", "use-platform-mode-state.ts"),
  "utf8",
);

test("Codex onboarding routes configuration through Platform Mode", () => {
  assert.match(source, /await onAcknowledge\(dismissPermanently\)/);
  assert.match(source, /buildStaticRouteUrl\("\/platform-mode"\)/);
  assert.match(source, /打开平台模式/);
  assert.match(platformModeState, /codexProfileClient\.applyGateway\(\{/);
});

test("Codex onboarding no longer publishes hand-written profile templates", () => {
  assert.doesNotMatch(source, /GUIDE_AUTH_JSON_TEXT/);
  assert.doesNotMatch(source, /replace_with_codexmanager_platform_key/);
  assert.doesNotMatch(source, /model_providers\.codex/);
  assert.doesNotMatch(source, /复制 auth\.json/);
  assert.doesNotMatch(source, /复制 config\.toml/);
});
