import { expect, test } from "@playwright/test";
import os from "node:os";
import path from "node:path";

test.use({ locale: "zh-CN", viewport: { width: 1600, height: 1000 } });

for (const source of ["account", "aggregateApi"] as const) {
  test(`${source} model association sends capabilities only for selected fetched models`, async ({ page }) => {
    const pageErrors: Error[] = [];
    page.on("pageerror", (error) => pageErrors.push(error));
    const metadata = {
      context_window: 272000,
      max_context_window: 872000,
      input_modalities: ["text", "image"],
      supports_parallel_tool_calls: true,
      supports_image_detail_original: false,
    };
    const associationPayloads: Record<string, unknown>[] = [];
    await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
      mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
    } }));
    await page.route("**/api/rpc**", async (route) => {
      const { id, method, params = {} } = route.request().postDataJSON();
      const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
      if (method === "appSettings/get") return ok({
        locale: "zh-CN", serviceAddr: "localhost:48760", theme: "tech",
        codexCliGuideDismissed: true, accountsAutoRefreshEnabled: false,
      });
      if (method === "initialize") return ok({ userAgent: "codex_cli_rs/test", codexHome: "C:/Test" });
      if (method === "accountManager/session/current") return ok({ mode: "none", role: "system_admin", permissions: ["system:admin"] });
      if (method === "account/list") return ok({ items: [{ id: "source-1", label: "Metadata account", status: "active", hasToken: true, sort: 0 }], total: 1, page: 1, pageSize: 20 });
      if (method === "account/usage/list") return ok([]);
      if (method === "aggregateApi/list") return ok({ items: [{ id: "source-1", supplierName: "Metadata supplier", providerType: "codex", url: "https://provider.invalid/v1", status: "active", modelSlugs: [], sort: 0 }] });
      if (method === "modelDiscovery/list") return ok({ items: [] });
      if (method === `${source}/fetchModels`) return ok({ items: [
        { upstreamModel: "selected-model", displayName: "Selected Model", metadata, existingModelSlug: null, alreadyLinked: false },
        { upstreamModel: "unselected-model", displayName: "Unselected Model", metadata: { context_window: 1000, input_modalities: ["text"] }, existingModelSlug: null, alreadyLinked: false },
      ] });
      if (method === `${source}/associateModels`) {
        associationPayloads.push(params);
        return ok({ createdModels: ["selected-model"], addedRoutes: ["selected-model"], unchangedRoutes: [] });
      }
      return ok({});
    });
    await page.goto(source === "account" ? "/accounts/" : "/aggregate-api/");
    await page.getByRole("button", { name: source === "account" ? "获取账号模型" : "拉取并关联模型", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "关联目录模型" });
    await dialog.getByRole("checkbox", { name: /^unselected-model\b/ }).click();
    const screenshotPath = path.join(os.tmpdir(), `codexmanager-model-metadata-${source}-${Date.now()}.png`);
    await page.screenshot({ path: screenshotPath, animations: "disabled" });
    console.log(`Metadata association screenshot: ${screenshotPath}`);
    await dialog.getByRole("button", { name: "关联所选模型 (1)", exact: true }).click();
    await expect.poll(() => associationPayloads.length).toBe(1);
    expect(associationPayloads[0].upstreamModels).toEqual(["selected-model"]);
    expect(associationPayloads[0].displayNames).toEqual({ "selected-model": "Selected Model" });
    expect(associationPayloads[0].modelMetadata).toEqual({ "selected-model": metadata });
    expect(associationPayloads[0].addr).toBe("localhost:48760");
    await expect(dialog).toBeHidden();
    expect(pageErrors).toEqual([]);
  });
}
