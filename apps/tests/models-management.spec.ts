import { expect, test, type Page } from "@playwright/test";

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

const LONG_AGGREGATE_ID =
  "aggregate-provider-with-an-exceptionally-long-identifier-for-layout-testing";
const LONG_AGGREGATE_NAME =
  "Aggregate Provider With An Exceptionally Long Display Name For Layout Testing";

type JsonObject = Record<string, unknown>;

type Rect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type MockState = {
  models: JsonObject[];
  upserts: JsonObject[];
  stateUpdates: JsonObject[];
  stateUpdateError: string | null;
  batchStateUpdates: JsonObject[];
  batchStateUpdateError: string | null;
  batchStateUpdateDelayMs: number;
  deletes: string[];
  importCalls: Array<{ method: string; params: JsonObject }>;
  listCalls: number;
  listError: string | null;
  listDelayMs: number;
  listErrorAfterDelete: string | null;
  listDelayAfterDeleteMs: number;
  deleteDelayMs: number;
  deleteErrors: Record<string, string>;
};

const PRICED_MODELS: Record<string, [number, number, number]> = {
  "gpt-5.6-sol": [5_000_000, 500_000, 30_000_000],
  "gpt-5.6-terra": [2_500_000, 250_000, 15_000_000],
  "gpt-5.6-luna": [1_000_000, 100_000, 6_000_000],
  "gpt-5.5": [5_000_000, 500_000, 30_000_000],
  "gpt-5.4": [2_500_000, 250_000, 15_000_000],
  "gpt-5.4-mini": [750_000, 75_000, 4_500_000],
  "gpt-5.2": [1_750_000, 175_000, 14_000_000],
  "gpt-image-2": [8_000_000, 2_000_000, 30_000_000],
};

function builtinModel(
  slug: string,
  sortOrder: number,
  visibility: "list" | "hide" = "list",
): JsonObject {
  const rates = PRICED_MODELS[slug] ?? null;
  const isImageModel = slug === "gpt-image-2";
  const price = rates
    ? {
        priceStatus: "official",
        priceSource: isImageModel
          ? "https://developers.openai.com/api/docs/pricing#image-generation"
          : slug.startsWith("gpt-5.6")
            ? "https://developers.openai.com/api/docs/models/compare"
            : "seed-2026-05-11",
        inputMicrousdPer1m: rates[0],
        cachedInputMicrousdPer1m: rates[1],
        outputMicrousdPer1m: rates[2],
      }
    : {
        priceStatus: "missing",
        priceSource: null,
        inputMicrousdPer1m: null,
        cachedInputMicrousdPer1m: null,
        outputMicrousdPer1m: null,
      };
  return {
    id: `builtin:${slug}`,
    slug,
    displayName: isImageModel ? "GPT Image 2" : slug.toUpperCase(),
    description: isImageModel
      ? "State-of-the-art image generation and editing model."
      : `${slug} builtin`,
    provider: "openai",
    family: isImageModel ? "gpt-image" : "gpt-5",
    category: isImageModel ? "image" : "reasoning",
    tags: isImageModel ? ["image-generation", "image-editing"] : ["coding"],
    origin: "builtin",
    enabled: true,
    supportedInApi: true,
    visibility,
    sortOrder,
    contextWindow: isImageModel
      ? null
      : slug.startsWith("gpt-5.6")
        ? 372_000
        : 272_000,
    maxContextWindow: isImageModel
      ? null
      : slug === "gpt-5.4"
        ? 1_000_000
        : 272_000,
    defaultReasoningEffort: isImageModel ? null : "medium",
    capabilities: isImageModel
      ? {
          reasoningEfforts: [],
          serviceTiers: [],
          additionalSpeedTiers: [],
          inputModalities: ["text", "image"],
          outputModalities: ["image"],
          supportedEndpoints: [
            "/v1/images/generations",
            "/v1/images/edits",
          ],
          snapshot: "gpt-image-2-2026-04-21",
          supportsTextGeneration: false,
          supportsImageGeneration: true,
          supportsImageEditing: true,
          supportsTransparentBackground: false,
        }
      : {
          reasoningEfforts: ["low", "medium", "high", "xhigh"],
          inputModalities: ["text", "image"],
          supportsParallelToolCalls: true,
        },
    instructionsMode: "passthrough",
    instructionsText: null,
    builtinRevision: isImageModel ? 5 : slug.startsWith("gpt-5.6") ? 4 : 2,
    userEdited: false,
    price,
    priceTiers: rates
      ? [
          {
            minInputTokens: 0,
            inputMicrousdPer1m: rates[0],
            cachedInputMicrousdPer1m: rates[1],
            outputMicrousdPer1m: rates[2],
          },
          ...(slug.startsWith("gpt-5.6")
            ? [
                {
                  minInputTokens: 272_000,
                  inputMicrousdPer1m: rates[0] * 2,
                  cachedInputMicrousdPer1m: rates[1] * 2,
                  outputMicrousdPer1m: (rates[2] * 3) / 2,
                },
              ]
            : []),
        ]
      : [],
    routes: [
      {
        id: `route:${slug}`,
        sourceKind: "account_pool",
        sourceId: "default",
        upstreamModel: slug,
        enabled: true,
        priority: 0,
        weight: 1,
      },
    ],
    permissionGroupIds: rates ? ["mg_default"] : [],
    createdAt: 1_770_000_000,
    updatedAt: 1_770_000_000,
  };
}

