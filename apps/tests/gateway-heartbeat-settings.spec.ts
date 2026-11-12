import { expect, test, type Page } from "@playwright/test";

const IMAGE_TOOL_MODEL_KEY = "CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL";
const COMPACT_PATH_KEY = "CODEXMANAGER_COMPACT_UPSTREAM_PATH";

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
  upstreamStreamTimeoutMs: 600_000,
  upstreamTotalTimeoutMs: 0,
  sseKeepaliveIntervalMs: 15_000,
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

async function mockRuntimeAndRpc(
  page: Page,
  imageToolModel?: string,
  beforeSettingsSet?: () => Promise<void>,
) {
  let settingsSnapshot = {
    ...SETTINGS_SNAPSHOT,
    envOverrides: (imageToolModel === undefined
      ? {}
      : { [IMAGE_TOOL_MODEL_KEY]: imageToolModel }) as Record<string, string>,
    envOverrideCatalog: [
      {
        key: IMAGE_TOOL_MODEL_KEY,
        label: "Codex 生图工具模型",
        defaultValue: "gpt-image-2",
        scope: "service",
        applyMode: "runtime",
        riskLevel: "medium",
        effectScope: "runtime_global",
        safetyNote: "修改后立即生效。",
      },
      {
        key: COMPACT_PATH_KEY,
        label: "Compact 上游路径",
        defaultValue: "/v1/responses/compact",
        scope: "service",
        applyMode: "runtime",
        riskLevel: "medium",
        effectScope: "runtime_global",
        safetyNote: "修改后立即生效。",
      },
    ],
  };
  const settingsPatches: Record<string, unknown>[] = [];

  await page.route(/\/api\/runtime\/?(?:\?.*)?$/, async (route) => {
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

  await page.route(/\/api\/rpc\/?(?:\?.*)?$/, async (route) => {
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
      await ok(settingsSnapshot);
      return;
    }
    if (method === "appSettings/set") {
      settingsPatches.push(params);
      await beforeSettingsSet?.();
      const envOverrides = { ...settingsSnapshot.envOverrides };
      if (params.envOverrides && typeof params.envOverrides === "object") {
        for (const [key, value] of Object.entries(params.envOverrides)) {
          if (value === "") {
            delete envOverrides[key];
          } else if (typeof value === "string") {
            envOverrides[key] = value;
          }
        }
      }
      settingsSnapshot = { ...settingsSnapshot, ...params, envOverrides };
      await ok(settingsSnapshot);
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
        permissions: [],
        distributionEnabled: false,
      });
      return;
    }
    if (method === "aggregateApi/list") {
      await ok({ items: [] });
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

    await route.fulfill({
      status: 500,
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        error: { code: -32000, message: `Unhandled RPC method: ${method}` },
      }),
    });
  });

  return settingsPatches;
}

test("gateway heartbeat switch persists and controls the interval input", async ({
  page,
}) => {
  const settingsPatches = await mockRuntimeAndRpc(page);

  await page.goto("/settings/");
  await page.getByRole("tab", { name: "网关" }).click();

  const heartbeatSwitch = page.getByRole("switch", { name: "保持连接心跳" });
  const intervalInput = page.getByLabel("SSE 保活间隔 (ms)");
  await expect(heartbeatSwitch).toBeChecked();
  await expect(intervalInput).toBeEnabled();

  await heartbeatSwitch.click();

  await expect.poll(() => settingsPatches.length).toBe(1);
  expect(settingsPatches[0]?.sseKeepaliveEnabled).toBe(false);
  await expect(heartbeatSwitch).not.toBeChecked();
  await expect(intervalInput).toBeDisabled();
});

test("Free account model ceiling persists the selected model", async ({ page }) => {
  const settingsPatches = await mockRuntimeAndRpc(page);

  await page.goto("/settings/");
  await page.getByRole("tab", { name: "网关" }).click();

  const ceilingSelect = page.getByRole("combobox", {
    name: "Free 账号模型上限",
  });
  await expect(ceilingSelect).toContainText("不限制");
  await ceilingSelect.click();
  await page.getByRole("option", { name: "gpt-5" }).click();

  await expect.poll(() => settingsPatches.length).toBe(1);
  expect(settingsPatches[0]?.freeAccountMaxModel).toBe("gpt-5");
  await expect(ceilingSelect).toContainText("gpt-5");
});

