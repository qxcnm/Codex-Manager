import { expect, test, type Page } from "@playwright/test";

const REMOTE_AUTHOR_CONTENT_URL = "https://author-config.example.com/api/public/author-content";

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
  authorSponsors: [
    {
      key: "aixiamo",
      name: "AI夏末 AIXiamo",
      description:
        "AIXiamo 面向 Codex CLI、Claude Code、Gemini CLI 等开发者场景，提供 ChatGPT Pro 5x / 20x、ChatGPT Plus、Claude Max、Gemini Pro、Grok 等 AI 会员开通与售后协助服务。支持支付宝 / 微信支付、自动充值、订单可查、教程说明与售后协助，适合需要稳定使用 AI 编程、代码生成、文档处理和高频对话的开发者用户。",
      href: "https://www.aixiamo.com/?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager",
      imageSrc: "/sponsors/aixiamo.jpg",
      imageAlt: "AI夏末 AIXiamo",
      actionLabel: "查看服务",
    },
    {
      key: "visioncoder",
      name: "VisionCoder",
      description:
        "VisionCoder 是一款高颜值、可灵活切换模型的桌面 AI 编程工具。它支持 Claude、Gemini、GPT，并集成 Claude Code、Gemini CLI、Codex、OpenCode 等多种 CLI 能力。",
      href: "https://coder.visioncoder.cn",
      imageSrc: "https://coder.visioncoder.cn/logo.png",
      imageAlt: "VisionCoder",
      actionLabel: "访问官网",
    },
    {
      key: "xingsiyan",
      name: "星思研中转站",
      description:
        "星思研中转站为 Claude Code、Codex、Gemini 等模型调用场景提供稳定中转与配套服务，适合需要高可用接口、便捷接入和持续交付支持的开发者与团队。",
      href: "https://gzxsy.vip/register?aff=eapz",
      imageSrc: "/sponsors/xingsiyan.jpg",
      imageAlt: "星思研中转站",
      actionLabel: "立即注册",
    },
  ],
  authorServerRecommendations: [
    {
      key: "racknerd",
      name: "RackNerd",
      description:
        "适合部署 CodexManager、网关转发服务和常规开发环境的 VPS 选择，适合需要稳定海外节点和可控成本的个人开发者或小团队。",
      href: "https://my.racknerd.com/aff.php?aff=19058",
      imageSrc: "https://racknerd.com/banners/125x125.gif",
      imageAlt: "RackNerd Square Banner",
      actionLabel: "查看套餐",
    },
  ],
  upstreamProxyUrl: "",
  upstreamStreamTimeoutMs: 600000,
  upstreamTotalTimeoutMs: 0,
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

async function mockRuntimeAndRpc(
  page: Page,
  authorContentMode: "success" | "empty" | "failure" = "success",
) {
  await page.route(/\/api\/runtime(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        mode: "web-gateway",
        rpcBaseUrl: "/api/rpc",
        authorContentUrl: REMOTE_AUTHOR_CONTENT_URL,
        canManageService: false,
        canSelfUpdate: false,
        canCloseToTray: false,
        canOpenLocalDir: false,
        canUseBrowserFileImport: true,
        canUseBrowserDownloadExport: true,
      }),
    });
  });

  await page.route(REMOTE_AUTHOR_CONTENT_URL, async (route) => {
    if (authorContentMode === "failure") {
      await route.fulfill({
        status: 503,
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({ error: "author content unavailable" }),
      });
      return;
    }

    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify(
        authorContentMode === "empty"
          ? {
              authorSponsors: [],
              authorServerRecommendations: [],
            }
          : {
              authorSponsors: [
                {
                  key: "remote-sponsor",
                  name: "远程赞助商",
                  description:
                    "感谢 **AIXiamo官方网站｜Codex / ChatGPT Pro** 赞助 CodexManager。  \nAIXiamo 面向 Codex CLI、 Claude Code、 Gemini CLI 等开发者场景，提供 ChatGPT Plus、ChatGPT Pro 5x / 20x、Codex 相关服务、Claude、Gemini、Grok 等 AI 会员开通与使用协助。",
                  href: "https://example.com/sponsor",
                  imageSrc: "assets/images/sponsors/aixiamo.jpg",
                  imageAlt: "远程赞助商",
                  actionLabel: "立即查看",
                },
              ],
              authorServerRecommendations: [
                {
                  key: "remote-server",
                  name: "远程服务器推荐",
                  description: "从独立管理站返回的服务器推荐内容。",
                  href: "https://example.com/server",
                  imageSrc: "https://example.com/server.png",
                  imageAlt: "远程服务器推荐",
                  actionLabel: "查看套餐",
                },
              ],
            },
      ),
    });
  });

  await page.route("**/api/rpc", async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const id = payload?.id ?? 1;

    const resultByMethod = {
      "appSettings/get": SETTINGS_SNAPSHOT,
      initialize: {
        userAgent: "codex_cli_rs/0.3.0",
        codexHome: "C:/Users/Test/.codex",
        platformFamily: "windows",
        platformOs: "windows",
      },
      "aggregateApi/list": { items: [] },
      "gateway/concurrencyRecommendation/get": {
        usageRefreshWorkers: 4,
        httpWorkerFactor: 4,
        httpWorkerMin: 8,
        httpStreamWorkerFactor: 1,
        httpStreamWorkerMin: 2,
        accountMaxInflight: 1,
      },
    } satisfies Record<string, unknown>;

    if (!(method in resultByMethod)) {
      await route.fulfill({
        status: 500,
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          error: {
            code: -32000,
            message: `Unhandled RPC method in test: ${method}`,
          },
        }),
      });
      return;
    }

    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        jsonrpc: "2.0",
        id,
        result: resultByMethod[method as keyof typeof resultByMethod],
      }),
    });
  });
}