function freshModels(): JsonObject[] {
  return [
    builtinModel("gpt-5.6-sol", 1),
    builtinModel("gpt-5.6-terra", 2),
    builtinModel("gpt-5.6-luna", 3),
    builtinModel("gpt-5.5", 7),
    builtinModel("gpt-5.4", 16),
    builtinModel("gpt-5.4-mini", 23),
    builtinModel("gpt-5.2", 29),
    builtinModel("gpt-image-2", 44),
    builtinModel("codex-auto-review", 43, "hide"),
  ];
}

function catalogResult(models: JsonObject[]) {
  return {
    items: models,
    stats: {
      total: models.length,
      enabled: models.filter((model) => model.enabled === true).length,
      builtin: models.filter((model) => model.origin === "builtin").length,
      custom: models.filter((model) => model.origin === "custom").length,
      priceMissing: models.filter(
        (model) => (model.price as JsonObject)?.priceStatus === "missing",
      ).length,
      missingRoute: models.filter(
        (model) =>
          !Array.isArray(model.routes) ||
          !model.routes.some((route) => (route as JsonObject).enabled === true),
      ).length,
    },
  };
}

function importedModel(): JsonObject {
  return {
    ...builtinModel("imported-local", 60),
    id: "custom:imported-local",
    displayName: "Imported Local",
    origin: "custom",
    builtinRevision: null,
    userEdited: true,
    price: {
      priceStatus: "custom",
      priceSource: "local-import",
      inputMicrousdPer1m: 1_000_000,
      cachedInputMicrousdPer1m: 100_000,
      outputMicrousdPer1m: 5_000_000,
    },
    permissionGroupIds: [],
  };
}

async function installMockRuntime(page: Page): Promise<MockState> {
  const state: MockState = {
    models: freshModels(),
    upserts: [],
    stateUpdates: [],
    stateUpdateError: null,
    batchStateUpdates: [],
    batchStateUpdateError: null,
    batchStateUpdateDelayMs: 0,
    deletes: [],
    importCalls: [],
    listCalls: 0,
    listError: null,
    listDelayMs: 0,
    listErrorAfterDelete: null,
    listDelayAfterDeleteMs: 0,
    deleteDelayMs: 0,
    deleteErrors: {},
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
    const params =
      payload?.params && typeof payload.params === "object"
        ? (payload.params as JsonObject)
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
    if (method === "accountManager/session/current") {
      await ok({
        mode: "server",
        role: "system_admin",
        currentUser: null,
        permissions: ["*"],
        distributionEnabled: true,
      });
      return;
    }
    if (method === "initialize") {
      await ok({
        version: "0.4.0",
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "/tmp/.codex",
        platformFamily: "linux",
        platformOs: "linux",
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
    if (method === "aggregateApi/list") {
      await ok({
        items: [
          {
            id: "agg-1",
            supplierName: "Aggregate Test",
            providerType: "openai_compat",
            baseUrl: "https://aggregate.invalid/v1",
            status: "enabled",
          },
          {
            id: LONG_AGGREGATE_ID,
            supplierName: LONG_AGGREGATE_NAME,
            providerType: "openai_compat",
            baseUrl: "https://long-aggregate.invalid/v1",
            status: "enabled",
          },
        ],
      });
      return;
    }
    if (method === "apikey/managedModelListV2") {
      state.listCalls += 1;
      if (state.listDelayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, state.listDelayMs));
      }
      if (state.listError) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: state.listError },
          }),
        });
        return;
      }
      await ok(catalogResult(state.models));
      return;
    }
    if (method === "apikey/managedModelGetV2") {
      const model = state.models.find((item) => item.slug === params.slug);
      await ok(model ?? null);
      return;
    }
    if (method === "apikey/managedModelUpsertV2") {
      state.upserts.push(structuredClone(params));
      const inputModel = structuredClone((params.model ?? {}) as JsonObject);
      const slug = String(inputModel.slug ?? "");
      const previousSlug = String(params.previousSlug ?? "");
      const existing = state.models.find(
        (item) => item.slug === previousSlug || item.slug === slug,
      );
      const saved = {
        ...inputModel,
        id: existing?.id ?? `custom:${slug}`,
        createdAt: existing?.createdAt ?? 1_770_000_100,
        updatedAt: 1_770_000_100,
      };
      state.models = state.models.filter(
        (item) => item.slug !== previousSlug && item.slug !== slug,
      );
      state.models.push(saved);
      await ok(saved);
      return;
    }
    if (method === "apikey/managedModelUpdateStateV2") {
      state.stateUpdates.push(structuredClone(params));
      if (state.stateUpdateError) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: state.stateUpdateError },
          }),
        });
        return;
      }
      const slug = String(params.slug ?? "");
      const model = state.models.find((item) => item.slug === slug);
      if (!model) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: "model_not_found" },
          }),
        });
        return;
      }
      model.enabled = params.enabled === true;
      model.visibility = params.visibility === "hide" ? "hide" : "list";
      model.userEdited = true;
      model.updatedAt = 1_770_000_150;
      await ok(structuredClone(model));
      return;
    }
    if (method === "apikey/managedModelBatchUpdateStateV2") {
      state.batchStateUpdates.push(structuredClone(params));
      if (state.batchStateUpdateDelayMs > 0) {
        await new Promise((resolve) =>
          setTimeout(resolve, state.batchStateUpdateDelayMs),
        );
      }
      if (state.batchStateUpdateError) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: state.batchStateUpdateError },
          }),
        });
        return;
      }
      const slugs = Array.isArray(params.slugs)
        ? params.slugs.map((slug) => String(slug))
        : [];
      const models = slugs.map((slug) =>
        state.models.find((item) => item.slug === slug),
      );
      if (models.some((model) => !model)) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: "model_not_found" },
          }),
        });
        return;
      }
      for (const model of models) {
        if (!model) continue;
        model.enabled = params.enabled === true;
        model.visibility = params.visibility === "hide" ? "hide" : "list";
        model.userEdited = true;
        model.updatedAt = 1_770_000_175;
      }
      await ok(structuredClone(models));
      return;
    }
    if (method === "apikey/managedModelDeleteV2") {
      const slug = String(params.slug ?? "");
      state.deletes.push(slug);
      if (state.deleteDelayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, state.deleteDelayMs));
      }
      const deleteError = state.deleteErrors[slug];
      if (deleteError) {
        await route.fulfill({
          contentType: "application/json; charset=utf-8",
          body: JSON.stringify({
            jsonrpc: "2.0",
            id,
            error: { code: -32000, message: deleteError },
          }),
        });
        return;
      }
      const model = state.models.find((item) => item.slug === slug);
      if (model?.origin === "builtin") {
        model.enabled = false;
        model.visibility = "hide";
        model.userEdited = true;
        model.updatedAt = 1_770_000_200;
      } else {
        state.models = state.models.filter((item) => item.slug !== slug);
      }
      if (state.listErrorAfterDelete) {
        state.listError = state.listErrorAfterDelete;
      }
      if (state.listDelayAfterDeleteMs > 0) {
        state.listDelayMs = state.listDelayAfterDeleteMs;
      }
      await ok(null);
      return;
    }
    if (
      method === "apikey/managedModelImportPreviewV2" ||
      method === "apikey/managedModelImportCommitV2"
    ) {
      state.importCalls.push({ method, params: structuredClone(params) });
      const isCommit = method.endsWith("CommitV2");
      if (isCommit && !state.models.some((item) => item.slug === "imported-local")) {
        state.models.push(importedModel());
      }
      await ok({
        added: ["imported-local"],
        updated: [],
        conflicts: [],
        skipped: [],
        errors: [],
        ignoredFields: ["base_instructions", "unknown_field"],
        committed: isCommit ? 1 : 0,
      });
      return;
    }

    await route.fulfill({
      status: 500,
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        error: { code: -32000, message: `Unhandled RPC method in test: ${method}` },
      }),
    });
  });

  return state;
}

