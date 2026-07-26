import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const appsRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("mobile shell preserves page titles and compact controls", async () => {
  const [headerSource, languageSource, workspaceSource] = await Promise.all([
    readSource("src/components/layout/header.tsx"),
    readSource("src/components/layout/language-switcher.tsx"),
    readSource("src/components/layout/page-workspace.tsx"),
  ]);

  assert.match(headerSource, /hidden h-10 w-10[\s\S]*sm:flex/);
  assert.match(headerSource, /<span className="hidden sm:inline">/);
  assert.match(headerSource, /triggerClassName="w-9 min-w-9/);
  assert.match(languageSource, /compact \? "hidden min-w-0 sm:inline"/);
  assert.match(workspaceSource, /line-clamp-2[\s\S]*sm:line-clamp-1/);
});

test("mobile management toolbars wrap without hidden page overflow", async () => {
  const [accountsSource, pluginsSource, skillsSource, settingsSource] =
    await Promise.all([
      readSource("src/app/accounts/accounts-page-view.tsx"),
      readSource("src/app/plugins/page.tsx"),
      readSource("src/app/skills/skills-catalog-panel.tsx"),
      readSource("src/app/settings/page.tsx"),
    ]);

  assert.match(accountsSource, /grid min-w-0 grid-cols-2 gap-2/);
  assert.match(accountsSource, /flex flex-col gap-3 px-2 sm:flex-row/);
  assert.match(pluginsSource, /w-full min-w-0[\s\S]*whitespace-normal/);
  assert.match(skillsSource, /grid-cols-3[\s\S]*sm:flex/);
  assert.match(settingsSource, /grid-cols-3[\s\S]*lg:flex/);
});

test("wide tables retain reachable actions and visible empty states", async () => {
  const [accountsSource, apiKeysSource, modelsSource] = await Promise.all([
    readSource("src/app/accounts/accounts-page-view.tsx"),
    readSource("src/app/apikeys/page.tsx"),
    readSource("src/app/models/page.tsx"),
  ]);

  assert.match(accountsSource, /w-\[calc\(100dvw-6rem\)\]/);
  assert.match(apiKeysSource, /w-\[calc\(100dvw-6rem\)\]/);
  assert.match(modelsSource, /table-sticky-action-head/);
  assert.match(modelsSource, /table-sticky-action-cell/);
});

test("primary and theme buttons expose clear interaction state", async () => {
  const [buttonSource, settingsSource] = await Promise.all([
    readSource("src/components/ui/button.tsx"),
    readSource("src/app/settings/page.tsx"),
  ]);

  assert.match(buttonSource, /hover:bg-primary\/90/);
  assert.match(settingsSource, /aria-pressed=\{isActive\}/);
});

test("dense management tables keep readable content and reachable row actions", async () => {
  const [accountsViewSource, accountHelpersSource, proxyCellSource, apiKeysSource, resetCreditSource, logCellsSource] =
    await Promise.all([
      readSource("src/app/accounts/accounts-page-view.tsx"),
      readSource("src/app/accounts/accounts-page-helpers.tsx"),
      readSource("src/components/accounts/account-proxy-cell.tsx"),
      readSource("src/app/apikeys/page.tsx"),
      readSource("src/components/account-reset-credit-control.tsx"),
      readSource("src/app/logs/page-cells.tsx"),
    ]);

  assert.match(accountHelpersSource, /text-\[15px\][^\"]*leading-5/);
  assert.match(accountHelpersSource, /h-5 shrink-0 px-2 text-\[10px\]/);
  assert.match(accountHelpersSource, /mt-1\.5 text-\[11px\] leading-4/);
  assert.match(accountsViewSource, /h-8 w-8 text-muted-foreground[\s\S]*<ArrowUp className="h-4 w-4"/);
  assert.match(proxyCellSource, /text-\[13px\] font-medium leading-5/);
  assert.match(apiKeysSource, /h-8 w-8 text-muted-foreground[\s\S]*<Eye className="h-4 w-4"/);
  assert.doesNotMatch(apiKeysSource, /className="scale-75"/);
  assert.match(resetCreditSource, /h-8 gap-1\.5 rounded-full/);
  assert.doesNotMatch(logCellsSource, /text-\[9px\]/);
});
