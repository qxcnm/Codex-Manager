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

test("models page supports creating and deleting a managed model", async ({ page }) => {
  const catalogItems: Array<Record<string, unknown>> = [
    {
      slug: "gpt-5.4",
      display_name: "GPT-5.4",
      description: "Latest frontier model",
      supported_in_api: true,
      sourceKind: "remote",
      userEdited: false,
      sortIndex: 0,
      updatedAt: 1_770_000_000,
      input_modalities: ["text", "image"],
    },
    {
      slug: "gpt-4.1",
      display_name: "GPT-4.1",
      description: "Legacy model",
      supported_in_api: true,
      sourceKind: "remote",
      userEdited: false,
      sortIndex: 1,
      updatedAt: 1_760_000_000,
      input_modalities: ["text"],
    },
  ];

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
      payload?.params && typeof payload.params === "object" ? payload.params : {};

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
    if (method === "gateway/concurrencyRecommendation/get") {
      await ok({
        usageRefreshWorkers: 4,
        httpWorkerFactor: 4,
        httpWorkerMin: 8,
        httpStreamWorkerFactor: 1,
        httpStreamWorkerMin: 2,
        accountMaxInflight: 1,
      });
      return;
    }
    if (method === "apikey/modelCatalogList") {
      await ok({ items: catalogItems });
      return;
    }
    if (method === "apikey/modelCatalogSave") {
      const source = params as Record<string, unknown>;
      const saved = {
        ...source,
        sourceKind:
          typeof source.sourceKind === "string" && source.sourceKind.trim()
            ? source.sourceKind
            : "custom",
        userEdited: source.userEdited !== false,
        sortIndex:
          typeof source.sortIndex === "number" ? source.sortIndex : catalogItems.length,
        updatedAt: 1_770_000_100,
      };
      const previousSlug =
        typeof source.previousSlug === "string" ? source.previousSlug.trim() : "";
      const nextSlug = typeof source.slug === "string" ? source.slug.trim() : "";
      const nextItems = catalogItems.filter((item) => {
        const slug = typeof item.slug === "string" ? item.slug : "";
        return slug !== previousSlug && slug !== nextSlug;
      });
      nextItems.push(saved);
      catalogItems.splice(0, catalogItems.length, ...nextItems);
      await ok(saved);
      return;
    }
    if (method === "apikey/modelCatalogDelete") {
      const slug = typeof (params as Record<string, unknown>).slug === "string"
        ? String((params as Record<string, unknown>).slug).trim()
        : "";
      const nextItems = catalogItems.filter((item) => item.slug !== slug);
      catalogItems.splice(0, catalogItems.length, ...nextItems);
      await ok({ ok: true });
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

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" })
  ).toBeVisible();
  await expect(page.locator("tr", { hasText: "gpt-5.4" })).toBeVisible();

  const exportDownloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出到本地 Codex 缓存" }).click();
  const exportDownload = await exportDownloadPromise;
  expect(exportDownload.suggestedFilename()).toBe("models_cache.json");

  await page.getByRole("button", { name: "新增自定义模型" }).click();
  await page.getByLabel("Slug").fill("my-custom-model");
  await page.getByLabel("显示名称").fill("My Custom Model");
  await page.getByLabel("描述").fill("local managed model");
  await page.getByRole("button", { name: "保存模型" }).click();

  const customRow = page.locator("tr", { hasText: "my-custom-model" });
  await expect(customRow).toBeVisible();
  await expect(page.locator("tr", { hasText: "gpt-4.1" })).toBeVisible();

  await page
    .locator("tr", { hasText: "my-custom-model" })
    .getByRole("checkbox")
    .click();
  await page
    .locator("tr", { hasText: "gpt-4.1" })
    .getByRole("checkbox")
    .click();
  await page.getByRole("button", { name: "批量删除模型" }).click();
  await page.getByRole("button", { name: "删除" }).click();
  await expect(customRow).toHaveCount(0);
  await expect(page.locator("tr", { hasText: "gpt-4.1" })).toHaveCount(0);
  await expect(page.locator("tr", { hasText: "gpt-5.4" })).toBeVisible();
});
