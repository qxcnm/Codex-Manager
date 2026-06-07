import type { UpdateCheckResult } from "@/lib/api/app-updates";
import type {
  AppSettings,
  BackgroundTaskSettings,
  EnvOverrideCatalogItem,
  RuntimeTimeZone,
} from "@/types";

export const ENV_DESCRIPTION_MAP: Record<string, string> = {
  CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED:
    "控制 OpenAI Images 兼容入口是否启用；默认 1，填 0 会关闭 /v1/images/generations 和 /v1/images/edits。",
  CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL:
    "控制普通 Responses 请求是否自动注入 image_generation tool；默认 1，填 0 时仅客户端显式传入 tool 才启用。",
  CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL:
    "控制 Images API 兼容入口内部使用的 Codex 主模型；默认 gpt-5.4-mini。",
  CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL:
    "控制 Images API 兼容入口注入的图片工具模型；默认 gpt-image-2。",
  CODEXMANAGER_COMPACT_API_PATH:
    "控制 compact 请求实际转发到哪个上游路径；默认 /v1/responses/compact，可改成 /v1/chat/completions。",
  CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS:
    "控制单次上游请求允许持续的最长时间，单位毫秒；超过后会主动结束请求并返回超时错误。",
  CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS:
    "控制流式上游请求允许持续的最长时间，单位毫秒；填 0 可关闭流式超时上限。",
  CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS:
    "控制向下游补发 SSE keep-alive 帧的间隔，单位毫秒；上游长时间安静时可避免客户端误判连接中断。",
  CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS:
    "控制连接上游服务器时的超时时间，单位秒；主要影响握手和网络建立阶段。",
  CODEXMANAGER_UPSTREAM_BASE_URL:
    "控制默认上游地址；修改后，网关会把请求转发到新的目标地址。",
};

export const ENV_RISK_LABELS: Record<string, string> = {
  low: "低风险",
  medium: "中风险",
  high: "高风险",
};

export const ENV_EFFECT_SCOPE_LABELS: Record<string, string> = {
  deployment: "部署级",
  "runtime-global": "运行时全局",
  "request-semantic": "请求语义",
};

export const ENV_RISK_BADGE_CLASSES: Record<string, string> = {
  low: "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
  medium: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
  high: "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300",
};

const ENV_RISK_ORDER: Record<string, number> = {
  low: 0,
  medium: 1,
  high: 2,
};

export function normalizeEnvRiskLevel(value: string | null | undefined): string {
  const normalized = String(value || "").trim().toLowerCase();
  return normalized in ENV_RISK_LABELS ? normalized : "medium";
}

export function compareEnvOverrideItems(
  left: EnvOverrideCatalogItem,
  right: EnvOverrideCatalogItem,
): number {
  const leftRisk = normalizeEnvRiskLevel(left.riskLevel);
  const rightRisk = normalizeEnvRiskLevel(right.riskLevel);
  const riskDelta =
    (ENV_RISK_ORDER[leftRisk] ?? ENV_RISK_ORDER.medium) -
    (ENV_RISK_ORDER[rightRisk] ?? ENV_RISK_ORDER.medium);
  if (riskDelta !== 0) return riskDelta;
  return left.key.localeCompare(right.key);
}

export const THEMES = [
  { id: "tech", name: "企业蓝", color: "#2563eb" },
  { id: "dark", name: "极夜黑", color: "#09090b" },
  { id: "dark-one", name: "深邃黑", color: "#282c34" },
  { id: "business", name: "事务金", color: "#c28100" },
  { id: "mint", name: "薄荷绿", color: "#059669" },
  { id: "sunset", name: "晚霞橙", color: "#ea580c" },
  { id: "grape", name: "葡萄灰紫", color: "#7c3aed" },
  { id: "ocean", name: "海湾青", color: "#0284c7" },
  { id: "forest", name: "松林绿", color: "#166534" },
  { id: "rose", name: "玫瑰粉", color: "#db2777" },
  { id: "slate", name: "石板灰", color: "#475569" },
  { id: "aurora", name: "极光青", color: "#0d9488" },
];

export const ROUTE_STRATEGY_LABELS: Record<string, string> = {
  ordered: "顺序优先 (Ordered)",
  balanced: "均衡轮询 (Balanced)",
};