function rectanglesOverlap(first: Rect, second: Rect): boolean {
  return (
    first.x < second.x + second.width &&
    first.x + first.width > second.x &&
    first.y < second.y + second.height &&
    first.y + first.height > second.y
  );
}

test("重新读取会更新目录并明确反馈成功与失败", async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on("pageerror", (error) => pageErrors.push(error));
  const state = await installMockRuntime(page);

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();
  await expect(page.getByText("gpt-5.6-sol", { exact: true })).toBeVisible();

  const callsBeforeReload = state.listCalls;
  state.models.push(importedModel());
  await page.getByRole("button", { name: "重新读取" }).click();
  await expect(page.getByText("imported-local", { exact: true })).toBeVisible();
  await expect(page.getByText("模型目录已重新读取", { exact: true })).toBeVisible();
  expect(state.listCalls).toBeGreaterThan(callsBeforeReload);

  state.listError = "catalog reload failed";
  await page.getByRole("button", { name: "重新读取" }).click();
  await expect(
    page.getByText(/\u8bfb取模型失败.*catalog reload failed/),
  ).toBeVisible();
  await expect(page.getByText("imported-local", { exact: true })).toBeVisible();
  expect(pageErrors).toEqual([]);
});

test("模型状态下拉支持四态切换并直接恢复隐藏模型", async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on("pageerror", (error) => pageErrors.push(error));
  const state = await installMockRuntime(page);
  const slug = "gpt-5.4";

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  const row = page.locator("tr", {
    has: page.getByText(slug, { exact: true }),
  });
  const stateButton = () =>
    row.getByRole("button", {
      name: `模型状态操作 ${slug}`,
      exact: true,
    });
  const chooseState = async (label: string) => {
    await stateButton().click();
    await page
      .getByRole("menuitemradio", { name: label, exact: true })
      .click();
  };
  const expectLastStateUpdate = async (
    count: number,
    enabled: boolean,
    visibility: "list" | "hide",
  ) => {
    await expect.poll(() => state.stateUpdates.length).toBe(count);
    expect(state.stateUpdates[count - 1]).toEqual({
      slug,
      enabled,
      visibility,
    });
  };

  await expect(stateButton()).toContainText("已启用");
  await chooseState("显示但禁用");
  await expectLastStateUpdate(1, false, "list");
  await expect(stateButton()).toContainText("已禁用");

  await chooseState("显示并启用");
  await expectLastStateUpdate(2, true, "list");
  await expect(stateButton()).toContainText("已启用");

  await chooseState("隐藏但启用");
  await expectLastStateUpdate(3, true, "hide");
  await expect(row).toHaveCount(0);

  const filter = page.getByRole("main").getByRole("combobox");
  await filter.click();
  await page.getByRole("option", { name: "已隐藏" }).click();
  await expect(row).toBeVisible();
  await expect(stateButton()).toContainText("隐藏且启用");

  await chooseState("隐藏并禁用");
  await expectLastStateUpdate(4, false, "hide");
  await expect(stateButton()).toContainText("隐藏且禁用");

  await chooseState("恢复显示但保持禁用");
  await expectLastStateUpdate(5, false, "list");
  await expect(row).toHaveCount(0);

  await filter.click();
  await page.getByRole("option", { name: "全部模型" }).click();
  await expect(row).toBeVisible();
  await expect(stateButton()).toContainText("已禁用");

  await chooseState("隐藏并禁用");
  await expectLastStateUpdate(6, false, "hide");
  await expect(row).toHaveCount(0);

  await filter.click();
  await page.getByRole("option", { name: "已隐藏" }).click();
  await expect(row).toBeVisible();
  await chooseState("恢复并启用");
  await expectLastStateUpdate(7, true, "list");
  await expect(row).toHaveCount(0);

  await filter.click();
  await page.getByRole("option", { name: "全部模型" }).click();
  await expect(row).toBeVisible();
  await expect(stateButton()).toContainText("已启用");
  expect(state.upserts).toHaveLength(0);
  expect(pageErrors).toEqual([]);
});

