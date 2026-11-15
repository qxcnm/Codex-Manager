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
  gatewayApplyGate: Promise<void> | null;
  candidates: { accounts: JsonObject[]; apiKeys: JsonObject[]; aggregateApis: JsonObject[] };
  candidateReads: number;
  unhandledMethods: string[];
};

async function installProfileRuntime(
  page: Page,
  options: { runtimeMode?: "web-gateway" | "desktop-tauri"; serviceAddr?: string } = {},
): Promise<MockState> {
  const settings = { ...SETTINGS_SNAPSHOT, serviceAddr: options.serviceAddr ?? SETTINGS_SNAPSHOT.serviceAddr };
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
    gatewayApplyGate: null,
    candidates: {
      accounts: [{ id: "account-fixture", label: "Fixture Account", status: "active" }],
      apiKeys: [{ id: "platform-fixture", name: "Fixture Platform Key", status: "active", rotationStrategy: "aggregate_api_rotation", catalogSource: "managed" }],
      aggregateApis: [{ id: "aggregate-fixture", label: "Fixture Aggregate", providerType: "codex", baseUrl: "https://fixture.example/v1", status: "active" }],
    },
    candidateReads: 0,
    unhandledMethods: [],
  };

  await page.route("**/api/runtime*", async (route) => {
    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        mode: options.runtimeMode ?? "web-gateway",
        rpcBaseUrl: "/api/rpc",
        canManageService: options.runtimeMode === "desktop-tauri",
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
        await ok(settings);
        return;
      case "appSettings/set":
        Object.assign(settings, params);
        await ok(settings);
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
        state.candidateReads += 1;
        await ok(state.candidates);
        return;
      case "codexProfile/applyGateway":
        state.writes.push({ method, params: structuredClone(params) });
        await state.gatewayApplyGate;
        if (state.gatewayError) {
          await fail(state.gatewayError);
          return;
        }
        state.status = { ...state.status, mode: "gateway", gatewayAuthMode: params.authMode, gatewayBaseUrl: params.baseUrl, supportsWebsockets: params.supportsWebsockets };
        await ok(state.status);
        return;
      case "codexProfile/applyDirectAccount":
        state.writes.push({ method, params: structuredClone(params) });
        state.status = { ...state.status, mode: "direct_account", gatewayAuthMode: "preserve", selectedAccountId: "account-fixture", selectedApiKeyId: null };
        await ok(state.status);
        return;
      case "codexProfile/applyDirectAggregate":
        state.writes.push({ method, params: structuredClone(params) });
        state.status = { ...state.status, mode: "direct_aggregate", gatewayAuthMode: "preserve", selectedAggregateApiId: params.aggregateApiId, selectedApiKeyId: null };
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

for (const runtimeMode of ["desktop-tauri", "web-gateway"] as const) {
  test(`${runtimeMode} gateway uses the connected endpoint instead of the old profile URL`, async ({ page }) => {
    const state = await installProfileRuntime(page, { runtimeMode, serviceAddr: "localhost:48762" });
    state.status.gatewayBaseUrl = "https://old-profile.example/v1";
    await page.goto("/platform-mode/");
    const gateway = page.locator('[data-slot="card"]').filter({ has: page.getByRole("switch", { name: "纯 API Key 模式", exact: true }) });
    const expectedUrl = runtimeMode === "desktop-tauri" ? "http://localhost:48762/v1" : `${new URL(page.url()).origin}/v1`;
    await expect(gateway).toContainText(expectedUrl);
    await expect(gateway).not.toContainText("old-profile.example");
    await gateway.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
    await expect(dialog).toContainText(expectedUrl);
    expect(state.writes).toEqual([]);
    await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect(state.writes).toMatchObject([{
      method: "codexProfile/applyGateway", params: { baseUrl: expectedUrl },
    }]);
    expect(state.unhandledMethods).toEqual([]);
  });
}

test("manual gateway override stays on its endpoint and changing the port follows the new endpoint", async ({ page }) => {
  const state = await installProfileRuntime(page, { runtimeMode: "desktop-tauri", serviceAddr: "localhost:48762" });
  await page.goto("/platform-mode/");
  await page.getByText("高级与恢复", { exact: true }).click();
  const gatewayInput = page.getByRole("textbox", { name: "OpenAI gateway base_url", exact: true });
  await expect(gatewayInput).toHaveValue("http://localhost:48762/v1");
  await gatewayInput.fill("https://custom-gateway.example/v1");
  const apply = page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true });
  await apply.click();
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  await expect(dialog).toContainText("https://custom-gateway.example/v1");
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toMatchObject([{
    method: "codexProfile/applyGateway", params: { baseUrl: "https://custom-gateway.example/v1" },
  }]);
  const port = page.getByPlaceholder("48760", { exact: true });
  await port.fill("49999");
  await port.press("Tab");
  await expect(gatewayInput).toHaveValue("http://localhost:49999/v1");
  await apply.click();
  await expect(dialog).toContainText("http://localhost:49999/v1");
  await expect(dialog).not.toContainText("custom-gateway.example");
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toHaveLength(2);
  expect(state.writes[1]).toMatchObject({ method: "codexProfile/applyGateway", params: { baseUrl: "http://localhost:49999/v1" } });
  expect(state.unhandledMethods).toEqual([]);
});

