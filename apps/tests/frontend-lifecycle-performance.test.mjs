import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  const source = await fs.readFile(path.join(appsRoot, relativePath), "utf8");
  return source.replaceAll("\r\n", "\n");
}

test("keep-alive 页面默认保留状态，并通过关闭标签释放面板", async () => {
  const source = await readSource(
    "src/components/layout/page-keep-alive-viewport.tsx",
  );
  const store = await readSource("src/lib/store/useAppStore.ts");

  assert.doesNotMatch(source, /MAX_KEEP_ALIVE_PANELS|keepAlivePaths|shellTabRecency/);
  assert.match(source, /openShellTabs\.map\(\(path\) =>/);
  assert.match(store, /closeShellTab: \(path\) =>/);
});

test("隐藏页面不会继续运行页面级轮询或后台定时任务", async () => {
  const [
    dashboard,
    models,
    platformMode,
    proxySettings,
    diagnostics,
    settingsPage,
    author,
  ] =
    await Promise.all([
      readSource("src/app/page.tsx"),
      readSource("src/app/models/page.tsx"),
      readSource("src/app/platform-mode/use-platform-mode-state.ts"),
      readSource("src/app/settings/components/proxy-settings-card.tsx"),
      readSource("src/app/settings/components/desktop-diagnostics-card.tsx"),
      readSource("src/app/settings/page.tsx"),
      readSource("src/app/author/page.tsx"),
    ]);

  assert.match(
    dashboard,
    /useCodexProfileModeStatus\(\{\s*enabled:\s*isPageActive\s*,/,
  );
  assert.match(
    models,
    /useCodexProfileModeStatus\(\{\s*enabled:\s*isAdminMode && isPageActive\s*,/,
  );
  assert.match(
    platformMode,
    /refetchInterval:\s*isServiceReady && isPageActive \? 5_000 : false/,
  );
  assert.match(proxySettings, /enabled:\s*canManage && active/);
  assert.match(
    proxySettings,
    /if \(!isMountedRef\.current \|\| !activeRef\.current\) return;/,
  );
  assert.match(proxySettings, /const cancelTrackedJobs = useCallback/);
  assert.match(
    proxySettings,
    /cancelProxyTestJob\(\{ jobId: normalizedJobId \}\)/,
  );
  assert.match(
    proxySettings,
    /if \(!isMountedRef\.current \|\| !activeRef\.current \|\| variables\.generation !== activeStateGenerationRef\.current\) \{\s*requestCancelJob\(result\.jobId\)/,
  );
  assert.match(diagnostics, /enabled:\s*active/);
  assert.match(
    settingsPage,
    /<DesktopDiagnosticsCard t=\{t\} active=\{isPageActive\}/,
  );
  assert.match(
    author,
    /if \(typeof window === "undefined" \|\| !isPageActive\) return;/,
  );
  assert.match(author, /controller\.abort\(\);\s*clearInterval\(timer\);/);
});

test("useLocalDayRange 在有订阅者时共享一个定时器并在无人订阅时清理", async () => {
  const source = await readSource("src/hooks/useLocalDayRange.ts");

  assert.match(source, /const listeners = new Set<\(\) => void>\(\);/);
  assert.match(
    source,
    /if \(listeners\.size === 1\) \{[\s\S]*?intervalId = setInterval\(/,
  );
  assert.match(
    source,
    /if \(listeners\.size === 0 && intervalId !== null\) \{[\s\S]*?clearInterval\(intervalId\)/,
  );
  assert.match(
    source,
    /return useSyncExternalStore\(subscribe, getSnapshot, getSnapshot\);/,
  );
});

test("账号测试终态会释放事件监听，并支持在 SSE 建连阶段取消", async () => {
  const [modalSource, eventSource] = await Promise.all([
    readSource("src/components/modals/account-test-modal.tsx"),
    readSource("src/lib/api/account-test-events.ts"),
  ]);

  assert.match(modalSource, /const listenerAbortRef = useRef<AbortController/);
  assert.match(modalSource, /const pendingTextChunksRef = useRef<string\[\]>/);
  assert.match(modalSource, /const appendPendingText = useCallback/);
  assert.match(modalSource, /const stopEventListener = useCallback/);
  assert.match(
    modalSource,
    /case "test_complete"[\s\S]{0,420}stopEventListener\(\)[\s\S]{0,120}testIdRef\.current = null/,
  );
  assert.match(
    modalSource,
    /listenAccountTestEvent\(testId, handleEvent, \{[\s\S]*signal: listenerAbortController\.signal/,
  );
  assert.match(
    modalSource,
    /listenerAbortController\.signal\.aborted[\s\S]{0,260}cancelAccountTest\(id, testId\)/,
  );
  assert.match(
    modalSource,
    /await accountClient\.testAccount\([\s\S]{0,420}if \(runToken !== runTokenRef\.current\) \{\s*void accountClient\.cancelAccountTest\(id, testId\)/,
  );
  assert.match(eventSource, /signal\?: AbortSignal/);
  assert.match(eventSource, /source\?\.close\(\);\s*finish\(createAbortError\(\)\)/);
  assert.match(eventSource, /signal\.addEventListener\("abort"/);
});

test("标签关闭会同步移除页面缓存项", async () => {
  const [storeSource, viewportSource] = await Promise.all([
    readSource("src/lib/store/useAppStore.ts"),
    readSource("src/components/layout/page-keep-alive-viewport.tsx"),
  ]);

  assert.match(storeSource, /const nextTabs = state\.openShellTabs\.filter/);
  assert.match(storeSource, /openShellTabs: nextTabs/);
  assert.match(viewportSource, /data-shell-path=\{path\}/);
});
