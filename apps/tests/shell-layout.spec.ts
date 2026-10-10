import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/runtime**", (route) =>
    route.fulfill({
      json: {
        mode: "web-gateway",
        rpcBaseUrl: "/api/rpc",
        canManageService: false,
        canSelfUpdate: false,
        canCloseToTray: false,
        canOpenLocalDir: false,
        canUseBrowserFileImport: true,
        canUseBrowserDownloadExport: true,
      },
    }),
  );

  await page.route("**/api/rpc**", (route) => {
    const request = route.request().postDataJSON();
    const resultByMethod: Record<string, unknown> = {
      "appSettings/get": {
        serviceAddr: "localhost:48760",
        theme: "tech",
        appearancePreset: "modern",
        locale: "zh-CN",
        lowTransparency: false,
        codexCliGuideDismissed: true,
        webAuthMode: "none",
      },
      initialize: {
        version: "0.6.2",
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "C:/Users/Test/.codex",
        platformFamily: "windows",
        platformOs: "windows",
      },
      "accountManager/session/current": {
        mode: "none",
        currentUser: null,
        role: "system_admin",
        permissions: [],
        distributionEnabled: false,
      },
    };

    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: request?.id ?? 1,
        result: resultByMethod[request?.method] ?? {},
      },
    });
  });
});

test("settings keep navigation and controls readable across window sizes", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/settings/");
  await expect(page.getByText("基础设置", { exact: true })).toBeVisible();

  const navigation = page.getByRole("navigation", { name: "CodexManager" });
  await expect(navigation.getByText("资源接入", { exact: true })).toBeVisible();
  await expect(navigation.getByText("平台配置", { exact: true })).toBeVisible();

  const basics = page.locator('[data-slot="card"]').filter({
    has: page.getByText("基础设置", { exact: true }),
  }).first();
  await expect(basics.locator('[data-slot="card"]')).toHaveCount(0);

  const mainText = await page.locator("main").innerText();
  expect(mainText.indexOf("基础设置")).toBeLessThan(mainText.indexOf("关于 CodexManager"));

  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => navigation.evaluate((element) => element.getBoundingClientRect().width)).toBeLessThanOrEqual(60);
  const tab = page.getByRole("tab", { name: "环境" });
  await tab.click();
  await expect(tab).toHaveAttribute("aria-selected", "true");
  const mainOverflow = await page.locator("main").evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(mainOverflow).toBeLessThanOrEqual(1);
});

test("short desktop windows can reach the last sidebar entry", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  await page.goto("/logs/");

  const sidebar = page.locator('[data-slot="app-sidebar"]');
  const scrollArea = page.locator('[data-slot="app-sidebar-scroll"]');
  const lastEntry = sidebar.getByRole("link", { name: "赞助与推荐" });
  const collapseButton = sidebar.getByRole("button", { name: "收起侧边栏" });

  await expect(lastEntry).toBeAttached();
  await expect.poll(() => scrollArea.evaluate((element) => element.scrollHeight - element.clientHeight)).toBeGreaterThan(0);
  await expect(scrollArea).toHaveCSS("scrollbar-width", "thin");
  await scrollArea.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  await expect(lastEntry).toBeInViewport();
  await expect(collapseButton).toBeInViewport();
  await lastEntry.click();
  await expect(lastEntry).toHaveAttribute("aria-current", "page");
  await page.reload();
  await expect(lastEntry).toBeInViewport();
});
