import { expect, test, type Page } from "@playwright/test";

async function mockSettings(page: Page) {
  let settings: Record<string, unknown> = {
    locale: "auto",
    localeOptions: ["auto", "zh-CN", "en", "ru", "ko"],
    codexCliGuideDismissed: true,
    serviceAddr: "localhost:48760",
    theme: "tech",
    appearancePreset: "classic",
  };
  const patches: Record<string, unknown>[] = [];

  await page.route(/\/api\/runtime\/?(?:\?.*)?$/, (route) => route.fulfill({
    json: { mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false },
  }));
  await page.route(/\/api\/rpc\/?(?:\?.*)?$/, async (route) => {
    const { id, method, params } = route.request().postDataJSON();
    let result: unknown = {};
    if (method === "appSettings/get") result = settings;
    if (method === "appSettings/set") {
      patches.push(params);
      settings = { ...settings, ...params };
      result = settings;
    }
    if (method === "initialize") {
      result = { userAgent: "codex_cli_rs/0.1.19", codexHome: "C:/Users/Test/.codex" };
    }
    if (method === "accountManager/session/current") {
      result = { mode: "none", role: "system_admin", permissions: [], distributionEnabled: false };
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });
  return patches;
}

test.describe("English system language", () => {
  test.use({ locale: "en-US" });

  test("auto, manual Chinese, and returning to auto survive reload", async ({ page }) => {
    const patches = await mockSettings(page);
    await page.goto("/settings/");
    const row = page.getByTestId("settings-language-row");
    const trigger = row.getByRole("combobox");
    await expect(trigger).toHaveText(/Auto-detect/);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");

    await trigger.click();
    await page.getByRole("option", { name: "简体中文", exact: true }).click();
    await expect(trigger).toHaveText(/简体中文/);
    await expect(row).toContainText("界面语言");
    await expect.poll(() => patches.some((patch) => patch.locale === "zh-CN")).toBe(true);
    await page.reload();
    await expect(trigger).toHaveText(/简体中文/);
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");

    await trigger.click();
    await page.getByRole("option", { name: "自动检测", exact: true }).click();
    await expect(trigger).toHaveText(/Auto-detect/);
    await expect.poll(() => patches.some((patch) => patch.locale === "auto")).toBe(true);
    await page.reload();
    await expect(trigger).toHaveText(/Auto-detect/);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });
});

test.describe("Chinese system language", () => {
  test.use({ locale: "zh-CN" });

  test("auto uses Chinese and reacts to system language changes", async ({ page }, testInfo) => {
    await mockSettings(page);
    await page.setViewportSize({ width: 1100, height: 760 });
    await page.goto("/settings/");
    const row = page.getByTestId("settings-language-row");
    const trigger = row.getByRole("combobox");
    await expect(trigger).toHaveText(/自动检测/);
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
    await trigger.click();
    await expect(page.getByRole("option", { name: "自动检测", exact: true })).toHaveAttribute("aria-selected", "true");
    await page.screenshot({ path: testInfo.outputPath("language-auto-detect.png") });
    await page.keyboard.press("Escape");

    await page.evaluate(() => {
      Object.defineProperty(navigator, "language", { configurable: true, value: "en-US" });
      window.dispatchEvent(new Event("languagechange"));
    });
    await expect(trigger).toHaveText(/Auto-detect/);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });
});

test.describe("Unsupported system language", () => {
  test.use({ locale: "fr-FR" });

  test("auto falls back to English", async ({ page }) => {
    await mockSettings(page);
    await page.goto("/settings/");
    await expect(page.getByTestId("settings-language-row").getByRole("combobox")).toHaveText(/Auto-detect/);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });
});
