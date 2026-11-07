import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import type { DashboardAdminUsageSummary, DashboardTokenUsage, DashboardUsageSeriesPoint } from "../src/types/dashboard";

type Scenario = "empty" | "models" | "requests-only";
const fixtureTime = new Date("2026-11-07T10:00:00+08:00");
const fixtureEnd = new Date("2026-11-08T00:00:00+08:00").getTime() / 1000;
const modelNames = ["fixture-reasoning", "fixture-fast", "fixture-mini"];

function usage(tokens = 0, requests = 0): DashboardTokenUsage {
  return {
    inputTokens: tokens * 0.8, cachedInputTokens: tokens * 0.2,
    outputTokens: tokens * 0.2, reasoningOutputTokens: tokens * 0.1,
    totalTokens: tokens, estimatedCostUsd: tokens / 1_000_000,
    requestCount: requests, successCount: requests, errorCount: 0,
  };
}

function sumUsage(items: DashboardTokenUsage[]): DashboardTokenUsage {
  return items.reduce((total, item) => {
    for (const key of Object.keys(total) as (keyof DashboardTokenUsage)[]) total[key] += item[key];
    return total;
  }, usage());
}

function summaryFixture(scenario: Scenario, start: number, end: number, bucket: number): DashboardAdminUsageSummary {
  const hours = Array.from({ length: Math.ceil((end - start) / 3600) }, (_, hour) => {
    const values = modelNames.map((_, model) => scenario === "models"
      ? usage((10 + hour % 17) * (3 - model) * 100, (1 + hour % 7) * (3 - model))
      : usage(0, scenario === "requests-only" && model === 0 ? 1 + hour % 4 : 0));
    return { bucketStartTs: start + hour * 3600, bucketEndTs: start + (hour + 1) * 3600, values };
  });
  const points = (seconds: number, model?: number): DashboardUsageSeriesPoint[] => {
    const groupSize = seconds / 3600;
    return Array.from({ length: Math.ceil(hours.length / groupSize) }, (_, index) => {
      const group = hours.slice(index * groupSize, (index + 1) * groupSize);
      return {
        bucketStartTs: group[0].bucketStartTs, bucketEndTs: group[group.length - 1].bucketEndTs,
        usage: sumUsage(group.flatMap((hour) => model == null ? hour.values : [hour.values[model]])),
      };
    });
  };
  const daily = points(86400);
  return {
    rangeStartTs: start, rangeEndTs: end, todayStartTs: end - 86400, todayEndTs: end,
    todayUsage: daily[daily.length - 1].usage,
    dailyUsage: daily.map((point) => ({ dayStartTs: point.bucketStartTs, dayEndTs: point.bucketEndTs, usage: point.usage })),
    seriesBucketSeconds: bucket, seriesUsage: points(bucket),
    modelUsage: scenario === "models" ? modelNames.map((model, index) => ({
      model, points: points(bucket, index), usage: sumUsage(hours.map((hour) => hour.values[index])),
    })) : [],
    users: [], openaiAccounts: [], aggregateApis: [],
  };
}

async function mockUsageRuntime(page: Page, theme: string, scenario: Scenario) {
  const queries: { includeSeries: boolean; bucket: number; points: number }[] = [];
  await page.clock.setFixedTime(fixtureTime);
  // Only local assets and these fixtures are allowed; no live service data enters screenshots.
  await page.route("**/*", (route) => ["localhost", "127.0.0.1", "::1"].includes(new URL(route.request().url()).hostname)
    ? route.continue() : route.abort());
  await page.route("**/api/**", (route) => route.abort());
  await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
    canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
    canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
  } }));
  await page.route("**/api/rpc**", (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    let result: unknown = {};
    if (method === "dashboard/adminUsageSummary") {
      const summary = summaryFixture(scenario, Number(params.startTs) || fixtureEnd - 7 * 86400,
        Number(params.endTs) || fixtureEnd, Number(params.seriesBucketSeconds) || 3600);
      queries.push({ includeSeries: params.includeSeries, bucket: summary.seriesBucketSeconds, points: summary.seriesUsage.length });
      result = summary;
    } else {
      const results: Record<string, unknown> = {
        "appSettings/get": { serviceAddr: "localhost:48760", theme, appearancePreset: "modern", locale: "zh-CN",
          lowTransparency: false, codexCliGuideDismissed: true, webAuthMode: "none",
          accountsAutoRefreshEnabled: false, envOverrides: {}, envOverrideCatalog: [] },
        initialize: { version: "0.7.1", userAgent: "codex_cli_rs/fixture", codexHome: "C:/UsageFixture/.codex",
          platformFamily: "windows", platformOs: "windows" },
        "accountManager/session/current": { mode: "none", currentUser: null, role: "system_admin",
          permissions: [], distributionEnabled: false },
        "account/list": { items: [], total: 0, page: 1, pageSize: 20 }, "account/usage/list": [],
        "aggregateApi/list": { items: [] }, "modelDiscovery/list": { items: [] },
        "codexProfile/get": { mode: "gateway", managedCatalogActive: true, warnings: [], profileWritable: true },
        "codexProfile/listCandidates": { accounts: [], apiKeys: [] },
        "startup/snapshot": { accounts: [], usageSnapshots: [], apiKeys: [], requestLogs: [],
          accountSummary: { accountCount: 0, availableCount: 0, lowQuotaCount: 0 },
          requestLogTodaySummary: { todayTokens: 0, cachedInputTokens: 0, reasoningOutputTokens: 0, estimatedCost: 0 } },
      };
      result = results[method] ?? {};
    }
    return route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });
  return queries;
}

