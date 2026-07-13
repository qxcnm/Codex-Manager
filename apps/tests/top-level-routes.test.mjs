import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "app-shell",
  "top-level-routes.ts"
);

async function loadTopLevelRoutesModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const testableSource = source
    .replace('"use client";', "")
    .replace(
      'import { normalizeRoutePath } from "@/lib/utils/static-routes";',
      "function normalizeRoutePath(path: string): string { return !path || path === '/' ? '/' : path.replace(/\\/+$/, ''); }"
    )
    .replace('import type { AppRole } from "@/types";', "type AppRole = string;");
  const compiled = ts.transpileModule(testableSource, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-top-level-routes-")
  );
  const tempFile = path.join(tempDir, "top-level-routes.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const routes = await loadTopLevelRoutesModule();

test("accounts 模式管理员菜单按任务域分组并保留账号体系入口", () => {
  const access = { role: "admin", mode: "accounts" };
  const sections = routes.getAllowedTopLevelRouteSections(access);
  assert.deepEqual(
    sections.map((section) => section.label),
    ["概览", "资源接入", "模型路由", "用户与密钥", "运行监控", "系统设置"]
  );
  assert.deepEqual(
    sections.map((section) => section.routes.map((route) => route.path)),
    [
      ["/"],
      ["/accounts", "/aggregate-api"],
      ["/models"],
      ["/account-manager", "/apikeys", "/model-groups"],
      ["/logs"],
      ["/settings", "/plugins"],
    ]
  );
  assert.equal(
    routes.isTopLevelRouteAllowedForRole("/account-manager", access),
    true
  );
  assert.equal(routes.isTopLevelRouteAllowedForRole("/model-groups", access), true);
  assert.equal(routes.getTopLevelRouteLabel("/account-manager", access), "成员账号");
  assert.equal(routes.getTopLevelRouteLabel("/model-groups", access), "模型组");
});

test("none/password 单人管理员模式隐藏账号体系入口但保留单人管理入口", () => {
  for (const mode of ["none", "password"]) {
    for (const role of ["system_admin", "admin"]) {
      const access = { role, mode };
      const paths = routes
        .getAllowedTopLevelRoutes(access)
        .map((route) => route.path);
      assert.deepEqual(paths, [
        "/",
        "/accounts",
        "/aggregate-api",
        "/models",
        "/apikeys",
        "/logs",
        "/settings",
        "/plugins",
      ]);
      assert.equal(
        routes.isTopLevelRouteAllowedForRole("/account-manager", access),
        false
      );
      assert.equal(
        routes.isTopLevelRouteAllowedForRole("/model-groups", access),
        false
      );
      assert.equal(routes.isTopLevelRouteAllowedForRole("/accounts", access), true);
      assert.equal(routes.isTopLevelRouteAllowedForRole("/apikeys", access), true);
      assert.equal(routes.getFirstAllowedTopLevelRoutePath(access), "/");
    }
  }
});

test("未解析 session mode 时不会闪现账号体系专属入口", () => {
  const access = { role: "system_admin", mode: null };
  const paths = routes.getAllowedTopLevelRoutes(access).map((route) => route.path);
  assert.equal(paths.includes("/account-manager"), false);
  assert.equal(paths.includes("/model-groups"), false);
  assert.equal(paths.includes("/accounts"), true);
  assert.equal(paths.includes("/apikeys"), true);
});

test("accounts 模式成员菜单只保留自助入口", () => {
  const access = { role: "member", mode: "accounts" };
  const sections = routes.getAllowedTopLevelRouteSections(access);
  assert.deepEqual(
    sections.map((section) => section.label),
    ["我的概览", "我的密钥", "可用模型", "使用记录", "账号设置"]
  );
  assert.deepEqual(
    sections.map((section) => section.routes.map((route) => route.path)),
    [["/"], ["/apikeys"], ["/models"], ["/logs"], ["/settings"]]
  );
  assert.equal(routes.getTopLevelRouteLabel("/apikeys", access), "我的密钥");
  assert.equal(routes.getTopLevelRouteLabel("/models", access), "可用模型");
  assert.equal(routes.getTopLevelRouteLabel("/settings", access), "账号设置");
  assert.equal(
    routes.isTopLevelRouteAllowedForRole("/account-manager", access),
    false
  );
  assert.equal(routes.isTopLevelRouteAllowedForRole("/model-groups", access), false);
});
