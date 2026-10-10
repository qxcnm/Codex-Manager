import { expect, test, type Page } from "@playwright/test";

const SETTINGS_SNAPSHOT = {
  updateAutoCheck: true,
  closeToTrayOnClose: false,
  closeToTraySupported: false,
  lowTransparency: false,
  lightweightModeOnCloseToTray: false,
  keepWindowUiMounted: true,
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

const SKILL_REPOSITORIES = [
  {
    id: "builtin-anthropics-skills",
    name: "anthropics/skills",
    owner: "anthropics",
    repository: "skills",
    sourceUrl: "https://github.com/anthropics/skills",
    refName: "main",
    skillCount: 18,
  },
  {
    id: "builtin-composiohq-awesome-claude-skills",
    name: "ComposioHQ/awesome-claude-skills",
    owner: "ComposioHQ",
    repository: "awesome-claude-skills",
    sourceUrl: "https://github.com/ComposioHQ/awesome-claude-skills",
    refName: "master",
    skillCount: 864,
  },
  {
    id: "builtin-cexll-myclaude",
    name: "cexll/myclaude",
    owner: "cexll",
    repository: "myclaude",
    sourceUrl: "https://github.com/cexll/myclaude",
    refName: "master",
    skillCount: 11,
  },
  {
    id: "builtin-jimliu-baoyu-skills",
    name: "JimLiu/baoyu-skills",
    owner: "JimLiu",
    repository: "baoyu-skills",
    sourceUrl: "https://github.com/JimLiu/baoyu-skills",
    refName: "main",
    skillCount: 22,
  },
].map((repository) => ({
  ...repository,
  builtin: true,
  enabled: true,
  lastScannedAt: 1784764800,
  lastError: null,
}));

const REPOSITORY_SKILLS = [
  "Repository Skill",
  "Document Workflows",
  "Brand Guidelines",
  "Frontend Design",
  "Research Assistant",
  "Social Content",
].map((name, index) => {
  const repository = SKILL_REPOSITORIES[index % SKILL_REPOSITORIES.length];
  const skillId = name.toLocaleLowerCase().replaceAll(" ", "-");
  return {
    skillId,
    repositoryId: repository.id,
    name,
    description:
      "A standalone Skill discovered from a built-in GitHub repository.",
    author: repository.owner,
    category: index % 2 === 0 ? "Productivity" : "Creative",
    path: `skills/${skillId}/SKILL.md`,
    repositoryName: repository.name,
    repositoryOwner: repository.owner,
    repositoryRef: repository.refName,
    sourceUrl: `${repository.sourceUrl}/tree/${repository.refName}/skills/${skillId}`,
    installs: 128 + index * 17,
    installed: false,
    installedDirectoryName: null,
  };
});

const MARKETPLACE_PLUGINS = Array.from({ length: 12 }, (_, index) => {
  const number = String(index + 1).padStart(2, "0");
  return {
    plugin_id: `marketplace-plugin-${number}@test-marketplace`,
    name: `Marketplace Plugin ${number}`,
    marketplace_name: "test-marketplace",
    version: `1.0.${index}`,
    installed: index === 11,
    enabled: index === 11,
    description:
      "A Codex plugin with enough descriptive content to exercise the marketplace card layout.",
    author: "CodexManager Test",
    category: "Testing",
    skills: Array.from({ length: 4 }, (_, skillIndex) => ({
      name: `plugin-${number}-skill-${skillIndex + 1}`,
      description: "A representative Codex Skill used by the UI regression fixture.",
    })),
  };
});

const LONG_INSTALL_ERROR = Array.from(
  { length: 120 },
  (_, index) =>
    `git checkout failed at fixture step ${index + 1}: unable to install marketplace plugin`,
).join("\n");

async function mockRuntimeAndSkillsRpc(page: Page) {
  await page.route(/\/api\/runtime\/?(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify({
        mode: "web-gateway",
        rpcBaseUrl: "/api/rpc",
        canManageService: false,
        canSelfUpdate: false,
        canCloseToTray: false,
        canOpenLocalDir: false,
        canUseBrowserFileImport: true,
        canUseBrowserDownloadExport: true,
      }),
    });
  });

  await page.route(/\/api\/rpc\/?(?:\?.*)?$/, async (route) => {
    const payload = route.request().postDataJSON();
    const method = typeof payload?.method === "string" ? payload.method : "";
    const id = payload?.id ?? 1;

    const fulfillResult = (result: unknown) =>
      route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({ jsonrpc: "2.0", id, result }),
      });

    if (method === "appSettings/get") {
      await fulfillResult(SETTINGS_SNAPSHOT);
      return;
    }
    if (method === "initialize") {
      await fulfillResult({
        userAgent: "codex_cli_rs/0.1.19",
        codexHome: "/srv/codex",
        platformFamily: "linux",
        platformOs: "linux",
      });
      return;
    }
    if (method === "accountManager/session/current") {
      await fulfillResult({
        mode: "none",
        currentUser: null,
        role: "system_admin",
        permissions: [],
        distributionEnabled: false,
      });
      return;
    }
    if (method === "codexSkills/list") {
      await fulfillResult({
        codex_home: "/srv/codex",
        skills_root: "/srv/codex/skills",
        items: [],
        warnings: [],
      });
      return;
    }
    if (method === "codexSkills/repositoryList") {
      await fulfillResult({
        repositories: SKILL_REPOSITORIES,
        items: REPOSITORY_SKILLS,
        warnings: [],
      });
      return;
    }
    if (method === "codexSkills/registrySearch") {
      await fulfillResult({
        items: [
          {
            skillId: "registry-skill",
            repositoryId: "skills-sh",
            name: "skills.sh Result",
            description: "A public registry result.",
            author: "Community",
            category: "Testing",
            path: "registry-skill/SKILL.md",
            repositoryName: "skills.sh",
            repositoryOwner: "community",
            repositoryRef: "main",
            sourceUrl: "https://skills.sh/community/registry-skill",
            installs: 64,
            installed: false,
            installedDirectoryName: null,
          },
        ],
        total: 1,
        query: "",
        limit: 48,
        offset: 0,
        warnings: [],
      });
      return;
    }
    if (method === "codexSkills/marketplaceList") {
      await fulfillResult({
        cli_available: true,
        codex_home: "/srv/codex",
        marketplaces: [
          {
            name: "test-marketplace",
            source_type: "git",
            source: "https://github.com/example/test-marketplace.git",
          },
        ],
        plugins: MARKETPLACE_PLUGINS,
        warnings: [],
      });
      return;
    }
    if (method === "codexSkills/marketplacePluginInstall") {
      await route.fulfill({
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify({
          jsonrpc: "2.0",
          id,
          error: { code: -32000, message: LONG_INSTALL_ERROR },
        }),
      });
      return;
    }

    await route.fulfill({
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
  });
}

