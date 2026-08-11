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

test("accounts page shows unavailable status reason and raw reason code", async ({
  page,
}) => {
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
        version: "0.3.1",
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "/tmp/.codex",
        platformFamily: "unix",
        platformOs: "macos",
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
            id: "acct-refresh-reused",
            label: "angiemooreja@hotmail.com",
            plan_type: "plus",
            status: "unavailable",
            status_reason: "refresh_token_invalid:refresh_token_reused",
            sort: 0,
          },
          {
            id: "acct-response-unauthorized",
            label: "NightingaleFinlay4274@outlook.com",
            plan_type: "plus",
            status: "active",
            status_reason: "usage_ok",
            credential_state: "healthy",
            credential_action: "none",
            gateway_probe_status: "failed",
            gateway_probe_reason: "codex_responses_unauthorized",
            sort: 5,
          },
          {
            id: "acct-response-verified",
            label: "callable@example.com",
            plan_type: "plus",
            status: "active",
            status_reason: "usage_ok",
            credential_state: "healthy",
            credential_action: "none",
            gateway_probe_status: "available",
            gateway_probe_reason: "codex_responses_verified",
            sort: 10,
          },
          {
            id: "acct-confirmed-deactivated",
            label: "deactivated@example.com",
            plan_type: "plus",
            status: "banned",
            status_reason: "account_deactivated",
            credential_state: "account_deactivated",
            credential_action: "stop",
            gateway_probe_status: "unavailable",
            sort: 15,
          },
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

  await page.goto("/");
  await page.getByRole("button", { name: "进入高级管理" }).first().click();

  await expect(page.getByRole("heading", { name: "Codex 资源池" })).toBeVisible();
  const reasonText = page.getByText("Refresh Token 已被重复使用，需要重新登录");
  await expect(reasonText).toBeVisible();

  await reasonText.hover();
  await expect(
    page.getByText("refresh_token_invalid:refresh_token_reused"),
  ).toBeVisible();

  const unauthorizedRow = page
    .getByRole("row")
    .filter({ hasText: "NightingaleFinlay4274@outlook.com" });
  await expect(unauthorizedRow.getByText("调用授权失败", { exact: true })).toBeVisible();
  const callableRow = page
    .getByRole("row")
    .filter({ hasText: "callable@example.com" });
  await expect(callableRow.getByText("可调用", { exact: true })).toBeVisible();
  const deactivatedRow = page
    .getByRole("row")
    .filter({ hasText: "deactivated@example.com" });
  await expect(deactivatedRow.getByText("已确认停用", { exact: true }).first()).toBeVisible();
});
