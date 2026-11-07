import { expect, test, type Page } from "@playwright/test";
import type { ModelDiscoverySourceKind, ModelDiscoveryTask, ModelDiscoveryUpdateInput } from "../src/types/model-discovery";

function task(sourceKind: ModelDiscoverySourceKind, sourceId: string, overrides: Partial<ModelDiscoveryTask> = {}): ModelDiscoveryTask {
  return {
    sourceKind, sourceId, enabled: false, intervalMinutes: 60, running: false,
    lastStartedAt: null, lastFinishedAt: null, lastSuccessAt: null, nextRunAt: null,
    lastError: null, lastWarning: null,
    discoveredCount: 0, createdModelCount: 0, addedRouteCount: 0,
    ...overrides,
  };
}

async function mockRuntime(page: Page, initialTasks: ModelDiscoveryTask[] = [], role = "system_admin") {
  const tasks = new Map(initialTasks.map((item) => [`${item.sourceKind}:${item.sourceId}`, item]));
  const updates: ModelDiscoveryUpdateInput[] = [];
  const calls: string[] = [];
  let rejectNextUpdate = false;
  await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
  } }));
  await page.route("**/api/rpc**", async (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    calls.push(method);
    const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "appSettings/get") return ok({
      locale: "zh-CN", serviceAddr: "localhost:48760", theme: "tech",
      codexCliGuideDismissed: true, appearancePreset: "classic",
      accountsAutoRefreshEnabled: false,
    });
    if (method === "initialize") return ok({ userAgent: "codex_cli_rs/test", codexHome: "C:/Test" });
    if (method === "accountManager/session/current") return ok({ mode: "none", role, permissions: [], distributionEnabled: false });
    if (method === "account/list") return ok({ items: [
      { id: "source-a", label: "Account A", status: "active", hasToken: true, sort: 0 },
      { id: "source-b", label: "Account B", status: "active", hasToken: true, sort: 1 },
    ], total: 2, page: 1, pageSize: 20 });
    if (method === "account/usage/list") return ok([]);
    if (method === "aggregateApi/list") return ok({ items: [
      { id: "source-a", supplierName: "Supplier A", providerType: "codex", url: "https://a.invalid/v1", status: "active", modelSlugs: [], sort: 0 },
      { id: "source-b", supplierName: "Supplier B", providerType: "claude", url: "https://b.invalid/v1", status: "active", modelSlugs: [], sort: 1 },
    ] });
    if (method === "modelDiscovery/list") return ok({ items: [...tasks.values()] });
    if (method === "modelDiscovery/update") {
      updates.push(params);
      if (rejectNextUpdate) {
        rejectNextUpdate = false;
        return route.fulfill({ json: { jsonrpc: "2.0", id, error: { code: -32000, message: "test update rejected" } } });
      }
      const key = `${params.sourceKind}:${params.sourceId}`;
      const previous = tasks.get(key) ?? task(params.sourceKind, params.sourceId);
      const next = {
        ...previous,
        enabled: params.enabled,
        intervalMinutes: params.intervalMinutes ?? previous.intervalMinutes,
        running: params.enabled,
        nextRunAt: params.enabled ? 1800003600 : null,
      };
      tasks.set(key, next);
      return ok(next);
    }
    return ok({});
  });
  return { tasks, updates, calls, rejectUpdate: () => { rejectNextUpdate = true; } };
}

test.use({ locale: "zh-CN", viewport: { width: 1600, height: 1000 } });

