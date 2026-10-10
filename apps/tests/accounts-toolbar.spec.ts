import { expect, test } from "@playwright/test";

test("quota reset auto wake defaults on and supports independent single and bulk changes", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const accounts: Array<{
    id: string;
    label: string;
    status: string;
    sort: number;
    resetWarmupEnabled?: boolean;
  }> = [
    { id: "reset-default", label: "default@example.com", status: "active", sort: 0 },
    { id: "reset-off", label: "off@example.com", status: "disabled", sort: 1, resetWarmupEnabled: false },
  ];
  const updates: Array<{ accountIds: string[]; enabled: boolean }> = [];
  const accountStatusUpdates: unknown[] = [];
  let rejectNextUpdate = false;

  await page.route("**/api/runtime**", async (route) => {
    await route.fulfill({ json: {
      mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
      canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
      canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
    } });
  });
  await page.route("**/api/rpc**", async (route) => {
    const payload = route.request().postDataJSON();
    const method = String(payload?.method || "");
    const id = payload?.id ?? 1;
    const params = payload?.params ?? {};
    const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "appSettings/get") return ok(SETTINGS_SNAPSHOT);
    if (method === "initialize") {
      return ok({ userAgent: "codex_cli_rs/0.1.19", codexHome: "C:/Test", platformFamily: "windows", platformOs: "windows" });
    }
    if (method === "accountManager/session/current") {
      return ok({ mode: "none", currentUser: null, role: "system_admin", permissions: ["system:admin"], distributionEnabled: false });
    }
    if (method === "account/list") return ok({ items: accounts, total: accounts.length, page: 1, pageSize: 20 });
    if (method === "account/usage/list") return ok([]);
    if (method === "account/resetWarmup/update") {
      if (rejectNextUpdate) {
        rejectNextUpdate = false;
        return route.fulfill({ json: {
          jsonrpc: "2.0", id, error: { code: -32000, message: "reset warmup update rejected" },
        } });
      }
      const update = { accountIds: params.accountIds as string[], enabled: params.enabled as boolean };
      updates.push(update);
      for (const account of accounts) {
        if (update.accountIds.includes(account.id)) account.resetWarmupEnabled = update.enabled;
      }
      return ok({ updated: update.accountIds.length });
    }
    if (method === "account/update") {
      accountStatusUpdates.push(params);
      return ok({});
    }
    return route.fulfill({ status: 500, json: {
      jsonrpc: "2.0", id, error: { code: -32000, message: `Unhandled RPC: ${method}` },
    } });
  });

  await page.goto("/accounts/");
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();
  const rows = page.locator(".account-pool-main-table tbody [data-account-pool-main-row]");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0).getByText("自动唤醒：开", { exact: true })).toBeVisible();
  await expect(rows.nth(1).getByText("自动唤醒：关", { exact: true })).toBeVisible();
  await page.screenshot({ path: test.info().outputPath("quota-reset-warmup-table.png"), animations: "disabled" });

  // A legacy account with no setting is enabled; a single action only changes this setting.
  await page.getByTitle("更多账号操作", { exact: true }).first().click();
  await page.getByRole("menuitem", { name: "关闭额度重置自动唤醒", exact: true }).click();
  await expect.poll(() => updates).toEqual([{ accountIds: ["reset-default"], enabled: false }]);
  await expect(rows.nth(0).getByText("自动唤醒：关", { exact: true })).toBeVisible();

  await rows.nth(0).getByRole("checkbox").check();
  await rows.nth(1).getByRole("checkbox").check();
  await page.getByText("账号操作", { exact: true }).click();
  await expect(page.getByRole("menuitem", { name: /批量关闭自动唤醒/ })).toBeDisabled();
  await page.screenshot({ path: test.info().outputPath("quota-reset-warmup-bulk-menu.png"), animations: "disabled" });
  await page.getByRole("menuitem", { name: /批量开启自动唤醒/ }).click();
  await expect.poll(() => updates.at(-1)).toEqual({ accountIds: ["reset-default", "reset-off"], enabled: true });
  await expect(page.getByText("自动唤醒：开", { exact: true })).toHaveCount(2);

  await page.getByText("账号操作", { exact: true }).click();
  await expect(page.getByRole("menuitem", { name: /批量开启自动唤醒/ })).toBeDisabled();
  await page.getByRole("menuitem", { name: /批量关闭自动唤醒/ }).click();
  await expect.poll(() => updates.at(-1)).toEqual({ accountIds: ["reset-default", "reset-off"], enabled: false });
  await expect(page.getByText("自动唤醒：关", { exact: true })).toHaveCount(2);

  // Failed writes leave the saved state visible, including after switching to cards.
  rejectNextUpdate = true;
  await page.getByRole("button", { name: "宫格视图" }).click();
  await expect(page.getByTestId("account-grid")).toBeVisible();
  await page.getByTitle("更多账号操作", { exact: true }).first().click();
  await page.screenshot({ path: test.info().outputPath("quota-reset-warmup-single-menu.png"), animations: "disabled" });
  await page.getByRole("menuitem", { name: "开启额度重置自动唤醒", exact: true }).click();
  await expect(page.getByText(/更新额度重置自动唤醒失败:/)).toBeVisible();
  await expect(page.getByText("自动唤醒：关", { exact: true })).toHaveCount(2);
  await expect(page.getByRole("menuitem", { name: "开启额度重置自动唤醒", exact: true })).toBeHidden();
  expect(updates).toHaveLength(3);
  expect(accountStatusUpdates).toEqual([]);
  expect(accounts.map((account) => account.status)).toEqual(["active", "disabled"]);
});

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

