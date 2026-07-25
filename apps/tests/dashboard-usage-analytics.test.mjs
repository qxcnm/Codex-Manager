import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const appsRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("管理员用量查询显式请求模型序列和时间粒度", async () => {
  const [clientSource, hookSource, pageSource] = await Promise.all([
    readSource("src/lib/api/dashboard-client.ts"),
    readSource("src/hooks/useDashboardAdminUsageSummary.ts"),
    readSource("src/app/page.tsx"),
  ]);

  assert.match(clientSource, /includeSeries: params\?\.includeSeries \?\? null/);
  assert.match(clientSource, /seriesBucketSeconds: params\?\.seriesBucketSeconds \?\? null/);
  assert.match(hookSource, /params\?\.seriesBucketSeconds \?\? null/);
  assert.match(pageSource, /includeSeries: true/);
  assert.match(
    pageSource,
    /seriesBucketSeconds: adminUsageGranularity === "hour" \? 3_600 : 86_400/,
  );
});

test("模型曲线保留原日曲线回退并提供可访问交互", async () => {
  const [pageSource, chartSource] = await Promise.all([
    readSource("src/app/page.tsx"),
    readSource("src/components/dashboard/admin-usage-trend-chart.tsx"),
  ]);

  assert.match(
    pageSource,
    /summary\.seriesUsage\.length > 0[\s\S]*<AdminUsageTrendChart[\s\S]*<DailyTokenLineChart/,
  );
  assert.match(chartSource, /type AdminUsageMetric = "tokens" \| "requests"/);
  assert.match(chartSource, /export type AdminUsageGranularity = "day" \| "hour"/);
  assert.match(chartSource, /aria-pressed=\{granularity === value\}/);
  assert.match(chartSource, /aria-pressed=\{isSelected\}/);
  assert.match(chartSource, /const MAX_SELECTED_MODELS = 5/);
  assert.match(chartSource, /var\(--usage-series-1\)/);
  assert.match(chartSource, /type="monotone"/);
  assert.match(chartSource, /strokeDasharray="7 5"/);
  assert.match(chartSource, /<Check/);
  assert.match(chartSource, /borderColor: color/);
  assert.match(chartSource, /color-mix\(in srgb/);
  assert.match(chartSource, /<Brush/);
  assert.match(chartSource, /itemSorter=/);
  assert.match(chartSource, /hoveredModel/);
  assert.match(chartSource, /已选 \{selected\}\/\{max\}/);
  assert.match(chartSource, /最多同时比较 \{count\} 个模型/);
  assert.match(chartSource, /accessibilityLayer/);
});

test("模型图例随当前指标排序并保持稳定颜色", async () => {
  const chartSource = await readSource(
    "src/components/dashboard/admin-usage-trend-chart.tsx",
  );

  assert.match(chartSource, /rankedModelSeries/);
  assert.match(chartSource, /metricValue\(right\.usage, metric\)/);
  assert.match(chartSource, /stableModelIndexByName/);
  assert.match(chartSource, /totalMetricForRange/);
  assert.match(chartSource, /share\.toFixed\(1\)/);
});

test("模型曲线在查询刷新时保留内容并显示明确反馈", async () => {
  const [pageSource, chartSource] = await Promise.all([
    readSource("src/app/page.tsx"),
    readSource("src/components/dashboard/admin-usage-trend-chart.tsx"),
  ]);

  assert.match(pageSource, /isFetching: isAdminUsageFetching/);
  assert.match(
    pageSource,
    /isRefreshing=\{isAdminUsageFetching && !isAdminUsageLoading\}/,
  );
  assert.match(chartSource, /正在更新曲线/);
  assert.match(chartSource, /setZoomWindow\(null\)/);
});