test("模型状态更新失败时保留原状态", async ({ page }) => {
  const pageErrors: Error[] = [];
  page.on("pageerror", (error) => pageErrors.push(error));
  const state = await installMockRuntime(page);
  state.stateUpdateError = "state update failed";
  const slug = "gpt-5.4";

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  const row = page.locator("tr", {
    has: page.getByText(slug, { exact: true }),
  });
  const stateButton = row.getByRole("button", {
    name: `模型状态操作 ${slug}`,
    exact: true,
  });
  await expect(stateButton).toContainText("已启用");
  await stateButton.click();
  await page
    .getByRole("menuitemradio", { name: "显示但禁用", exact: true })
    .click();

  await expect.poll(() => state.stateUpdates.length).toBe(1);
  await expect(
    page.getByText(/更新模型状态失败.*state update failed/),
  ).toBeVisible();
  await expect(row).toBeVisible();
  await expect(stateButton).toContainText("已启用");
  expect(state.models.find((model) => model.slug === slug)).toMatchObject({
    enabled: true,
    visibility: "list",
  });
  expect(pageErrors).toEqual([]);
});

test("批量状态下拉一次更新多个模型并保持原子失败", async ({
  page,
}) => {
  const pageErrors: Error[] = [];
  page.on("pageerror", (error) => pageErrors.push(error));
  const state = await installMockRuntime(page);
  const slugs = ["gpt-5.6-sol", "gpt-5.6-terra"];
  const row = (slug: string) =>
    page.locator("tr", {
      has: page.getByText(slug, { exact: true }),
    });
  const checkbox = (slug: string) =>
    page.getByRole("checkbox", {
      name: `选择模型 ${slug}`,
      exact: true,
    });
  const batchStateButton = (count: number) =>
    page.getByRole("button", {
      name: `批量修改模型状态 (${count})`,
      exact: true,
    });
  const chooseBatchState = async (label: string, count: number) => {
    await batchStateButton(count).click();
    await page.getByRole("menuitem", { name: label, exact: true }).click();
  };

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();
  await expect(batchStateButton(0)).toBeDisabled();

  for (const slug of slugs) await checkbox(slug).click();
  await chooseBatchState("隐藏并禁用", 2);
  await expect.poll(() => state.batchStateUpdates.length).toBe(1);
  expect(state.batchStateUpdates[0]).toEqual({
    slugs,
    enabled: false,
    visibility: "hide",
  });
  for (const slug of slugs) await expect(row(slug)).toHaveCount(0);
  await expect(
    page.getByText("已更新 2 个模型的状态", { exact: true }),
  ).toBeVisible();
  await expect(batchStateButton(0)).toBeDisabled();

  const filter = page.getByRole("combobox", { name: "筛选模型" });
  await filter.click();
  await page.getByRole("option", { name: "已隐藏" }).click();
  for (const slug of slugs) {
    await expect(row(slug)).toBeVisible();
    await checkbox(slug).click();
  }
  await chooseBatchState("显示并启用", 2);
  await expect.poll(() => state.batchStateUpdates.length).toBe(2);
  expect(state.batchStateUpdates[1]).toEqual({
    slugs,
    enabled: true,
    visibility: "list",
  });
  for (const slug of slugs) await expect(row(slug)).toHaveCount(0);

  await filter.click();
  await page.getByRole("option", { name: "全部模型" }).click();
  for (const slug of slugs) {
    await expect(row(slug)).toBeVisible();
    await expect(
      row(slug).getByRole("button", {
        name: `模型状态操作 ${slug}`,
        exact: true,
      }),
    ).toContainText("已启用");
    await checkbox(slug).click();
  }

  state.batchStateUpdateError = "atomic batch update failed";
  state.batchStateUpdateDelayMs = 1_000;
  await chooseBatchState("隐藏但启用", 2);
  await expect.poll(() => state.batchStateUpdates.length).toBe(3);
  await expect(batchStateButton(2)).toBeDisabled();
  await expect(page.getByRole("button", { name: "重新读取" })).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "批量分配路由 (2)" }),
  ).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "批量删除模型 (2)" }),
  ).toBeDisabled();
  for (const slug of slugs) await expect(checkbox(slug)).toBeDisabled();
  await expect(
    page.getByText(
      /批量更新模型状态失败.*atomic batch update failed/,
    ),
  ).toBeVisible();
  for (const slug of slugs) {
    await expect(row(slug)).toBeVisible();
    await expect(checkbox(slug)).toBeChecked();
    expect(state.models.find((model) => model.slug === slug)).toMatchObject({
      enabled: true,
      visibility: "list",
    });
  }
  await expect(batchStateButton(2)).toBeEnabled();
  for (const slug of slugs) await expect(checkbox(slug)).toBeEnabled();
  expect(state.upserts).toHaveLength(0);
  expect(state.stateUpdates).toHaveLength(0);
  expect(pageErrors).toEqual([]);
});

