import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const appFramePath = path.join(
  testDir,
  "..",
  "src",
  "components",
  "layout",
  "app-frame.tsx",
);

test("宽屏主内容区使用 90% 视觉比例并补偿布局尺寸", async () => {
  const source = await fs.readFile(appFramePath, "utf8");

  assert.match(source, /data-slot="app-main-scale"/);
  assert.match(source, /xl:scale-90/);
  assert.match(source, /xl:h-\[111\.111111%\]/);
  assert.match(source, /xl:w-\[111\.111111%\]/);
  assert.match(source, /origin-top-left/);
});
