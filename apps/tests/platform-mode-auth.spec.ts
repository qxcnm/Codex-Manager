import { expect, test, type Page } from "@playwright/test";

type JsonObject = Record<string, unknown>;

const SETTINGS_SNAPSHOT = {
  updateAutoCheck: false,
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
  freeAccountMaxModelOptions: ["auto", "gpt-6-sol"],
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
    usagePollingEnabled: false,
    usagePollIntervalSecs: 600,
    gatewayKeepaliveEnabled: false,
    gatewayKeepaliveIntervalSecs: 180,
    tokenRefreshPollingEnabled: false,
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

type MockState = {
  status: JsonObject;
  writes: Array<{ method: string; params: JsonObject }>;
  gatewayError: string | null;
  unhandledMethods: string[];
};

async function installProfileRuntime(page: Page): Promise<MockState> {
  const state: MockState = {
    // The missing field represents status responses from existing installations.
    status: {
      codexHome: "/fixture/.codex",
      authPath: "/fixture/.codex/auth.json",
      configPath: "/fixture/.codex/config.toml",
      mode: "gateway",
      selectedAccountId: null,
      selectedApiKeyId: "platform-fixture",
      selectedAggregateApiId: null,
      gatewayBaseUrl: "http://localhost:48760/v1",
      supportsWebsockets: false,
      profileWritable: true,
      hasBackup: true,
      warnings: [],
    },
    writes: [],
    gatewayError: null,
    unhandledMethods: [],
  };

  await page.route("**/api/runtime*", async (route) => {
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

  await page.route("**/api/rpc*", async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const id = payload?.id ?? 1;
    const params = (payload?.params ?? {}) as JsonObject;
    const ok = (result: unknown) => route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({ jsonrpc: "2.0", id, result }),
    });
    const fail = (message: string) => route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({ jsonrpc: "2.0", id, error: { code: -32000, message } }),
    });

    switch (method) {
      case "appSettings/get":
        await ok(SETTINGS_SNAPSHOT);
        return;
      case "initialize":
        await ok({ version: "0.7.5", userAgent: "codex_cli_rs/fixture", codexHome: "/fixture/.codex", platformFamily: "linux", platformOs: "linux" });
        return;
      case "accountManager/session/current":
        await ok({ mode: "none", currentUser: null, role: "system_admin", permissions: [], distributionEnabled: false });
        return;
      case "gateway/concurrencyRecommendation/get":
        await ok({ usageRefreshWorkers: 4, httpWorkerFactor: 4, httpWorkerMin: 8, httpStreamWorkerFactor: 1, httpStreamWorkerMin: 2, accountMaxInflight: 1 });
        return;
      case "codexProfile/get":
        await ok(state.status);
        return;
      case "codexProfile/setConfig": {
        state.writes.push({ method, params: structuredClone(params) });
        const codexHome = String(params.codexHome);
        state.status = { ...state.status, codexHome, authPath: `${codexHome}/auth.json`, configPath: `${codexHome}/config.toml`, gatewayAuthMode: "preserve" };
        await ok(state.status);
        return;
      }
      case "codexProfile/listCandidates":
        await ok({
          accounts: [{ id: "account-fixture", label: "Fixture Account", status: "active" }],
          apiKeys: [{ id: "platform-fixture", name: "Fixture Platform Key", status: "active", rotationStrategy: "aggregate_api_rotation", catalogSource: "managed" }],
          aggregateApis: [],
        });
        return;
      case "codexProfile/applyGateway":
        state.writes.push({ method, params: structuredClone(params) });
        if (state.gatewayError) {
          await fail(state.gatewayError);
          return;
        }
        state.status = { ...state.status, mode: "gateway", gatewayAuthMode: params.authMode };
        await ok(state.status);
        return;
      case "codexProfile/applyDirectAccount":
        state.writes.push({ method, params: structuredClone(params) });
        state.status = { ...state.status, mode: "direct_account", gatewayAuthMode: "preserve", selectedAccountId: "account-fixture", selectedApiKeyId: null };
        await ok(state.status);
        return;
      case "codexProfile/restore":
        state.writes.push({ method, params: structuredClone(params) });
        state.status = { ...state.status, mode: "unmanaged", gatewayAuthMode: "preserve", selectedApiKeyId: null, hasBackup: false };
        await ok(state.status);
        return;
      default:
        state.unhandledMethods.push(method);
        await fail(`Unhandled RPC method in test: ${method}`);
    }
  });

  return state;
}