async function openUsage(page: Page, theme: string) {
  await page.setViewportSize({ width: 1100, height: 1000 });
  await page.goto("/");
  const card = page.locator("#admin-usage-analytics");
  await expect(card.getByText("管理员用量分析", { exact: true })).toBeVisible();
  await expect(card.getByRole("combobox").locator('[data-slot="select-value"]')).toHaveText("最近 7 天");
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
  await expect(card.locator('[data-slot="skeleton"]')).toHaveCount(0);
  await expect(card.getByRole("button", { name: "按小时", exact: true })).toHaveAttribute("aria-pressed", "true");
  return card;
}

async function assertLayout(page: Page, card: Locator) {
  const layout = await card.evaluate((element) => {
    const grid = element.querySelector<HTMLElement>(".usage-summary-grid")!;
    const children = Array.from(grid.children, (child) => child.getBoundingClientRect());
    const plot = element.querySelector('[data-slot="chart"], [data-slot="usage-chart-empty"]')!.getBoundingClientRect();
    const main = document.querySelector("main")!;
    return { count: children.length, tops: children.map((child) => Math.round(child.top)),
      gridBottom: grid.getBoundingClientRect().bottom, plotTop: plot.top,
      card: element.scrollWidth - element.clientWidth, main: main.scrollWidth - main.clientWidth,
      document: document.documentElement.scrollWidth - window.innerWidth };
  });
  expect(layout.count).toBe(4);
  expect(layout.gridBottom).toBeLessThanOrEqual(layout.plotTop);
  expect(layout.card, "usage card must fit the viewport").toBeLessThanOrEqual(1);
  expect(layout.main, "dashboard must not scroll horizontally").toBeLessThanOrEqual(1);
  expect(layout.document).toBeLessThanOrEqual(1);
  expect(new Set(layout.tops).size).toBe(page.viewportSize()!.width === 1100 ? 1 : 2);
  const chart = card.locator('[data-slot="chart"]');
  if (await chart.count()) {
    const bounds = await chart.boundingBox();
    const brush = await card.locator(".recharts-brush").boundingBox();
    expect(bounds!.height).toBeLessThanOrEqual(280);
    expect(brush!.x).toBeGreaterThanOrEqual(bounds!.x);
    expect(brush!.x + brush!.width).toBeLessThanOrEqual(bounds!.x + bounds!.width);
    expect(brush!.y + brush!.height).toBeLessThanOrEqual(bounds!.y + bounds!.height);
  }
}

