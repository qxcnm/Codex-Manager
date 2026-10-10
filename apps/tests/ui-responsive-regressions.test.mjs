import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const appsRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("shell preserves page titles and compacts header controls by content width", async () => {
  const [
    headerSource,
    languageSource,
    workspaceSource,
    appFrameSource,
    stylesSource,
    disclaimerSource,
  ] = await Promise.all([
      readSource("src/components/layout/header.tsx"),
      readSource("src/components/layout/language-switcher.tsx"),
      readSource("src/components/layout/page-workspace.tsx"),
      readSource("src/components/layout/app-frame.tsx"),
      readSource("src/app/globals.css"),
      readSource("src/components/layout/disclaimer-ticker.tsx"),
    ]);

  assert.match(
    headerSource,
    /gap-2 glass-header px-3[\s\S]*sm:gap-3 sm:px-4/,
  );
  assert.match(headerSource, /flex min-w-0 flex-1[\s\S]*overflow-hidden/);
  assert.match(headerSource, /truncate text-sm font-medium text-muted-foreground/);
  assert.match(
    headerSource,
    /triggerClassName="w-\[124px\] min-w-\[124px\] gap-2 px-2\.5"/,
  );
  assert.match(
    languageSource,
    /flex min-w-0 flex-1 items-center justify-center gap-2 overflow-hidden/,
  );
  assert.match(languageSource, /data-slot="language-switcher-label"/);
  assert.match(disclaimerSource, /header-disclaimer-label whitespace-nowrap/);
  assert.match(languageSource, /<SelectValue className="min-w-0 truncate">/);
  assert.match(appFrameSource, /data-slot="app-main-column"/);
  assert.match(stylesSource, /\[data-slot="app-main-column"\][\s\S]*container-type: inline-size;/);
  assert.doesNotMatch(headerSource, /header-page-date/);
  assert.match(headerSource, /header-service-port-label/);
  assert.match(headerSource, /aria-label=\{t\("刷新数据"\)\}/);
  assert.match(headerSource, /className="header-language-switcher"/);
  assert.match(stylesSource, /@container \(max-width: 1320px\)/);
  assert.match(
    stylesSource,
    /\.header-service-port \.header-service-port-label[\s\S]*display: none;/,
  );
  assert.match(
    stylesSource,
    /@container \(max-width: 760px\)[\s\S]*\.header-disclaimer \.header-disclaimer-label,[\s\S]*\.header-language-switcher \[data-slot="select-trigger"\][\s\S]*width: 2\.5rem;/,
  );
  assert.match(workspaceSource, /max-w-3xl text-sm leading-5/);
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
  assert.match(settingsSource, /TabsList variant="line"[\s\S]*overflow-x-auto/);
});

test("wide tables retain reachable actions and visible empty states", async () => {
  const [accountsSource, apiKeysSource, modelsSource, stylesSource] =
    await Promise.all([
      readSource("src/app/accounts/accounts-page-view.tsx"),
      readSource("src/app/apikeys/page.tsx"),
      readSource("src/app/models/page.tsx"),
      readSource("src/app/globals.css"),
    ]);

  assert.match(accountsSource, /w-\[calc\(100dvw-6rem\)\]/);
  assert.match(apiKeysSource, /w-\[calc\(100dvw-6rem\)\]/);
  assert.match(modelsSource, /table-sticky-action-head/);
  assert.match(modelsSource, /table-sticky-action-cell/);
  assert.match(
    accountsSource,
    /account-pool-layout[\s\S]*account-pool-main-pane[\s\S]*account-pool-main-table[\s\S]*account-pool-col-status[\s\S]*account-pool-action-rail/,
  );
  assert.doesNotMatch(accountsSource, /table-sticky-action-(?:head|cell)/);
  assert.match(accountsSource, /new ResizeObserver\(syncRowHeights\)/);
  assert.match(accountsSource, /data-account-pool-main-row/);
  assert.match(accountsSource, /data-account-pool-action-row/);
  assert.match(
    stylesSource,
    /\.account-pool-layout[\s\S]*grid-template-columns: minmax\(0, 1fr\) var\(--account-pool-action-width\);/,
  );
  assert.match(stylesSource, /\.account-pool-main-pane[\s\S]*overflow-x: auto;/);
  assert.match(
    stylesSource,
    /\.account-pool-main-table[\s\S]*table-layout: fixed;[\s\S]*width: 100%;[\s\S]*min-width: 1206px;/,
  );
  assert.match(stylesSource, /\.account-pool-col-quota[\s\S]*width: 330px;/);
  assert.match(stylesSource, /\.account-pool-quota-grid[\s\S]*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\)/);
  assert.match(stylesSource, /\.account-pool-col-order[\s\S]*width: 168px;/);
  assert.match(accountsSource, /w-\[168px\].*顺序/);
  assert.match(
    stylesSource,
    /\.account-pool-action-rail[\s\S]*position: relative;[\s\S]*z-index: 5;[\s\S]*width: var\(--account-pool-action-width\);/,
  );
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