test("批量删除会隐藏内置模型并删除自定义模型", async ({ page }) => {
  const state = await installMockRuntime(page);
  state.models.push(importedModel());

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  const emptyBatchDelete = page.getByRole("button", {
    name: "批量删除模型 (0)",
  });
  await expect(emptyBatchDelete).toBeVisible();
  await expect(emptyBatchDelete).toBeDisabled();

  await page.getByLabel("选择模型 gpt-5.6-sol").click();
  await page.getByLabel("选择模型 imported-local").click();
  await page.getByRole("button", { name: "批量删除模型 (2)" }).click();

  const confirmDialog = page.getByRole("dialog", { name: "批量删除模型" });
  await expect(confirmDialog).toContainText(
    "1 个内置模型会被隐藏并禁用，其余自定义模型会被删除",
  );
  await confirmDialog.getByRole("button", { name: "删除", exact: true }).click();
  await expect(confirmDialog).toHaveCount(0);

  expect(state.deletes).toEqual(["gpt-5.6-sol", "imported-local"]);
  await expect(page.locator("tr", { hasText: "gpt-5.6-sol" })).toHaveCount(0);
  await expect(page.locator("tr", { hasText: "imported-local" })).toHaveCount(0);
  await expect(
    page.getByText("已隐藏 1 个内置模型，并删除 1 个自定义模型", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(emptyBatchDelete).toBeVisible();
  await expect(emptyBatchDelete).toBeDisabled();

  await page.getByRole("main").getByRole("combobox").click();
  await page.getByRole("option", { name: "已隐藏" }).click();
  const hiddenBuiltinRow = page.locator("tr", { hasText: "gpt-5.6-sol" });
  await expect(hiddenBuiltinRow).toBeVisible();
  await expect(hiddenBuiltinRow).toContainText("隐藏且禁用");
  await expect(hiddenBuiltinRow).toContainText("隐藏");
});

test("删除提交成功后刷新失败仍关闭确认框并保留成功结果", async ({
  page,
}) => {
  const state = await installMockRuntime(page);
  state.deleteDelayMs = 500;
  state.listErrorAfterDelete = "post-delete catalog reload failed";
  state.listDelayAfterDeleteMs = 1_500;

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "隐藏模型 gpt-5.4", exact: true })
    .click();

  const confirmDialog = page.getByRole("dialog", { name: "删除模型" });
  await confirmDialog.getByRole("button", { name: "删除", exact: true }).click();
  await expect.poll(() => state.deletes).toEqual(["gpt-5.4"]);

  await page.keyboard.press("Escape");
  await page.mouse.click(2, 2);
  await expect(confirmDialog).toBeVisible();
  await expect(
    confirmDialog.getByRole("button", { name: "处理中...", exact: true }),
  ).toBeDisabled();

  await expect(confirmDialog).toHaveCount(0, { timeout: 1_000 });
  await expect(
    page.getByRole("button", { name: "新增自定义模型" }),
  ).toBeDisabled();
  await expect(
    page.locator("tr", {
      has: page.getByText("gpt-5.4", { exact: true }),
    }),
  ).toHaveCount(0);
  await expect(
    page.getByText("已隐藏内置模型 gpt-5.4", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText(/读取模型失败.*post-delete catalog reload failed/),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "新增自定义模型" }),
  ).toBeEnabled();
  expect(state.models.find((model) => model.slug === "gpt-5.4")).toMatchObject({
    enabled: false,
    visibility: "hide",
    userEdited: true,
  });
});

