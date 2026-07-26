import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const headerPath = path.join(
  testDir,
  "..",
  "src",
  "components",
  "layout",
  "header.tsx",
);

test("管理员仪表盘保留服务开关和语言选择", async () => {
  const source = await fs.readFile(headerPath, "utf8");

  assert.match(source, /<Switch[\s\S]*?onCheckedChange=\{handleToggleService\}/);
  assert.match(
    source,
    /<LanguageSwitcher compact triggerClassName="w-\[106px\]" \/>/,
  );
  assert.match(source, /<DisclaimerTicker compact \/>/);
  assert.match(source, /v\{serviceStatus\.version\}/);
  assert.doesNotMatch(source, /!isCommandCenter\s*\?\s*\(\s*<Switch/);
  assert.doesNotMatch(source, /!isCommandCenter\s*\?\s*<LanguageSwitcher/);
});
