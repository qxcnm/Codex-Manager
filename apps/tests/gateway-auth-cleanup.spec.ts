import { expect, test } from "@playwright/test";

test("gateway auth cleanup defaults on and explicit preservation survives reload", async ({ page }) => {
  let settings: Record<string, unknown> = {
    locale: "en",
    codexCliGuideDismissed: true,
    serviceAddr: "localhost:48760",
    theme: "tech",
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
    if (method === "initialize") result = { userAgent: "codex_cli_rs/test", codexHome: "C:/Test/.codex" };
    if (method === "accountManager/session/current") {
      result = { mode: "none", role: "system_admin", permissions: [], distributionEnabled: false };
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });
  await page.goto("/settings/");
  const control = page.getByRole("switch", { name: "Remove requires_openai_auth", exact: true });
  await expect(control).toBeChecked();
  await expect(page.locator("#settings-remove-openai-auth-description")).toContainText("may disable image extensions");
  await control.click();
  await expect.poll(() => patches.some((patch) => patch.removeRequiresOpenaiAuth === false)).toBe(true);
  await page.reload();
  await expect(control).not.toBeChecked();
  await control.click();
  await expect.poll(() => patches.some((patch) => patch.removeRequiresOpenaiAuth === true)).toBe(true);
  await page.reload();
  await expect(control).toBeChecked();
});