test("批量删除部分失败时只保留失败模型并允许重试", async ({ page }) => {
  const state = await installMockRuntime(page);
  state.models.push(importedModel());
  state.deleteErrors["imported-local"] = "custom delete failed";

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();
  await page.getByLabel("选择模型 gpt-5.6-sol").click();
  await page.getByLabel("选择模型 imported-local").click();
  await page.getByRole("button", { name: "批量删除模型 (2)" }).click();
  await page
    .getByRole("dialog", { name: "批量删除模型" })
    .getByRole("button", { name: "删除", exact: true })
    .click();

  const retryDialog = page.getByRole("dialog", { name: "删除模型" });
  await expect(retryDialog).toBeVisible();
  await expect(
    page.getByText("批量处理完成：隐藏1个，删除0个，失败1个", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.locator("tr", { hasText: "gpt-5.6-sol" })).toHaveCount(0);
  await expect(page.locator("tr", { hasText: "imported-local" })).toBeVisible();
  await expect(page.getByLabel("选择模型 imported-local")).toBeChecked();

  delete state.deleteErrors["imported-local"];
  await retryDialog.getByRole("button", { name: "删除", exact: true }).click();
  await expect(retryDialog).toHaveCount(0);
  await expect(page.locator("tr", { hasText: "imported-local" })).toHaveCount(0);
  expect(state.deletes).toEqual([
    "gpt-5.6-sol",
    "imported-local",
    "imported-local",
  ]);
});

test("编辑器不依赖后续动画帧即可载入目标模型", async ({ page }) => {
  await installMockRuntime(page);
  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  await page.evaluate(() => {
    const testWindow = window as typeof window & {
      __nativeRequestAnimationFrame?: typeof window.requestAnimationFrame;
    };
    testWindow.__nativeRequestAnimationFrame = window.requestAnimationFrame;
    window.requestAnimationFrame = () => 1;
  });
  await page
    .getByRole("button", { name: "编辑模型 gpt-5.4", exact: true })
    .click();
  await expect(page.getByLabel("模型标识（Slug）")).toHaveValue("gpt-5.4");
  await expect(page.getByLabel("显示名称")).toHaveValue("GPT-5.4");
  await expect(page.getByLabel("描述")).toHaveValue("gpt-5.4 builtin");
  await expect(page.getByLabel("提供方")).toHaveValue("openai");
  await expect(page.getByLabel("模型系列")).toHaveValue("gpt-5");
  await expect(page.getByLabel("模型分类")).toHaveValue("reasoning");
  await expect(page.getByLabel("标签")).toHaveValue("coding");
  await expect(page.getByLabel("标签")).toHaveAttribute(
    "placeholder",
    "例如：编程, 推理",
  );
  await expect(page.getByLabel("排序")).toHaveValue("16");
  await expect(page.getByLabel("上下文窗口", { exact: true })).toHaveValue(
    "272000",
  );
  await expect(page.getByLabel("最大上下文窗口", { exact: true })).toHaveValue(
    "1000000",
  );
  await expect(page.getByLabel("默认推理强度")).toHaveValue("medium");
  await expect(page.getByRole("combobox", { name: "可见性" })).toBeVisible();
});

test("长路由来源不会覆盖相邻的模型和批量路由字段", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await installMockRuntime(page);
  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  await page.getByRole("button", { name: "新增自定义模型" }).click();
  const modelDialog = page.getByRole("dialog");
  await modelDialog.getByRole("tab", { name: "路由" }).click();
  await modelDialog.getByRole("button", { name: "添加聚合路由" }).click();
  await modelDialog.locator("#route-source-1").click();
  await page.getByRole("option", { name: `聚合 API：${LONG_AGGREGATE_NAME}` }).click();

  const routeSource = modelDialog.locator("#route-source-1");
  const upstreamModel = modelDialog.locator("#route-model-1");
  const routeCard = routeSource.locator('xpath=ancestor::div[@data-slot="card"][1]');
  await expect(routeSource).toContainText(`聚合 API：${LONG_AGGREGATE_NAME}`);
  const [routeSourceBox, upstreamModelBox, routeCardBox] = await Promise.all([
    routeSource.boundingBox(),
    upstreamModel.boundingBox(),
    routeCard.boundingBox(),
  ]);
  expect(routeSourceBox).not.toBeNull();
  expect(upstreamModelBox).not.toBeNull();
  expect(routeCardBox).not.toBeNull();
  expect(routeSourceBox!.x + routeSourceBox!.width).toBeLessThanOrEqual(
    upstreamModelBox!.x + 1,
  );
  expect(routeSourceBox!.x + routeSourceBox!.width).toBeLessThanOrEqual(
    routeCardBox!.x + routeCardBox!.width + 1,
  );

  await modelDialog.getByRole("button", { name: "取消" }).click();
  await page.getByLabel("选择模型 gpt-5.6-sol").click();
  await page.getByRole("button", { name: "批量分配路由 (1)" }).click();

  const batchDialog = page.getByRole("dialog", { name: "批量分配模型路由" });
  await batchDialog.getByRole("button", { name: "添加聚合路由" }).click();
  await batchDialog.locator("#batch-route-source-1").click();
  await page.getByRole("option", { name: `聚合 API：${LONG_AGGREGATE_NAME}` }).click();

  const batchSource = batchDialog.locator("#batch-route-source-1");
  const batchPriority = batchDialog.locator("#batch-route-priority-1");
  const batchCard = batchSource.locator('xpath=ancestor::div[@data-slot="card"][1]');
  await expect(batchSource).toContainText(`聚合 API：${LONG_AGGREGATE_NAME}`);
  const [compactDialogBox, batchSourceBox, batchPriorityBox, batchCardBox] =
    await Promise.all([
      batchDialog.boundingBox(),
      batchSource.boundingBox(),
      batchPriority.boundingBox(),
      batchCard.boundingBox(),
    ]);
  expect(compactDialogBox).not.toBeNull();
  expect(compactDialogBox!.width).toBeGreaterThanOrEqual(866);
  expect(batchSourceBox).not.toBeNull();
  expect(batchPriorityBox).not.toBeNull();
  expect(batchCardBox).not.toBeNull();
  expect(rectanglesOverlap(batchSourceBox!, batchPriorityBox!)).toBe(false);
  expect(batchSourceBox!.x + batchSourceBox!.width).toBeLessThanOrEqual(
    batchCardBox!.x + batchCardBox!.width + 1,
  );

  await page.setViewportSize({ width: 1600, height: 900 });
  const [wideDialogBox, wideSourceBox, widePriorityBox] = await Promise.all([
    batchDialog.boundingBox(),
    batchSource.boundingBox(),
    batchPriority.boundingBox(),
  ]);
  expect(wideDialogBox).not.toBeNull();
  expect(wideSourceBox).not.toBeNull();
  expect(widePriorityBox).not.toBeNull();
  expect(wideDialogBox!.width).toBeGreaterThanOrEqual(1228);
  expect(Math.abs(wideSourceBox!.y - widePriorityBox!.y)).toBeLessThanOrEqual(10);
  expect(wideSourceBox!.x + wideSourceBox!.width).toBeLessThanOrEqual(
    widePriorityBox!.x + 1,
  );
});

