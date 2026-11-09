import { expect, test, type Page } from "@playwright/test";

interface FixtureAccount {
  id: string;
  label: string;
  status: string;
  sort: number;
  bpsEnabled?: boolean;
  bpsAuthorized?: boolean;
}

async function mountAccounts(page: Page, options: { pending?: boolean; admin?: boolean } = {}) {
  const accounts: FixtureAccount[] = [
    { id: "needs-auth", label: "first@example.com", status: "active", sort: 0 },
    { id: "authorized", label: "second@example.com", status: "active", sort: 1, bpsAuthorized: true },
  ];
  const updates: Array<{ accountIds: string[]; enabled: boolean }> = [];
  const starts: string[] = [];
  let polls = 0;
  let rejectNext = false;
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
    if (method === "accountManager/session/current") return ok({
      mode: "none", currentUser: null,
      role: options.admin === false ? "member" : "system_admin",
      permissions: options.admin === false ? [] : ["system:admin"], distributionEnabled: false,
    });
    if (method === "account/list") return ok({ items: accounts, total: accounts.length, page: 1, pageSize: 20 });
    if (method === "account/usage/list") return ok([]);
    if (method === "modelDiscovery/list") return ok({ items: [] });
    if (method === "account/bps/login/start") {
      starts.push(params.accountId);
      return ok({ loginId: "bps-test", userCode: "BPS-CODE", verificationUri: "https://auth.openai.com/api/accounts/deviceauth/authorize", interval: 1, expiresAt: Math.floor(Date.now() / 1000) + 300 });
    }
    if (method === "account/bps/login/status") {
      polls++;
      expect(params.accountId).toBe("needs-auth");
      expect(params.loginId).toBe("bps-test");
      if (options.pending) return ok({ status: "pending" });
      accounts[0].bpsAuthorized = true;
      return ok({ status: "completed" });
    }
    if (method === "account/bps/update") {
      if (rejectNext) {
        rejectNext = false;
        return route.fulfill({ json: { jsonrpc: "2.0", id, error: { code: -32000, message: "BPS test rejection" } } });
      }
      const update = { accountIds: params.accountIds as string[], enabled: params.enabled as boolean };
      updates.push(update);
      for (const account of accounts) {
        if (update.accountIds.includes(account.id)) account.bpsEnabled = update.enabled;
      }
      return ok({ updated: update.accountIds.length });
    }
    return ok({});
  });
  await page.goto("/accounts/");
  await expect(page.getByRole("heading", { name: "OpenAI 账号池" })).toBeVisible();
  return { accounts, updates, starts, polls: () => polls, rejectNextUpdate: () => { rejectNext = true; } };
}

