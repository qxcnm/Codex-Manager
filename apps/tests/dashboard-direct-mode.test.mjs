import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readDashboardSource() {
  return fs.readFile(path.join(appsRoot, "src/app/page.tsx"), "utf8");
}

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("账号直连模式在网关状态块内提示并遮罩用量分析", async () => {
  const source = await readDashboardSource();
  const gatewayStatusSource = await readSource("src/components/dashboard/dashboard-gateway-status.tsx");
  assert.match(source, /useCodexProfileModeStatus/);
  assert.match(source, /function DirectModeUnavailable/);
  assert.match(source, /账号直连模式下不可用/);
  assert.match(source, /切换到本地网关后可统计请求日志、Token 和费用/);
  assert.match(source, /buildStaticRouteUrl\("\/platform-mode"\)/);
  assert.match(gatewayStatusSource, /当前为账号直连模式/);
  assert.match(gatewayStatusSource, /CodexManager 无法统计 CLI 请求日志和用量。/);
  assert.match(
    source,
    /<DirectModeUnavailable active=\{isDirectAccountMode\}>\s*<AdminUsageAnalyticsCard/s,
  );
  assert.doesNotMatch(source, /当前活跃账号/);
  assert.doesNotMatch(source, /智能推荐/);
});

test("日志页 direct 模式只提示日志口径不遮罩历史日志", async () => {
  const source = await readSource("src/app/logs/page.tsx");
  assert.match(source, /useCodexProfileModeStatus/);
  assert.doesNotMatch(source, /DirectModeUnavailable/);
});

test("启动快照只预取轻量日志样本", async () => {
  const source = await readSource("src/lib/api/startup-snapshot.ts");
  assert.match(source, /STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT = 24/);
});

test("启动快照缓存键包含完整日期边界", async () => {
  const startupSource = await readSource("src/lib/api/startup-snapshot.ts");
  assert.match(startupSource, /dayStartTs \|\| null,\s*dayEndTs \|\| null,/s);

  const dashboardSource = await readSource("src/hooks/useDashboardStats.ts");
  assert.match(
    dashboardSource,
    /buildStartupSnapshotQueryKey\(\s*serviceStatus\.addr,\s*requestLogLimit,\s*localDayRange\.dayStartTs,\s*localDayRange\.dayEndTs,\s*includeApiModels,\s*includeApiKeys,\s*includeAccounts,\s*includeUsageSnapshots,\s*includeAccountRuntime,\s*includeAccountDetails,/s,
  );
});

test("仪表盘只预取状态块所需的轻量数据", async () => {
  const source = await readDashboardSource();
  assert.match(source, /useDashboardStats\(\{\s*requestLogLimit: 0,\s*includeAccountHints: false,/s);
  assert.match(
    source,
    /includeApiModels: false,\s*includeApiKeys: false,\s*includeAccounts: true,\s*includeUsageSnapshots: true,\s*includeAccountRuntime: true,\s*includeAccountDetails: false,/s,
  );
});

test("桌面启动快照命令会透传轻量快照参数", async () => {
  const source = await readSource("src-tauri/src/commands/startup.rs");
  assert.match(source, /day_start_ts: Option<i64>/);
  assert.match(source, /day_end_ts: Option<i64>/);
  assert.match(source, /include_api_models: Option<bool>/);
  assert.match(source, /include_api_keys: Option<bool>/);
  assert.match(source, /include_accounts: Option<bool>/);
  assert.match(source, /include_usage_snapshots: Option<bool>/);
  assert.match(source, /include_account_runtime: Option<bool>/);
  assert.match(source, /include_account_details: Option<bool>/);
  assert.match(source, /"includeApiModels": include_api_models/);
  assert.match(source, /"includeApiKeys": include_api_keys/);
  assert.match(source, /"includeAccounts": include_accounts/);
  assert.match(source, /"includeUsageSnapshots": include_usage_snapshots/);
  assert.match(source, /"includeAccountRuntime": include_account_runtime/);
  assert.match(source, /"includeAccountDetails": include_account_details/);
  assert.match(
    source,
    /rpc_call_in_background\("startup\/snapshot", addr, Some\(params\)\)\.await/,
  );
});

test("托盘预览使用较轻的启动快照", async () => {
  const source = await readSource("src/app/tray-preview/page.tsx");
  assert.match(source, /requestLogLimit: TRAY_PREVIEW_REQUEST_LOG_LIMIT/);
  assert.match(source, /includeApiModels: false/);
  assert.match(source, /includeApiKeys: false/);
  assert.match(source, /includeAccountDetails: false/);
});

test("首页账户统计优先使用启动快照汇总", async () => {
  const hookSource = await readSource("src/hooks/useDashboardStats.ts");
  assert.match(hookSource, /const accountSummary = data\?\.accountSummary;/);
  assert.match(
    hookSource,
    /const totalAccounts = accountSummary\?\.accountCount \?\? accounts\.length;/,
  );
  assert.match(
    hookSource,
    /accountSummary\?\.availableCount \?\? accounts\.filter\(\(item\) => item\.isAvailable\)\.length;/,
  );

  const normalizeSource = await readSource("src/lib/api/normalize.ts");
  assert.match(normalizeSource, /function normalizeStartupAccountSummary/);
  assert.match(normalizeSource, /accountSummary: normalizeStartupAccountSummary/);
});

test("成员用量趋势卡不再重复展示 Top Key", async () => {
  const source = await readDashboardSource();
  const trendCard = source.slice(
    source.indexOf("function MemberUsageTrendCard"),
    source.indexOf("function TopUsageList"),
  );
  assert.match(trendCard, /title=\{t\("Top 模型"\)\}/);
  assert.doesNotMatch(trendCard, /title=\{t\("Top Key"\)\}/);
});