test("批量路由弹窗在小窗口内保留底部操作并允许正文滚动", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 560 });
  await installMockRuntime(page);
  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  for (const slug of Object.keys(PRICED_MODELS)) {
    await page
      .getByRole("checkbox", { name: `选择模型 ${slug}`, exact: true })
      .click();
  }
  await page.getByRole("button", { name: "批量分配路由 (8)" }).click();

  const dialog = page.getByRole("dialog", { name: "批量分配模型路由" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "添加聚合路由" }).click();
  await dialog.getByRole("button", { name: "添加账号池路由" }).click();
  await dialog.getByRole("button", { name: "添加聚合路由" }).click();

  const body = dialog.getByTestId("batch-route-dialog-body");
  const applyButton = dialog.getByRole("button", { name: "应用到 8 个模型" });
  const [dialogBox, applyButtonBox, bodyMetrics] = await Promise.all([
    dialog.boundingBox(),
    applyButton.boundingBox(),
    body.evaluate((element) => ({
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
    })),
  ]);
  const viewport = page.viewportSize();
  expect(dialogBox).not.toBeNull();
  expect(applyButtonBox).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(dialogBox!.height).toBeLessThanOrEqual(viewport!.height - 32 + 1);
  expect(applyButtonBox!.y + applyButtonBox!.height).toBeLessThanOrEqual(
    viewport!.height - 8,
  );
  expect(bodyMetrics.scrollHeight).toBeGreaterThan(bodyMetrics.clientHeight);

  const scrollTop = await body.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    return element.scrollTop;
  });
  expect(scrollTop).toBeGreaterThan(0);
});

test("模型目录支持中文展示并为多个模型批量分配路由", async ({ page }) => {
  const state = await installMockRuntime(page);

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  await expect(page.getByText("内置模型", { exact: true })).toBeVisible();
  await expect(page.getByText("自定义模型", { exact: true })).toBeVisible();
  await expect(page.getByText("价格缺失", { exact: true })).toBeVisible();
  await expect(page.getByText("路由缺失", { exact: true })).toBeVisible();
  await expect(
    page.getByText("最新的前沿智能体编程模型。", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Latest frontier agentic coding model.", { exact: true }),
  ).toHaveCount(0);
  await expect(page.getByRole("columnheader", { name: "来源" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "指令" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "路由" })).toBeVisible();
  await expect(
    page.getByText("请先勾选一个或多个模型，再使用批量分配路由。"),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "批量分配路由 (0)" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "批量分配路由 (0)" }),
  ).toBeDisabled();

  await page.getByLabel("选择模型 gpt-5.6-sol").click();
  await page.getByLabel("选择模型 gpt-5.6-terra").click();
  const batchRoutesButton = page.getByRole("button", {
    name: "批量分配路由 (2)",
  });
  await expect(batchRoutesButton).toBeEnabled();
  await batchRoutesButton.click();

  const dialog = page.getByRole("dialog", { name: "批量分配模型路由" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("gpt-5.6-sol", { exact: true })).toBeVisible();
  await expect(dialog.getByText("gpt-5.6-terra", { exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "添加聚合路由" }).click();
  await dialog.locator("#batch-route-source-1").click();
  await page.getByRole("option", { name: "聚合 API：Aggregate Test" }).click();
  await dialog.getByRole("button", { name: "应用到 2 个模型" }).click();

  await expect(dialog).toHaveCount(0);
  expect(state.upserts).toHaveLength(2);
  for (const upsert of state.upserts) {
    const model = upsert.model as JsonObject;
    const slug = String(model.slug);
    expect(model.routes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          sourceKind: "account_pool",
          sourceId: "default",
          upstreamModel: slug,
          enabled: true,
        }),
        expect.objectContaining({
          sourceKind: "aggregate_api",
          sourceId: "agg-1",
          upstreamModel: slug,
          enabled: true,
        }),
      ]),
    );
  }
});

