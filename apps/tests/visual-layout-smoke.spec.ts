import { expect, test, type Page } from "@playwright/test";
import type { ManagedModelV2 } from "../src/types/model-v2";

const fixtureTime = new Date("2026-11-07T10:00:00+08:00");
const fixtureNow = fixtureTime.getTime() / 1000;

const accounts = [
  { id: "visual-a", label: "设计团队 · 主账号", status: "active", hasToken: true, planType: "plus", sort: 0 },
  { id: "visual-b", label: "开发环境 · 备用账号", status: "active", hasToken: true, planType: "free", sort: 1 },
  { id: "visual-c", label: "测试账号 · 已停用", status: "disabled", hasToken: true, sort: 2 },
];
const usageSnapshots = accounts.map((account, index) => ({
  accountId: account.id, availabilityStatus: index === 2 ? "unavailable" : "available",
  usedPercent: [26, 62, 100][index], secondaryUsedPercent: [18, 41, 100][index],
  windowMinutes: 300, secondaryWindowMinutes: 10080, capturedAt: fixtureNow - 300,
  resetsAt: fixtureNow + 5 * 3600, secondaryResetsAt: fixtureNow + 7 * 86400,
}));
const connections = [
  { id: "visual-api-a", supplierName: "团队网关", providerType: "codex", status: "active", modelSlugs: ["demo-reasoning"] },
  { id: "visual-api-b", supplierName: "开发备用连接", providerType: "compatible", status: "active", modelSlugs: ["demo-fast"] },
  { id: "visual-api-c", supplierName: "测试连接", providerType: "claude", status: "disabled", modelSlugs: [] },
].map((item, index) => ({ ...item, sort: index, url: `https://${item.id}.invalid/v1`, authType: "bearer" }));

const models: ManagedModelV2[] = ["demo-reasoning", "demo-fast", "demo-preview"].map((slug, index) => ({
  id: `visual:${slug}`, slug, displayName: ["Reasoning Model", "Fast Model", "Preview Model"][index],
  description: "仅供界面预览的模拟模型", provider: "fixture", family: null, category: null, tags: [],
  origin: "custom", enabled: index !== 2, supportedInApi: true, visibility: "list", sortOrder: index,
  contextWindow: 128000, maxContextWindow: null, defaultReasoningEffort: null, capabilities: {},
  instructionsMode: "passthrough", instructionsText: null, fastPolicy: "passthrough",
  builtinRevision: null, userEdited: false, permissionGroupIds: [], createdAt: 1, updatedAt: 1,
  price: { priceStatus: "custom", priceSource: "visual fixture", inputMicrousdPer1m: 1000000,
    cachedInputMicrousdPer1m: 100000, cacheWriteMicrousdPer1m: null, outputMicrousdPer1m: 5000000 },
  priceTiers: [], routes: [{ id: `route:${slug}`, sourceKind: "account_pool", sourceId: "default",
    upstreamModel: slug, enabled: true, priority: 0, weight: 1 }],
}));

function usage(scale = 1) {
  const requestCount = Math.round(96 * scale), successCount = Math.round(95 * scale);
  return { inputTokens: Math.round(168000 * scale), cachedInputTokens: Math.round(46000 * scale),
    outputTokens: Math.round(32000 * scale), reasoningOutputTokens: Math.round(18000 * scale),
    totalTokens: Math.round(200000 * scale), estimatedCostUsd: 1.24 * scale,
    requestCount, successCount, errorCount: requestCount - successCount };
}

