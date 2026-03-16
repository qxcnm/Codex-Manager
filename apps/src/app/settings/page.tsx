"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { appClient } from "@/lib/api/app-client";
import { useAppStore } from "@/lib/store/useAppStore";
import { AppSettings, BackgroundTaskSettings } from "@/types";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
  AppWindow,
  Check,
  Cpu,
  Globe,
  Info,
  Palette,
  RotateCcw,
  Save,
  Search,
  Settings as SettingsIcon,
  Variable,
} from "lucide-react";
import { cn } from "@/lib/utils";

const ENV_DESCRIPTION_MAP: Record<string, string> = {
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

const THEMES = [
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

const ROUTE_STRATEGY_LABELS: Record<string, string> = {
  ordered: "顺序优先 (Ordered)",
  balanced: "均衡轮询 (Balanced)",
};

const DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS = [
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
  "gpt-5.4",
] as const;

const SETTINGS_TABS = ["general", "appearance", "gateway", "tasks", "env"] as const;
type SettingsTab = (typeof SETTINGS_TABS)[number];
const SETTINGS_ACTIVE_TAB_KEY = "codexmanager.settings.active-tab";

function readInitialSettingsTab(): SettingsTab {
  if (typeof window === "undefined") return "general";
  const savedTab = window.sessionStorage.getItem(SETTINGS_ACTIVE_TAB_KEY);
  if (savedTab && SETTINGS_TABS.includes(savedTab as SettingsTab)) {
    return savedTab as SettingsTab;
  }
  return "general";
}

function stringifyNumber(value: number | null | undefined): string {
  return value == null ? "" : String(value);
}

function parseIntegerInput(value: string, minimum = 0): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return null;
  const rounded = Math.trunc(numeric);
  if (rounded < minimum) return null;
  return rounded;
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error || "");
}

