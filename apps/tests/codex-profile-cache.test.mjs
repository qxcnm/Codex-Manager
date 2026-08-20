import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

function readConstFunctionBody(source, functionName) {
  const normalizedSource = source.replaceAll("\r\n", "\n");
  const start = normalizedSource.indexOf(`const ${functionName} = async () => {`);
  assert.notEqual(start, -1, `${functionName} not found`);
  const end = normalizedSource.indexOf("\n  };\n", start);
  assert.notEqual(end, -1, `${functionName} body end not found`);
  return normalizedSource.slice(start, end);
}

test("账号登录和导入会刷新 Codex profile 候选账号", async () => {
  const source = await readSource("src/components/modals/add-account-modal.tsx");
  assert.match(source, /CODEX_PROFILE_CANDIDATES_QUERY_KEY/);
  assert.match(
    source,
    /queryClient\.invalidateQueries\(\{\s*queryKey:\s*CODEX_PROFILE_CANDIDATES_QUERY_KEY\s*,?\s*\}\)/s,
  );
});

test("账号池页面变更会刷新 Codex profile 候选账号", async () => {
  const source = await readSource("src/hooks/useAccounts.ts");
  const invalidateUsageBody = readConstFunctionBody(source, "invalidateUsageData");
  assert.match(source, /CODEX_PROFILE_CANDIDATES_QUERY_KEY/);
  assert.match(
    invalidateUsageBody,
    /queryClient\.invalidateQueries\(\{\s*queryKey:\s*CODEX_PROFILE_CANDIDATES_QUERY_KEY\s*,?\s*\}\)/,
  );
});

test("平台模式页面可见时会主动刷新候选列表", async () => {
  const source = `${await readSource("src/app/platform-mode/page.tsx")}\n${await readSource("src/app/platform-mode/page-sections.tsx")}\n${await readSource("src/app/platform-mode/use-platform-mode-state.ts")}`;
  assert.match(source, /useDesktopPageActive\("\/platform-mode\/"\)/);
  assert.match(source, /refetchInterval:\s*isServiceReady && isPageActive \? 5_000 : false/);
  assert.match(source, /pickAvailableCandidateId/);
});

test("Codex 接入方式页面展示当前状态和切换影响", async () => {
  const source = `${await readSource("src/app/platform-mode/page.tsx")}\n${await readSource("src/app/platform-mode/page-sections.tsx")}`;
  assert.match(source, /Codex 接入方式/);
  assert.match(source, /state\.mode === "web-gateway"/);
  assert.match(source, /Web \/ Docker 模式/);
  assert.match(source, /\/api\/rpc 写入 codexmanager-service/);
  assert.match(source, /当前 Codex 接入/);
  assert.match(source, /直接连接 OpenAI/);
  assert.match(source, /通过 CodexManager/);
  assert.match(source, /OpenAI 账号池/);
  assert.match(source, /聚合 API/);
  assert.match(source, /混合路由/);
  assert.match(source, /OpenAI 官方目录/);
  assert.match(source, /CodexManager 本地目录/);
  assert.match(source, /应用后/);
  assert.match(source, /高级与恢复/);
  assert.match(source, /不会产生 CodexManager 请求日志/);
  assert.match(source, /请求日志、Token、费用估算和仪表盘统计可用/);
  assert.match(source, /CodexManager 管理文件/);
  assert.match(source, /备份保存在 CodexManager 数据目录/);
  assert.match(source, /清理历史备份/);
  assert.match(source, /pruneHistoryBackups/);
  assert.match(source, /href=\{buildStaticRouteUrl\(href\)\}/);
  assert.match(
    source,
    /min-h-12 w-full whitespace-normal py-2 data-\[size=default\]:h-auto/,
  );
  assert.match(source, /<SelectGroup className="pb-0">/);
});

test("模型目录不再暴露 Codex models_cache 覆盖入口", async () => {
  const hook = await readSource("src/hooks/useManagedModels.ts");
  const page = await readSource("src/app/models/page.tsx");
  const client = await readSource("src/lib/api/service-client.ts");
  const tauriService = await readSource("src-tauri/src/commands/service.rs");
  const tauriRegistry = await readSource("src-tauri/src/commands/registry.rs");
  const readme = await readSource("README.md");

  assert.doesNotMatch(hook, /exportCodexModelsCache|exportCodexCache|models_cache\.json/);
  assert.doesNotMatch(page, /导出到本地 Codex 缓存|canExportCodexCache/);
  assert.doesNotMatch(client, /service_export_codex_models_cache/);
  assert.doesNotMatch(tauriService, /service_export_codex_models_cache|models_cache\.json/);
  assert.doesNotMatch(tauriRegistry, /service_export_codex_models_cache/);
  assert.match(readme, /不提供写入或下载 `~\/\.codex\/models_cache\.json`/);
  assert.match(page, /当前 Codex 模型来源/);
  assert.match(page, /本地目录是否影响当前 Codex/);
  assert.match(page, /刷新本地目录/);
  assert.match(page, /新增网关自定义模型/);
  assert.match(page, /导入到本地网关目录/);
  assert.doesNotMatch(page, /本地模型目录是唯一运行时真相源/);
});

test("平台模式切换透传并持久化 Codex 后台重载开关", async () => {
  const state = await readSource("src/app/platform-mode/use-platform-mode-state.ts");
  const sections = await readSource("src/app/platform-mode/page-sections.tsx");
  const client = await readSource("src/lib/api/codex-profile-client.ts");
  const tauri = await readSource("src-tauri/src/commands/codex_profile.rs");

  assert.match(state, /codexmanager\.platform-mode\.reload-after-switch/);
  assert.match(state, /reloadAfterSwitchMemoryValue = false/);
  assert.match(state, /reload-safe-default-v2/);
  assert.match(state, /reloadAfterSwitch,/);
  assert.match(sections, /切换后重载 Codex 后台/);
  assert.match(client, /reloadAfterSwitch: params\.reloadAfterSwitch/);
  assert.match(tauri, /"reloadAfterSwitch": reload_after_switch\.unwrap_or\(false\)/);
});

test("平台密钥变更会刷新 Codex profile 候选密钥", async () => {
  const source = await readSource("src/hooks/useApiKeys.ts");
  assert.match(source, /CODEX_PROFILE_CANDIDATES_QUERY_KEY/);
  assert.match(
    source,
    /queryClient\.invalidateQueries\(\{\s*queryKey:\s*CODEX_PROFILE_CANDIDATES_QUERY_KEY\s*,?\s*\}\)/s,
  );
});

test("平台密钥弹窗创建和编辑会刷新 Codex profile 候选密钥", async () => {
  const source = await readSource("src/components/modals/api-key-modal.tsx");
  assert.match(source, /CODEX_PROFILE_CANDIDATES_QUERY_KEY/);
  assert.match(
    source,
    /queryClient\.invalidateQueries\(\{\s*queryKey:\s*CODEX_PROFILE_CANDIDATES_QUERY_KEY\s*,?\s*\}\)/s,
  );
});