export const SERVICE_LISTEN_MODE_LABELS: Record<string, string> = {
  loopback: "仅本机 (localhost)",
  all_interfaces: "全部网卡 (0.0.0.0)",
};

export const RESIDENCY_REQUIREMENT_LABELS: Record<string, string> = {
  "": "不限制",
  us: "仅美国 (us)",
};

export const EMPTY_RESIDENCY_OPTION = "__none__";

export const WORKER_PRESET_KEYS = [
  "usageRefreshWorkers",
  "httpWorkerFactor",
  "httpWorkerMin",
  "httpStreamWorkerFactor",
  "httpStreamWorkerMin",
] as const;

export type WorkerPresetKey = (typeof WORKER_PRESET_KEYS)[number];

export type WorkerPreset = {
  key: string;
  label: string;
  simpleLabel: string;
  rangeLabel: string;
  summary: string;
  hints: string[];
  backgroundTasks: Pick<BackgroundTaskSettings, WorkerPresetKey>;
};

export type WorkerRecommendedSettings = {
  backgroundTasks: Pick<BackgroundTaskSettings, WorkerPresetKey>;
  accountMaxInflight: number;
};

export const WORKER_PRESETS: WorkerPreset[] = [
  {
    key: "recommended",
    label: "常规推荐",
    simpleLabel: "推荐",
    rangeLabel: "8-16 核",
    summary: "默认平衡档，适合大多数服务器和办公室电脑。",
    hints: ["几百并发通常先从这里开始", "速度和资源占用比较均衡"],
    backgroundTasks: {
      usageRefreshWorkers: 4,
      httpWorkerFactor: 4,
      httpWorkerMin: 8,
      httpStreamWorkerFactor: 1,
      httpStreamWorkerMin: 2,
    },
  },
  {
    key: "light",
    label: "轻量稳定",
    simpleLabel: "省资源",
    rangeLabel: "4-8 核",
    summary: "更少后台占用，适合低配机器、笔记本或只求稳。",
    hints: ["更省 CPU 和内存", "适合小规模或低峰值场景"],
    backgroundTasks: {
      usageRefreshWorkers: 2,
      httpWorkerFactor: 2,
      httpWorkerMin: 4,
      httpStreamWorkerFactor: 1,
      httpStreamWorkerMin: 1,
    },
  },
  {
    key: "performance",
    label: "高并发",
    simpleLabel: "高吞吐",
    rangeLabel: "16 核以上",
    summary: "更积极地并发处理，适合高核数机器和繁忙时段。",
    hints: ["更适合上千并发峰值", "机器资源充足时再选"],
    backgroundTasks: {
      usageRefreshWorkers: 6,
      httpWorkerFactor: 6,
      httpWorkerMin: 12,
      httpStreamWorkerFactor: 2,
      httpStreamWorkerMin: 4,
    },
  },
];

export const CUSTOM_WORKER_MODE_VALUE = "__custom__";

export const DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS = [
  "auto",
  "gpt-5",
  "gpt-5-codex",
  "gpt-5-codex-mini",
  "gpt-5.1",
  "gpt-5.1-codex",
  "gpt-5.1-codex-max",
  "gpt-5.1-codex-mini",
  "gpt-5.2",
  "gpt-5.2-codex",
  "gpt-5.3-codex",
  "gpt-5.4-mini",
  "gpt-5.4",
] as const;

export function formatFreeAccountModelLabel(
  value: string | null | undefined,
): string {
  const normalized = String(value || "").trim();
  if (!normalized || normalized === "auto") {
    return "跟随请求";
  }
  return normalized;
}

export const SETTINGS_TABS = [
  "general",
  "appearance",
  "gateway",
  "tasks",
  "env",
] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number];
export const SETTINGS_ACTIVE_TAB_KEY = "codexmanager.settings.active-tab";

export function readInitialSettingsTab(): SettingsTab {
  if (typeof window === "undefined") return "general";
  const savedTab = window.sessionStorage.getItem(SETTINGS_ACTIVE_TAB_KEY);
  if (savedTab && SETTINGS_TABS.includes(savedTab as SettingsTab)) {
    return savedTab as SettingsTab;
  }
  return "general";
}

export function stringifyNumber(value: number | null | undefined): string {
  return value == null ? "" : String(value);
}