export default function SettingsPage() {
  const { setAppSettings: setStoreSettings } = useAppStore();
  const { theme, setTheme } = useTheme();
  const queryClient = useQueryClient();
  const lastSyncedSnapshotThemeRef = useRef<string | null>(null);
  const [activeTab, setActiveTab] = useState<SettingsTab>(readInitialSettingsTab);
  const [envSearch, setEnvSearch] = useState("");
  const [selectedEnvKey, setSelectedEnvKey] = useState<string | null>(null);
  const [envDrafts, setEnvDrafts] = useState<Record<string, string>>({});
  const [upstreamProxyDraft, setUpstreamProxyDraft] = useState<string | null>(null);
  const [transportDraft, setTransportDraft] = useState<
    Partial<Record<"sseKeepaliveIntervalMs" | "upstreamStreamTimeoutMs", string>>
  >({});
  const [backgroundTaskDraft, setBackgroundTaskDraft] = useState<Record<string, string>>({});

  const { data: snapshot, isLoading } = useQuery({
    queryKey: ["app-settings-snapshot"],
    queryFn: () => appClient.getSettings(),
  });

  const updateSettings = useMutation({
    mutationFn: (patch: Partial<AppSettings> & { _silent?: boolean }) => {
      const { _silent, ...actualPatch } = patch;
      return appClient.setSettings(actualPatch);
    },
    onSuccess: (nextSnapshot, variables) => {
      queryClient.setQueryData(["app-settings-snapshot"], nextSnapshot);
      setStoreSettings(nextSnapshot);
      if (nextSnapshot.lowTransparency) {
        document.body.classList.add("low-transparency");
      } else {
        document.body.classList.remove("low-transparency");
      }
      if (!variables._silent) {
        toast.success("设置已更新");
      }
    },
    onError: (error: unknown) => {
      toast.error(`更新失败: ${getErrorMessage(error)}`);
    },
  });

  useEffect(() => {
    if (!snapshot?.theme) return;
    if (lastSyncedSnapshotThemeRef.current === snapshot.theme) return;

    lastSyncedSnapshotThemeRef.current = snapshot.theme;
    const currentAppliedTheme =
      typeof document !== "undefined"
        ? document.documentElement.getAttribute("data-theme")
        : null;

    if (snapshot.theme !== currentAppliedTheme) {
      setTheme(snapshot.theme);
    }
  }, [setTheme, snapshot?.theme]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.sessionStorage.setItem(SETTINGS_ACTIVE_TAB_KEY, activeTab);
  }, [activeTab]);

  const filteredEnvCatalog = useMemo(() => {
    const catalog = snapshot?.envOverrideCatalog || [];
    if (!envSearch) return catalog;
    const keyword = envSearch.toLowerCase();
    return catalog.filter(
      (item) =>
        item.key.toLowerCase().includes(keyword) ||
        item.label.toLowerCase().includes(keyword)
    );
  }, [envSearch, snapshot?.envOverrideCatalog]);

  const selectedEnvItem = useMemo(
    () => snapshot?.envOverrideCatalog.find((item) => item.key === selectedEnvKey),
    [selectedEnvKey, snapshot?.envOverrideCatalog]
  );

  const upstreamProxyInput = upstreamProxyDraft ?? (snapshot?.upstreamProxyUrl || "");
  const transportInputValues = {
    sseKeepaliveIntervalMs:
      transportDraft.sseKeepaliveIntervalMs ??
      stringifyNumber(snapshot?.sseKeepaliveIntervalMs),
    upstreamStreamTimeoutMs:
      transportDraft.upstreamStreamTimeoutMs ??
      stringifyNumber(snapshot?.upstreamStreamTimeoutMs),
  };
  const selectedEnvValue = selectedEnvKey
    ? envDrafts[selectedEnvKey] ??
      snapshot?.envOverrides[selectedEnvKey] ??
      selectedEnvItem?.defaultValue ??
      ""
    : "";

  const lastIntentThemeRef = useRef<string | null>(null);

  const handleThemeChange = (nextTheme: string) => {
    if (!snapshot || nextTheme === snapshot.theme) return;
    const previousSnapshot = snapshot;
    const previousTheme = snapshot.theme || "tech";

    // 1. Immediately update local UI and intent lock
    lastIntentThemeRef.current = nextTheme;
    lastSyncedSnapshotThemeRef.current = nextTheme;
    
    setActiveTab("appearance");
    if (typeof window !== "undefined") {
      window.sessionStorage.setItem(SETTINGS_ACTIVE_TAB_KEY, "appearance");
    }
    
    setTheme(nextTheme);

    // 2. Optimistic local update
    queryClient.setQueryData(["app-settings-snapshot"], {
      ...snapshot,
      theme: nextTheme,
    });
    setStoreSettings({ ...snapshot, theme: nextTheme });

    // 3. Immediate persist to backend (No debounce)
    updateSettings.mutate(
      { theme: nextTheme, _silent: true },
      {
        onSuccess: (updatedSnapshot) => {
          // Double check if this is still our intent
          if (lastIntentThemeRef.current === nextTheme) {
            queryClient.setQueryData(["app-settings-snapshot"], updatedSnapshot);
            setStoreSettings(updatedSnapshot);
          }
        },
        onError: () => {
          // Only revert if no newer intent has been made
          if (lastIntentThemeRef.current === nextTheme) {
            queryClient.setQueryData(["app-settings-snapshot"], previousSnapshot);
            setStoreSettings(previousSnapshot);
            setTheme(previousTheme);
          }
        },
      }
    );
  };

  const updateBackgroundTasks = (patch: Partial<BackgroundTaskSettings>) => {
    if (!snapshot) return;
    updateSettings.mutate({
      backgroundTasks: {
        ...snapshot.backgroundTasks,
        ...patch,
      },
    });
  };

  const saveTransportField = (
    key: "sseKeepaliveIntervalMs" | "upstreamStreamTimeoutMs",
    minimum: number
  ) => {
    const nextValue = parseIntegerInput(transportInputValues[key], minimum);
    if (nextValue == null) {
      toast.error("请输入合法的数值");
      setTransportDraft((current) => {
        const nextDraft = { ...current };
        delete nextDraft[key];
        return nextDraft;
      });
      return;
    }
    void updateSettings
      .mutateAsync({ [key]: nextValue } as Partial<AppSettings>)
      .then(() => {
        setTransportDraft((current) => {
          const nextDraft = { ...current };
          delete nextDraft[key];
          return nextDraft;
        });
      })
      .catch(() => undefined);
  };

  const saveBackgroundTaskField = (key: keyof BackgroundTaskSettings, minimum = 1) => {
    if (!snapshot) return;
    const draftKey = String(key);
    const sourceValue =
      backgroundTaskDraft[draftKey] ?? stringifyNumber(snapshot.backgroundTasks[key] as number);
    const nextValue = parseIntegerInput(sourceValue, minimum);
    if (nextValue == null) {
      toast.error("请输入合法的数值");
      setBackgroundTaskDraft((current) => {
        const nextDraft = { ...current };
        delete nextDraft[draftKey];
        return nextDraft;
      });
      return;
    }
    void updateSettings
      .mutateAsync({
        backgroundTasks: {
          ...snapshot.backgroundTasks,
          [key]: nextValue,
        },
      })
      .then(() => {
        setBackgroundTaskDraft((current) => {
          const nextDraft = { ...current };
          delete nextDraft[draftKey];
          return nextDraft;
        });
      })
      .catch(() => undefined);
  };

  const handleSaveEnv = () => {
    if (!selectedEnvKey || !snapshot) return;
    void updateSettings
      .mutateAsync({
        envOverrides: {
          ...snapshot.envOverrides,
          [selectedEnvKey]: selectedEnvValue,
        },
      })
      .then(() => {
        setEnvDrafts((current) => {
          const nextDraft = { ...current };
          delete nextDraft[selectedEnvKey];
          return nextDraft;
        });
      })
      .catch(() => undefined);
  };

  const handleResetEnv = () => {
    if (!selectedEnvKey || !snapshot) return;
    const nextOverrides = { ...snapshot.envOverrides };
    delete nextOverrides[selectedEnvKey];
    void updateSettings
      .mutateAsync({ envOverrides: nextOverrides })
      .then(() => {
        setEnvDrafts((current) => {
          const nextDraft = { ...current };
          delete nextDraft[selectedEnvKey];
          return nextDraft;
        });
      })
      .catch(() => undefined);
  };

  if (isLoading || !snapshot) {
    return <div className="flex h-64 items-center justify-center text-muted-foreground">加载配置中...</div>;
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">系统设置</h2>
        <p className="mt-1 text-sm text-muted-foreground">管理应用行为、网关策略及后台任务</p>
      </div>

      <Tabs
        value={activeTab}
        onValueChange={(value) => {
          if (value && SETTINGS_TABS.includes(value as SettingsTab)) {
            setActiveTab(value as SettingsTab);
          }
        }}
        className="w-full"
      >
        <TabsList className="mb-6 flex h-11 w-full justify-start overflow-x-auto rounded-xl bg-muted/50 p-1 no-scrollbar lg:w-fit">
          <TabsTrigger value="general" className="gap-2 px-5 shrink-0">
            <SettingsIcon className="h-4 w-4" /> 通用
          </TabsTrigger>
          <TabsTrigger value="appearance" className="gap-2 px-5 shrink-0">
            <Palette className="h-4 w-4" /> 外观
          </TabsTrigger>
          <TabsTrigger value="gateway" className="gap-2 px-5 shrink-0">
            <Globe className="h-4 w-4" /> 网关
          </TabsTrigger>
          <TabsTrigger value="tasks" className="gap-2 px-5 shrink-0">
            <Cpu className="h-4 w-4" /> 任务
          </TabsTrigger>
          <TabsTrigger value="env" className="gap-2 px-5 shrink-0">
            <Variable className="h-4 w-4" /> 环境
          </TabsTrigger>
        </TabsList>

        <TabsContent value="general" className="space-y-6">
          <Card className="border-none bg-card/50 shadow-md backdrop-blur-md">
            <CardHeader>
              <div className="flex items-center gap-2">
                <AppWindow className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">基础设置</CardTitle>
              </div>
              <CardDescription>控制应用启动和窗口行为</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label>自动检查更新</Label>
                  <p className="text-xs text-muted-foreground">启动时自动检测新版本</p>
                </div>
                <Switch
                  checked={snapshot.updateAutoCheck}
                  onCheckedChange={(value) => updateSettings.mutate({ updateAutoCheck: value })}
                />
              </div>
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label>关闭时最小化到托盘</Label>
                  <p className="text-xs text-muted-foreground">点击关闭按钮不会直接退出程序</p>
                </div>
                <Switch
                  checked={snapshot.closeToTrayOnClose}
                  disabled={!snapshot.closeToTraySupported}
                  onCheckedChange={(value) =>
                    updateSettings.mutate({ closeToTrayOnClose: value })
                  }
                />
              </div>
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label>视觉性能模式</Label>
                  <p className="text-xs text-muted-foreground">关闭毛玻璃等特效以提升低配电脑性能</p>
                </div>
                <Switch
                  checked={snapshot.lowTransparency}
                  onCheckedChange={(value) => updateSettings.mutate({ lowTransparency: value })}
                />
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="appearance" className="space-y-6">
          <Card className="border-none bg-card/50 shadow-md backdrop-blur-md">
            <CardHeader>
              <div className="flex items-center gap-2">
                <Palette className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">界面主题</CardTitle>
              </div>
              <CardDescription>选择您喜爱的配色方案，适配不同工作心情</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-12">
                {THEMES.map((item) => (
                  <button
                    key={item.id}
                    onClick={() => handleThemeChange(item.id)}
                    className={cn(
                      "group relative flex flex-col items-center gap-2.5 rounded-2xl border p-4 transition-all duration-300 hover:scale-105",
                      theme === item.id
                        ? "border-primary bg-primary/10 shadow-lg ring-1 ring-primary"
                        : "border-transparent bg-muted/20 hover:bg-accent/40"
                    )}
                  >
                    <div
                      className="h-10 w-10 rounded-full border-2 border-white/20 shadow-md"
                      style={{ backgroundColor: item.color }}
                    />
                    <span
                      className={cn(
                        "whitespace-nowrap text-[10px] font-semibold transition-colors",
                        theme === item.id
                          ? "text-primary"
                          : "text-muted-foreground group-hover:text-foreground"
                      )}
                    >
                      {item.name}
                    </span>
                    {theme === item.id ? (
                      <div className="absolute right-2 top-2 rounded-full bg-primary p-0.5 text-primary-foreground shadow-sm">
                        <Check className="h-2.5 w-2.5" />
                      </div>
                    ) : null}
                  </button>
                ))}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="gateway" className="space-y-4">
          <Card className="border-none bg-card/50 shadow-md backdrop-blur-md">
            <CardHeader>
              <CardTitle className="text-base">网关策略</CardTitle>
              <CardDescription>配置账号选路和请求头处理方式</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="grid gap-2">
                <Label>账号选路策略</Label>
                <Select
                  value={snapshot.routeStrategy || "ordered"}
                  onValueChange={(value) =>
                    updateSettings.mutate({ routeStrategy: value || "ordered" })
                  }
                >
                  <SelectTrigger className="w-full md:w-[300px]">
                    <SelectValue placeholder="选择策略">
                      {(value) => {
                        const nextValue = String(value || "").trim();
                        if (!nextValue) return "选择策略";
                        return ROUTE_STRATEGY_LABELS[nextValue] || nextValue;
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="ordered">顺序优先 (Ordered)</SelectItem>
                    <SelectItem value="balanced">均衡轮询 (Balanced)</SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-[10px] text-muted-foreground">
                  顺序优先：按账号候选顺序优先尝试，默认只会在头部小窗口内按健康度做轻微换头；
                  均衡轮询：按“平台密钥 + 模型”维度严格轮询可用账号，默认不做健康度换头。
                </p>
              </div>

              <div className="grid gap-2">
                <Label>Free 账号最高模型</Label>
                <Select
                  value={snapshot.freeAccountMaxModel || "gpt-5.2"}
                  onValueChange={(value) =>
                    updateSettings.mutate({ freeAccountMaxModel: value || "gpt-5.2" })
                  }
                >
                  <SelectTrigger className="w-full md:w-[300px]">
                    <SelectValue placeholder="选择最高模型" />
                  </SelectTrigger>
                  <SelectContent>
                    {(snapshot.freeAccountMaxModelOptions?.length
                      ? snapshot.freeAccountMaxModelOptions
                      : DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
                    ).map((model) => (
                      <SelectItem key={model} value={model}>
                        {model}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-[10px] text-muted-foreground">
                  当请求模型高于这个上限时，free 账号不会进入候选池，避免自动切换后被上游拒绝。
                </p>
              </div>

              <div className="flex items-center justify-between border-t pt-6">
                <div className="space-y-0.5">
                  <Label>请求头收敛策略</Label>
                  <p className="text-xs text-muted-foreground">移除高风险会话头，降低 Cloudflare 验证命中率</p>
                </div>
                <Switch
                  checked={snapshot.cpaNoCookieHeaderModeEnabled}
                  onCheckedChange={(value) =>
                    updateSettings.mutate({ cpaNoCookieHeaderModeEnabled: value })
                  }
                />
              </div>

              <div className="grid gap-2 pt-2">
                <Label>上游代理 (Proxy)</Label>
                <Input
                  placeholder="http://127.0.0.1:7890"
                  className="h-10 max-w-md font-mono"
                  value={upstreamProxyInput}
                  onChange={(event) => setUpstreamProxyDraft(event.target.value)}
                  onBlur={() => {
                    if (upstreamProxyDraft == null) return;
                    if (upstreamProxyInput === (snapshot.upstreamProxyUrl || "")) {
                      setUpstreamProxyDraft(null);
                      return;
                    }
                    void updateSettings
                      .mutateAsync({ upstreamProxyUrl: upstreamProxyInput })
                      .then(() => setUpstreamProxyDraft(null))
                      .catch(() => undefined);
                  }}
                />
                <p className="text-[10px] text-muted-foreground">支持 http/https/socks5，留空表示直连。</p>
              </div>

              <div className="grid grid-cols-2 gap-4 border-t pt-6">
                <div className="grid gap-2">
                  <Label>SSE 保活间隔 (ms)</Label>
                  <Input
                    type="number"
                    value={transportInputValues.sseKeepaliveIntervalMs}
                    onChange={(event) =>
                      setTransportDraft((current) => ({
                        ...current,
                        sseKeepaliveIntervalMs: event.target.value,
                      }))
                    }
                    onBlur={() => saveTransportField("sseKeepaliveIntervalMs", 1)}
                  />
                </div>
                <div className="grid gap-2">
                  <Label>上游流式超时 (ms)</Label>
                  <Input
                    type="number"
                    value={transportInputValues.upstreamStreamTimeoutMs}
                    onChange={(event) =>
                      setTransportDraft((current) => ({
                        ...current,
                        upstreamStreamTimeoutMs: event.target.value,
                      }))
                    }
                    onBlur={() => saveTransportField("upstreamStreamTimeoutMs", 0)}
                  />
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="tasks" className="space-y-4">
          <Card className="border-none bg-card/50 shadow-md backdrop-blur-md">
            <CardHeader>
              <CardTitle className="text-base">后台任务线程</CardTitle>
              <CardDescription>管理自动轮询和保活任务</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              {[
                {
                  label: "用量轮询线程",
                  enabledKey: "usagePollingEnabled",
                  intervalKey: "usagePollIntervalSecs",
                },
                {
                  label: "网关保活线程",
                  enabledKey: "gatewayKeepaliveEnabled",
                  intervalKey: "gatewayKeepaliveIntervalSecs",
                },
                {
                  label: "令牌刷新轮询",
                  enabledKey: "tokenRefreshPollingEnabled",
                  intervalKey: "tokenRefreshPollIntervalSecs",
                },
              ].map((task) => (
                <div
                  key={task.enabledKey}
                  className="flex items-center justify-between gap-4 rounded-lg bg-accent/20 p-3"
                >
                  <div className="flex items-center gap-3">
                    <Switch
                      checked={snapshot.backgroundTasks[task.enabledKey as keyof BackgroundTaskSettings] as boolean}
                      onCheckedChange={(value) =>
                        updateBackgroundTasks({
                          [task.enabledKey]: value,
                        } as Partial<BackgroundTaskSettings>)
                      }
                    />
                    <Label>{task.label}</Label>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">间隔(秒)</span>
                    <Input
                      className="h-8 w-20"
                      type="number"
                      value={
                        backgroundTaskDraft[task.intervalKey] ||
                        stringifyNumber(
                          snapshot.backgroundTasks[
                            task.intervalKey as keyof BackgroundTaskSettings
                          ] as number
                        )
                      }
                      onChange={(event) =>
                        setBackgroundTaskDraft((current) => ({
                          ...current,
                          [task.intervalKey]: event.target.value,
                        }))
                      }
                      onBlur={() =>
                        saveBackgroundTaskField(
                          task.intervalKey as keyof BackgroundTaskSettings,
                          1
                        )
                      }
                    />
                  </div>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card className="border-none bg-card/50 shadow-md backdrop-blur-md">
            <CardHeader>
              <CardTitle className="text-base">Worker 并发参数</CardTitle>
              <CardDescription>调整执行单元并发规模（重启后生效）</CardDescription>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4 md:grid-cols-2">
              {[
                { label: "用量刷新并发", key: "usageRefreshWorkers" },
                { label: "HTTP 因子", key: "httpWorkerFactor" },
                { label: "HTTP 最小并发", key: "httpWorkerMin" },
                { label: "流式因子", key: "httpStreamWorkerFactor" },
                { label: "流式最小并发", key: "httpStreamWorkerMin" },
              ].map((worker) => (
                <div key={worker.key} className="grid gap-1.5">
                  <Label className="text-xs">{worker.label}</Label>
                  <Input
                    type="number"
                    className="h-9"
                    value={
                      backgroundTaskDraft[worker.key] ||
                      stringifyNumber(
                        snapshot.backgroundTasks[
                          worker.key as keyof BackgroundTaskSettings
                        ] as number
                      )
                    }
                    onChange={(event) =>
                      setBackgroundTaskDraft((current) => ({
                        ...current,
                        [worker.key]: event.target.value,
                      }))
                    }
                    onBlur={() =>
                      saveBackgroundTaskField(worker.key as keyof BackgroundTaskSettings, 1)
                    }
                  />
                </div>
              ))}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="env" className="space-y-4">
          <div className="grid gap-6 md:grid-cols-[300px_1fr]">
            <Card className="flex h-[500px] flex-col border-none bg-card/50 shadow-md">
              <CardHeader className="pb-3">
                <div className="relative">
                  <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                  <Input
                    placeholder="搜索变量..."
                    className="h-9 pl-9"
                    value={envSearch}
                    onChange={(event) => setEnvSearch(event.target.value)}
                  />
                </div>
              </CardHeader>
              <CardContent className="flex-1 overflow-y-auto p-2">
                <div className="space-y-1">
                  {filteredEnvCatalog.map((item) => (
                    <button
                      key={item.key}
                      onClick={() => setSelectedEnvKey(item.key)}
                      className={cn(
                        "w-full rounded-md px-3 py-2 text-left text-sm transition-colors",
                        selectedEnvKey === item.key
                          ? "bg-primary text-primary-foreground"
                          : "hover:bg-accent"
                      )}
                    >
                      <div className="truncate font-medium">{item.label}</div>
                      <code className="block truncate text-[10px] opacity-70">{item.key}</code>
                    </button>
                  ))}
                </div>
              </CardContent>
            </Card>

            <Card className="min-h-[500px] border-none bg-card/50 shadow-md">
              {selectedEnvKey ? (
                <>
                  <CardHeader>
                    <div className="flex flex-col gap-1">
                      <CardTitle className="text-lg">{selectedEnvItem?.label}</CardTitle>
                      <code className="w-fit rounded bg-primary/10 px-2 py-0.5 text-xs text-primary">
                        {selectedEnvKey}
                      </code>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-6">
                    <div className="rounded-lg border bg-accent/30 p-4 text-sm leading-relaxed text-muted-foreground">
                      <Info className="mr-2 inline-block h-4 w-4 text-primary" />
                      {ENV_DESCRIPTION_MAP[selectedEnvKey] ||
                        `${selectedEnvItem?.label} 对应环境变量，修改后会应用到相关模块。`}
                    </div>

                    <div className="space-y-2">
                      <Label>当前值</Label>
                      <Input
                        value={selectedEnvValue}
                        onChange={(event) => {
                          if (!selectedEnvKey) return;
                          setEnvDrafts((current) => ({
                            ...current,
                            [selectedEnvKey]: event.target.value,
                          }));
                        }}
                        className="h-11 font-mono"
                        placeholder="输入变量值"
                      />
                      <p className="text-[10px] text-muted-foreground">
                        默认值:{" "}
                        <span className="font-mono italic">
                          {selectedEnvItem?.defaultValue || "空"}
                        </span>
                      </p>
                    </div>

                    <div className="flex gap-3 border-t pt-4">
                      <Button onClick={handleSaveEnv} className="gap-2">
                        <Save className="h-4 w-4" /> 保存修改
                      </Button>
                      <Button variant="outline" onClick={handleResetEnv} className="gap-2">
                        <RotateCcw className="h-4 w-4" /> 恢复默认
                      </Button>
                    </div>
                  </CardContent>
                </>
              ) : (
                <CardContent className="flex h-full flex-col items-center justify-center gap-4 text-muted-foreground">
                  <div className="rounded-full bg-accent/30 p-4">
                    <Variable className="h-12 w-12 opacity-20" />
                  </div>
                  <p>请从左侧列表选择一个环境变量进行配置</p>
                </CardContent>
              )}
            </Card>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}