async function waitForLines(card: Locator, count: number) {
  const lines = card.locator(".recharts-line-curve");
  await expect(lines).toHaveCount(count);
  await expect.poll(() => lines.evaluateAll((elements) => elements.every((element) => {
    const path = element as SVGPathElement;
    const bounds = path.getBBox();
    if (bounds.width <= 50 || path.getTotalLength() <= 50) return false;
    for (let ancestor = path.parentElement; ancestor; ancestor = ancestor.parentElement) {
      const clipId = ancestor.getAttribute("clip-path")?.match(/url\(#([^)]*)\)/)?.[1];
      const clip = clipId ? document.getElementById(clipId)?.querySelector("rect") : null;
      if (clip) {
        const clipBounds = clip.getBBox();
        if (clipBounds.x + clipBounds.width < bounds.x + bounds.width - 1) return false;
      }
    }
    return true;
  }))).toBe(true);
}

async function screenshot(page: Page, card: Locator, testInfo: TestInfo, name: string) {
  await page.mouse.move(0, 0);
  const path = testInfo.outputPath(`${name}.png`);
  await card.screenshot({ path, animations: "disabled", scale: "css" });
  await testInfo.attach(name, { path, contentType: "image/png" });
}

test.use({ locale: "zh-CN", timezoneId: "Asia/Shanghai", deviceScaleFactor: 1 });

for (const theme of ["grape", "dark-one"]) {
  test(`empty hourly usage stays compact: ${theme}`, async ({ page }, testInfo) => {
    const queries = await mockUsageRuntime(page, theme, "empty");
    const card = await openUsage(page, theme);
    const empty = card.locator('[data-slot="usage-chart-empty"]');
    await expect(empty.getByText("所选区间暂无请求", { exact: true })).toBeVisible();
    expect(queries.some((query) => query.includeSeries && query.bucket === 3600 && query.points === 168)).toBe(true);
    await expect(card.locator(".recharts-cartesian-axis, .recharts-brush, [data-slot=chart]")).toHaveCount(0);
    expect(await empty.evaluate((element) => element.getBoundingClientRect().height)).toBeLessThanOrEqual(224);
    expect(await card.evaluate((element) => element.getBoundingClientRect().height)).toBeLessThanOrEqual(480);
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await assertLayout(page, card);
      await screenshot(page, card, testInfo, `usage-empty-${theme}-${width}`);
    }
  });

  test(`hourly models support filtering, metrics and zoom: ${theme}`, async ({ page }, testInfo) => {
    const queries = await mockUsageRuntime(page, theme, "models");
    const card = await openUsage(page, theme);
    await expect(card.locator('[data-slot="usage-chart-empty"]')).toHaveCount(0);
    await waitForLines(card, 3);
    await expect(card.locator(".recharts-brush")).toBeVisible();
    const model = card.getByRole("button", { name: /^fixture-fast:/ });
    await model.click();
    await expect(model).toHaveAttribute("aria-pressed", "false");
    await waitForLines(card, 2);
    await card.getByRole("button", { name: "恢复默认", exact: true }).click();
    await expect(model).toHaveAttribute("aria-pressed", "true");
    const tokenLabel = await model.getAttribute("aria-label");
    const requests = card.getByRole("button", { name: "请求数", exact: true });
    await requests.click();
    await expect(requests).toHaveAttribute("aria-pressed", "true");
    await expect(model).not.toHaveAttribute("aria-label", tokenLabel!);
    await waitForLines(card, 3);
    const range = card.locator("#usage-chart-visible-range");
    const fullRange = await range.innerText();
    const chart = card.locator('[data-slot="chart"]');
    await chart.hover();
    await page.mouse.wheel(0, -300);
    const reset = card.getByRole("button", { name: "重置缩放", exact: true });
    await expect(reset).toBeVisible();
    await expect(range).not.toHaveText(fullRange);
    await reset.click();
    await expect(range).toHaveText(fullRange);
    await expect(reset).toHaveCount(0);
    await card.getByRole("button", { name: "按天", exact: true }).click();
    await expect.poll(() => queries.some((query) => query.bucket === 86400 && query.points === 7)).toBe(true);
    await card.getByRole("button", { name: "按小时", exact: true }).click();
    await expect(card.getByRole("button", { name: "按小时", exact: true })).toHaveAttribute("aria-pressed", "true");
    await expect(card.getByText("正在更新曲线", { exact: true })).toHaveCount(0);
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await waitForLines(card, 3);
      await assertLayout(page, card);
      await screenshot(page, card, testInfo, `usage-models-${theme}-${width}`);
    }
  });

  test(`requests without tokens or model breakdown retain a total curve: ${theme}`, async ({ page }, testInfo) => {
    await mockUsageRuntime(page, theme, "requests-only");
    const card = await openUsage(page, theme);
    await expect(card.locator('[data-slot="usage-chart-empty"]')).toHaveCount(0);
    await expect(card.getByRole("button", { name: "全部模型", exact: true })).toHaveCount(0);
    await waitForLines(card, 1);
    await expect(card.locator(".recharts-brush")).toBeVisible();
    const requests = card.getByRole("button", { name: "请求数", exact: true });
    await requests.click();
    await expect(requests).toHaveAttribute("aria-pressed", "true");
    await expect.poll(() => card.locator(".recharts-line-curve").evaluate((element) => (element as SVGPathElement).getBBox().height)).toBeGreaterThan(10);
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await waitForLines(card, 1);
      await assertLayout(page, card);
      await screenshot(page, card, testInfo, `usage-requests-only-${theme}-${width}`);
    }
  });
}
