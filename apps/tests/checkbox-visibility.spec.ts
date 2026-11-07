import { expect, test, type Locator, type Page } from "@playwright/test";
import type { ManagedModelV2 } from "../src/types/model-v2";

const themes = ["tech", "dark", "dark-one", "business", "mint", "sunset", "grape", "ocean", "forest", "rose", "slate", "aurora"];

function fixtureModel(slug: string, sortOrder: number): ManagedModelV2 {
  return {
    id: `fixture:${slug}`, slug, displayName: slug, description: null,
    provider: "fixture", family: null, category: null, tags: [], origin: "custom",
    enabled: true, supportedInApi: true, visibility: "list", sortOrder,
    contextWindow: null, maxContextWindow: null, defaultReasoningEffort: null,
    capabilities: {}, instructionsMode: "passthrough", instructionsText: null,
    fastPolicy: "passthrough", builtinRevision: null, userEdited: false,
    price: { priceStatus: "missing", priceSource: null, inputMicrousdPer1m: null,
      cachedInputMicrousdPer1m: null, cacheWriteMicrousdPer1m: null, outputMicrousdPer1m: null },
    priceTiers: [], permissionGroupIds: [], createdAt: 1, updatedAt: 1,
    routes: [{ id: `route:${slug}`, sourceKind: "account_pool", sourceId: "default",
      upstreamModel: slug, enabled: true, priority: 0, weight: 1 }],
  };
}

// Same account/model RPC contracts as source-model-sync and models-management;
// every record and response is local to this browser test.
async function mockRuntime(page: Page) {
  const models = [fixtureModel("visibility-a", 0), fixtureModel("visibility-b", 1)];
  let releaseUpdate = () => {};
  const updateGate = new Promise<void>((resolve) => { releaseUpdate = resolve; });
  let updateCalls = 0;
  await page.route("**/api/runtime**", (route) => route.fulfill({ json: {
    mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false,
  } }));
  await page.route("**/api/rpc**", async (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    const ok = (result: unknown) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "appSettings/get") return ok({
      locale: "zh-CN", serviceAddr: "localhost:48760", theme: "tech",
      codexCliGuideDismissed: true, appearancePreset: "modern", lowTransparency: false,
      accountsAutoRefreshEnabled: false,
    });
    if (method === "initialize") return ok({ userAgent: "codex_cli_rs/test", codexHome: "C:/Test" });
    if (method === "accountManager/session/current") return ok({ mode: "none", role: "system_admin", permissions: ["*"], distributionEnabled: false });
    if (method === "account/list") return ok({ items: [
      { id: "visibility-a", label: "Visibility A", status: "active", hasToken: true, sort: 0 },
      { id: "visibility-b", label: "Visibility B", status: "active", hasToken: true, sort: 1 },
    ], total: 2, page: 1, pageSize: 20 });
    if (method === "account/usage/list") return ok([]);
    if (method === "aggregateApi/list" || method === "modelDiscovery/list") return ok({ items: [] });
    if (method === "codexProfile/get") return ok({ mode: "gateway", managedCatalogActive: false, warnings: [] });
    if (method === "codexProfile/listCandidates") return ok({ accounts: [], apiKeys: [] });
    if (method === "apikey/managedModelListV2") return ok({ items: models,
      stats: { total: 2, enabled: models.filter((model) => model.enabled).length,
        builtin: 0, custom: 2, priceMissing: 2, missingRoute: 0 } });
    if (method === "apikey/managedModelUpdateStateV2") {
      updateCalls += 1;
      await updateGate;
      const model = models.find((item) => item.slug === params.slug)!;
      Object.assign(model, { enabled: params.enabled, visibility: params.visibility });
      return ok(model);
    }
    return ok({});
  });
  return { releaseUpdate, updateCalls: () => updateCalls };
}

async function appearance(page: Page, theme: string, preset = "modern", lowTransparency = false) {
  await page.evaluate(({ theme, preset, lowTransparency }) => {
    document.documentElement.setAttribute("data-theme", theme);
    document.documentElement.setAttribute("data-appearance", preset);
    document.body.classList.toggle("low-transparency", lowTransparency);
  }, { theme, preset, lowTransparency });
  await page.mouse.move(0, 0);
}

async function prepare(page: Page, path = "/accounts/") {
  const runtime = await mockRuntime(page);
  await page.goto(path);
  await expect(page.getByRole("checkbox").first()).toBeVisible();
  // Remove timing noise only; colors, surfaces and control styles stay untouched.
  await page.addStyleTag({ content: "*, *::before, *::after { animation: none !important; transition: none !important; }" });
  return runtime;
}

type ContrastSample = { label: string; ratio: number; border: string; surfaces: number[][] };

