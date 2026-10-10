import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import vm from "node:vm";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "api", "normalize.ts");

async function loadNormalizeModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const commonJsModule = { exports: {} };
  const emptyDependency = new Proxy(
    {},
    {
      get: () => undefined,
    },
  );

  vm.runInNewContext(compiled.outputText, {
    exports: commonJsModule.exports,
    module: commonJsModule,
    require: (specifier) => {
      if (specifier === "@/lib/utils/usage") {
        return {
          toNullableNumber(value) {
            if (typeof value === "number" && Number.isFinite(value)) return value;
            if (typeof value !== "string" || value.trim() === "") return null;
            const parsed = Number(value);
            return Number.isFinite(parsed) ? parsed : null;
          },
        };
      }
      return emptyDependency;
    },
  });

  return commonJsModule.exports;
}

const normalizeModule = await loadNormalizeModule();

test("aggregate API normalization preserves negative sort values", () => {
  assert.equal(normalizeModule.normalizeAggregateApi({ id: "api-1", sort: -1 }).sort, -1);
  assert.equal(
    normalizeModule.normalizeAggregateApi({ id: "api-2", priority: -7.9 }).sort,
    -7,
  );
  assert.equal(
    normalizeModule.normalizeAggregateApi({ id: "api-3", sort: "invalid" }).sort,
    0,
  );
});
