import assert from "node:assert/strict";
import fs from "node:fs/promises";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

const source = await fs.readFile(
  new URL("../src/lib/i18n/config.ts", import.meta.url),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
});
const i18n = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputText).toString("base64")}`);

test("auto detection handles system locale variants and falls back to English", () => {
  for (const language of ["zh-CN", "zh-Hans-CN", "zh_TW.UTF-8", "zh-HK"]) {
    assert.equal(i18n.resolveLocale("auto", language), "zh-CN");
  }
  assert.equal(i18n.resolveLocale("auto", "en-AU"), "en");
  assert.equal(i18n.resolveLocale("auto", "ru-RU"), "ru");
  assert.equal(i18n.resolveLocale("auto", "ko-KR"), "ko");
  assert.equal(i18n.resolveLocale("auto", "fr-FR"), "en");
  assert.equal(i18n.resolveLocale("auto", undefined), "en");
});

test("saved manual choices override the system while auto stays a preference", () => {
  assert.equal(i18n.normalizeLocalePreference("AUTO"), "auto");
  assert.equal(i18n.normalizeLocalePreference(undefined), "auto");
  assert.equal(i18n.normalizeLocalePreference("unknown"), "auto");
  assert.equal(i18n.normalizeLocalePreference("zh-cn"), "zh-CN");
  assert.equal(i18n.resolveLocale("en", "zh-CN"), "en");
  assert.equal(i18n.resolveLocale("zh-CN", "en-US"), "zh-CN");
});