const USAGE_DETAILS = {
  accountId: "acct-plus-1",
  availabilityStatus: "available",
  usedPercent: 23,
  windowMinutes: 300,
  resetsAt: 1_900_000_000,
  secondaryUsedPercent: 23,
  secondaryWindowMinutes: 10_080,
  secondaryResetsAt: 1_900_604_800,
  creditsJson: JSON.stringify({
    _codexmanager_extra_rate_limits: [
      {
        limit_name: "Spark",
        primary_window: {
          used_percent: 0,
          limit_window_seconds: 18_000,
          reset_at: 1_900_010_000,
        },
        secondary_window: {
          used_percent: 10,
          limit_window_seconds: 604_800,
          reset_at: 1_900_614_800,
        },
      },
      {
        limit_name: "Code Review",
        primary_window: {
          used_percent: 5,
          limit_window_seconds: 18_000,
          reset_at: 1_900_020_000,
        },
        secondary_window: {
          used_percent: 15,
          limit_window_seconds: 604_800,
          reset_at: 1_900_624_800,
        },
      },
    ],
  }),
  capturedAt: 1_900_000_000,
};

test("accounts toolbar shows warmup button and tooltip", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
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
        distributionEnabled: false,
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
      await ok([USAGE_DETAILS]);
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

  const usageScrollBody = usageDialog.locator(
    '[data-slot="usage-modal-scroll-body"]',
  );
  await expect(usageScrollBody).toHaveCSS("overflow-y", "auto");
  await expect(usageDialog.getByText(/Code Review 额度/).last()).toBeAttached();
  const scrollMetrics = await usageScrollBody.evaluate((element) => {
    const styles = window.getComputedStyle(element);
    return {
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
      scrollbarGutter: styles.scrollbarGutter,
    };
  });
  expect(scrollMetrics.scrollHeight).toBeGreaterThan(scrollMetrics.clientHeight);
  expect(scrollMetrics.scrollbarGutter).toContain("stable");

  await usageScrollBody.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(() => usageScrollBody.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  await expect(usageDialog.getByText(/Code Review 额度/).last()).toBeInViewport();
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

test("bulk account status actions follow single-account status rules", async ({
  page,
}) => {
  const statusUpdates: Array<{ accountId: string; status: string }> = [];

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
    const params =
      payload?.params && typeof payload.params === "object"
        ? (payload.params as Record<string, unknown>)
        : {};

    const ok = (result: unknown) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({ jsonrpc: "2.0", id, result }),
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
        distributionEnabled: false,
      });
      return;
    }
    if (method === "account/list") {
      await ok({
        items: [
          { id: "active", label: "active@example.com", status: "active", sort: 0 },
          { id: "disabled", label: "disabled@example.com", status: "disabled", sort: 1 },
          { id: "inactive", label: "inactive@example.com", status: "inactive", sort: 2 },
          { id: "banned", label: "banned@example.com", status: "banned", sort: 3 },
        ],
        total: 4,
        page: 1,
        pageSize: 20,
      });
      return;
    }
    if (method === "account/usage/list") {
      await ok([]);
      return;
    }
    if (method === "account/update") {
      statusUpdates.push({
        accountId: String(params.accountId || params.account_id || ""),
        status: String(params.status || ""),
      });
      await ok({});
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

  const selectedRows = page.locator("tbody tr").getByRole("checkbox");
  await expect(selectedRows).toHaveCount(4);
  for (let index = 0; index < 4; index += 1) {
    await selectedRows.nth(index).check();
  }

  await page.getByText("账号操作", { exact: true }).click();
  await expect(
    page.getByRole("menuitem", { name: /批量开启选中账号/ }),
  ).toHaveText(/批量开启选中账号\s*2/);

  const disableSelectedItem = page.getByRole("menuitem", {
    name: /批量关闭选中账号/,
  });
  await expect(disableSelectedItem).toHaveText(/批量关闭选中账号\s*1/);
  await disableSelectedItem.click();

  await expect.poll(() => statusUpdates).toEqual([
    { accountId: "active", status: "disabled" },
  ]);
});