async function mockVisualRuntime(page: Page, theme: string) {
  // Freeze Date only; timers and the chart's requestAnimationFrame keep running.
  await page.clock.setFixedTime(fixtureTime);
  // Only local assets and mocked RPCs are allowed. Screenshots cannot contain a
  // running service's accounts, tokens, endpoint responses or usage history.
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    return ["localhost", "127.0.0.1", "::1"].includes(url.hostname)
      ? route.continue() : route.abort();
  });
  await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
    canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
    canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
  } }));
  await page.route("**/api/rpc**", (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    const start = Number(params.startTs) || fixtureNow - 6 * 86400;
    const end = Number(params.endTs) || start + 7 * 86400;
    const results: Record<string, unknown> = {
      "appSettings/get": { serviceAddr: "localhost:48760", theme, appearancePreset: "modern",
        locale: "zh-CN", lowTransparency: false, codexCliGuideDismissed: true, webAuthMode: "none",
        accountsAutoRefreshEnabled: false, envOverrides: {}, envOverrideCatalog: [] },
      initialize: { version: "0.7.1", userAgent: "codex_cli_rs/fixture", codexHome: "C:/VisualFixture/.codex",
        platformFamily: "windows", platformOs: "windows" },
      "accountManager/session/current": { mode: "none", currentUser: null, role: "system_admin",
        permissions: [], distributionEnabled: false },
      "account/list": { items: accounts, total: accounts.length, page: 1, pageSize: 20 },
      "account/usage/list": usageSnapshots,
      "aggregateApi/list": { items: connections },
      "modelDiscovery/list": { items: [] },
      "apikey/managedModelListV2": { items: models,
        stats: { total: 3, enabled: 2, builtin: 0, custom: 3, priceMissing: 0, missingRoute: 0 } },
      "codexProfile/get": { mode: "gateway", managedCatalogActive: true, warnings: [], profileWritable: true },
      "codexProfile/listCandidates": { accounts: [], apiKeys: [] },
      "startup/snapshot": { accounts, usageSnapshots, apiKeys: [], requestLogs: [],
        accountSummary: { accountCount: 3, availableCount: 2, lowQuotaCount: 0 },
        usageAggregateSummary: { primaryBucketCount: 2, primaryKnownCount: 2, primaryRemainPercent: 56,
          secondaryBucketCount: 2, secondaryKnownCount: 2, secondaryRemainPercent: 70.5 },
        requestLogTodaySummary: { todayTokens: 200000, cachedInputTokens: 46000, reasoningOutputTokens: 18000, estimatedCost: 1.24 } },
      "dashboard/adminUsageSummary": { rangeStartTs: start, rangeEndTs: end,
        todayStartTs: end - 86400, todayEndTs: end, todayUsage: usage(),
        dailyUsage: [0.4, 0.65, 0.5, 0.85, 0.72, 0.9, 1].map((scale, index) => ({
          dayStartTs: start + index * 86400, dayEndTs: start + (index + 1) * 86400, usage: usage(scale),
        })), seriesBucketSeconds: 86400, seriesUsage: [], modelUsage: [], users: [], openaiAccounts: [], aggregateApis: [] },
    };
    return route.fulfill({ json: { jsonrpc: "2.0", id, result: results[method] ?? {} } });
  });
}

