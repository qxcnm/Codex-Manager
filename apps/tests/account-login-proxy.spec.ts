import { expect, test } from "@playwright/test";

for (const loginType of ["chatgpt", "chatgptDeviceCode"]) {
  test(`${loginType} selects an existing proxy before login and resets the selection on close`, async ({ page }) => {
    const starts: Record<string, unknown>[] = [];
    const cancellations: string[] = [];
    const writes: string[] = [];
    await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
      mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
      canSelfUpdate: false, canCloseToTray: false, canOpenLocalDir: false,
      canUseBrowserFileImport: true, canUseBrowserDownloadExport: true,
    } }));
    await page.route("**/api/rpc**", async (route) => {
      const { id, method, params = {} } = route.request().postDataJSON();
      const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
      if (method === "appSettings/get") return ok({
        locale: "zh-CN", theme: "tech", serviceAddr: "localhost:48760",
        codexCliGuideDismissed: true, webAccessPasswordConfigured: false,
      });
      if (method === "initialize") return ok({ userAgent: "codex_cli_rs/0.164.0", codexHome: "C:/Test", platformFamily: "windows", platformOs: "windows" });
      if (method === "accountManager/session/current") return ok({ mode: "none", currentUser: null, role: "system_admin", permissions: ["system:admin"], distributionEnabled: false });
      if (method === "account/list") return ok({ items: [], total: 0, page: 1, pageSize: 20 });
      if (method === "account/usage/list") return ok([]);
      if (method === "system/proxy/list") return ok({ items: [
        { id: "proxy-sg", name: "Singapore", enabled: true, ip: "203.0.113.8" },
        { id: "proxy-disabled", name: "Disabled proxy", enabled: false },
      ] });
      if (method === "account/login/start") {
        starts.push(params);
        return ok(params.type === "chatgptDeviceCode"
          ? { type: "chatgptDeviceCode", loginId: `login-${starts.length}`, userCode: "FIXTURE", verificationUrl: "https://example.invalid/device" }
          : { type: "chatgpt", loginId: `login-${starts.length}`, authUrl: "https://example.invalid/authorize" });
      }
      if (method === "account/login/status") return ok({ status: "pending" });
      if (method === "account/login/cancel") {
        cancellations.push(params.loginId);
        return ok({ cancelled: true });
      }
      if (/set|update|create/i.test(method)) writes.push(method);
      return ok({});
    });
    await page.goto("/accounts/");
    await page.getByRole("button", { name: "账号操作", exact: true }).first().click();
    await page.getByRole("menuitem", { name: "添加账号", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog.getByText("浏览器授权页面仍使用浏览器自身的网络设置。", { exact: false })).toBeVisible();
    if (loginType === "chatgptDeviceCode") {
      await dialog.locator("#account-login-type").click();
      await page.getByRole("option", { name: "设备码登录", exact: true }).click();
    }
    await expect(dialog.locator("#account-login-proxy")).toBeEnabled();
    await dialog.locator("#account-login-proxy").click();
    await expect(page.getByRole("option", { name: /Disabled proxy/ })).toBeDisabled();
    await page.getByRole("option", { name: /Singapore/ }).click();
    await dialog.getByRole("button", { name: loginType === "chatgptDeviceCode" ? "生成设备验证码" : "登录授权", exact: true }).click();
    await expect.poll(() => starts.length).toBe(1);
    expect(starts[0]).toMatchObject({ type: loginType, proxyProfileId: "proxy-sg", openBrowser: false });

    // Changing the selected route cancels the in-flight session instead of silently changing it.
    await dialog.locator("#account-login-proxy").click();
    await page.getByRole("option", { name: "不指定（沿用默认网络）", exact: true }).click();
    await expect.poll(() => cancellations).toContain("login-1");
    await dialog.locator("#account-login-proxy").click();
    await page.getByRole("option", { name: /Singapore/ }).click();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await page.getByRole("button", { name: "账号操作", exact: true }).first().click();
    await page.getByRole("menuitem", { name: "添加账号", exact: true }).click();
    await expect(dialog.locator("#account-login-proxy")).toContainText("不指定（沿用默认网络）");
    expect(writes.filter((method) => method.includes("proxy"))).toEqual([]);
  });
}
