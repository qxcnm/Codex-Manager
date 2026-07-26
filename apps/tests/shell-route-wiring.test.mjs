import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const testDir = path.dirname(fileURLToPath(import.meta.url));
const srcRoot = path.join(testDir, "..", "src");

const [routeSource, sidebarSource, viewportSource] = await Promise.all([
  fs.readFile(path.join(srcRoot, "lib", "app-shell", "top-level-routes.ts"), "utf8"),
  fs.readFile(path.join(srcRoot, "components", "layout", "sidebar.tsx"), "utf8"),
  fs.readFile(
    path.join(srcRoot, "components", "layout", "page-keep-alive-viewport.tsx"),
    "utf8",
  ),
]);

const configuredPaths = Array.from(
  routeSource.matchAll(/\bpath:\s*"([^"]+)"/g),
  (match) => match[1],
);

test("每个顶级功能路由同时接入菜单图标和页面缓存", () => {
  assert.ok(configuredPaths.length > 10, "应读取完整顶级路由配置");
  assert.equal(new Set(configuredPaths).size, configuredPaths.length);

  for (const routePath of configuredPaths) {
    assert.match(
      sidebarSource,
      new RegExp(`\\["${routePath.replaceAll("/", "\\/")}"\\s*,\\s*\\{\\s*icon:`),
      `${routePath} 缺少侧边栏图标接线`,
    );

    if (routePath === "/") {
      assert.match(viewportSource, /ROOT_PAGE_COMPONENT/);
      continue;
    }

    assert.match(
      viewportSource,
      new RegExp(`"${routePath.replaceAll("/", "\\/")}"\\s*:\\s*lazy\\(`),
      `${routePath} 缺少 keep-alive 页面接线`,
    );
  }
});