test("BPS forwarding requires per-account authorization and bulk changes preserve saved state", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const fixture = await mountAccounts(page);
  const rows = page.locator(".account-pool-main-table tbody [data-account-pool-main-row]");
  await expect(rows).toHaveCount(2);
  await expect(page.getByRole("switch", { name: "BPS 转发：关", exact: true })).toHaveCount(2);
  for (const row of await rows.all()) await row.getByRole("checkbox").check();
  await page.getByText("账号操作", { exact: true }).click();
  await expect(page.getByText("BPS 转发（选中 2 个）", { exact: true })).toBeVisible();
  await page.getByRole("menuitem", { name: /批量开启 BPS 转发/ }).click();
  await expect(page.getByText(/本次未开启任何账号/)).toBeVisible();
  expect(fixture.updates).toEqual([]);

  await rows.first().getByRole("switch", { name: "BPS 转发：关", exact: true }).click();
  const riskDialog = page.getByRole("dialog", { name: "BPS 转发风险提示" });
  await expect(riskDialog.getByText(/使用风险由你承担/)).toBeVisible();
  await riskDialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(fixture.starts).toEqual([]);
  expect(fixture.updates).toEqual([]);
  await rows.first().getByRole("switch", { name: "BPS 转发：关", exact: true }).click();
  await page.screenshot({ path: test.info().outputPath("bps-risk-confirmation.png"), animations: "disabled" });
  await riskDialog.getByRole("button", { name: "了解风险并授权", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "BPS 独立授权" });
  await expect(dialog.getByText("BPS 独立授权", { exact: true })).toBeVisible();
  await expect(dialog.getByText("先开启授权码登录", { exact: true })).toBeVisible();
  await expect(dialog.getByText(/授权前，请先在 ChatGPT/)).toBeVisible();
  expect(fixture.starts).toEqual([]);
  await dialog.getByRole("button", { name: "开始 BPS 授权", exact: true }).click();
  await expect(dialog.getByRole("textbox", { name: "授权验证码" })).toHaveValue("BPS-CODE");
  await expect.poll(() => fixture.updates).toEqual([{ accountIds: ["needs-auth"], enabled: true }]);
  await expect(dialog.getByText("BPS 授权已保存", { exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "关闭", exact: true }).click();
  await expect(rows.first().getByRole("switch", { name: "BPS 转发：开", exact: true })).toBeChecked();

  await page.getByText("账号操作", { exact: true }).click();
  await page.getByRole("menuitem", { name: /批量开启 BPS 转发/ }).click();
  await expect(riskDialog.getByText(/本次将为 2 个账号开启/)).toBeVisible();
  await riskDialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(fixture.updates).toHaveLength(1);
  await page.getByText("账号操作", { exact: true }).click();
  await page.getByRole("menuitem", { name: /批量开启 BPS 转发/ }).click();
  await expect(riskDialog).toHaveCount(1);
  await riskDialog.getByRole("button", { name: "了解风险并开启", exact: true }).click();
  await expect.poll(() => fixture.updates.at(-1)).toEqual({ accountIds: ["needs-auth", "authorized"], enabled: true });
  await expect(page.getByRole("switch", { name: "BPS 转发：开", exact: true })).toHaveCount(2);
  await page.getByText("账号操作", { exact: true }).click();
  await page.getByRole("menuitem", { name: /批量关闭 BPS 转发/ }).click();
  await expect(riskDialog).toBeHidden();
  await expect(page.getByRole("switch", { name: "BPS 转发：关", exact: true })).toHaveCount(2);

  fixture.rejectNextUpdate();
  await rows.first().getByRole("switch", { name: "BPS 转发：关", exact: true }).click();
  await riskDialog.getByRole("button", { name: "了解风险并开启", exact: true }).click();
  await expect(page.getByText(/更新 BPS 转发失败/)).toBeVisible();
  await expect(rows.first().getByRole("switch", { name: "BPS 转发：关", exact: true })).not.toBeChecked();
  await page.getByRole("button", { name: "宫格视图" }).click();
  await expect(page.getByTestId("account-grid")).toBeVisible();
  await expect(page.getByRole("switch", { name: "BPS 转发：关", exact: true })).toHaveCount(2);
  await expect(page.getByRole("button", { name: /后台模型同步/ })).toHaveCount(2);
  await page.screenshot({ path: test.info().outputPath("bps-account-card.png"), animations: "disabled" });
  expect(fixture.accounts.map((account) => account.status)).toEqual(["active", "active"]);
});

test("closing BPS authorization stops polling and never enables the account", async ({ page }) => {
  const fixture = await mountAccounts(page, { pending: true });
  await page.getByRole("switch", { name: "BPS 转发：关", exact: true }).first().click();
  await page.getByRole("dialog", { name: "BPS 转发风险提示" }).getByRole("button", { name: "了解风险并授权", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "BPS 独立授权" });
  await dialog.getByRole("button", { name: "开始 BPS 授权", exact: true }).click();
  await expect.poll(fixture.polls).toBeGreaterThanOrEqual(1);
  await dialog.getByRole("button", { name: "关闭", exact: true }).click();
  await expect(dialog).toBeHidden();
  const previousPolls = fixture.polls();
  await page.waitForTimeout(1500);
  expect(fixture.polls()).toBe(previousPolls);
  expect(fixture.updates).toEqual([]);
});

test("BPS controls are hidden from non-admin account views", async ({ page }) => {
  const fixture = await mountAccounts(page, { admin: false });
  await expect(page.getByRole("link", { name: "我的概览", exact: true })).toBeVisible();
  await expect(page.getByRole("switch", { name: /BPS 转发/ })).toHaveCount(0);
  await expect(page.getByRole("menuitem", { name: /BPS 转发/ })).toHaveCount(0);
  expect(fixture.updates).toEqual([]);
});
