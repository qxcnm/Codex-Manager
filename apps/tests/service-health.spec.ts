import { expect, test, type Page } from "@playwright/test";
import type {
  ServiceHealthReport,
  ServiceHealthReportInput,
  ServiceHealthSummary,
} from "../src/types/service-health";

interface HealthFixture {
  reports: Array<ServiceHealthReport & { owner: string }>;
  submitted: ServiceHealthReportInput[];
  resolved: string[];
  methods: string[];
  failSummary: boolean;
  samples: number;
}

function fixture(): HealthFixture {
  return { reports: [], submitted: [], resolved: [], methods: [], failSummary: false, samples: 0 };
}

function summary(state: HealthFixture, owner: string): ServiceHealthSummary {
  const now = Math.floor(Date.now() / 1000);
  return {
    source: "local", coverage: "this_deployment", scope: "insufficient_data",
    windowStart: now - 3600, windowEnd: now, updatedAt: now,
    sampled: false, sampleLimit: 2000, sampleCount: state.samples,
    sourceCount: state.samples > 0 ? 1 : 0,
    regionCount: new Set(state.reports.map((report) => report.region)).size,
    providers: (["codex", "claude"] as const).map((provider) => {
      const reports = state.reports.filter((report) => report.provider === provider);
      const samples = provider === "codex" ? state.samples : 0;
      return {
        provider, status: samples ? "healthy" : "unknown", sampleCount: samples,
        successCount: samples, rateLimitedCount: 0, upstreamErrorCount: 0,
        localErrorCount: 0, networkErrorCount: 0, unknownErrorCount: 0,
        availabilityPercent: samples ? 100 : null,
        avgFirstResponseMs: samples ? 1300 : null,
        p95FirstResponseMs: samples ? 1800 : null,
        activeReportCount: reports.length,
        reporterCount: new Set(reports.map((report) => report.owner)).size,
        sourceCount: samples ? 1 : 0,
        regions: [...new Set(reports.map((report) => report.region))],
      };
    }),
    myReports: state.reports.filter((report) => report.owner === owner).map((report) => ({
      id: report.id, provider: report.provider, symptom: report.symptom, region: report.region,
      createdAt: report.createdAt, updatedAt: report.updatedAt, expiresAt: report.expiresAt,
    })),
  };
}

async function mockHealth(
  page: Page,
  state: HealthFixture,
  owner = "member-a",
  role: "member" | "admin" | "system_admin" = "member",
  theme = "tech",
) {
  await page.route("**/*", (route) => ["localhost", "127.0.0.1", "::1"].includes(new URL(route.request().url()).hostname)
    ? route.continue() : route.abort());
  await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
    canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
    canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
  } }));
  await page.route("**/api/rpc**", async (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    state.methods.push(method);
    const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "appSettings/get") return ok({
      locale: "zh-CN", theme, lowTransparency: theme === "dark", serviceAddr: "localhost:48760",
      codexCliGuideDismissed: true, webAccessPasswordConfigured: false,
    });
    if (method === "initialize") return ok({ userAgent: "codex_cli_rs/0.164.0", codexHome: "C:/Test", platformFamily: "windows", platformOs: "windows" });
    if (method === "accountManager/session/current") return ok({
      mode: "multi_user", currentUser: { id: owner, username: owner, role },
      role, permissions: role === "member" ? [] : ["system:admin"], distributionEnabled: true,
    });
    if (method === "serviceHealth/summary") {
      expect(params).toEqual({});
      if (state.failSummary) return route.fulfill({ json: {
        jsonrpc: "2.0", id, error: { code: -32000, message: "health fixture offline" },
      } });
      return ok(summary(state, owner));
    }
    if (method === "serviceHealth/report") {
      expect(Object.keys(params).sort()).toEqual(["provider", "region", "symptom"]);
      const input = { provider: params.provider, symptom: params.symptom, region: params.region } as ServiceHealthReportInput;
      state.submitted.push(input);
      const now = Math.floor(Date.now() / 1000);
      const report = { ...input, id: `report-${owner}`, createdAt: now, updatedAt: now, expiresAt: now + 1800 };
      state.reports = state.reports.filter((item) => !(item.owner === owner && item.provider === input.provider && item.region === input.region));
      state.reports.push({ ...report, owner });
      return ok({ report });
    }
    if (method === "serviceHealth/resolve") {
      expect(Object.keys(params)).toEqual(["id"]);
      const ownReport = state.reports.find((report) => report.id === params.id && report.owner === owner);
      expect(ownReport, "members can only resolve their own report").toBeDefined();
      state.resolved.push(params.id);
      state.reports = state.reports.filter((report) => report.id !== params.id);
      return ok({ ok: true });
    }
    return ok({});
  });
}