async function waitForCompleteChart(page: Page) {
  await page.waitForFunction(() => {
    const curve = document.querySelector<SVGPathElement>("main .recharts-area-curve");
    if (!curve) return false;
    const bounds = curve.getBBox();
    if (bounds.width < 100 || curve.getTotalLength() < 100) return false;
    for (let ancestor = curve.parentElement; ancestor; ancestor = ancestor.parentElement) {
      const clipId = ancestor.getAttribute("clip-path")?.match(/url\(#([^)]*)\)/)?.[1];
      if (!clipId) continue;
      const clip = document.getElementById(clipId)?.querySelector("rect");
      if (clip) {
        const clipBounds = clip.getBBox();
        if (clipBounds.x + clipBounds.width < bounds.x + bounds.width - 1) return false;
      }
    }
    return true;
  });
}

async function assertAccountToolbar(page: Page) {
  const search = page.getByPlaceholder("搜索账号名 / 编号...", { exact: true });
  const controls = [search, page.getByRole("main").getByRole("combobox").nth(0),
    page.getByRole("main").getByRole("combobox").nth(1),
    page.getByRole("button", { name: "列表视图", exact: true }),
    page.getByRole("button", { name: "宫格视图", exact: true }),
    page.getByRole("button", { name: "预热", exact: true }),
    page.getByRole("button", { name: "账号操作", exact: true }).first()];
  const heights = await Promise.all(controls.map((control) => control.evaluate((element) => element.getBoundingClientRect().height)));
  expect(Math.max(...heights) - Math.min(...heights), `account toolbar heights: ${heights}`).toBeLessThanOrEqual(1);
  await search.focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(search).toBeFocused();
  const focus = await search.evaluate((element) => {
    const style = getComputedStyle(element);
    return { visible: element.matches(":focus-visible"), width: parseFloat(style.outlineWidth),
      style: style.outlineStyle, color: style.outlineColor };
  });
  expect(focus.visible).toBe(true);
  expect(focus.width).toBeGreaterThanOrEqual(2);
  expect(focus.style).not.toBe("none");
  expect(focus.color).not.toBe("rgba(0, 0, 0, 0)");
  await page.getByRole("heading", { name: "OpenAI 账号池", exact: true }).click();
}

async function assertLineTab(page: Page) {
  const active = page.getByRole("tab", { name: "通用", exact: true });
  await expect(active).toHaveAttribute("aria-selected", "true");
  const borders = await active.evaluate((element) => {
    const style = getComputedStyle(element);
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d")!;
    const alpha = (color: string) => {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      return context.getImageData(0, 0, 1, 1).data[3];
    };
    return { top: alpha(style.borderTopColor) * parseFloat(style.borderTopWidth),
      left: alpha(style.borderLeftColor) * parseFloat(style.borderLeftWidth),
      right: alpha(style.borderRightColor) * parseFloat(style.borderRightWidth),
      bottom: alpha(style.borderBottomColor) * parseFloat(style.borderBottomWidth) };
  });
  expect([borders.top, borders.left, borders.right], "line tabs must not show a full active rectangle").toEqual([0, 0, 0]);
  expect(borders.bottom).toBeGreaterThanOrEqual(255);
}

const pages = [
  { name: "dashboard", path: "/", readyText: "管理员用量分析" },
  { name: "accounts", path: "/accounts/", readyText: "设计团队 · 主账号" },
  { name: "aggregate", path: "/aggregate-api/", readyText: "团队网关" },
  { name: "models", path: "/models/", readyText: "Reasoning Model" },
  { name: "settings", path: "/settings/", readyText: "基础设置" },
];

test.use({ locale: "zh-CN", timezoneId: "Asia/Shanghai", deviceScaleFactor: 1 });

// Existing shell-layout owns navigation resizing; checkbox-visibility owns
// component contrast/interaction. This smoke test catches cross-page layout
// regressions and supplies populated screenshots for a human design review.
for (const theme of ["grape", "dark-one"]) {
  test(`modern workspace visual review: ${theme}`, async ({ page }, testInfo) => {
    test.setTimeout(120_000);
    const pageErrors: string[] = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await mockVisualRuntime(page, theme);
    for (const width of [1440, 1100]) {
      await page.setViewportSize({ width, height: 1000 });
      const headerHeights: number[] = [];
      for (const sample of pages) {
        await test.step(`${sample.name} at ${width}px`, async () => {
          await page.goto(sample.path);
          const main = page.getByRole("main");
          await expect(main.getByText(sample.readyText, { exact: true })).toBeVisible();
          await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
          await expect(page.locator("html")).toHaveAttribute("data-appearance", "modern");
          await expect(main.locator('[data-slot="skeleton"]:visible')).toHaveCount(0);
          if (sample.name === "dashboard") {
            await expect(main.locator(".recharts-surface").first()).toBeVisible();
            await waitForCompleteChart(page);
          }
          if (sample.name === "accounts") await assertAccountToolbar(page);
          if (sample.name === "settings") await assertLineTab(page);
          if (sample.name === "aggregate") {
            for (const action of ["编辑聚合 API", "删除聚合 API"]) {
              const buttons = main.getByRole("button", { name: action, exact: true });
              await expect(buttons).toHaveCount(3);
              for (const button of await buttons.all()) await expect(button).toBeInViewport({ ratio: 1 });
            }
          }
          headerHeights.push(await page.locator("header.glass-header").evaluate((element) => element.getBoundingClientRect().height));
          expect(Math.max(...headerHeights) - Math.min(...headerHeights), "shared header height must remain consistent across pages").toBeLessThanOrEqual(1);
          // Tables may scroll internally; the workspace and document must fit.
          const overflow = await main.evaluate((element) => ({
            main: element.scrollWidth - element.clientWidth,
            document: document.documentElement.scrollWidth - window.innerWidth,
          }));
          expect(overflow.main, `${sample.name} main overflow at ${width}px`).toBeLessThanOrEqual(1);
          expect(overflow.document, `${sample.name} document overflow at ${width}px`).toBeLessThanOrEqual(1);
          expect(pageErrors).toEqual([]);
          await page.mouse.move(0, 0);
          const file = testInfo.outputPath(`${sample.name}-${theme}-${width}.png`);
          await page.screenshot({ path: file, animations: "disabled", scale: "css" });
          await testInfo.attach(`${sample.name}-${theme}-${width}`, { path: file, contentType: "image/png" });
        });
      }
    }
  });
}
