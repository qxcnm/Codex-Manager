import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const chartPath = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "components",
  "ui",
  "chart.tsx",
);

test("hidden keep-alive pages do not mount zero-sized charts", async () => {
  const source = await fs.readFile(chartPath, "utf8");

  assert.match(source, /new ResizeObserver/);
  assert.match(source, /width > 0 && height > 0/);
  assert.match(source, /\{hasPositiveSize \? \(/);
  assert.match(source, /<RechartsPrimitive\.ResponsiveContainer/);
});
