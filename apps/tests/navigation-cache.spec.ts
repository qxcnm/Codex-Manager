import { expect, test } from "@playwright/test";

const SETTINGS_SNAPSHOT = {
  updateAutoCheck: true,
  closeToTrayOnClose: false,
  closeToTraySupported: false,
  lowTransparency: false,
  lightweightModeOnCloseToTray: false,
  codexCliGuideDismissed: true,
  webAccessPasswordConfigured: false,
  locale: "zh-CN",
  localeOptions: ["zh-CN", "en"],
  serviceAddr: "localhost:48760",
  serviceListenMode: "loopback",
  serviceListenModeOptions: ["loopback", "all_interfaces"],
  routeStrategy: "ordered",
  routeStrategyOptions: ["ordered", "balanced"],
  freeAccountMaxModel: "auto",
  freeAccountMaxModelOptions: ["auto", "gpt-5"],
  modelForwardRules: "",
  accountMaxInflight: 1,
  gatewayOriginator: "codex-cli",
  gatewayOriginatorDefault: "codex-cli",
  gatewayUserAgentVersion: "1.0.0",
  gatewayUserAgentVersionDefault: "1.0.0",
  gatewayResidencyRequirement: "",
  gatewayResidencyRequirementOptions: ["", "us"],
  pluginMarketMode: "builtin",
  pluginMarketSourceUrl: "",
  upstreamProxyUrl: "",
  upstreamStreamTimeoutMs: 600000,
  sseKeepaliveIntervalMs: 15000,
  backgroundTasks: {
    usagePollingEnabled: true,
    usagePollIntervalSecs: 600,
    gatewayKeepaliveEnabled: true,
    gatewayKeepaliveIntervalSecs: 180,
    tokenRefreshPollingEnabled: true,
    tokenRefreshPollIntervalSecs: 60,
    usageRefreshWorkers: 4,
    httpWorkerFactor: 4,
    httpWorkerMin: 8,
    httpStreamWorkerFactor: 1,
    httpStreamWorkerMin: 2,
  },
  envOverrides: {},
  envOverrideCatalog: [],
  envOverrideReservedKeys: [],
  envOverrideUnsupportedKeys: [],
  theme: "tech",
  appearancePreset: "classic",
};

test.beforeEach(async ({ page }) => {
  await page.route("**/api/runtime**", async (route) => {
    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        mode: "web-gateway",
        rpcBaseUrl: "/api/rpc",
        canManageService: false,
        canSelfUpdate: false,
        canCloseToTray: false,
        canOpenLocalDir: false,
        canUseBrowserFileImport: true,
        canUseBrowserDownloadExport: true,
      }),
    });
  });

  await page.route("**/api/rpc", async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const id = payload?.id ?? 1;

    const resultByMethod = {
      "appSettings/get": SETTINGS_SNAPSHOT,
      initialize: {
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "C:/Users/Test/.codex",
        platformFamily: "windows",
        platformOs: "windows",
      },
      "accountManager/session/current": {
        mode: "none",
        currentUser: null,
        role: "system_admin",
        permissions: [],
        distributionEnabled: false,
      },
      "aggregateApi/list": { items: [] },
      "gateway/concurrencyRecommendation/get": {
        usageRefreshWorkers: 4,
        httpWorkerFactor: 4,
        httpWorkerMin: 8,
        httpStreamWorkerFactor: 1,
        httpStreamWorkerMin: 2,
        accountMaxInflight: 1,
      },
    } satisfies Record<string, unknown>;

    if (!(method in resultByMethod)) {
      await route.fulfill({
        status: 500,
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          error: {
            code: -32000,
            message: `Unhandled RPC method in test: ${method}`,
          },
        }),
      });
      return;
    }

    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        result: resultByMethod[method as keyof typeof resultByMethod],
      }),
    });
  });
});

test("revisiting visited routes stays responsive after idle time", async ({ page }) => {
  const aggregateHeader = page
    .getByRole("columnheader", { name: "供应商 / URL" })
    .last();
  const settingsSectionTitle = page.getByText("基础设置", { exact: true }).last();

  await page.goto("/aggregate-api/");

  await expect(aggregateHeader).toBeVisible();
  await expect(page.getByText("正在准备环境")).not.toBeVisible();

  await page.getByRole("link", { name: "设置" }).click();
  await expect(page).toHaveURL(/\/settings\/$/);
  await expect(settingsSectionTitle).toBeVisible({ timeout: 5_000 });

  await page.getByRole("link", { name: /聚合\s*API/ }).click();
  await expect(page).toHaveURL(/\/aggregate-api\/$/);
  await expect(aggregateHeader).toBeVisible({ timeout: 5_000 });

  await page.waitForTimeout(1_200);

  const revisitSettingsStartedAt = Date.now();
  await page.getByRole("link", { name: "设置" }).click();
  await expect(page).toHaveURL(/\/settings\/$/);
  await expect(settingsSectionTitle).toBeVisible({ timeout: 1_500 });
  expect(Date.now() - revisitSettingsStartedAt).toBeLessThan(1_500);

  await page.waitForTimeout(1_200);

  const revisitAggregateStartedAt = Date.now();
  await page.getByRole("link", { name: /聚合\s*API/ }).click();
  await expect(page).toHaveURL(/\/aggregate-api\/$/);
  await expect(aggregateHeader).toBeVisible({ timeout: 1_500 });
  expect(Date.now() - revisitAggregateStartedAt).toBeLessThan(1_500);
});