test("模型目录 V2 完成本地管理、原子保存和导入", async ({ page }) => {
  const state = await installMockRuntime(page);

  await page.goto("/models/");
  await expect(
    page.getByRole("main").getByRole("heading", { name: "模型管理" }),
  ).toBeVisible();

  const rows = page.getByRole("main").locator("tbody tr");
  await expect(rows).toHaveCount(8);
  const solRow = page.locator("tr", { hasText: "gpt-5.6-sol" });
  await expect(solRow).toContainText("官方价格");
  await expect(solRow).toContainText("5 / 0.5 / 30");
  const imageRow = page.locator("tr", { hasText: "gpt-image-2" });
  await expect(imageRow).toContainText("先进的图像生成和编辑模型。");
  await expect(imageRow).toContainText("官方价格");
  await expect(imageRow).toContainText("8 / 2 / 30");
  await expect(page.getByText("codex-auto-review", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "远端并入" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "清理远端旧模型" })).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "导出到本地 Codex 缓存" }),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "新增自定义模型" }).click();
  await page.getByLabel("模型标识（Slug）").fill("my-custom-model");
  await page.getByLabel("显示名称").fill("My Custom Model");
  await page.getByLabel("描述").fill("local managed model");

  await page.getByRole("tab", { name: "价格" }).click();
  await page.locator("#price-input").fill("2.5");
  await page.locator("#price-cached").fill("0.25");
  await page.locator("#price-output").fill("15");
  await page.locator("#price-long-threshold").fill("272000");
  await page.locator("#price-long-input").fill("5");
  await page.locator("#price-long-cached").fill("0.5");
  await page.locator("#price-long-output").fill("22.5");

  await page.getByRole("tab", { name: "路由" }).click();
  await page.getByRole("button", { name: "添加聚合路由" }).click();
  await expect(page.getByRole("combobox", { name: "来源类型" })).toHaveCount(2);
  await expect(page.getByRole("switch", { name: "启用路由" })).toHaveCount(2);
  await page.locator("#route-source-1").click();
  await page.getByRole("option", { name: "聚合 API：Aggregate Test" }).click();
  await page.locator("#route-model-1").fill("upstream-custom-v1");

  await page.getByRole("tab", { name: "指令策略" }).click();
  await page.getByRole("combobox", { name: "指令模式" }).click();
  await page.getByRole("option", { name: "兜底" }).click();
  await page.locator("#model-instructions-text").fill("Use the local policy.");
  await page.getByRole("button", { name: "保存模型" }).click();

  const customRow = page.locator("tr", { hasText: "my-custom-model" });
  await expect(customRow).toBeVisible();
  expect(state.upserts).toHaveLength(1);
  const atomicSave = state.upserts[0];
  expect(atomicSave.previousSlug).toBeNull();
  const savedModel = atomicSave.model as JsonObject;
  expect(savedModel.price).toEqual({
    priceStatus: "custom",
    priceSource: "local-ui",
    inputMicrousdPer1m: 2_500_000,
    cachedInputMicrousdPer1m: 250_000,
    outputMicrousdPer1m: 15_000_000,
  });
  expect(savedModel.priceTiers).toEqual([
    {
      minInputTokens: 0,
      inputMicrousdPer1m: 2_500_000,
      cachedInputMicrousdPer1m: 250_000,
      outputMicrousdPer1m: 15_000_000,
    },
    {
      minInputTokens: 272_000,
      inputMicrousdPer1m: 5_000_000,
      cachedInputMicrousdPer1m: 500_000,
      outputMicrousdPer1m: 22_500_000,
    },
  ]);
  expect(savedModel.instructionsMode).toBe("fallback");
  expect(savedModel.instructionsText).toBe("Use the local policy.");
  expect(savedModel.routes).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        sourceKind: "account_pool",
        sourceId: "default",
        upstreamModel: "my-custom-model",
      }),
      expect.objectContaining({
        sourceKind: "aggregate_api",
        sourceId: "agg-1",
        upstreamModel: "upstream-custom-v1",
      }),
    ]),
  );

  await page.getByRole("button", { name: "隐藏模型 gpt-5.6-sol" }).click();
  await page.getByRole("button", { name: "删除", exact: true }).click();
  const builtinRow = page.locator("tr", { hasText: "gpt-5.6-sol" });
  await expect(builtinRow).toHaveCount(0);

  await page.getByRole("main").getByRole("combobox").click();
  await page.getByRole("option", { name: "已隐藏" }).click();
  await expect(builtinRow).toBeVisible();
  await expect(builtinRow).toContainText("隐藏且禁用");

  await page.getByRole("main").getByRole("combobox").click();
  await page.getByRole("option", { name: "全部模型" }).click();

  await page
    .getByRole("button", { name: "删除模型 my-custom-model" })
    .click();
  await page.getByRole("button", { name: "删除", exact: true }).click();
  await expect(customRow).toHaveCount(0);
  expect(state.deletes).toEqual(["gpt-5.6-sol", "my-custom-model"]);

  await page.getByRole("button", { name: "从本地 JSON 导入" }).click();
  const importDialog = page.getByRole("dialog");
  await importDialog.getByLabel("JSON", { exact: true }).fill(
    JSON.stringify({
      models: [
        {
          slug: "imported-local",
          display_name: "Imported Local",
          base_instructions: "must be ignored",
          unknown_field: true,
        },
      ],
    }),
  );
  await importDialog.getByRole("button", { name: "预览导入" }).click();
  await expect(importDialog.getByText("base_instructions", { exact: true })).toBeVisible();

  await importDialog.getByRole("combobox").click();
  await page.getByRole("option", { name: "替换自定义模型" }).click();
  await importDialog.getByRole("button", { name: "预览导入" }).click();
  await importDialog.getByRole("button", { name: "提交导入" }).click();
  await expect(page.locator("tr", { hasText: "imported-local" })).toBeVisible();

  expect(
    state.importCalls.map((call) => [call.method, call.params.conflictStrategy]),
  ).toEqual([
    ["apikey/managedModelImportPreviewV2", "keep_existing"],
    ["apikey/managedModelImportPreviewV2", "replace_custom"],
    ["apikey/managedModelImportCommitV2", "replace_custom"],
  ]);
});
