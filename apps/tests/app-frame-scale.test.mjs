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

test("主内容区使用原生尺寸，浮层和 sticky 使用同一坐标系", async () => {
  const source = await fs.readFile(appFramePath, "utf8");

  assert.match(source, /data-slot="app-main-surface"/);
  assert.match(source, /className="flex h-full w-full flex-col"/);
  assert.doesNotMatch(source, /xl:scale-90/);
  assert.match(source, /px-4 pb-5 pt-2/);
  assert.match(source, /lg:px-6 lg:pb-6 lg:pt-3/);
});