function expectOnlyLocalHealthMethods(state: HealthFixture) {
  expect(state.methods.filter((method) => method.startsWith("serviceHealth/") && ![
    "serviceHealth/summary", "serviceHealth/report", "serviceHealth/resolve",
  ].includes(method))).toEqual([]);
}

for (const width of [390, 1100]) {
  test(`member sees unknown status without samples and fits a ${width}px viewport`, async ({ page }) => {
    const state = fixture();
    await page.setViewportSize({ width, height: 900 });
    await mockHealth(page, state, "member-a", "member", width === 390 ? "dark" : "tech");
    await page.goto("/service-health/");
    await expect(page.getByRole("heading", { name: "服务健康", exact: true, level: 2 })).toBeVisible();
    await expect(page.getByText("最近 60 分钟暂无请求样本", { exact: false })).toBeVisible();
    await expect(page.getByTestId("health-provider-codex")).toBeVisible();
    await expect(page.getByTestId("health-provider-claude")).toBeVisible();
    await expect(page.getByRole("button", { name: "报告异常", exact: true })).toBeEnabled();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
    expectOnlyLocalHealthMethods(state);
    await page.screenshot({ path: test.info().outputPath(`service-health-${width}.png`), fullPage: true, animations: "disabled" });
  });
}

for (const role of ["admin", "system_admin"] as const) {
  test(`${role} can read the shared health summary without additional health privileges`, async ({ page }) => {
    const state = fixture();
    state.samples = 12;
    await mockHealth(page, state, role, role);
    await page.goto("/service-health/");
    await expect(page.getByRole("heading", { name: "服务健康", exact: true, level: 2 })).toBeVisible();
    const card = page.getByTestId("health-provider-codex");
    await expect(card.getByText("100%", { exact: true })).toBeVisible();
    await expect(card.getByText("样本正常", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "报告异常", exact: true })).toBeEnabled();
    expect([...new Set(state.methods.filter((method) => method.startsWith("serviceHealth/")))]).toEqual(["serviceHealth/summary"]);
    expectOnlyLocalHealthMethods(state);
  });
}

test("members report and resolve their own incident while other members only see shared counts", async ({ page, context }) => {
  const state = fixture();
  await mockHealth(page, state);
  await page.goto("/service-health/");
  await page.getByRole("button", { name: "报告异常", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.locator("#health-provider").click();
  await page.getByRole("option", { name: "Claude", exact: true }).click();
  await dialog.locator("#health-symptom").click();
  await page.getByRole("option", { name: "响应缓慢", exact: true }).click();
  await dialog.locator("#health-region").click();
  await page.getByRole("option", { name: "亚太", exact: true }).click();
  await dialog.getByRole("button", { name: "提交报告", exact: true }).click();
  await expect(dialog).toBeHidden();
  expect(state.submitted).toEqual([{ provider: "claude", symptom: "slow", region: "apac" }]);
  await expect(page.getByRole("button", { name: "标记恢复", exact: true })).toBeVisible();
  const reportCount = page.getByTestId("health-provider-claude").getByText(/有效反馈/);
  await expect(reportCount).toContainText("有效反馈 1 条");

  const otherMember = await context.newPage();
  await mockHealth(otherMember, state, "member-b");
  await otherMember.goto("/service-health/");
  await expect(otherMember.getByTestId("health-provider-claude").getByText(/有效反馈/)).toContainText("有效反馈 1 条");
  await expect(otherMember.getByRole("button", { name: "标记恢复", exact: true })).toHaveCount(0);
  await expect(otherMember.locator("body")).not.toContainText("member-a");

  await page.getByRole("button", { name: "标记恢复", exact: true }).click();
  await expect.poll(() => state.resolved).toEqual(["report-member-a"]);
  await expect(page.getByRole("button", { name: "标记恢复", exact: true })).toHaveCount(0);
  await expect(reportCount).toContainText("有效反馈 0 条");
  await otherMember.getByRole("button", { name: "刷新", exact: true }).click();
  await expect(otherMember.getByTestId("health-provider-claude").getByText(/有效反馈/)).toContainText("有效反馈 0 条");
  expectOnlyLocalHealthMethods(state);
});

test("a failed summary is shown as an error and never as a healthy result", async ({ page }) => {
  const state = fixture();
  state.failSummary = true;
  await mockHealth(page, state);
  await page.goto("/service-health/");
  await expect(page.getByText("health fixture offline", { exact: false })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("health-provider-codex")).toHaveCount(0);
  await expect(page.getByTestId("health-provider-claude")).toHaveCount(0);
  expectOnlyLocalHealthMethods(state);
});
