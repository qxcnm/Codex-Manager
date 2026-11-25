import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

const sourcePath = path.resolve(import.meta.dirname, "../src/lib/model-route-policy.ts");
const compiled = ts.transpileModule(await fs.readFile(sourcePath, "utf8"), {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 }, fileName: sourcePath,
});
const { modelRoutePolicy, withModelRoutePolicy } = await import(
  `data:text/javascript;base64,${Buffer.from(compiled.outputText).toString("base64")}`,
);

test("model route scheduling is opt-in and roundtrips without losing other capabilities", () => {
  const original = Object.freeze({ reasoning_efforts: ["medium"], supports_images: true });
  assert.equal(modelRoutePolicy(original), "legacy");
  for (const policy of ["account_first", "aggregate_first", "weighted"]) {
    const saved = withModelRoutePolicy(original, policy);
    assert.equal(modelRoutePolicy(saved), policy);
    assert.deepEqual(withModelRoutePolicy(saved, "legacy"), original);
  }
  assert.equal(original.route_policy, undefined);
});

test("invalid model route policy is rejected instead of silently ignoring weights", () => {
  for (const value of [null, true, 123, {}, "typo"]) {
    assert.throws(() => modelRoutePolicy({ route_policy: value }), /无效/);
  }
});