test("account background sync starts independently, validates intervals, and stops only the selected source", async ({ page }) => {
  const runtime = await mockRuntime(page, [task("aggregate_api", "source-a", { enabled: true })]);
  const pageErrors: Error[] = [];
  page.on("pageerror", (error) => pageErrors.push(error));
  await page.goto("/accounts/");
  const controls = page.getByRole("button", { name: "后台模型同步：未开启", exact: true, includeHidden: true });
  await expect(controls).toHaveCount(2);
  await controls.first().click();
  const dialog = page.getByRole("dialog", { name: "后台模型同步", exact: true });
  await expect(dialog.getByText("Account A", { exact: true })).toBeVisible();
  await expect(dialog.getByText(/使用这个账号获取模型列表/)).toBeVisible();
  const interval = dialog.getByRole("spinbutton", { name: "同步间隔（分钟）" });
  await expect(interval).toHaveValue("60");
  await interval.fill("4");
  await expect(dialog.getByRole("button", { name: "启动后台同步" })).toBeDisabled();
  await interval.fill("10081");
  await expect(dialog.getByRole("button", { name: "启动后台同步" })).toBeDisabled();
  await interval.fill("15");
  await dialog.getByRole("button", { name: "启动后台同步" }).click();
  await expect.poll(() => runtime.updates).toEqual([{ sourceKind: "openai_account", sourceId: "source-a", enabled: true, intervalMinutes: 15, addr: "localhost:48760" }]);
  await expect(dialog.getByRole("button", { name: "停止后台同步" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "后台模型同步：正在同步", exact: true, includeHidden: true })).toHaveCount(1);
  await interval.fill("30");
  await dialog.getByRole("button", { name: "保存同步间隔" }).click();
  await expect.poll(() => runtime.updates.at(-1)?.intervalMinutes).toBe(30);
  await expect(interval).toHaveValue("30");
  // A broken interval draft must never prevent stopping an existing task.
  await interval.fill("");
  await dialog.getByRole("button", { name: "停止后台同步" }).click();
  await expect.poll(() => runtime.updates.at(-1)).toEqual({ sourceKind: "openai_account", sourceId: "source-a", enabled: false, addr: "localhost:48760" });
  await expect(dialog.getByText("未安排", { exact: true })).toBeVisible();
  await expect(controls).toHaveCount(2);
  expect(runtime.tasks.get("aggregate_api:source-a")?.enabled).toBe(true);
  expect(runtime.tasks.has("openai_account:source-b")).toBe(false);
  expect(runtime.calls.filter((method) => /fetchModels|associateModels/.test(method))).toEqual([]);
  await page.screenshot({ path: test.info().outputPath("account-background-sync.png"), animations: "disabled" });
  expect(pageErrors).toEqual([]);
});

test("aggregate background sync shows saved results, retains state on update failure, and polls completion", async ({ page }) => {
  const runtime = await mockRuntime(page, [task("aggregate_api", "source-a", {
    enabled: true, intervalMinutes: 120, lastStartedAt: 1800000000,
    lastFinishedAt: 1800000010, lastSuccessAt: 1800000010, nextRunAt: 1800007210,
    discoveredCount: 3, createdModelCount: 2, addedRouteCount: 3,
    lastWarning: "models were saved but the active Codex profile could not be refreshed",
  })]);
  await page.goto("/aggregate-api/");
  await page.getByRole("button", { name: "后台模型同步：已开启", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "后台模型同步", exact: true });
  await expect(dialog.getByText("Supplier A", { exact: true })).toBeVisible();
  await expect(dialog.getByText("发现 3，新增模型 2，新增路由 3", { exact: true })).toBeVisible();
  await expect(dialog.getByText("模型已保存，但当前 Codex 配置未能刷新。")).toBeVisible();
  await expect(dialog.getByRole("spinbutton")).toHaveValue("120");
  runtime.rejectUpdate();
  await dialog.getByRole("button", { name: "停止后台同步" }).click();
  await expect(page.getByText("更新后台同步失败：test update rejected")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "停止后台同步" })).toBeEnabled();
  expect(runtime.updates).toHaveLength(1);
  expect(runtime.tasks.get("aggregate_api:source-a")?.enabled).toBe(true);
  // Simulate the service finishing a later run; list polling must reveal its error.
  const current = runtime.tasks.get("aggregate_api:source-a")!;
  runtime.tasks.set("aggregate_api:source-a", { ...current, lastError: "model discovery fetch failed; check source authorization and connectivity", lastFinishedAt: 1800000020 });
  await expect(dialog.getByText("拉取模型失败，请检查来源的授权和连接。", { exact: true })).toBeVisible({ timeout: 10_000 });
  await expect(dialog.getByText("等待重试", { exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "停止后台同步" }).click();
  await expect(dialog.getByRole("button", { name: "启动后台同步" })).toBeVisible();
  expect(runtime.tasks.has("aggregate_api:source-b")).toBe(false);
  await page.screenshot({ path: test.info().outputPath("aggregate-background-sync.png"), animations: "disabled" });
});

test("ordinary members do not request or control background model discovery", async ({ page }) => {
  const runtime = await mockRuntime(page, [], "member");
  await page.goto("/accounts/");
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();
  await expect(page.getByRole("button", { name: /后台模型同步/ })).toHaveCount(0);
  expect(runtime.calls.filter((method) => method.startsWith("modelDiscovery/"))).toEqual([]);
});
