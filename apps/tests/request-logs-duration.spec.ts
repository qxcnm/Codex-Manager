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

const REQUEST_LOG = {
  trace_id: "trace-duration-1",
  key_id: "key-duration-1",
  account_id: "account-duration-1",
  actual_source_kind: "openai_account",
  actual_source_id: "account-duration-1",
  request_path: "/v1/responses",
  original_path: "/v1/responses",
  adapted_path: "/v1/chat/completions",
  method: "POST",
  request_type: "http",
  gateway_mode: "compact",
  model: "gpt-5.3-codex",
  upstream_model: "gpt-5.4-openai-compact",
  upstream_url: "https://chatgpt.com/backend-api/codex/responses",
  status_code: 200,
  duration_ms: 2345,
  first_response_ms: 340,
  input_tokens: 120,
  cached_input_tokens: 0,
  output_tokens: 34,
  total_tokens: 154,
  error: "",
  created_at: 1770000000,
};

async function mockLogs(
  page: Page,
  options: {
    log?: Partial<typeof REQUEST_LOG>;
    role?: "member" | "admin";
    theme?: "tech" | "dark";
    aggregateKey?: boolean;
  } = {},
) {
  const log = { ...REQUEST_LOG, ...options.log };
  const role = options.role || "admin";
  const state = { requestedPages: [] as number[], detailTraces: [] as string[], methods: [] as string[] };
  await page.route("**/*", (route) => ["localhost", "127.0.0.1", "::1"].includes(new URL(route.request().url()).hostname)
    ? route.continue() : route.abort());
  await page.route("**/api/runtime*", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
    canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
    canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
  } }));
  await page.route("**/api/rpc*", async (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    state.methods.push(method);
    const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "appSettings/get") return ok({
      ...SETTINGS_SNAPSHOT, theme: options.theme || "tech", lowTransparency: options.theme === "dark",
    });
    if (method === "initialize") return ok({
      userAgent: "codex_cli_rs/0.164.0", codexHome: "C:/Test", platformFamily: "windows", platformOs: "windows",
    });
    if (method === "accountManager/session/current") return ok({
      mode: "multi_user", currentUser: { id: "logs-user", username: "logs-user", role },
      role, permissions: role === "member" ? [] : ["system:admin"], distributionEnabled: true,
    });
    if (method === "account/list") return ok({
      items: [{ id: "account-duration-1", label: "测试账号", status: "active" }], total: 1, page: 1, pageSize: 10,
    });
    if (method === "apikey/list") return ok({ items: [{
      id: log.key_id, name: "测试密钥", aggregateApiId: options.aggregateKey ? "aggregate-fallback" : undefined,
    }] });
    if (method === "aggregateApi/list") return ok({ items: options.aggregateKey ? [{
      id: "aggregate-fallback", supplierName: "不应展示的聚合 API", url: "https://upstream.example/v1",
    }] : [] });
    if (method === "requestlog/list" || method === "requestlog/list_with_summary") {
      const requestedPage = Number(params.page) || 1;
      state.requestedPages.push(requestedPage);
      return ok({
        items: [log], total: 30, page: requestedPage, pageSize: 10,
        summary: {
          totalCount: 30, filteredCount: 30, successCount: log.error ? 0 : 1,
          errorCount: log.error ? 1 : 0, totalTokens: 154, totalCostUsd: 0,
        },
      });
    }
    if (method === "requestlog/detail") {
      state.detailTraces.push(params.traceId);
      return ok({
        traceId: params.traceId, stage: "client", stages: ["client"], storageMode: "preview",
        payload: JSON.stringify({ model: log.model, input: "fixture request payload" }),
        payloadBytes: 74, payloadTruncated: false, redacted: true, complete: true,
        createdAt: log.created_at,
      });
    }
    return ok({});
  });
  return state;
}

test("request logs retain durations, routes and pagination in table view", async ({ page }) => {
  const state = await mockLogs(page);
  await page.goto("/logs/");
  await expect(page.getByTestId("request-log-entry-trace-duration-1")).toBeVisible();
  await expect(page.getByRole("table")).toHaveCount(0);
  await page.getByRole("button", { name: "表格视图", exact: true }).click();

  await expect(page.getByRole("columnheader", { name: "用时 / 首响" })).toBeVisible();
  await expect(page.getByText("2.3s/340ms")).toBeVisible();
  await expect(page.getByText("/v1/responses", { exact: true })).toBeVisible();
  await expect(page.getByText("压缩", { exact: true })).toBeVisible();
  await expect(page.getByText("-> /v1/chat/completions")).toBeVisible();
  await expect(page.getByText("=> chatgpt.com/backend-api/codex/responses")).toBeVisible();
  await expect(page.getByText("转发 gpt-5.4-openai-compact")).toBeVisible();

  await page.getByLabel("跳至").fill("999");
  await page.getByRole("button", { name: "确定", exact: true }).click();
  await expect.poll(() => state.requestedPages).toContain(3);
  await expect(page.getByText("第 3 / 3 页")).toBeVisible();
  await page.getByRole("button", { name: "首页", exact: true }).click();
  await expect(page.getByText("第 1 / 3 页")).toBeVisible();
  await expect(page.getByRole("button", { name: "首页", exact: true })).toBeDisabled();
});

test("timeline expands request metadata without fetching the payload until requested", async ({ page }) => {
  const state = await mockLogs(page);
  await page.goto("/logs/");
  const entry = page.getByTestId("request-log-entry-trace-duration-1");
  await expect(entry).toBeVisible();
  await entry.locator("summary").click();
  await expect(entry.locator("details")).toHaveAttribute("open", "");
  await expect(entry.getByText("trace-duration-1", { exact: true }).last()).toBeVisible();
  expect(state.detailTraces).toEqual([]);
  await entry.getByRole("button", { name: "查看请求内容", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("fixture request payload");
  expect(state.detailTraces).toEqual(["trace-duration-1"]);
});

test("timeline displays explicit upstream errors as failures even with HTTP 200", async ({ page }) => {
  await mockLogs(page, { log: { status_code: 200, error: "upstream stream closed before completion" } });
  await page.goto("/logs/");
  const entry = page.getByTestId("request-log-entry-trace-duration-1");
  await expect(entry).toContainText("请求异常");
  await expect(entry.getByText("HTTP 502", { exact: true })).toBeVisible();
  await expect(entry).toContainText("upstream stream closed before completion");
});

test("timeline and expanded metadata prefer the actual account over a key's aggregate binding", async ({ page }) => {
  await mockLogs(page, { aggregateKey: true });
  await page.goto("/logs/");
  const entry = page.getByTestId("request-log-entry-trace-duration-1");
  await expect(entry.getByText("测试账号", { exact: true }).first()).toBeVisible();
  await entry.locator("summary").click();
  await expect(entry.getByText("测试账号", { exact: true }).last()).toBeVisible();
  await expect(entry.getByText("不应展示的聚合 API", { exact: true })).toHaveCount(0);
});

test("long request metadata fits narrow dark viewports and members cannot clear logs", async ({ page }) => {
  const longValue = "request-metadata-".repeat(40);
  await page.setViewportSize({ width: 390, height: 844 });
  await mockLogs(page, {
    role: "member", theme: "dark",
    log: { original_path: "/v1/" + longValue, model: longValue, error: longValue },
  });
  await page.goto("/logs/");
  const entry = page.getByTestId("request-log-entry-trace-duration-1");
  await expect(entry).toBeVisible();
  await expect(page.getByRole("button", { name: "清空日志", exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  await entry.locator("summary").click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
});
