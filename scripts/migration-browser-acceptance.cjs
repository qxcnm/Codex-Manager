// Real Chromium against the isolated codexmanager-web process created by the
// migration probe. No mocked transport, static test server, or real credentials.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("../apps/node_modules/@playwright/test");

async function main() {
  const [base, folder] = process.argv.slice(2);
  const endpoint = new URL(base);
  assert.equal(endpoint.hostname, "127.0.0.1", "fixture must be loopback only");
  const report = { scope: "local Chromium + real Web/Service; fixture identities", checks: [], result: "FAIL" };
  const browser = await chromium.launch({ headless: true });
  report.browser_version = browser.version();
  const context = await browser.newContext({ viewport: { width: 1365, height: 900 } });
  const page = await context.newPage();
  const pageErrors = [];
  const rpcStatuses = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("response", (response) => {
    if (new URL(response.url()).pathname === "/api/rpc") rpcStatuses.push(response.status());
  });
  try {
    await page.goto(base);
    await page.waitForURL("**/__login");
    await page.locator('input[name="username"]').waitFor({ state: "visible" });
    report.checks.push("anonymous_ui_redirects_to_login");
    await page.locator('input[name="username"]').fill("fixture-admin");
    await page.locator('input[name="password"]').fill("fixture-admin-password");
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(base + "/");
    await page.getByRole("button", { name: "本次关闭", exact: true }).click();
    report.checks.push("first_launch_guide_dismissed_through_ui");
    const logout = page.getByRole("button", { name: /退出登录|Sign out|Log out/i });
    await logout.waitFor({ state: "visible" });
    report.checks.push("login_bootstrap_executes_and_react_shell_renders");
    const cookies = await context.cookies(base);
    assert(cookies.some((cookie) => cookie.httpOnly && cookie.sameSite === "Lax"));
    const state = await page.evaluate(async () => {
      const response = await fetch("/__auth_status");
      const auth = await response.json();
      return { role: auth.role, sessionStorageEntries: sessionStorage.length };
    });
    assert.equal(state.role, "admin");
    assert(state.sessionStorageEntries > 0);
    report.checks.push("browser_cookie_and_tab_session_resolve_admin");
    await page.locator('a[href="/apikeys/"]').first().click();
    await page.waitForURL(base + "/apikeys/");
    await logout.waitFor({ state: "visible" });
    await page.getByText("Migration fixture", { exact: true }).first().waitFor({ state: "visible" });
    assert(rpcStatuses.includes(200), "rendered UI must call real RPC transport");
    report.checks.push("api_key_page_reads_fixture_through_real_rpc");
    await page.screenshot({ path: path.join(folder, "browser-api-keys.png"), fullPage: true });
    await logout.click();
    await page.waitForURL("**/__login?force=1");
    const status = await page.evaluate(async () => (await fetch("/api/rpc", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: "browser-logout", method: "accountManager/status" }),
    })).status);
    assert.equal(status, 401);
    report.checks.push("ui_logout_revokes_browser_rpc_session");
    assert.deepEqual(pageErrors, [], "uncaught browser exceptions");
    report.checks.push("no_uncaught_browser_exceptions");
    report.result = "PASS";
  } catch (error) {
    report.failure = error.message;
    await page.screenshot({ path: path.join(folder, "browser-failure.png"), fullPage: true }).catch(() => {});
  } finally {
    report.page_errors = pageErrors;
    report.rpc_status_counts = Object.fromEntries([...new Set(rpcStatuses)].map((status) =>
      [status, rpcStatuses.filter((value) => value === status).length]));
    await browser.close();
    fs.writeFileSync(path.join(folder, "browser-report.json"), JSON.stringify(report, null, 2) + "\n");
  }
  process.exitCode = report.result === "PASS" ? 0 : 1;
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
