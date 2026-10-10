import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

test("aggregate API list exposes status filtering and order values", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src/app/aggregate-api/page.tsx"),
    "utf8",
  );

  assert.match(source, /const \[statusFilter, setStatusFilter\] = useState\("all"\)/);
  assert.match(source, /aggregateApiStatusMatchesFilter\(api\.status, statusFilter\)/);
  assert.match(source, /<SelectItem value="active">\{t\("已启用"\)\}<\/SelectItem>/);
  assert.match(source, /<SelectItem value="disabled">\{t\("已禁用"\)\}<\/SelectItem>/);
  assert.match(source, /<TableHead className="w-\[84px\]">\{t\("顺序值"\)\}<\/TableHead>/);
  assert.match(source, /\{api\.sort\}/);
  assert.match(source, /Array\.from\(\{ length: 9 \}\)/);
  assert.match(source, /<TableCell colSpan=\{9\}/);
});