test("Skills and plugins are split while the inline plugin marketplace stays usable", async ({
  page,
}) => {
  const browserErrors: string[] = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") browserErrors.push(message.text());
  });
  const captureDirectory = process.env.CODEX_SKILLS_CAPTURE_DIR?.trim();
  await page.setViewportSize(
    captureDirectory ? { width: 2048, height: 1189 } : { width: 1280, height: 800 },
  );
  await mockRuntimeAndSkillsRpc(page);

  await page.goto("/skills/");
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { name: "Skills 与插件" })).toBeVisible();
  const skillsTab = main.getByRole("tab", { name: "Skills 安装" });
  const pluginsTab = main.getByRole("tab", { name: "Codex 插件安装" });
  await expect(skillsTab).toHaveAttribute("aria-selected", "true");
  await expect(main.getByRole("button", { name: "安装 ZIP" })).toBeVisible();
  await expect(main.getByTestId("codex-plugins-panel")).not.toBeVisible();
  const skillsPanel = main.getByTestId("skills-install-panel");
  await expect(
    skillsPanel.getByRole("heading", { name: "Repository Skill" }),
  ).toBeVisible();
  await expect(skillsPanel.getByText("128 次安装")).toBeVisible();
  await expect(skillsPanel.getByText("全部仓库", { exact: true })).toBeVisible();
  await expect(skillsPanel.getByText("全部状态", { exact: true })).toBeVisible();
  const repositorySearch = skillsPanel.getByRole("textbox", {
    name: "搜索 Skill、描述或作者",
  });
  const repositoryFilter = skillsPanel
    .getByRole("combobox")
    .filter({ hasText: "全部仓库" });
  const installFilter = skillsPanel
    .getByRole("combobox")
    .filter({ hasText: "全部状态" });
  const refreshButton = skillsPanel.getByRole("button", { name: "刷新" });
  const [searchBounds, repositoryBounds, installBounds, refreshBounds] =
    await Promise.all([
      repositorySearch.boundingBox(),
      repositoryFilter.boundingBox(),
      installFilter.boundingBox(),
      refreshButton.boundingBox(),
    ]);
  expect(searchBounds).not.toBeNull();
  expect(repositoryBounds).not.toBeNull();
  expect(installBounds).not.toBeNull();
  expect(refreshBounds).not.toBeNull();
  expect(searchBounds!.width).toBeGreaterThan(240);
  expect(repositoryBounds!.width).toBeGreaterThanOrEqual(200);
  expect(repositoryBounds!.width).toBeLessThanOrEqual(240);
  expect(installBounds!.width).toBeGreaterThanOrEqual(140);
  expect(installBounds!.width).toBeLessThanOrEqual(180);
  expect(searchBounds!.x + searchBounds!.width).toBeLessThan(repositoryBounds!.x);
  expect(repositoryBounds!.x + repositoryBounds!.width).toBeLessThan(
    installBounds!.x,
  );
  expect(installBounds!.x + installBounds!.width).toBeLessThan(refreshBounds!.x);
  if (captureDirectory) {
    await page.screenshot({
      path: `${captureDirectory}/skills-repository-catalog.png`,
      fullPage: true,
    });
  }

  await skillsPanel.getByRole("tab", { name: "skills.sh" }).click();
  await skillsPanel
    .getByRole("textbox", { name: "搜索 skills.sh" })
    .fill("registry");
  await expect(
    skillsPanel.getByRole("heading", { name: "skills.sh Result" }),
  ).toBeVisible();

  await skillsPanel.getByRole("button", { name: "管理仓库" }).click();
  const repositoriesDialog = page.getByRole("dialog", { name: "管理技能仓库" });
  await expect(repositoriesDialog).toBeVisible();
  await expect(
    repositoriesDialog.getByText("anthropics/skills", { exact: true }),
  ).toBeVisible();
  await expect(
    repositoriesDialog.getByText("ComposioHQ/awesome-claude-skills", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    repositoriesDialog.getByText("cexll/myclaude", { exact: true }),
  ).toBeVisible();
  await expect(
    repositoriesDialog.getByText("JimLiu/baoyu-skills", { exact: true }),
  ).toBeVisible();
  await expect(
    repositoriesDialog.getByRole("button", { name: "删除" }),
  ).toHaveCount(0);
  if (captureDirectory) {
    // Base UI dialogs animate independently from the page workspace.
    await page.waitForTimeout(300);
    await page.screenshot({
      path: `${captureDirectory}/skills-repository-management.png`,
      fullPage: true,
    });
  }
  await repositoriesDialog.getByRole("button", { name: "关闭" }).click();

  await pluginsTab.click();
  await expect(pluginsTab).toHaveAttribute("aria-selected", "true");
  await expect(main.getByRole("button", { name: "安装 ZIP" })).toHaveCount(0);

  const panel = main.getByTestId("codex-plugins-panel");
  await expect(panel).toBeVisible();
  await expect(
    panel.getByRole("heading", { name: "Codex 插件市场" }),
  ).toBeVisible();
  await expect(panel.getByText("已安装 1")).toBeVisible();
  await expect(panel.getByText("12 个兼容插件")).toBeVisible();
  await expect(
    panel.getByText(
      "插件中的 Skills 会随完整插件一起安装，不能在这里单独安装。",
    ),
  ).toBeVisible();

  const installedPluginCard = panel
    .getByRole("heading", { name: "Marketplace Plugin 12" })
    .locator("xpath=ancestor::article[1]");
  await expect(installedPluginCard).toBeVisible();
  await expect(
    installedPluginCard.getByRole("button", { name: "已由 Codex 安装" }),
  ).toBeDisabled();

  const scrollArea = panel.getByTestId("skills-marketplace-scroll");
  const viewport = scrollArea.locator('[data-slot="scroll-area-viewport"]');
  const scrollbar = scrollArea.locator(
    '[data-slot="scroll-area-scrollbar"][data-orientation="vertical"]',
  );
  const thumb = scrollbar.locator('[data-slot="scroll-area-thumb"]');

  await expect(viewport).toBeVisible();
  await expect
    .poll(() =>
      viewport.evaluate((element) => element.scrollHeight - element.clientHeight),
    )
    .toBeGreaterThan(0);
  await expect(scrollbar).toBeVisible();
  await expect(thumb).toBeVisible();

  const [scrollbarBox, thumbBox, scrollStyles, scrollbarWidth] = await Promise.all([
    scrollbar.boundingBox(),
    thumb.boundingBox(),
    scrollbar.evaluate((element) => {
      const styles = window.getComputedStyle(element);
      const thumb = element.querySelector<HTMLElement>(
        '[data-slot="scroll-area-thumb"]',
      );
      const thumbStyles = thumb ? window.getComputedStyle(thumb) : null;
      return {
        background: styles.backgroundColor,
        borderColor: styles.borderColor,
        opacity: styles.opacity,
        visibility: styles.visibility,
        thumbBackground: thumbStyles?.backgroundColor ?? "",
      };
    }),
    scrollbar.evaluate((element) =>
      Number.parseFloat(window.getComputedStyle(element).width),
    ),
  ]);
  expect(scrollbarBox).not.toBeNull();
  expect(thumbBox).not.toBeNull();
  expect(Math.round(scrollbarWidth)).toBe(12);
  expect(thumbBox!.width).toBeGreaterThanOrEqual(4);
  expect(thumbBox!.height).toBeGreaterThanOrEqual(52);
  expect(scrollStyles.visibility).toBe("visible");
  expect(scrollStyles.opacity).toBe("1");
  expect(scrollStyles.background).not.toBe("rgba(0, 0, 0, 0)");
  expect(scrollStyles.borderColor).not.toBe("rgba(0, 0, 0, 0)");
  expect(scrollStyles.thumbBackground).not.toBe("rgba(0, 0, 0, 0)");

  const viewportStyles = await viewport.evaluate((element) => {
    const styles = window.getComputedStyle(element);
    return {
      overflowY: styles.overflowY,
    };
  });
  expect(viewportStyles.overflowY).toBe("scroll");

  const lastPlugin = panel.getByRole("heading", {
    name: "Marketplace Plugin 11",
  });
  await viewport.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(() => viewport.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  await expect(lastPlugin).toBeInViewport();

  const lastPluginCard = lastPlugin.locator("xpath=ancestor::article[1]");
  await lastPluginCard.getByRole("button", { name: "安装完整插件" }).click();
  const confirmDialog = page.getByRole("dialog", {
    name: "安装完整 Codex 插件",
  });
  await expect(confirmDialog).toBeVisible();
  await confirmDialog.getByRole("button", { name: "确认安装插件" }).click();

  const errorToast = page.locator(
    '[data-sonner-toast][data-type="error"].skills-marketplace-install-error-toast',
  );
  const errorDescription = errorToast.locator("[data-description]");
  await expect(errorToast).toBeVisible();
  await expect(errorToast.getByText("安装插件失败", { exact: true })).toBeVisible();
  await expect(errorDescription).toContainText("git checkout failed");

  await expect
    .poll(async () => Math.round((await errorToast.boundingBox())?.width ?? 0))
    .toBe(416);
  const toastBox = await errorToast.boundingBox();
  expect(toastBox).not.toBeNull();
  expect(toastBox!.height).toBeLessThanOrEqual(417);

  const descriptionMetrics = await errorDescription.evaluate((element) => {
    const styles = window.getComputedStyle(element);
    return {
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
      overflowY: styles.overflowY,
    };
  });
  expect(descriptionMetrics.overflowY).toBe("auto");
  expect(descriptionMetrics.scrollHeight).toBeGreaterThan(
    descriptionMetrics.clientHeight,
  );

  await errorDescription.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect
    .poll(() => errorDescription.evaluate((element) => element.scrollTop))
    .toBeGreaterThan(0);
  expect(browserErrors).toEqual([]);
});
