import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "app",
  "settings",
  "settings-page-helpers.ts"
);

async function loadSettingsPageHelpersModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-settings-helpers-")
  );
  const tempFile = path.join(tempDir, "settings-page-helpers.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const helpers = await loadSettingsPageHelpersModule();

test("normalizeEnvRiskLevel 对未知值回退为中风险", () => {
  assert.equal(helpers.normalizeEnvRiskLevel("high"), "high");
  assert.equal(helpers.normalizeEnvRiskLevel("HIGH"), "high");
  assert.equal(helpers.normalizeEnvRiskLevel(""), "medium");
  assert.equal(helpers.normalizeEnvRiskLevel("other"), "medium");
});

test("compareEnvOverrideItems 将高风险请求语义项排在普通项之后", () => {
  const items = [
    { key: "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST", riskLevel: "high" },
    { key: "CODEXMANAGER_WEB_ROOT", riskLevel: "low" },
    { key: "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS", riskLevel: "medium" },
  ];

  const sortedKeys = items
    .slice()
    .sort(helpers.compareEnvOverrideItems)
    .map((item) => item.key);

  assert.deepEqual(sortedKeys, [
    "CODEXMANAGER_WEB_ROOT",
    "CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS",
    "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST",
  ]);
});

test("formatRuntimeTimeZoneLabel 显示后端传回的时区和偏移", () => {
  assert.equal(
    helpers.formatRuntimeTimeZoneLabel({
      name: "Asia/Shanghai",
      offset: "+08:00",
      source: "TZ",
    }),
    "Asia/Shanghai (UTC+08:00)"
  );

  assert.equal(
    helpers.formatRuntimeTimeZoneLabel({
      name: "Local",
      offset: "-05:00",
      source: "system",
    }),
    "服务端本地时区 (UTC-05:00)"
  );

  assert.equal(
    helpers.formatRuntimeTimeZoneLabel(
      {
        name: "Local",
        offset: "+01:00",
        source: "system",
      },
      "Server local time zone"
    ),
    "Server local time zone (UTC+01:00)"
  );
});
