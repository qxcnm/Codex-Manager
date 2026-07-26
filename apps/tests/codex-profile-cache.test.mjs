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

test("平台模式页面采用当前模式优先的切换结构", async () => {
  const source = `${await readSource("src/app/platform-mode/page.tsx")}\n${await readSource("src/app/platform-mode/page-sections.tsx")}`;
  assert.match(source, /平台模式选择/);
  assert.match(source, /state\.mode === "web-gateway"/);
  assert.match(source, /Web \/ Docker 模式/);
  assert.match(source, /\/api\/rpc 写入 codexmanager-service/);
  assert.match(source, /当前模式/);
  assert.match(source, /账号直连/);
  assert.match(source, /本地网关/);
  assert.match(source, /高级与恢复/);
  assert.match(source, /不会产生 CodexManager 请求日志/);
  assert.match(source, /请求日志、Token、费用估算和仪表盘统计可用/);
  assert.match(source, /CodexManager 管理文件/);
  assert.match(source, /备份保存在 CodexManager 数据目录/);
  assert.match(source, /清理历史备份/);
  assert.match(source, /pruneHistoryBackups/);
  assert.match(source, /href=\{buildStaticRouteUrl\(href\)\}/);
});

test("模型目录不再自动覆盖 Codex models_cache", async () => {
  const hook = await readSource("src/hooks/useManagedModels.ts");
  const readme = await readSource("README.md");
  const exportCalls = hook.match(/exportCodexModelsCache\(/g) || [];

  assert.equal(exportCalls.length, 1, "cache writes should only remain in explicit export");
  assert.match(hook, /const exportMutation = useMutation/);
  assert.match(readme, /不会自动改写 `~\/\.codex\/models_cache\.json`/);
});

test("平台模式切换透传并持久化 Codex 后台重载开关", async () => {
  const state = await readSource("src/app/platform-mode/use-platform-mode-state.ts");
  const sections = await readSource("src/app/platform-mode/page-sections.tsx");
  const client = await readSource("src/lib/api/codex-profile-client.ts");
  const tauri = await readSource("src-tauri/src/commands/codex_profile.rs");

  assert.match(state, /codexmanager\.platform-mode\.reload-after-switch/);
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