test("gateway auth stays a draft until apply and persists through reload", async ({ page }, testInfo) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  const authSwitch = page.getByRole("switch", { name: "纯 API Key 模式", exact: true });
  const apply = page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true });
  await expect(apply).toBeEnabled();
  await expect(authSwitch).not.toBeChecked();

  await authSwitch.click();
  await expect(authSwitch).toBeChecked();
  expect(state.writes).toEqual([]);
  await apply.click();
  await expect.poll(() => state.writes.length).toBe(1);
  expect(state.writes[0]).toMatchObject({
    method: "codexProfile/applyGateway",
    params: { apiKeyId: "platform-fixture", authMode: "apikey", reloadAfterSwitch: false },
  });
  await expect(apply).toBeEnabled();
  await page.reload();
  await expect(authSwitch).toBeChecked();
  await page.locator('[data-slot="card"]').filter({ has: authSwitch }).screenshot({ path: testInfo.outputPath("gateway-apikey-desktop.png") });
  await page.setViewportSize({ width: 390, height: 844 });
  await authSwitch.scrollIntoViewIfNeeded();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("gateway-apikey-mobile.png"), fullPage: true });
  await page.setViewportSize({ width: 1280, height: 720 });

  await authSwitch.click();
  await expect(authSwitch).not.toBeChecked();
  expect(state.writes).toHaveLength(1);
  await apply.click();
  await expect.poll(() => state.writes.length).toBe(2);
  expect(state.writes[1].params.authMode).toBe("preserve");
  await expect(apply).toBeEnabled();
  await page.reload();
  await expect(authSwitch).not.toBeChecked();
  expect(state.unhandledMethods).toEqual([]);
});

test("failed gateway apply retains the auth draft and reports the error", async ({ page }) => {
  const state = await installProfileRuntime(page);
  state.gatewayError = "fixture cannot write profile";
  await page.goto("/platform-mode/");
  const authSwitch = page.getByRole("switch", { name: "纯 API Key 模式", exact: true });
  const apply = page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true });
  await expect(apply).toBeEnabled();
  await authSwitch.click();
  await apply.click();
  await expect(page.getByText("切换失败: fixture cannot write profile", { exact: true })).toBeVisible();
  await expect(authSwitch).toBeChecked();
  await expect(page.getByText("已切换为通过 CodexManager", { exact: true })).toHaveCount(0);
  expect(state.status.gatewayAuthMode).toBeUndefined();
  expect(state.writes).toHaveLength(1);
  expect(state.unhandledMethods).toEqual([]);
});

test("switching to an account clears an unapplied auth draft", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  const authSwitch = page.getByRole("switch", { name: "纯 API Key 模式", exact: true });
  const direct = page.getByRole("button", { name: "切换为直接连接 OpenAI", exact: true });
  await expect(direct).toBeEnabled();
  await authSwitch.click();
  await expect(authSwitch).toBeChecked();
  await direct.click();
  await expect(authSwitch).not.toBeChecked();
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0].method).toBe("codexProfile/applyDirectAccount");
  expect(state.unhandledMethods).toEqual([]);
});

test("restoring the original profile clears an unapplied auth draft", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  const authSwitch = page.getByRole("switch", { name: "纯 API Key 模式", exact: true });
  await expect(authSwitch).toBeEnabled();
  await authSwitch.click();
  await expect(authSwitch).toBeChecked();
  await page.getByText("高级与恢复", { exact: true }).click();
  await page.getByRole("button", { name: "恢复接管前配置", exact: true }).click();
  await expect(authSwitch).not.toBeChecked();
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0].method).toBe("codexProfile/restore");
  expect(state.unhandledMethods).toEqual([]);
});

test("saving another profile directory clears the previous profile auth draft", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  const authSwitch = page.getByRole("switch", { name: "纯 API Key 模式", exact: true });
  await expect(authSwitch).toBeEnabled();
  await authSwitch.click();
  await expect(authSwitch).toBeChecked();
  await page.getByText("高级与恢复", { exact: true }).click();
  const profilePath = page.getByRole("textbox", { name: "Codex profile 目录", exact: true });
  await profilePath.fill("/fixture/other-codex");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByText("Codex profile 路径已保存", { exact: true })).toBeVisible();
  await expect(profilePath).toHaveValue("/fixture/other-codex");
  await expect(authSwitch).not.toBeChecked();
  expect(state.writes).toHaveLength(1);
  expect(state.writes[0]).toMatchObject({
    method: "codexProfile/setConfig", params: { codexHome: "/fixture/other-codex" },
  });
  expect(state.unhandledMethods).toEqual([]);
});