export function formatRuntimeTimeZoneLabel(
  runtimeTimeZone: RuntimeTimeZone | null | undefined,
  localTimeZoneLabel = "服务端本地时区",
): string {
  const name = String(runtimeTimeZone?.name || "").trim();
  const offset = String(runtimeTimeZone?.offset || "").trim();
  const displayName =
    name && name !== "Local" ? name : localTimeZoneLabel;
  const normalizedOffset =
    offset && !offset.toUpperCase().startsWith("UTC")
      ? `UTC${offset.startsWith("+") || offset.startsWith("-") ? offset : `+${offset}`}`
      : offset;
  return normalizedOffset ? `${displayName} (${normalizedOffset})` : displayName;
}

export function readNumberField(
  source: Record<string, unknown>,
  key: string,
  fallback = 0,
): number {
  const value = source[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function normalizeWorkerRecommendation(
  value: unknown,
): WorkerRecommendedSettings | null {
  const source = asRecord(value);
  if (!source) return null;
  return {
    backgroundTasks: {
      usageRefreshWorkers: readNumberField(source, "usageRefreshWorkers", 4),
      httpWorkerFactor: readNumberField(source, "httpWorkerFactor", 4),
      httpWorkerMin: readNumberField(source, "httpWorkerMin", 8),
      httpStreamWorkerFactor: readNumberField(source, "httpStreamWorkerFactor", 1),
      httpStreamWorkerMin: readNumberField(source, "httpStreamWorkerMin", 2),
    },
    accountMaxInflight: readNumberField(source, "accountMaxInflight", 1),
  };
}

export function matchesRecommendedWorkerSettings(
  snapshot: AppSettings,
  recommendation: WorkerRecommendedSettings,
): boolean {
  return (
    WORKER_PRESET_KEYS.every(
      (key) => snapshot.backgroundTasks[key] === recommendation.backgroundTasks[key],
    ) && snapshot.accountMaxInflight === recommendation.accountMaxInflight
  );
}

export function parseIntegerInput(value: string, minimum = 0): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return null;
  const rounded = Math.trunc(numeric);
  if (rounded < minimum) return null;
  return rounded;
}

export type ModelForwardRuleItem = {
  pattern: string;
  target: string;
};

export function createEmptyModelForwardRule(): ModelForwardRuleItem {
  return { pattern: "", target: "" };
}

export function parseModelForwardRules(
  value: string | null | undefined,
): ModelForwardRuleItem[] {
  return String(value || "")
    .split(/\r?\n/)
    .map((rawLine) => rawLine.trim())
    .filter(Boolean)
    .map((line) => {
      const separatorIndex = line.indexOf("=");
      if (separatorIndex < 0) {
        return { pattern: line, target: "" };
      }
      return {
        pattern: line.slice(0, separatorIndex).trim(),
        target: line.slice(separatorIndex + 1).trim(),
      };
    });
}

export function ensureModelForwardRuleRows(
  items: ModelForwardRuleItem[],
): ModelForwardRuleItem[] {
  return items.length > 0 ? items : [createEmptyModelForwardRule()];
}

export function serializeModelForwardRules(
  items: ModelForwardRuleItem[],
): string {
  return items
    .map((item) => ({
      pattern: item.pattern.trim(),
      target: item.target.trim(),
    }))
    .filter((item) => item.pattern.length > 0 || item.target.length > 0)
    .map((item) => `${item.pattern}=${item.target}`)
    .join("\n");
}

export function inferServiceBindPreview(addr: string, mode: string): string {
  const normalizedAddr = String(addr || "").trim() || "localhost:48760";
  const [, port = "48760"] = normalizedAddr.split(":");
  return mode === "all_interfaces" ? `0.0.0.0:${port}` : `localhost:${port}`;
}

export type CheckUpdateRequest = {
  silent?: boolean;
};

export function buildReleaseUrl(summary: UpdateCheckResult | null): string {
  if (!summary?.repo) {
    return "https://github.com/qxcnm/Codex-Manager/releases";
  }
  const normalizedTag =
    summary.releaseTag ||
    (summary.latestVersion ? `v${summary.latestVersion}` : "");
  if (!normalizedTag) {
    return `https://github.com/${summary.repo}/releases`;
  }
  return `https://github.com/${summary.repo}/releases/tag/${normalizedTag}`;
}
