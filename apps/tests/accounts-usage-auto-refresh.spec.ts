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
  upstreamTotalTimeoutMs: 0,
  sseKeepaliveIntervalMs: 15000,
  backgroundTasks: {
    usagePollingEnabled: true,
    usagePollIntervalSecs: 30,
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

const OLD_USAGE = {
  accountId: "acct-auto-refresh",
  availabilityStatus: "available",
  usedPercent: 15,
  windowMinutes: 300,
  resetsAt: 1900000000,
  secondaryUsedPercent: 25,
  secondaryWindowMinutes: 10080,
  secondaryResetsAt: 1900003600,
  creditsJson: null,
  capturedAt: 100,
};

const NEW_USAGE = {
  ...OLD_USAGE,
  usedPercent: 40,
  capturedAt: 200,
};

const UNCHANGED_NEWER_USAGE = {
  accountId: "acct-newer-snapshot",
  availabilityStatus: "available",
  usedPercent: 20,
  windowMinutes: 300,
  resetsAt: 1900007200,
  secondaryUsedPercent: 30,
  secondaryWindowMinutes: 10080,
  secondaryResetsAt: 1900010800,
  creditsJson: null,
  capturedAt: 1_000,
};

test("accounts page refreshes usage after backend polling writes a new snapshot", async ({
  page,
}) => {
  let usageListCount = 0;
  let newSnapshotAvailable = false;

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
            id: "acct-auto-refresh",
            name: "auto-refresh@example.com",
            label: "auto-refresh@example.com",
            plan_type: "plus",
            status: "active",
            sort: 0,
          },
          {
            id: "acct-newer-snapshot",
            name: "newer-snapshot@example.com",
            label: "newer-snapshot@example.com",
            plan_type: "plus",
            status: "active",
            sort: 1,
          },
        ],
        total: 2,
        page: 1,
        pageSize: 20,
      });
      return;
    }
    if (method === "account/usage/read") {
      await ok({ snapshot: UNCHANGED_NEWER_USAGE });
      return;
    }
    if (method === "account/usage/list") {
      usageListCount += 1;
      await ok({
        items: [
          newSnapshotAvailable ? NEW_USAGE : OLD_USAGE,
          UNCHANGED_NEWER_USAGE,
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

  const row = page
    .locator("tbody tr")
    .filter({ hasText: "auto-refresh@example.com" })
    .first();

  await expect(row.getByText("85%", { exact: true }).first()).toBeVisible();
  newSnapshotAvailable = true;
  await page.evaluate(() => {
    window.dispatchEvent(
      new CustomEvent("usage-refresh-completed", {
        detail: { source: "polling", processed: 1, total: 2 },
      })
    );
  });
  await expect(row.getByText("60%", { exact: true }).first()).toBeVisible({
    timeout: 2_000,
  });
  expect(usageListCount).toBeGreaterThanOrEqual(2);
});
