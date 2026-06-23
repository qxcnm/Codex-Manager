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

test("accounts toolbar shows warmup button and tooltip", async ({ page }) => {
  const usageRefreshPayloads: Record<string, unknown>[] = [];
  const rtRefreshPayloads: Record<string, unknown>[] = [];
  let refreshAllRtCount = 0;

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

  await page.route("**/api/rpc**", async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const id = payload?.id ?? 1;

    const ok = (result: unknown) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          result,
        }),
      });

    if (method === "appSettings/get") {
      await ok(SETTINGS_SNAPSHOT);
      return;
    }
    if (method === "initialize") {
      await ok({
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "C:/Users/Test/.codex",
        platformFamily: "windows",
        platformOs: "windows",
      });
      return;
    }
    if (method === "accountManager/session/current") {
      await ok({
        mode: "none",
        currentUser: null,
        role: "system_admin",
        permissions: ["system:admin"],
        billingModeLock: { locked: false, reason: null },
      });
      return;
    }
    if (method === "account/list") {
      await ok({
        items: [
          {
            id: "acct-plus-1",
            name: "qxcnms@gmail.com",
            label: "qxcnms@gmail.com",
            plan_type: "plus",
            status: "active",
            sort: 0,
          },
        ],
        total: 1,
        page: 1,
        pageSize: 20,
      });
      return;
    }
    if (method === "account/usage/list") {
      await ok([]);
      return;
    }
    if (method === "account/usage/refresh") {
      usageRefreshPayloads.push(
        payload?.params && typeof payload.params === "object"
          ? (payload.params as Record<string, unknown>)
          : {},
      );
      await ok({});
      return;
    }
    if (method === "account/chatgptAuthTokens/refresh") {
      rtRefreshPayloads.push(
        payload?.params && typeof payload.params === "object"
          ? (payload.params as Record<string, unknown>)
          : {},
      );
      await ok({
        accessToken: "access-token",
        chatgptAccountId: "org-plus-1",
        chatgptPlanType: "plus",
        hasSubscription: true,
        subscriptionPlan: "plus",
      });
      return;
    }
    if (method === "account/chatgptAuthTokens/refreshAll") {
      refreshAllRtCount += 1;
      await ok({
        requested: 1,
        succeeded: 1,
        failed: 0,
        skipped: 0,
        results: [
          {
            accountId: "acct-plus-1",
            accountName: "qxcnms@gmail.com",
            ok: true,
            message: null,
          },
        ],
      });
      return;
    }

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
  });

  await page.goto("/accounts/");

  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();

  const warmupButton = page.getByRole("button", { name: "预热" });
  await expect(warmupButton).toBeVisible();
  await warmupButton.hover();
  await expect(
    page.getByText(
      "向选中账号发送 hi 进行预热；如果未选中账号，则默认预热全部账号。",
    ),
  ).toBeVisible();

  await page.getByRole("button", { name: "用量详情" }).click();
  const usageDialog = page.getByRole("dialog", { name: "用量详情" });
  await expect(usageDialog.getByRole("button", { name: "刷新 AT/RT" })).toBeVisible();

  await usageDialog.getByRole("button", { name: "立即刷新" }).click();
  await expect.poll(() => usageRefreshPayloads.length).toBe(1);
  expect(usageRefreshPayloads[0].accountId).toBe("acct-plus-1");
  expect(usageRefreshPayloads[0].account_id).toBe("acct-plus-1");

  await usageDialog.getByRole("button", { name: "刷新 AT/RT" }).click();
  await expect.poll(() => rtRefreshPayloads.length).toBe(1);
  expect(rtRefreshPayloads[0].accountId).toBe("acct-plus-1");
  expect(rtRefreshPayloads[0].previousAccountId).toBe("acct-plus-1");

  await usageDialog.getByRole("button", { name: "关闭" }).click();
  await expect(usageDialog).toBeHidden();
  await page.getByText("账号操作", { exact: true }).click();
  await page.getByRole("menuitem", { name: /刷新全部 AT\/RT/ }).click();
  await expect.poll(() => refreshAllRtCount).toBe(1);

  await page.locator("tbody tr").first().getByRole("checkbox").check();
  await page.getByText("账号操作", { exact: true }).click();

  const deleteSelectedItem = page.getByRole("menuitem", {
    name: /删除选中账号/,
  });
  const cleanupByStatusItem = page.getByRole("menuitem", {
    name: /按状态清理账号/,
  });

  await expect(deleteSelectedItem).toBeEnabled();
  await deleteSelectedItem.hover();
  await expect
    .poll(async () =>
      deleteSelectedItem.evaluate((element) =>
        element.hasAttribute("data-highlighted"),
      ),
    )
    .toBe(true);

  await cleanupByStatusItem.hover();
  await expect
    .poll(async () =>
      cleanupByStatusItem.evaluate((element) =>
        element.hasAttribute("data-highlighted"),
      ),
    )
    .toBe(true);
});