test("connection details open on demand and cancellation never writes configuration", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  const choices = [
    { button: "切换为直接连接 OpenAI", target: "Fixture Account", detail: "应用后会更新 auth.json，切换为所选 OpenAI 账号。" },
    { button: "切换为直连聚合 API", target: "Fixture Aggregate", detail: "仅更新 config.toml，聚合 API 密钥也保存在该文件；保留现有 auth.json。" },
    { button: "重新应用 CodexManager 接入", target: "Fixture Platform Key", detail: "通过 CodexManager 本地网关转发 Codex CLI 请求；请求日志、Token、费用估算和仪表盘统计可用。" },
  ];
  for (const choice of choices) {
    const trigger = page.getByRole("button", { name: choice.button, exact: true });
    await expect(trigger).toBeEnabled();
    await expect(page.getByText(choice.detail, { exact: true })).toHaveCount(0);
    await trigger.click();
    const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
    await expect(dialog).toContainText(choice.target);
    await expect(dialog.getByText(choice.detail, { exact: true })).toBeVisible();
    await expect(dialog.getByText("应用后", { exact: true })).toBeVisible();
    expect(state.writes).toEqual([]);
    const cancel = dialog.getByRole("button", { name: "取消", exact: true });
    await expect(cancel).toBeFocused();
    await cancel.click();
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    expect(state.writes).toEqual([]);
  }
  expect(state.unhandledMethods).toEqual([]);
});

test("confirming aggregate connection applies the previewed target once", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  await page.getByRole("button", { name: "切换为直连聚合 API", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  await expect(dialog).toContainText("Fixture Aggregate");
  expect(state.writes).toEqual([]);
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toMatchObject([{
    method: "codexProfile/applyDirectAggregate",
    params: { aggregateApiId: "aggregate-fixture", codexHome: "/fixture/.codex", reloadAfterSwitch: false },
  }]);
  await expect(page.getByRole("button", { name: "重新应用直连聚合 API", exact: true })).toBeEnabled();
  expect(state.unhandledMethods).toEqual([]);
});

test("background candidate refresh cannot replace the target awaiting confirmation", async ({ page }) => {
  const state = await installProfileRuntime(page);
  await page.goto("/platform-mode/");
  await page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  await expect(dialog).toContainText("Fixture Platform Key");
  const readsBeforeRefresh = state.candidateReads;
  state.candidates.apiKeys = [{ id: "another-platform", name: "Another Platform Key", status: "active", rotationStrategy: "account_rotation", catalogSource: "official" }];
  await expect.poll(() => state.candidateReads, { timeout: 10_000 }).toBeGreaterThan(readsBeforeRefresh);
  await expect(dialog).toContainText("Fixture Platform Key");
  await expect(dialog).not.toContainText("Another Platform Key");
  expect(state.writes).toEqual([]);
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toMatchObject([{
    method: "codexProfile/applyGateway", params: { apiKeyId: "platform-fixture", authMode: "preserve" },
  }]);
  expect(state.unhandledMethods).toEqual([]);
});

test("pending application blocks duplicate submission and dismissal", async ({ page }) => {
  const state = await installProfileRuntime(page);
  let releaseApply = () => {};
  state.gatewayApplyGate = new Promise<void>((resolve) => { releaseApply = resolve; });
  await page.goto("/platform-mode/");
  await page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  const confirm = dialog.getByRole("button", { name: "确认应用", exact: true });
  try {
    await confirm.click();
    await expect.poll(() => state.writes.length).toBe(1);
    await expect(confirm).toBeDisabled();
    await expect(dialog.getByRole("button", { name: "取消", exact: true })).toBeDisabled();
    await page.keyboard.press("Escape");
    await page.mouse.click(5, 5);
    await expect(dialog).toBeVisible();
    expect(state.writes).toHaveLength(1);
  } finally {
    releaseApply();
  }
  await expect(dialog).toHaveCount(0);
  expect(state.writes).toHaveLength(1);
  expect(state.unhandledMethods).toEqual([]);
});

test("small-screen details scroll while confirmation controls remain visible", async ({ page }, testInfo) => {
  const state = await installProfileRuntime(page);
  await page.setViewportSize({ width: 390, height: 520 });
  await page.goto("/platform-mode/");
  await page.getByRole("switch", { name: "纯 API Key 模式", exact: true }).click();
  await page.getByRole("button", { name: "重新应用 CodexManager 接入", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  const confirm = dialog.getByRole("button", { name: "确认应用", exact: true });
  await expect(dialog).toContainText("仅使用所选平台密钥");
  await expect(confirm).toBeInViewport({ ratio: 1 });
  await expect(dialog.getByRole("button", { name: "取消", exact: true })).toBeInViewport({ ratio: 1 });
  const content = dialog.locator(".overflow-y-auto");
  await expect.poll(() => content.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await content.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  await expect(dialog.getByText("重载行为", { exact: true })).toBeInViewport();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("connection-dialog-mobile.png") });
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(state.writes).toEqual([]);
  expect(state.unhandledMethods).toEqual([]);
});

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
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  await expect(dialog).toContainText("仅使用所选平台密钥");
  expect(state.writes).toEqual([]);
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
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
  await expect(dialog).toContainText("保留账号信息；从纯 API Key 模式切回时恢复原认证");
  expect(state.writes).toHaveLength(1);
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
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
  const dialog = page.getByRole("dialog", { name: "应用接入配置", exact: true });
  await dialog.getByRole("button", { name: "确认应用", exact: true }).click();
  await expect(page.getByText("切换失败: fixture cannot write profile", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("alert")).toContainText("fixture cannot write profile");
  await expect(dialog.getByRole("button", { name: "确认应用", exact: true })).toBeEnabled();
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
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
  expect(state.writes).toEqual([]);
  await page.getByRole("dialog").getByRole("button", { name: "确认应用", exact: true }).click();
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