test("author page splits sponsor content and contact content into two tabs", async ({
  page,
}) => {
  await mockRuntimeAndRpc(page);

  await page.goto("/author/");

  await expect(
    page.getByRole("heading", { name: "赞助与推荐", level: 2 }),
  ).toBeVisible();
  await expect(page.getByRole("tab", { name: "赞助 / 推荐" })).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "远程赞助商" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "远程服务器推荐" }),
  ).toBeVisible();

  const sponsorDescription = page.getByTestId(
    "author-partner-description-remote-sponsor",
  );
  await expect(sponsorDescription).toBeVisible();
  await expect(
    sponsorDescription.locator("strong", {
      hasText: "AIXiamo官方网站｜Codex / ChatGPT Pro",
    }),
  ).toBeVisible();
  await expect(sponsorDescription.locator("p")).toHaveCount(2);
  await expect(
    page.getByRole("img", { name: "远程赞助商" }),
  ).toHaveAttribute("src", /\/sponsors\/aixiamo\.jpg$/);

  const sponsorDescriptionMetrics = await sponsorDescription.evaluate((node) => {
    const paragraph = node.querySelectorAll("p")[1];
    return {
      clientWidth: node.clientWidth,
      scrollWidth: node.scrollWidth,
      paragraphHeight: paragraph?.getBoundingClientRect().height ?? 0,
    };
  });
  expect(sponsorDescriptionMetrics.scrollWidth).toBeLessThanOrEqual(
    sponsorDescriptionMetrics.clientWidth + 1,
  );
  expect(sponsorDescriptionMetrics.paragraphHeight).toBeGreaterThan(32);

  const partnerListOverflow = await page
    .getByTestId("author-partner-list")
    .evaluateAll((nodes) =>
      nodes.map((node) => node.scrollWidth - node.clientWidth),
    );
  expect(partnerListOverflow.every((overflow) => overflow <= 1)).toBe(true);

  const pageHorizontalOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(pageHorizontalOverflow).toBeLessThanOrEqual(1);

  await page.getByRole("tab", { name: "联系作者" }).click();

  await expect(page.getByRole("heading", { name: "联系作者" })).toBeVisible();
  await expect(page.getByText("ProsperGao", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "加入 TG 群聊" })).toBeVisible();
});

test("author page does not fall back to configured sponsor placeholders", async ({
  page,
}) => {
  await mockRuntimeAndRpc(page, "failure");

  await page.goto("/author/");

  await expect(page.getByText("暂无内容")).toBeVisible();
  await expect(page.getByRole("heading", { name: "AI夏末 AIXiamo" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "VisionCoder" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "RackNerd" })).toHaveCount(0);
});

test("author page shows an empty state when remote author content is empty", async ({
  page,
}) => {
  await mockRuntimeAndRpc(page, "empty");

  await page.goto("/author/");

  await expect(page.getByText("暂无内容")).toBeVisible();
  await expect(page.getByRole("heading", { name: "赞助商" })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "服务器推荐" })).toHaveCount(0);
});