test("account models can be associated and the persisted grid view keeps all actions", async ({
  page,
}) => {
  const fetchedAccountIds: string[] = [];
  const associationPayloads: Record<string, unknown>[] = [];

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
    const params =
      payload?.params && typeof payload.params === "object"
        ? (payload.params as Record<string, unknown>)
        : {};
    const ok = (result: unknown) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({ jsonrpc: "2.0", id, result }),
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
        distributionEnabled: false,
      });
      return;
    }
    if (method === "account/list") {
      await ok({
        items: [
          {
            id: "acct-models-1",
            name: "models@example.com",
            label: "models@example.com",
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
    if (method === "account/fetchModels") {
      fetchedAccountIds.push(String(params.accountId || ""));
      await ok({
        accountId: "acct-models-1",
        fetchedAt: 1_900_000_000,
        items: [
          {
            upstreamModel: "gpt-next",
            displayName: "GPT Next",
            existingModelSlug: null,
            alreadyLinked: false,
          },
          {
            upstreamModel: "gpt-existing",
            displayName: "GPT Existing",
            existingModelSlug: "gpt-existing",
            alreadyLinked: true,
          },
        ],
      });
      return;
    }
    if (method === "account/associateModels") {
      associationPayloads.push(params);
      await ok({
        createdModels: ["gpt-next"],
        addedRoutes: ["gpt-next"],
        unchangedRoutes: ["gpt-existing"],
      });
      return;
    }
    await ok({});
  });

  await page.goto("/accounts/");
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();

  await page.getByRole("button", { name: "获取账号模型" }).click();
  await expect.poll(() => fetchedAccountIds).toEqual(["acct-models-1"]);
  const associationDialog = page.getByRole("dialog", { name: "关联目录模型" });
  await expect(associationDialog).toContainText("models@example.com");
  await associationDialog
    .getByRole("button", { name: /关联所选模型 \(2\)/ })
    .click();
  await expect.poll(() => associationPayloads.length).toBe(1);
  expect(associationPayloads[0].accountId).toBe("acct-models-1");
  expect(associationPayloads[0].upstreamModels).toEqual([
    "gpt-next",
    "gpt-existing",
  ]);
  expect(associationPayloads[0].displayNames).toEqual({
    "gpt-next": "GPT Next",
    "gpt-existing": "GPT Existing",
  });
  await expect(associationDialog).toBeHidden();

  await page.getByRole("button", { name: "宫格视图" }).click();
  await expect(page.getByTestId("account-grid")).toBeVisible();
  await expect(page.getByTestId("account-card")).toHaveCount(1);
  await expect(
    page.getByTestId("account-card").getByRole("button", {
      name: "获取账号模型",
    }),
  ).toBeVisible();

  await page.reload();
  await expect(page.getByTestId("account-grid")).toBeVisible();
  await expect(page.getByTestId("account-card")).toHaveCount(1);

  await page.getByRole("button", { name: "列表视图" }).click();
  await expect(page.getByTestId("account-grid")).toHaveCount(0);
  await expect(page.locator(".account-pool-main-table")).toBeVisible();

  await page.addInitScript((storageKey) => {
    const originalGetItem = Storage.prototype.getItem;
    const originalSetItem = Storage.prototype.setItem;
    Storage.prototype.getItem = function getItem(key: string) {
      if (key === storageKey) throw new DOMException("denied", "SecurityError");
      return originalGetItem.call(this, key);
    };
    Storage.prototype.setItem = function setItem(key: string, value: string) {
      if (key === storageKey) throw new DOMException("denied", "SecurityError");
      return originalSetItem.call(this, key, value);
    };
  }, "codexmanager.accounts.view-mode");
  await page.reload();
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();
  await expect(page.locator(".account-pool-main-table")).toBeVisible();
  await page.getByRole("button", { name: "宫格视图" }).click();
  await expect(page.getByTestId("account-grid")).toBeVisible();
});