async function measureBorders(page: Page, controls: Array<{ label: string; control: Locator }>): Promise<ContrastSample[]> {
  const boxes = await Promise.all(controls.map(async ({ label, control }) => {
    await expect(control).toBeVisible();
    return control.evaluate((element, label) => {
      const bounds = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      let opacity = 1;
      for (let parent: Element | null = element; parent; parent = parent.parentElement) {
        opacity *= Number(getComputedStyle(parent).opacity);
      }
      return { label, x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height,
        border: style.borderTopColor, background: style.backgroundColor, opacity,
        borderWidth: parseFloat(style.borderTopWidth), borderStyle: style.borderTopStyle };
    }, label);
  }));
  for (const box of boxes) {
    expect(box.borderWidth, `${box.label}: visible border width`).toBeGreaterThanOrEqual(1);
    expect(box.borderStyle).not.toBe("none");
  }
  const screenshot = await page.screenshot({ animations: "disabled", scale: "css" });
  return page.evaluate(async ({ png, boxes }) => {
    const bitmap = new Image();
    bitmap.src = `data:image/png;base64,${png}`;
    await bitmap.decode();
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const pixels = canvas.getContext("2d", { willReadFrequently: true })!;
    pixels.drawImage(bitmap, 0, 0);
    const colorCanvas = document.createElement("canvas");
    colorCanvas.width = colorCanvas.height = 1;
    const colors = colorCanvas.getContext("2d", { willReadFrequently: true })!;
    const rgba = (value: string) => {
      colors.clearRect(0, 0, 1, 1);
      colors.fillStyle = value;
      colors.fillRect(0, 0, 1, 1);
      const data = [...colors.getImageData(0, 0, 1, 1).data];
      return [data[0], data[1], data[2], data[3] / 255];
    };
    const over = (front: number[], back: number[]) => front.slice(0, 3).map((value, index) => value * front[3] + back[index] * (1 - front[3]));
    const luminance = (rgb: number[]) => rgb.map((value) => {
      const channel = value / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
    const contrast = (a: number[], b: number[]) => {
      const left = luminance(a), right = luminance(b);
      return (Math.max(left, right) + 0.05) / (Math.min(left, right) + 0.05);
    };
    return boxes.map((box) => {
      // Sample the rendered surface on all four sides. This includes gradients,
      // glass transparency and row hover/selection, unlike comparing CSS tokens.
      const surfaces = [
        [box.x - 3, box.y + box.height / 2],
        [box.x + box.width + 3, box.y + box.height / 2],
        [box.x + box.width / 2, box.y - 3],
        [box.x + box.width / 2, box.y + box.height + 3],
      ].map(([x, y]) => [...pixels.getImageData(Math.floor(x), Math.floor(y), 1, 1).data].slice(0, 3));
      const border = rgba(box.border), background = rgba(box.background);
      const ratio = Math.min(...surfaces.map((surface) => {
        const paintedBorder = over(border, over(background, surface));
        return contrast(over([...paintedBorder, box.opacity], surface), surface);
      }));
      return { label: box.label, ratio, border: box.border, surfaces };
    });
  }, { png: screenshot.toString("base64"), boxes });
}

async function assertContrast(page: Page, controls: Array<{ label: string; control: Locator }>, context: string) {
  const measurements = await measureBorders(page, controls);
  for (const sample of measurements) {
    expect(sample.ratio, `${context} ${sample.label}: ${JSON.stringify(sample)}`).toBeGreaterThanOrEqual(3);
  }
  return measurements;
}

test.use({ locale: "zh-CN", viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });

test("modern account table and cards retain visible unchecked, checked and mixed borders across every theme", async ({ page }, testInfo) => {
  test.setTimeout(90_000);
  await prepare(page);
  await page.getByRole("button", { name: "列表视图", exact: true }).click();
  const checked = page.getByRole("checkbox", { name: "选择账号 Visibility A", exact: true });
  const unchecked = page.getByRole("checkbox", { name: "选择账号 Visibility B", exact: true });
  const mixed = page.getByRole("checkbox", { name: "全选", exact: true });
  await checked.check();
  await page.getByRole("heading", { name: "OpenAI 账号池" }).click();
  await expect(mixed).toHaveAttribute("aria-checked", "mixed");
  const readings: unknown[] = [];
  for (const view of ["列表视图", "宫格视图"]) {
    await page.getByRole("button", { name: view, exact: true }).click();
    await expect(checked).toBeChecked();
    await expect(unchecked).not.toBeChecked();
    await expect(mixed).toHaveAttribute("aria-checked", "mixed");
    for (const theme of themes) {
      await appearance(page, theme);
      readings.push({ theme, view, samples: await assertContrast(page, [
        { label: "unchecked", control: unchecked }, { label: "checked", control: checked }, { label: "mixed", control: mixed },
      ], `${theme}/${view}`) });
    }
    await appearance(page, "grape");
    await page.screenshot({ path: testInfo.outputPath(`checkbox-modern-${view === "列表视图" ? "table" : "cards"}.png`), animations: "disabled" });
  }
  await testInfo.attach("checkbox-contrast-measurements", { body: JSON.stringify(readings, null, 2), contentType: "application/json" });
});

test("light and dark selections preserve contrast in classic and low transparency modes and work from the keyboard", async ({ page }, testInfo) => {
  await prepare(page);
  await page.getByRole("button", { name: "列表视图", exact: true }).click();
  const first = page.getByRole("checkbox", { name: "选择账号 Visibility A", exact: true });
  const second = page.getByRole("checkbox", { name: "选择账号 Visibility B", exact: true });
  const all = page.getByRole("checkbox", { name: "全选", exact: true });
  await first.check();
  await page.getByRole("heading", { name: "OpenAI 账号池" }).click();
  for (const theme of ["tech", "dark-one"]) {
    for (const [preset, lowTransparency] of [["classic", false], ["modern", true]] as const) {
      await appearance(page, theme, preset, lowTransparency);
      await assertContrast(page, [
        { label: "unchecked", control: second }, { label: "checked", control: first }, { label: "mixed", control: all },
      ], `${theme}/${preset}/lowTransparency=${lowTransparency}`);
    }
  }
  const mixedImage = await all.screenshot();
  const restingImage = await second.screenshot();
  await second.hover();
  expect((await second.screenshot()).equals(restingImage), "hover must provide a visible response").toBe(false);
  await assertContrast(page, [{ label: "hovered unchecked", control: second }], "dark hover");
  await page.mouse.move(0, 0);
  await all.focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(all).toBeFocused();
  expect(await all.evaluate((element) => element.matches(":focus-visible"))).toBe(true);
  const focus = await all.evaluate((element) => {
    const style = getComputedStyle(element);
    return { outline: parseFloat(style.outlineWidth), outlineStyle: style.outlineStyle,
      shadow: style.boxShadow };
  });
  expect(focus.outline >= 2 && focus.outlineStyle !== "none" || focus.shadow !== "none", "keyboard focus has a visible outline or ring").toBe(true);
  await page.keyboard.press("Space");
  await expect(all).toBeChecked();
  await expect(first).toBeChecked();
  await expect(second).toBeChecked();
  await page.getByRole("heading", { name: "OpenAI 账号池" }).click();
  expect((await all.screenshot()).equals(mixedImage), "mixed and checked indicators must look different").toBe(false);
  await all.focus();
  await page.keyboard.press("Space");
  await expect(first).not.toBeChecked();
  await expect(second).not.toBeChecked();
  await page.screenshot({ path: testInfo.outputPath("checkbox-dark-low-transparency-keyboard.png"), animations: "disabled" });
});

test("model selection becomes visibly disabled while a real UI operation is pending and cannot change", async ({ page }, testInfo) => {
  const runtime = await prepare(page, "/models/");
  const first = page.getByRole("checkbox", { name: "选择模型 visibility-a", exact: true });
  const second = page.getByRole("checkbox", { name: "选择模型 visibility-b", exact: true });
  const all = page.getByRole("checkbox", { name: "选择全部模型", exact: true });
  await first.check();
  await expect(all).toHaveAttribute("aria-checked", "mixed");
  await appearance(page, "tech");
  await assertContrast(page, [{ label: "model unchecked", control: second }, { label: "model checked", control: first }, { label: "model mixed", control: all }], "models");
  const enabledOpacity = await second.evaluate((element) => getComputedStyle(element).opacity);
  await page.getByRole("button", { name: "模型状态操作 visibility-a", exact: true }).click();
  await page.getByRole("menuitemradio", { name: "显示但禁用", exact: true }).click();
  try {
    await expect.poll(runtime.updateCalls).toBe(1);
    await expect(first).toBeDisabled();
    await expect(second).toBeDisabled();
    await expect(all).toBeDisabled();
    const disabled = await second.evaluate((element) => ({ opacity: getComputedStyle(element).opacity, cursor: getComputedStyle(element).cursor }));
    expect(Number(disabled.opacity)).toBeLessThan(Number(enabledOpacity));
    expect(disabled.cursor).toBe("not-allowed");
    const bounds = await second.boundingBox();
    await page.mouse.click(bounds!.x + bounds!.width / 2, bounds!.y + bounds!.height / 2);
    await page.keyboard.press("Space");
    await expect(first).toBeChecked();
    await expect(second).not.toBeChecked();
    await expect(all).toHaveAttribute("aria-checked", "mixed");
    await page.screenshot({ path: testInfo.outputPath("checkbox-models-disabled.png"), animations: "disabled" });
  } finally {
    runtime.releaseUpdate();
  }
  await expect(second).toBeEnabled();
  await second.check();
  await expect(all).toBeChecked();
});