test("image model choices preserve drafts and replace unsupported stored values only on save", async ({
  page,
}) => {
  let releaseFirstSave!: () => void;
  const firstSavePending = new Promise<void>((resolve) => {
    releaseFirstSave = resolve;
  });
  let firstSave = true;
  const settingsPatches = await mockRuntimeAndRpc(page, "gpt-image-2.5", async () => {
    if (firstSave) {
      firstSave = false;
      await firstSavePending;
    }
  });

  await page.goto("/settings/");
  await page.getByRole("tab", { name: "环境", exact: true }).click();
  await page.getByRole("button", { name: /Codex 生图工具模型/ }).click();

  const modelSelect = page.getByRole("combobox", { name: "当前值", exact: true });
  const saveButton = page.getByRole("button", { name: "保存修改", exact: true });
  const invalidModelWarning = page.getByText("当前型号不受支持，请重新选择或恢复默认。", { exact: true });
  await expect(modelSelect).toHaveAttribute("aria-invalid", "true");
  await expect(modelSelect).toContainText("请选择图片型号");
  await expect(invalidModelWarning).toBeVisible();
  await expect(page.locator("#env-image-model-error code")).toHaveText("gpt-image-2.5");
  await expect(saveButton).toBeDisabled();

  await modelSelect.click();
  await expect(page.getByRole("option")).toHaveText([
    "gpt-image-2",
    "gpt-image-2.5-sunburst",
    "gpt-image-2.5-flare",
  ]);
  await page.getByRole("option", { name: "gpt-image-2.5-flare", exact: true }).click();
  await expect(saveButton).toBeEnabled();
  await expect(invalidModelWarning).toBeHidden();
  expect(settingsPatches).toHaveLength(0);

  await page.getByRole("tab", { name: "网关", exact: true }).click();
  await page.getByRole("tab", { name: "环境", exact: true }).click();
  await expect(modelSelect).toContainText("gpt-image-2.5-flare");
  await page.getByRole("button", { name: /Compact 上游路径/ }).click();
  const genericValueInput = page.getByRole("textbox", { name: "当前值", exact: true });
  await expect(genericValueInput).toHaveValue("/v1/responses/compact");
  await genericValueInput.fill("/v1/chat/completions");
  await page.getByRole("button", { name: /Codex 生图工具模型/ }).click();
  await expect(modelSelect).toContainText("gpt-image-2.5-flare");

  await saveButton.click();
  await expect.poll(() => settingsPatches.length).toBe(1);
  expect(settingsPatches[0]?.envOverrides).toEqual({
    [IMAGE_TOOL_MODEL_KEY]: "gpt-image-2.5-flare",
  });
  try {
    await expect(modelSelect).toBeDisabled();
    await expect(saveButton).toBeDisabled();
    await page.getByRole("button", { name: /Compact 上游路径/ }).click();
    await expect(genericValueInput).toBeDisabled();
    await expect(genericValueInput).toHaveValue("/v1/chat/completions");
  } finally {
    releaseFirstSave();
  }
  await expect(genericValueInput).toBeEnabled();
  await page.getByRole("button", { name: /Codex 生图工具模型/ }).click();
  await expect(modelSelect).toBeEnabled();
  await expect(saveButton).toBeEnabled();

  await page.getByRole("button", { name: "恢复默认", exact: true }).click();
  await expect.poll(() => settingsPatches.length).toBe(2);
  expect(settingsPatches[1]?.envOverrides).toEqual({ [IMAGE_TOOL_MODEL_KEY]: "" });
  await expect(modelSelect).toContainText("gpt-image-2");
  await expect(saveButton).toBeEnabled();
  await page.getByRole("button", { name: /Compact 上游路径/ }).click();
  await expect(genericValueInput).toHaveValue("/v1/chat/completions");
});

test("unsupported image model can be reset without choosing a replacement", async ({ page }) => {
  const settingsPatches = await mockRuntimeAndRpc(page, "retired-image-model");

  await page.goto("/settings/");
  await page.getByRole("tab", { name: "环境", exact: true }).click();
  await page.getByRole("button", { name: /Codex 生图工具模型/ }).click();
  await expect(page.getByRole("button", { name: "保存修改", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "恢复默认", exact: true }).click();

  await expect.poll(() => settingsPatches.length).toBe(1);
  expect(settingsPatches[0]?.envOverrides).toEqual({ [IMAGE_TOOL_MODEL_KEY]: "" });
  await expect(
    page.getByRole("combobox", { name: "当前值", exact: true }).locator('[data-slot="select-value"]'),
  ).toHaveText("gpt-image-2");
  await expect(page.locator("#env-image-model-error")).toBeHidden();
});
