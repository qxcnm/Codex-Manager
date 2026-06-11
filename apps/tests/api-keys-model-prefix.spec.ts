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

async function mockRuntime(page: import("@playwright/test").Page) {
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
}

async function mockApiKeyRpc(
  page: import("@playwright/test").Page,
  options: {
    apiKeys?: unknown[];
    onMethod?: (method: string, payload: Record<string, unknown>) => unknown | undefined;
  } = {},
) {
  const apiKeys =
    options.apiKeys ||
    [
      {
        id: "key-spark",
        name: "Spark Key",
        model_slug: "gpt-5.3-codex-unknown",
        reasoning_effort: "medium",
        service_tier: "default",
        protocol_type: "openai_compat",
        rotation_strategy: "account_rotation",
        status: "enabled",
        created_at: 1_770_000_000,
      },
    ];

  await page.route("**/api/rpc**", async (route) => {
    const payload = route.request().postDataJSON() as Record<string, unknown>;
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

    const customResult = options.onMethod?.(method, payload);
    if (customResult !== undefined) {
      await ok(customResult);
      return;
    }

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
    if (method === "apikey/list") {
      await ok({ items: apiKeys });
      return;
    }
    if (method === "apikey/models") {
      await ok({
        models: [
          {
            slug: "gpt-5.3-codex",
            display_name: "GPT-5.3 Codex",
            description: "Latest frontier agentic coding model.",
            supported_in_api: true,
            visibility: "list",
            input_modalities: ["text", "image"],
          },
        ],
      });
      return;
    }
    if (method === "apikey/usageStats") {
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
}

test("api key modal reuses prefix model metadata for long model slugs", async ({ page }) => {
  await mockRuntime(page);
  await mockApiKeyRpc(page);

  await page.goto("/apikeys/");
  await expect(page.getByRole("main").getByRole("heading", { name: "平台密钥" })).toBeVisible();
  await expect(page.locator("tr", { hasText: "Spark Key" })).toBeVisible();
  await expect(page.locator("tr", { hasText: "gpt-5.3-codex-unknown" })).toBeVisible();

  await page.locator("tr", { hasText: "Spark Key" }).getByTitle("编辑配置").click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "编辑平台密钥" })).toBeVisible();
  await dialog.getByText("GPT-5.3 Codex", { exact: true }).click();
  await expect(dialog.getByText("GPT-5.3 Codex", { exact: true }).first()).toBeVisible();
});

test("api key modal displays and submits hybrid rotation", async ({ page }) => {
  const updatePayloads: Record<string, unknown>[] = [];
  await mockRuntime(page);
  await mockApiKeyRpc(page, {
    apiKeys: [
      {
        id: "key-hybrid",
        name: "Hybrid Key",
        model_slug: "gpt-5.3-codex-unknown",
        reasoning_effort: "medium",
        service_tier: "default",
        protocol_type: "openai_compat",
        rotation_strategy: "hybrid_rotation",
        account_plan_filter: "plus",
        status: "enabled",
        created_at: 1_770_000_001,
      },
    ],
    onMethod: (method, payload) => {
      if (method === "apikey/updateModel") {
        updatePayloads.push(payload);
        return { ok: true };
      }
      return undefined;
    },
  });

  await page.goto("/apikeys/");
  const row = page.locator("tr", { hasText: "Hybrid Key" });
  await expect(row).toBeVisible();
  await expect(row.getByText("混合轮转（账号优先）", { exact: true })).toBeVisible();

  await row.getByTitle("编辑配置").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "编辑平台密钥" })).toBeVisible();
  await expect(dialog.getByText("混合轮转（账号优先）", { exact: true })).toBeVisible();
  await expect(dialog.getByText("账号组筛选", { exact: true })).toBeVisible();
  await expect(dialog.getByText("Plus", { exact: true })).toBeVisible();

  await dialog.getByRole("button", { name: "完成" }).click();

  await expect.poll(() => updatePayloads.length).toBe(1);
  const params = updatePayloads[0]?.params as Record<string, unknown>;
  expect(params.rotationStrategy).toBe("hybrid_rotation");
  expect(params.accountPlanFilter).toBe("plus");
});

test("api key modal can select hybrid rotation on create", async ({ page }) => {
  const createPayloads: Record<string, unknown>[] = [];
  await mockRuntime(page);
  await mockApiKeyRpc(page, {
    apiKeys: [],
    onMethod: (method, payload) => {
      if (method === "apikey/create") {
        createPayloads.push(payload);
        return { id: "key-created", key: "cm-test-key" };
      }
      return undefined;
    },
  });

  await page.goto("/apikeys/");
  await page.getByRole("button", { name: "创建密钥" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "创建平台密钥" })).toBeVisible();
  await expect(dialog.getByLabel("自定义 API Key (可选)")).toBeVisible();
  await dialog.getByLabel("自定义 API Key (可选)").fill("sk-cm-custom-fixed");
  await dialog.getByText("账号轮转", { exact: true }).click();
  await page.getByText("混合轮转（账号优先）", { exact: true }).click();
  await expect(dialog.getByText("账号组筛选", { exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "完成" }).click();

  await expect.poll(() => createPayloads.length).toBe(1);
  const params = createPayloads[0]?.params as Record<string, unknown>;
  expect(params.rotationStrategy).toBe("hybrid_rotation");
  expect(params.customKey).toBe("sk-cm-custom-fixed");
});
