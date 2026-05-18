"use client";

import { useEffect, useRef, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { appClient } from "@/lib/api/app-client";
import type {
  UpdateCheckResult,
  UpdatePrepareResult,
} from "@/lib/api/app-updates";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useAppStore } from "@/lib/store/useAppStore";
import { DEFAULT_CODEX_ORIGINATOR } from "@/lib/constants/codex";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import {
  APP_SESSION_QUERY_KEY,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import {
  APPEARANCE_PRESETS,
  applyAppearancePreset,
  normalizeAppearancePreset,
} from "@/lib/appearance";
import { AppSettings, BackgroundTaskSettings } from "@/types";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Separator } from "@/components/ui/separator";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  AppWindow,
  Check,
  Cpu,
  Download,
  ExternalLink,
  FolderOpen,
  Globe,
  Info,
  Palette,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Settings as SettingsIcon,
  ShieldCheck,
  Variable,
  UserRound,
  LockKeyhole,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { WebPasswordModal } from "@/components/modals/web-password-modal";
import { useI18n } from "@/lib/i18n/provider";
import {
  CUSTOM_WORKER_MODE_VALUE,
  DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS,
  EMPTY_RESIDENCY_OPTION,
  ENV_DESCRIPTION_MAP,
  ENV_EFFECT_SCOPE_LABELS,
  ENV_RISK_BADGE_CLASSES,
  ENV_RISK_LABELS,
  RESIDENCY_REQUIREMENT_LABELS,
  ROUTE_STRATEGY_LABELS,
  SERVICE_LISTEN_MODE_LABELS,
  SETTINGS_ACTIVE_TAB_KEY,
  SETTINGS_TABS,
  THEMES,
  WORKER_PRESET_KEYS,
  WORKER_PRESETS,
  asRecord,
  buildReleaseUrl,
  type CheckUpdateRequest,
  compareEnvOverrideItems,
  formatFreeAccountModelLabel,
  inferServiceBindPreview,
  matchesRecommendedWorkerSettings,
  normalizeEnvRiskLevel,
  normalizeWorkerRecommendation,
  parseIntegerInput,
  readInitialSettingsTab,
  stringifyNumber,
  type SettingsTab,
  type WorkerPreset,
} from "@/app/settings/settings-page-helpers";

function MemberSettingsPage() {
  const { t } = useI18n();
  const { theme, setTheme } = useTheme();
  const queryClient = useQueryClient();
  const { data: session } = useAppSession();
  const [displayName, setDisplayName] = useState(
    session?.currentUser?.displayName || "",
  );
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");

  useEffect(() => {
    setDisplayName(session?.currentUser?.displayName || "");
  }, [session?.currentUser?.displayName]);

  const updateProfile = useMutation({
    mutationFn: () =>
      appClient.updateProfile({
        displayName: displayName.trim() || null,
      }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: APP_SESSION_QUERY_KEY });
      toast.success(t("个人资料已更新"));
    },
    onError: (error: unknown) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const changePassword = useMutation({
    mutationFn: () =>
      appClient.changePassword({
        currentPassword,
        newPassword,
      }),
    onSuccess: () => {
      setCurrentPassword("");
      setNewPassword("");
      toast.success(t("密码已更新"));
    },
    onError: (error: unknown) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">{t("个人设置")}</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("管理你的账号资料、登录密码和界面偏好")}
        </p>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="glass-card shadow-sm">
          <CardHeader>
            <div className="flex items-center gap-2">
              <UserRound className="h-4 w-4 text-primary" />
              <CardTitle className="text-base">{t("账号资料")}</CardTitle>
            </div>
            <CardDescription>
              {session?.currentUser?.username || t("当前登录账号")}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid gap-2">
              <Label>{t("显示名称")}</Label>
              <Input
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder={t("可选")}
              />
            </div>
            <Button
              className="gap-2"
              onClick={() => updateProfile.mutate()}
              disabled={updateProfile.isPending}
            >
              <Save className="h-4 w-4" />
              {updateProfile.isPending ? t("保存中...") : t("保存资料")}
            </Button>
          </CardContent>
        </Card>

        <Card className="glass-card shadow-sm">
          <CardHeader>
            <div className="flex items-center gap-2">
              <LockKeyhole className="h-4 w-4 text-primary" />
              <CardTitle className="text-base">{t("登录密码")}</CardTitle>
            </div>
            <CardDescription>{t("修改当前账号的登录密码")}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid gap-2">
              <Label>{t("当前密码")}</Label>
              <Input
                type="password"
                value={currentPassword}
                onChange={(event) => setCurrentPassword(event.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label>{t("新密码")}</Label>
              <Input
                type="password"
                value={newPassword}
                onChange={(event) => setNewPassword(event.target.value)}
              />
            </div>
            <Button
              className="gap-2"
              onClick={() => changePassword.mutate()}
              disabled={
                changePassword.isPending ||
                !currentPassword.trim() ||
                !newPassword.trim()
              }
            >
              <LockKeyhole className="h-4 w-4" />
              {changePassword.isPending ? t("保存中...") : t("修改密码")}
            </Button>
          </CardContent>
        </Card>
      </div>

      <Card className="glass-card shadow-sm">
        <CardHeader>
          <div className="flex items-center gap-2">
            <Palette className="h-4 w-4 text-primary" />
            <CardTitle className="text-base">{t("界面偏好")}</CardTitle>
          </div>
          <CardDescription>{t("这些偏好只影响当前浏览器会话")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {THEMES.map((item) => (
              <Button
                key={item.id}
                type="button"
                variant="outline"
                onClick={() => setTheme(item.id)}
                className={cn(
                  "flex h-auto items-center justify-start gap-3 rounded-xl border border-border/60 bg-background/45 p-3 text-left transition-colors hover:bg-accent/50",
                  theme === item.id ? "ring-2 ring-primary/40" : "",
                )}
              >
                <span
                  className="h-5 w-5 rounded-full border border-border/50"
                  style={{ backgroundColor: item.color }}
                />
                <span className="text-sm font-medium">{t(item.name)}</span>
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function AdminSettingsPage() {
  const { t } = useI18n();
  const setStoreSettings = useAppStore((state) => state.setAppSettings);
  const storedSettings = useAppStore((state) => state.appSettings);
  const { theme, setTheme } = useTheme();
  const queryClient = useQueryClient();
  const {
    isDesktopRuntime,
    canAccessManagementRpc,
    canSelfUpdate,
    canOpenLocalDir,
    canCloseToTray,
  } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/settings/");
  const isSnapshotQueryEnabled = useDeferredDesktopActivation(
    canAccessManagementRpc,
  );
  const lastSyncedSnapshotThemeRef = useRef<string | null>(null);
  const lastSyncedAppearancePresetRef = useRef<string | null>(null);
  const manualUpdateCheckPendingRef = useRef(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>(
    readInitialSettingsTab,
  );
  const [envSearch, setEnvSearch] = useState("");
  const [selectedEnvKey, setSelectedEnvKey] = useState<string | null>(null);
  const [envDrafts, setEnvDrafts] = useState<Record<string, string>>({});
  const [resetAllEnvDialogOpen, setResetAllEnvDialogOpen] = useState(false);
  const [upstreamProxyDraft, setUpstreamProxyDraft] = useState<string | null>(
    null,
  );
  const [gatewayOriginatorDraft, setGatewayOriginatorDraft] = useState<
    string | null
  >(null);
  const [modelForwardRulesDraft, setModelForwardRulesDraft] =
    useState<string | null>(null);
  const [lastUpdateCheck, setLastUpdateCheck] =
    useState<UpdateCheckResult | null>(null);
  const [updateDialogCheck, setUpdateDialogCheck] =
    useState<UpdateCheckResult | null>(null);
  const [preparedUpdate, setPreparedUpdate] =
    useState<UpdatePrepareResult | null>(null);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [manualUpdateCheckPending, setManualUpdateCheckPending] =
    useState(false);
  const [transportDraft, setTransportDraft] = useState<
    Partial<
      Record<
        | "sseKeepaliveIntervalMs"
        | "upstreamStreamTimeoutMs"
        | "upstreamTotalTimeoutMs",
        string
      >
    >
  >({});
  const [backgroundTaskDraft, setBackgroundTaskDraft] = useState<
    Record<string, string>
  >({});
  const [quotaGuardDraft, setQuotaGuardDraft] = useState<Record<string, string>>(
    {},
  );
  const [workerAdvancedDialogOpen, setWorkerAdvancedDialogOpen] =
    useState(false);
  const [webPasswordModalOpen, setWebPasswordModalOpen] = useState(false);
  const { data: workerRecommendation } = useQuery({
    queryKey: ["gateway-concurrency-recommendation"],
    queryFn: async () =>
      normalizeWorkerRecommendation(
        await appClient.getGatewayConcurrencyRecommendation(),
      ),
    enabled: isSnapshotQueryEnabled && isPageActive,
    staleTime: 60_000,
  });
  const deriveConcurrencyRecommendation = useMutation({
    mutationFn: () => appClient.getGatewayConcurrencyRecommendation(),
    onSuccess: (result) => {
      const recommendation = normalizeWorkerRecommendation(result);
      if (!recommendation) {
        toast.error(t("系统推导失败"));
        return;
      }
      if (!snapshot) return;
      queryClient.setQueryData(
        ["gateway-concurrency-recommendation"],
        recommendation,
      );
      void updateSettings
        .mutateAsync({
          backgroundTasks: {
            ...snapshot.backgroundTasks,
            ...recommendation.backgroundTasks,
          },
          accountMaxInflight: recommendation.accountMaxInflight,
          _silent: true,
        })
        .then(() => {
          clearBackgroundTaskDraftKeys([
            "usageRefreshWorkers",
            "httpWorkerFactor",
            "httpWorkerMin",
            "httpStreamWorkerFactor",
            "httpStreamWorkerMin",
            "accountMaxInflight",
          ]);
          toast.success(t("系统推导已应用"));
        })
        .catch((error: unknown) => {
          toast.error(
            `${t("系统推导保存失败")}: ${getAppErrorMessage(error)}`,
          );
        });
    },
    onError: (error: unknown) => {
      toast.error(`${t("系统推导失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const { data: fetchedSnapshot, isError: isSnapshotError } = useQuery({
    queryKey: ["app-settings-snapshot"],
    queryFn: () => appClient.getSettings(),
    enabled: isSnapshotQueryEnabled && isPageActive,
  });
  const snapshot = fetchedSnapshot ?? storedSettings;
  const modelForwardRulesInput =
    modelForwardRulesDraft ?? (snapshot?.modelForwardRules || "");
  usePageTransitionReady(
    "/settings/",
    !canAccessManagementRpc || Boolean(snapshot) || isSnapshotError,
  );

  const updateSettings = useMutation({
    mutationFn: (patch: Partial<AppSettings> & { _silent?: boolean }) => {
      const actualPatch = { ...patch };
      delete actualPatch._silent;
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
      applyAppearancePreset(nextSnapshot.appearancePreset);
      if (!variables._silent) {
        toast.success(t("设置已更新"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const checkUpdate = useMutation({
    mutationFn: (request?: CheckUpdateRequest) => {
      void request;
      return appClient.checkUpdate();
    },
    onSuccess: (summary, request) => {
      setLastUpdateCheck(summary);
      setUpdateDialogCheck(summary);
      if (summary.hasUpdate) {
        setPreparedUpdate((current) =>
          current && current.latestVersion === summary.latestVersion
            ? current
            : null,
        );
        if (!request?.silent) {
          toast.success(
            `${t("发现新版本")} ${summary.latestVersion || summary.releaseTag || t("可用")}${t("，可立即下载更新")}`,
          );
        }
        return;
      }
      setPreparedUpdate(null);
      setUpdateDialogOpen(false);
      if (!request?.silent) {
        toast.success(
          summary.reason
            ? `${t("已检查更新：")}${summary.reason}`
            : `${t("当前已是最新版本")} ${summary.currentVersion || ""}`.trim(),
        );
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("检查更新失败")}: ${getAppErrorMessage(error)}`);
    },
    onSettled: () => {
      if (manualUpdateCheckPendingRef.current) {
        manualUpdateCheckPendingRef.current = false;
        setManualUpdateCheckPending(false);
      }
    },
  });

  const prepareUpdate = useMutation({
    mutationFn: () => appClient.prepareUpdate(),
    onSuccess: (summary) => {
      setPreparedUpdate(summary);
      setUpdateDialogOpen(true);
      toast.success(
        summary.isPortable
          ? `${t("更新已下载完成，确认后即可替换到")} ${summary.latestVersion || t("新版本")}`
          : `${t("更新包已下载完成，确认后开始替换到")} ${summary.latestVersion || t("新版本")}`,
      );
    },
    onError: (error: unknown) => {
      toast.error(`${t("下载更新失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const applyPreparedUpdate = useMutation({
    mutationFn: (payload: { isPortable: boolean }) =>
      payload.isPortable
        ? appClient.applyUpdatePortable()
        : appClient.launchInstaller(),
    onSuccess: (result, payload) => {
      setPreparedUpdate(null);
      setLastUpdateCheck(null);
      setUpdateDialogCheck(null);
      setUpdateDialogOpen(false);
      const message = result.message.trim();
      toast.success(
        message ||
          (payload.isPortable ? t("即将重启并替换更新") : t("已开始替换更新流程")),
      );
    },
    onError: (error: unknown, payload) => {
      toast.error(
        `${payload.isPortable ? t("替换更新") : t("启动安装程序")}${t("失败")}: ${getAppErrorMessage(error)}`,
      );
    },
  });

  useEffect(() => {
    if (!isDesktopRuntime) {
      return;
    }

    let cancelled = false;
    void appClient
      .getStatus()
      .then((summary) => {
        if (cancelled) {
          return;
        }
        if (summary.lastCheck) {
          setLastUpdateCheck(summary.lastCheck);
          setUpdateDialogCheck(summary.lastCheck);
        }
        if (summary.pending) {
          setPreparedUpdate(summary.pending);
        }
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [isDesktopRuntime]);

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
    if (!snapshot) return;
    const nextPreset = normalizeAppearancePreset(snapshot.appearancePreset);
    if (lastSyncedAppearancePresetRef.current === nextPreset) return;

    lastSyncedAppearancePresetRef.current = nextPreset;
    applyAppearancePreset(nextPreset);
  }, [snapshot]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.sessionStorage.setItem(SETTINGS_ACTIVE_TAB_KEY, activeTab);
  }, [activeTab]);

  useEffect(() => {
    if (isPageActive) {
      return;
    }
    if (typeof window === "undefined") {
      return;
    }
    const frameId = window.requestAnimationFrame(() => {
      setUpdateDialogOpen(false);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [isPageActive]);

  /**
   * 函数 `handleOpenReleasePage`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleOpenReleasePage = () => {
    void appClient
      .openInBrowser(buildReleaseUrl(updateDialogCheck ?? lastUpdateCheck))
      .catch((error) => {
        toast.error(`${t("打开发布页失败")}: ${getAppErrorMessage(error)}`);
      });
  };

  /**
   * 函数 `handleManualCheckUpdate`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleManualCheckUpdate = () => {
    manualUpdateCheckPendingRef.current = true;
    setManualUpdateCheckPending(true);
    checkUpdate.mutate({ silent: false });
  };

  const hasPreparedUpdate = Boolean(preparedUpdate);
  const canDownloadUpdate = Boolean(
    !preparedUpdate && lastUpdateCheck?.hasUpdate && lastUpdateCheck.canPrepare,
  );
  const shouldShowUpdateLogsEntry = Boolean(
    canOpenLocalDir && (preparedUpdate || lastUpdateCheck),
  );
  const updateActionLabel = hasPreparedUpdate
    ? t("替换更新")
    : canDownloadUpdate
      ? t("下载更新")
      : t("检查更新");
  const updateActionDescription = !canSelfUpdate
    ? t("Web / Docker 版不提供桌面应用更新检查")
    : hasPreparedUpdate
      ? t("更新包已下载完成，点击后确认替换当前版本")
      : canDownloadUpdate
        ? t("已发现新版本，点击后开始下载更新包")
        : t("立即检查 GitHub Releases 是否有新版本可用");
  const updateActionBusy = Boolean(
    manualUpdateCheckPending ||
    prepareUpdate.isPending ||
    applyPreparedUpdate.isPending,
  );
  const updateActionBusyLabel = manualUpdateCheckPending
    ? t("正在检查...")
    : prepareUpdate.isPending
      ? t("正在下载...")
      : applyPreparedUpdate.isPending
        ? t("正在替换...")
        : updateActionLabel;

  /**
   * 函数 `handleUpdateAction`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleUpdateAction = () => {
    if (preparedUpdate) {
      setUpdateDialogCheck((current) => current ?? lastUpdateCheck);
      setUpdateDialogOpen(true);
      return;
    }

    if (lastUpdateCheck?.hasUpdate && lastUpdateCheck.canPrepare) {
      setUpdateDialogCheck(lastUpdateCheck);
      prepareUpdate.mutate();
      return;
    }

    handleManualCheckUpdate();
  };

  /**
   * 函数 `handleOpenUpdateLogsDir`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleOpenUpdateLogsDir = () => {
    void appClient
      .openUpdateLogsDir(preparedUpdate?.assetPath)
      .catch((error) => {
        toast.error(`${t("打开日志目录失败")}: ${getAppErrorMessage(error)}`);
      });
  };

  const envOverrideCatalog = snapshot?.envOverrideCatalog ?? [];
  const filteredEnvCatalog = (!envSearch
    ? envOverrideCatalog
    : envOverrideCatalog.filter((item) => {
        const keyword = envSearch.toLowerCase();
        return (
          item.key.toLowerCase().includes(keyword) ||
          item.label.toLowerCase().includes(keyword)
        );
      })
  )
    .slice()
    .sort(compareEnvOverrideItems);
  const selectedEnvItem =
    envOverrideCatalog.find((item) => item.key === selectedEnvKey) ?? null;
  const selectedEnvRiskLevel = normalizeEnvRiskLevel(selectedEnvItem?.riskLevel);
  const selectedEnvEffectScope =
    selectedEnvItem?.effectScope || "runtime-global";
  const selectedEnvSafetyNote =
    selectedEnvItem?.safetyNote ||
    t("会影响运行时配置；修改后请观察请求链路是否稳定。");

  const upstreamProxyInput =
    upstreamProxyDraft ?? (snapshot?.upstreamProxyUrl || "");
  const gatewayOriginatorDefault =
    snapshot?.gatewayOriginatorDefault || DEFAULT_CODEX_ORIGINATOR;
  const gatewayOriginatorInput =
    gatewayOriginatorDraft ??
    (snapshot?.gatewayOriginator || gatewayOriginatorDefault);
  const transportInputValues = {
    sseKeepaliveIntervalMs:
      transportDraft.sseKeepaliveIntervalMs ??
      stringifyNumber(snapshot?.sseKeepaliveIntervalMs),
    upstreamStreamTimeoutMs:
      transportDraft.upstreamStreamTimeoutMs ??
      stringifyNumber(snapshot?.upstreamStreamTimeoutMs),
    upstreamTotalTimeoutMs:
      transportDraft.upstreamTotalTimeoutMs ??
      stringifyNumber(snapshot?.upstreamTotalTimeoutMs),
  };
  const quotaGuardInputValues = {
    primaryMinRemainingPercent:
      quotaGuardDraft.primaryMinRemainingPercent ??
      stringifyNumber(snapshot?.quotaGuard.primaryMinRemainingPercent),
    secondaryMinRemainingPercent:
      quotaGuardDraft.secondaryMinRemainingPercent ??
      stringifyNumber(snapshot?.quotaGuard.secondaryMinRemainingPercent),
  };
  const selectedEnvValue = selectedEnvKey
    ? (envDrafts[selectedEnvKey] ??
      snapshot?.envOverrides[selectedEnvKey] ??
      selectedEnvItem?.defaultValue ??
      "")
    : "";
  const hasCustomizedEnvOverrides = envOverrideCatalog.some((item) => {
    const defaultValue = item.defaultValue ?? "";
    const currentValue = snapshot?.envOverrides[item.key] ?? defaultValue;
    const effectiveValue = envDrafts[item.key] ?? currentValue;
    return effectiveValue !== defaultValue;
  });

  const activeWorkerPreset = snapshot
    ? (workerRecommendation &&
      matchesRecommendedWorkerSettings(snapshot, workerRecommendation)
        ? (WORKER_PRESETS.find((preset) => preset.key === "recommended") ?? null)
        : (WORKER_PRESETS.find(
            (preset) =>
              preset.key !== "recommended" &&
              WORKER_PRESET_KEYS.every(
                (key) =>
                  snapshot.backgroundTasks[key] === preset.backgroundTasks[key],
              ),
          ) ?? null))
    : null;
  const activeWorkerModeValue = activeWorkerPreset?.key ?? CUSTOM_WORKER_MODE_VALUE;
  const activeWorkerSummary = activeWorkerPreset
    ? activeWorkerPreset.key === "recommended"
      ? t("已按当前机器资源自动推荐，适合作为这台机器的默认档位。")
      : t(activeWorkerPreset.summary)
    : t("当前配置来自高级参数，可在高级参数中继续微调。");
  const webAuthModeLabel =
    snapshot.webAuthMode === "accounts"
      ? "账号系统"
      : snapshot.webAuthMode === "password"
        ? "访问密码"
        : "公开访问";
  const showAccessControlSettings = !isDesktopRuntime;

  const lastIntentThemeRef = useRef<string | null>(null);
  const lastIntentAppearancePresetRef = useRef<string | null>(null);

  /**
   * 函数 `handleThemeChange`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - nextTheme: 参数 nextTheme
   *
   * # 返回
   * 返回函数执行结果
   */
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
            queryClient.setQueryData(
              ["app-settings-snapshot"],
              updatedSnapshot,
            );
            setStoreSettings(updatedSnapshot);
          }
        },
        onError: () => {
          // Only revert if no newer intent has been made
          if (lastIntentThemeRef.current === nextTheme) {
            queryClient.setQueryData(
              ["app-settings-snapshot"],
              previousSnapshot,
            );
            setStoreSettings(previousSnapshot);
            setTheme(previousTheme);
          }
        },
      },
    );
  };

  /**
   * 函数 `handleAppearancePresetChange`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - nextPreset: 参数 nextPreset
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleAppearancePresetChange = (nextPreset: string) => {
    if (!snapshot) return;

    const normalizedPreset = normalizeAppearancePreset(nextPreset);
    const previousSnapshot = snapshot;
    const previousPreset = normalizeAppearancePreset(snapshot.appearancePreset);
    if (normalizedPreset === previousPreset) return;

    lastIntentAppearancePresetRef.current = normalizedPreset;
    lastSyncedAppearancePresetRef.current = normalizedPreset;
    applyAppearancePreset(normalizedPreset);

    queryClient.setQueryData(["app-settings-snapshot"], {
      ...snapshot,
      appearancePreset: normalizedPreset,
    });
    setStoreSettings({ ...snapshot, appearancePreset: normalizedPreset });

    updateSettings.mutate(
      { appearancePreset: normalizedPreset, _silent: true },
      {
        onSuccess: (updatedSnapshot) => {
          if (lastIntentAppearancePresetRef.current === normalizedPreset) {
            queryClient.setQueryData(
              ["app-settings-snapshot"],
              updatedSnapshot,
            );
            setStoreSettings(updatedSnapshot);
          }
        },
        onError: () => {
          if (lastIntentAppearancePresetRef.current === normalizedPreset) {
            queryClient.setQueryData(
              ["app-settings-snapshot"],
              previousSnapshot,
            );
            setStoreSettings(previousSnapshot);
            applyAppearancePreset(previousPreset);
          }
        },
      },
    );
  };

  /**
   * 函数 `updateBackgroundTasks`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - patch: 参数 patch
   *
   * # 返回
   * 返回函数执行结果
   */
  const updateBackgroundTasks = (patch: Partial<BackgroundTaskSettings>) => {
    if (!snapshot) return;
    updateSettings.mutate({
      backgroundTasks: {
        ...snapshot.backgroundTasks,
        ...patch,
      },
    });
  };

  /**
   * 函数 `clearBackgroundTaskDraftKeys`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - keys: 参数 keys
   *
   * # 返回
   * 返回函数执行结果
   */
  const clearBackgroundTaskDraftKeys = (keys: readonly string[]) => {
    setBackgroundTaskDraft((current) => {
      const nextDraft = { ...current };
      for (const key of keys) {
        delete nextDraft[key];
      }
      return nextDraft;
    });
  };

  /**
   * 函数 `applyWorkerPreset`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - preset: 参数 preset
   *
   * # 返回
   * 返回函数执行结果
   */
  const applyWorkerPreset = (preset: WorkerPreset) => {
    if (!snapshot) return;
    void updateSettings
      .mutateAsync({
        backgroundTasks: {
          ...snapshot.backgroundTasks,
          ...preset.backgroundTasks,
        },
        _silent: true,
      })
      .then(() => {
        clearBackgroundTaskDraftKeys(WORKER_PRESET_KEYS);
        toast.success(`${t("已切换为")} ${t(preset.label)}`);
      })
      .catch(() => undefined);
  };

  /**
   * 函数 `saveTransportField`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - key: 参数 key
   * - minimum: 参数 minimum
   *
   * # 返回
   * 返回函数执行结果
   */
  const saveTransportField = (
    key:
      | "sseKeepaliveIntervalMs"
      | "upstreamStreamTimeoutMs"
      | "upstreamTotalTimeoutMs",
    minimum: number,
  ) => {
    const nextValue = parseIntegerInput(transportInputValues[key], minimum);
    if (nextValue == null) {
      toast.error(t("请输入合法的数值"));
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

  const saveQuotaGuardField = (
    key: "primaryMinRemainingPercent" | "secondaryMinRemainingPercent",
  ) => {
    if (!snapshot) return;
    const nextValue = parseIntegerInput(quotaGuardInputValues[key], 0);
    if (nextValue == null || nextValue > 100) {
      toast.error(t("请输入 0-100 之间的百分比"));
      setQuotaGuardDraft((current) => {
        const nextDraft = { ...current };
        delete nextDraft[key];
        return nextDraft;
      });
      return;
    }
    void updateSettings
      .mutateAsync({
        quotaGuard: {
          ...snapshot.quotaGuard,
          [key]: nextValue,
        },
      })
      .then(() => {
        setQuotaGuardDraft((current) => {
          const nextDraft = { ...current };
          delete nextDraft[key];
          return nextDraft;
        });
      })
      .catch(() => undefined);
  };

  /**
   * 函数 `saveBackgroundTaskField`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - key: 参数 key
   * - minimum: 参数 minimum
   *
   * # 返回
   * 返回函数执行结果
   */
  const saveBackgroundTaskField = (
    key: keyof BackgroundTaskSettings,
    minimum = 1,
  ) => {
    if (!snapshot) return;
    const draftKey = String(key);
    const sourceValue =
      backgroundTaskDraft[draftKey] ??
      stringifyNumber(snapshot.backgroundTasks[key] as number);
    const nextValue = parseIntegerInput(sourceValue, minimum);
    if (nextValue == null) {
      toast.error(t("请输入合法的数值"));
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

  /**
   * 函数 `saveAccountMaxInflightField`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - minimum: 参数 minimum
   *
   * # 返回
   * 返回函数执行结果
   */
  const saveAccountMaxInflightField = (minimum = 0) => {
    if (!snapshot) return;
    const draftKey = "accountMaxInflight";
    const sourceValue =
      backgroundTaskDraft[draftKey] ?? stringifyNumber(snapshot.accountMaxInflight);
    const nextValue = parseIntegerInput(sourceValue, minimum);
    if (nextValue == null) {
      toast.error(t("请输入合法的数值"));
      setBackgroundTaskDraft((current) => {
        const nextDraft = { ...current };
        delete nextDraft[draftKey];
        return nextDraft;
      });
      return;
    }
    void updateSettings
      .mutateAsync({
        accountMaxInflight: nextValue,
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

  /**
   * 函数 `handleSaveEnv`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleSaveEnv = () => {
    if (!selectedEnvKey || !snapshot) return;
    void updateSettings
      .mutateAsync({
        envOverrides: {
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

  /**
   * 函数 `handleResetEnv`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleResetEnv = () => {
    if (!selectedEnvKey || !snapshot) return;
    void updateSettings
      .mutateAsync({
        envOverrides: {
          // 中文注释：后端把“当前键为空字符串”视为恢复默认值，
          // 仅仅省略该键会被解释为“保持原值不变”。
          [selectedEnvKey]: "",
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

  const handleResetAllEnv = () => {
    if (!snapshot || envOverrideCatalog.length === 0) return;
    const resetPatch = envOverrideCatalog.reduce<Record<string, string>>(
      (result, item) => {
        result[item.key] = "";
        return result;
      },
      {},
    );
    void updateSettings
      .mutateAsync({
        envOverrides: resetPatch,
        _silent: true,
      })
      .then(() => {
        setEnvDrafts({});
        toast.success(t("环境变量已全部恢复默认值"));
      })
      .catch(() => undefined);
  };

  if ((canAccessManagementRpc && !isSnapshotQueryEnabled) || !snapshot) {
    return (
      <div className="flex h-64 items-center justify-center text-muted-foreground">
        {t("加载配置中...")}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">{t("系统设置")}</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("管理应用行为、网关策略及后台任务")}
        </p>
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
        <TabsList className="glass-card mb-6 flex h-11 w-full justify-start overflow-x-auto rounded-xl p-1 no-scrollbar lg:w-fit">
          <TabsTrigger value="general" className="gap-2 px-5 shrink-0">
            <SettingsIcon className="h-4 w-4" /> {t("通用")}
          </TabsTrigger>
          <TabsTrigger value="appearance" className="gap-2 px-5 shrink-0">
            <Palette className="h-4 w-4" /> {t("外观")}
          </TabsTrigger>
          <TabsTrigger value="gateway" className="gap-2 px-5 shrink-0">
            <Globe className="h-4 w-4" /> {t("网关")}
          </TabsTrigger>
          <TabsTrigger value="tasks" className="gap-2 px-5 shrink-0">
            <Cpu className="h-4 w-4" /> {t("任务")}
          </TabsTrigger>
          <TabsTrigger value="env" className="gap-2 px-5 shrink-0">
            <Variable className="h-4 w-4" /> {t("环境")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="general" className="space-y-6">
          <Card className="glass-card shadow-sm">
            <CardHeader>
              <div className="flex items-center gap-2">
                <AppWindow className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">{t("基础设置")}</CardTitle>
              </div>
              <CardDescription>{t("控制应用启动和窗口行为")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <Card size="sm">
                <CardContent className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                  <div className="space-y-1">
                    <Label>{updateActionLabel}</Label>
                    <p className="text-xs text-muted-foreground">
                      {updateActionDescription}
                    </p>
                    {lastUpdateCheck ? (
                      <p className="text-xs text-muted-foreground">
                        {preparedUpdate
                          ? `${t("已下载")} ${preparedUpdate.latestVersion || preparedUpdate.releaseTag || t("新版本")}${t("，等待替换更新")}`
                          : lastUpdateCheck.hasUpdate
                            ? `${t("发现新版本")} ${lastUpdateCheck.latestVersion || lastUpdateCheck.releaseTag || t("可用")}`
                            : lastUpdateCheck.reason ||
                              `${t("当前版本")} ${lastUpdateCheck.currentVersion || t("未知")} ${t("已是最新")}`}
                      </p>
                    ) : null}
                    {shouldShowUpdateLogsEntry ? (
                      <div className="pt-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-auto px-0 text-xs text-muted-foreground hover:text-foreground"
                          onClick={handleOpenUpdateLogsDir}
                        >
                          <FolderOpen className="h-3.5 w-3.5" />
                          {t("打开日志目录")}
                        </Button>
                      </div>
                    ) : null}
                  </div>
                  <Button
                    variant="outline"
                    className="gap-2 self-start md:self-auto"
                    disabled={!canSelfUpdate || updateActionBusy}
                    onClick={handleUpdateAction}
                  >
                    {manualUpdateCheckPending ? (
                      <RefreshCw className="h-4 w-4 animate-spin" />
                    ) : prepareUpdate.isPending ? (
                      <Download className="h-4 w-4 animate-pulse" />
                    ) : applyPreparedUpdate.isPending ? (
                      <RefreshCw className="h-4 w-4 animate-spin" />
                    ) : hasPreparedUpdate ? (
                      <Check className="h-4 w-4" />
                    ) : canDownloadUpdate ? (
                      <Download className="h-4 w-4" />
                    ) : (
                      <RefreshCw className="h-4 w-4" />
                    )}
                    {updateActionBusyLabel}
                  </Button>
                </CardContent>
              </Card>
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label>{t("关闭时最小化到托盘")}</Label>
                  <p className="text-xs text-muted-foreground">
                    {t("点击关闭按钮不会直接退出程序")}
                  </p>
                </div>
                <Switch
                  checked={snapshot.closeToTrayOnClose}
                  disabled={!canCloseToTray || !snapshot.closeToTraySupported}
                  onCheckedChange={(value) =>
                    updateSettings.mutate({ closeToTrayOnClose: value })
                  }
                />
              </div>
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label>{t("视觉性能模式")}</Label>
                  <p className="text-xs text-muted-foreground">
                    {t("关闭毛玻璃等特效以提升低配电脑性能")}
                  </p>
                </div>
                <Switch
                  checked={snapshot.lowTransparency}
                  onCheckedChange={(value) =>
                    updateSettings.mutate({ lowTransparency: value })
                  }
                />
              </div>
            </CardContent>
          </Card>

          <Card className="glass-card shadow-sm">
            <CardHeader>
              <div className="flex items-center gap-2">
                <Globe className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">{t("服务监听")}</CardTitle>
              </div>
              <CardDescription>
                {t("统一控制 Service 与 Web 的监听模式，决定仅本机访问还是开放给局域网")}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="grid gap-2">
                <Label>{t("监听地址")}</Label>
                <Select
                  value={snapshot.serviceListenMode || "loopback"}
                  onValueChange={(value) => {
                    const nextValue = String(value || "").trim() || "loopback";
                    if (nextValue === snapshot.serviceListenMode) {
                      return;
                    }
                    updateSettings.mutate({ serviceListenMode: nextValue });
                  }}
                >
                  <SelectTrigger className="w-full md:w-[320px]">
                    <SelectValue placeholder={t("选择监听地址模式")}>
                      {(value) =>
                        t(
                          SERVICE_LISTEN_MODE_LABELS[
                            String(value || "").trim()
                          ] || String(value || "").trim() || "仅本机 (localhost)",
                        )
                      }
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    {(snapshot.serviceListenModeOptions?.length
                      ? snapshot.serviceListenModeOptions
                      : ["loopback", "all_interfaces"]
                    ).map((mode) => (
                      <SelectItem key={mode} value={mode}>
                        {t(SERVICE_LISTEN_MODE_LABELS[mode] || mode)}
                      </SelectItem>
                    ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>

              <Card size="sm">
                <CardContent className="text-sm">
                <div className="flex items-center justify-between gap-4">
                  <span className="text-muted-foreground">{t("当前访问地址")}</span>
                  <code className="text-xs text-primary">
                    {snapshot.serviceAddr}
                  </code>
                </div>
                <Separator className="my-2" />
                <div className="mt-2 flex items-center justify-between gap-4">
                  <span className="text-muted-foreground">{t("实际监听地址")}</span>
                  <code className="text-xs text-primary">
                    {inferServiceBindPreview(
                      snapshot.serviceAddr,
                      snapshot.serviceListenMode || "loopback",
                    )}
                  </code>
                </div>
                </CardContent>
              </Card>

              <p className="text-[10px] text-muted-foreground">
                {t("切换到")} <code>0.0.0.0</code>{" "}
                {t(
                  "后，局域网设备可通过当前机器 IP 访问；设置保存后需要重启相关进程才会生效，Web 监听地址会默认跟随这里的模式。",
                )}
              </p>
            </CardContent>
          </Card>

          {showAccessControlSettings ? (
            <Card className="glass-card shadow-sm">
              <CardHeader>
                <div className="flex items-center gap-2">
                  <ShieldCheck className="h-4 w-4 text-primary" />
                  <CardTitle className="text-base">{t("访问控制")}</CardTitle>
                </div>
                <CardDescription>
                  {t("统一管理 Web 登录方式、访问密码和团队额度分发。")}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <Card size="sm">
                  <CardContent className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                    <div className="space-y-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <Label>{t("当前访问方式")}</Label>
                        <Badge variant="secondary">{t(webAuthModeLabel)}</Badge>
                      </div>
                      <p className="text-xs text-muted-foreground">
                        {snapshot.distributionEnabled
                          ? t("额度分发已开启，平台 Key 会按归属钱包扣减额度。")
                          : t("额度分发未开启，平台 Key 不会扣减成员钱包额度。")}
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      className="gap-2 self-start md:self-auto"
                      disabled={!canAccessManagementRpc}
                      onClick={() => setWebPasswordModalOpen(true)}
                    >
                      <ShieldCheck className="h-4 w-4" />
                      {t("访问控制")}
                    </Button>
                  </CardContent>
                </Card>
              </CardContent>
            </Card>
          ) : null}

        </TabsContent>

        <TabsContent value="appearance" className="space-y-6">
          <Card className="glass-card shadow-sm">
            <CardHeader>
              <div className="flex items-center gap-2">
                <Palette className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">{t("样式版本")}</CardTitle>
              </div>
              <CardDescription>{t("在渐变版本和默认版本之间切换")}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid gap-3 md:grid-cols-2">
                {APPEARANCE_PRESETS.map((item) => {
                  const currentPreset = normalizeAppearancePreset(
                    snapshot.appearancePreset,
                  );
                  const isActive = currentPreset === item.id;
                  return (
                    <Button
                      key={item.id}
                      type="button"
                      variant="outline"
                      onClick={() => handleAppearancePresetChange(item.id)}
                      className={cn(
                        "group relative h-auto justify-start rounded-xl p-4 text-left transition-all duration-300",
                        isActive
                          ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary"
                          : "border-border/60 bg-background/50 hover:bg-accent/30",
                      )}
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="space-y-1.5">
                          <div className="text-sm font-semibold">
                            {t(item.name)}
                          </div>
                          <p className="text-xs leading-5 text-muted-foreground">
                            {t(item.description)}
                          </p>
                        </div>
                        {isActive ? (
                          <div className="rounded-full bg-primary p-1 text-primary-foreground shadow-sm">
                            <Check className="h-3 w-3" />
                          </div>
                        ) : null}
                      </div>
                      <div className="mt-3 flex items-end gap-2.5">
                        <div
                          className={cn(
                            "h-14 flex-1 rounded-xl border",
                            item.id === "modern"
                              ? "border-primary/20 bg-accent/50"
                              : "border-border/70 bg-muted/70",
                          )}
                        />
                        <div className="flex w-16 flex-col gap-1.5">
                          <div
                            className={cn(
                              "h-4 rounded-lg border",
                              item.id === "modern"
                                ? "border-primary/15 bg-card shadow-sm"
                                : "border-border/70 bg-card",
                            )}
                          />
                          <div
                            className={cn(
                              "h-4 rounded-lg border",
                              item.id === "modern"
                                ? "border-primary/15 bg-card/80 shadow-sm"
                                : "border-border/70 bg-card/80",
                            )}
                          />
                        </div>
                      </div>
                    </Button>
                  );
                })}
              </div>
            </CardContent>
          </Card>

          <Card className="glass-card shadow-sm">
            <CardHeader>
              <div className="flex items-center gap-2">
                <Palette className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">{t("界面主题")}</CardTitle>
              </div>
              <CardDescription>
                {t("选择您喜爱的配色方案，适配不同工作心情")}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6 xl:grid-cols-12">
                {THEMES.map((item) => (
                  <Button
                    key={item.id}
                    type="button"
                    variant="ghost"
                    onClick={() => handleThemeChange(item.id)}
                    className={cn(
                      "group relative h-auto flex-col items-center gap-2.5 rounded-xl border p-4 transition-all duration-300 hover:bg-accent/40",
                      theme === item.id
                        ? "border-primary bg-primary/10 shadow-sm ring-1 ring-primary"
                        : "border-transparent bg-muted/20 hover:bg-accent/40",
                    )}
                  >
                    <div
                      className="h-10 w-10 rounded-full border-2 border-white/20 shadow-sm"
                      style={{ backgroundColor: item.color }}
                    />
                    <span
                      className={cn(
                        "whitespace-nowrap text-[10px] font-semibold transition-colors",
                        theme === item.id
                          ? "text-primary"
                          : "text-muted-foreground group-hover:text-foreground",
                      )}
                    >
                      {t(item.name)}
                    </span>
                    {theme === item.id ? (
                      <div className="absolute right-2 top-2 rounded-full bg-primary p-0.5 text-primary-foreground shadow-sm">
                        <Check className="h-2.5 w-2.5" />
                      </div>
                    ) : null}
                  </Button>
                ))}
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="gateway" className="space-y-4">
          <Card className="glass-card shadow-sm">
            <CardHeader>
              <CardTitle className="text-base">{t("网关策略")}</CardTitle>
              <CardDescription>{t("配置账号选路和请求头处理方式")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="grid gap-2">
                <Label>{t("账号选路策略")}</Label>
                <Select
                  value={snapshot.routeStrategy || "ordered"}
                  onValueChange={(value) =>
                    updateSettings.mutate({ routeStrategy: value || "ordered" })
                  }
                >
                  <SelectTrigger className="w-full md:w-[300px]">
                    <SelectValue placeholder={t("选择策略")}>
                      {(value) => {
                        const nextValue = String(value || "").trim();
                        if (!nextValue) return t("选择策略");
                        return t(ROUTE_STRATEGY_LABELS[nextValue] || nextValue);
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="ordered">{t("顺序优先 (Ordered)")}</SelectItem>
                    <SelectItem value="balanced">
                      {t("均衡轮询 (Balanced)")}
                    </SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <p className="text-[10px] text-muted-foreground">
                  {t(
                    "顺序优先：按账号候选顺序优先尝试，默认只会在头部小窗口内按健康度做轻微换头；均衡轮询：按“平台密钥 + 模型”维度严格轮询可用账号，默认不做健康度换头。",
                  )}
                </p>
              </div>

              <div className="grid gap-4 border-t pt-6">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <ShieldCheck className="h-4 w-4 text-primary" />
                      <Label>{t("额度保护")}</Label>
                    </div>
                    <p className="text-[10px] text-muted-foreground">
                      {t("低于保留百分比的账号会从网关路由和远端模型刷新候选中跳过。")}
                    </p>
                  </div>
                  <Switch
                    checked={snapshot.quotaGuard.enabled}
                    onCheckedChange={(checked) =>
                      updateSettings.mutate({
                        quotaGuard: {
                          ...snapshot.quotaGuard,
                          enabled: checked,
                        },
                      })
                    }
                  />
                </div>

                <div className="grid gap-4 md:grid-cols-3">
                  <div className="grid gap-2">
                    <Label>{t("5 小时窗口保留 (%)")}</Label>
                    <Input
                      type="number"
                      min={0}
                      max={100}
                      value={quotaGuardInputValues.primaryMinRemainingPercent}
                      onChange={(event) =>
                        setQuotaGuardDraft((current) => ({
                          ...current,
                          primaryMinRemainingPercent: event.target.value,
                        }))
                      }
                      onBlur={() =>
                        saveQuotaGuardField("primaryMinRemainingPercent")
                      }
                      disabled={!snapshot.quotaGuard.enabled}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label>{t("周窗口保留 (%)")}</Label>
                    <Input
                      type="number"
                      min={0}
                      max={100}
                      value={quotaGuardInputValues.secondaryMinRemainingPercent}
                      onChange={(event) =>
                        setQuotaGuardDraft((current) => ({
                          ...current,
                          secondaryMinRemainingPercent: event.target.value,
                        }))
                      }
                      onBlur={() =>
                        saveQuotaGuardField("secondaryMinRemainingPercent")
                      }
                      disabled={!snapshot.quotaGuard.enabled}
                    />
                  </div>
                  <div className="flex items-center justify-between gap-3 rounded-md border border-border/60 px-3 py-2">
                    <div className="space-y-1">
                      <Label>{t("全部低额度时兜底")}</Label>
                      <p className="text-[10px] text-muted-foreground">
                        {t("关闭后如果所有账号都低于阈值，网关会返回无可用账号。")}
                      </p>
                    </div>
                    <Switch
                      checked={snapshot.quotaGuard.allowAllLowQuotaFallback}
                      onCheckedChange={(checked) =>
                        updateSettings.mutate({
                          quotaGuard: {
                            ...snapshot.quotaGuard,
                            allowAllLowQuotaFallback: checked,
                          },
                        })
                      }
                      disabled={!snapshot.quotaGuard.enabled}
                    />
                  </div>
                </div>
              </div>

              <div className="grid gap-2">
                <Label>{t("Free 账号使用模型")}</Label>
                <Select
                  value={snapshot.freeAccountMaxModel || "auto"}
                  onValueChange={(value) =>
                    updateSettings.mutate({
                      freeAccountMaxModel: value || "auto",
                    })
                  }
                >
                  <SelectTrigger className="w-full md:w-[300px]">
                    <SelectValue placeholder={t("选择 free 账号使用模型")}>
                      {(value) =>
                        t(formatFreeAccountModelLabel(String(value || "")))
                      }
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    {(snapshot.freeAccountMaxModelOptions?.length
                      ? snapshot.freeAccountMaxModelOptions
                      : DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
                    ).map((model) => (
                      <SelectItem key={model} value={model}>
                        {t(formatFreeAccountModelLabel(model))}
                      </SelectItem>
                    ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <p className="text-[10px] text-muted-foreground">
                  {t(
                    "设为“跟随请求”时，不会额外改写 free / 7天单窗口账号的模型；只有你选了具体模型后，命中这些账号时才会统一改写为该模型。",
                  )}
                </p>
              </div>

              <div className="grid gap-2">
                <Label>{t("模型转发规则")}</Label>
                <Textarea
                  className="min-h-[132px] max-w-2xl font-mono text-xs"
                  placeholder={"spark*=gpt-5.4-mini\nclaude-sonnet-4*=gpt-5.4"}
                  value={modelForwardRulesInput}
                  onChange={(event) =>
                    setModelForwardRulesDraft(event.target.value)
                  }
                  onBlur={() => {
                    if (modelForwardRulesDraft == null) return;
                    if (
                      modelForwardRulesInput.trim() ===
                      (snapshot.modelForwardRules || "").trim()
                    ) {
                      setModelForwardRulesDraft(null);
                      return;
                    }
                    void updateSettings
                      .mutateAsync({
                        modelForwardRules: modelForwardRulesInput,
                      })
                      .then(() => setModelForwardRulesDraft(null))
                      .catch(() => undefined);
                  }}
                />
                <p className="text-[10px] text-muted-foreground">
                  {t("一行一条，格式为")} <code>{t("源模型=目标模型")}</code>，{t("支持")}
                  <code>*</code>{" "}
                  {t("通配。平台 Key 没有强绑模型时，会先按这里把请求模型改写，再进入账号路由。")}
                </p>
              </div>

              <div className="grid gap-2 border-t pt-6">
                <Label>{t("上游 Originator")}</Label>
                <Input
                  className="h-10 max-w-md font-mono"
                  value={gatewayOriginatorInput}
                  onChange={(event) =>
                    setGatewayOriginatorDraft(event.target.value)
                  }
                  onBlur={() => {
                    if (gatewayOriginatorDraft == null) return;
                    if (
                      gatewayOriginatorInput ===
                      (snapshot.gatewayOriginator || gatewayOriginatorDefault)
                    ) {
                      setGatewayOriginatorDraft(null);
                      return;
                    }
                    void updateSettings
                      .mutateAsync({
                        gatewayOriginator: gatewayOriginatorInput,
                      })
                      .then(() => setGatewayOriginatorDraft(null))
                      .catch(() => undefined);
                  }}
                />
                <p className="text-[10px] text-muted-foreground">
                  {t("对齐官方 Codex 的上游 Originator。默认值为")}{" "}
                  <code>{gatewayOriginatorDefault}</code>
                  {t("，会同步影响登录和网关上游请求头。")}
                </p>
              </div>

              <div className="grid gap-2">
                <Label>{t("区域驻留要求")}</Label>
                <Select
                  value={
                    (snapshot.gatewayResidencyRequirement ?? "") ||
                    EMPTY_RESIDENCY_OPTION
                  }
                  onValueChange={(value) =>
                    updateSettings.mutate({
                      gatewayResidencyRequirement:
                        value === EMPTY_RESIDENCY_OPTION ? "" : (value ?? ""),
                    })
                  }
                >
                  <SelectTrigger className="w-full md:w-[300px]">
                  <SelectValue placeholder={t("选择地域约束")}>
                      {(value) => {
                        const nextValue =
                          String(value || "") === EMPTY_RESIDENCY_OPTION
                            ? ""
                            : String(value || "");
                        return (
                          t(RESIDENCY_REQUIREMENT_LABELS[nextValue] || nextValue)
                        );
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    {(snapshot.gatewayResidencyRequirementOptions?.length
                      ? snapshot.gatewayResidencyRequirementOptions
                      : ["", "us"]
                    ).map((value) => (
                      <SelectItem
                        key={value || EMPTY_RESIDENCY_OPTION}
                        value={value || EMPTY_RESIDENCY_OPTION}
                      >
                        {t(RESIDENCY_REQUIREMENT_LABELS[value] || value)}
                      </SelectItem>
                    ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <p className="text-[10px] text-muted-foreground">
                  {t("对齐官方 Codex 的")}{" "}
                  <code>x-openai-internal-codex-residency</code>
                  {t("头。")}
                  {t("当前只支持留空或")} <code>us</code>
                  {t("。")}
                </p>
              </div>

              <div className="grid gap-2 pt-2">
                <Label>{t("上游代理 (Proxy)")}</Label>
                <Input
                  placeholder="http://127.0.0.1:7890"
                  className="h-10 max-w-md font-mono"
                  value={upstreamProxyInput}
                  onChange={(event) =>
                    setUpstreamProxyDraft(event.target.value)
                  }
                  onBlur={() => {
                    if (upstreamProxyDraft == null) return;
                    if (
                      upstreamProxyInput === (snapshot.upstreamProxyUrl || "")
                    ) {
                      setUpstreamProxyDraft(null);
                      return;
                    }
                    void updateSettings
                      .mutateAsync({ upstreamProxyUrl: upstreamProxyInput })
                      .then(() => setUpstreamProxyDraft(null))
                      .catch(() => undefined);
                  }}
                />
                <p className="text-[10px] text-muted-foreground">
                  {t("支持 http/https/socks5，留空表示直连。")}
                </p>
              </div>

              <div className="grid gap-4 border-t pt-6 md:grid-cols-3">
                <div className="grid gap-2">
                  <Label>{t("SSE 保活间隔 (ms)")}</Label>
                  <Input
                    type="number"
                    value={transportInputValues.sseKeepaliveIntervalMs}
                    onChange={(event) =>
                      setTransportDraft((current) => ({
                        ...current,
                        sseKeepaliveIntervalMs: event.target.value,
                      }))
                    }
                    onBlur={() =>
                      saveTransportField("sseKeepaliveIntervalMs", 1)
                    }
                  />
                </div>
                <div className="grid gap-2">
                  <Label>{t("上游总超时 (ms，0 为关闭)")}</Label>
                  <Input
                    type="number"
                    value={transportInputValues.upstreamTotalTimeoutMs}
                    onChange={(event) =>
                      setTransportDraft((current) => ({
                        ...current,
                        upstreamTotalTimeoutMs: event.target.value,
                      }))
                    }
                    onBlur={() =>
                      saveTransportField("upstreamTotalTimeoutMs", 0)
                    }
                  />
                </div>
                <div className="grid gap-2">
                  <Label>{t("上游流式空闲超时 (ms)")}</Label>
                  <Input
                    type="number"
                    value={transportInputValues.upstreamStreamTimeoutMs}
                    onChange={(event) =>
                      setTransportDraft((current) => ({
                        ...current,
                        upstreamStreamTimeoutMs: event.target.value,
                      }))
                    }
                    onBlur={() =>
                      saveTransportField("upstreamStreamTimeoutMs", 0)
                    }
                  />
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="tasks" className="space-y-4">
          <Card className="glass-card shadow-sm">
            <CardHeader>
              <CardTitle className="text-base">{t("后台任务线程")}</CardTitle>
              <CardDescription>{t("管理自动轮询和保活任务；")}</CardDescription>
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
                      checked={
                        snapshot.backgroundTasks[
                          task.enabledKey as keyof BackgroundTaskSettings
                        ] as boolean
                      }
                      onCheckedChange={(value) =>
                        updateBackgroundTasks({
                          [task.enabledKey]: value,
                        } as Partial<BackgroundTaskSettings>)
                      }
                    />
                    <Label>{t(task.label)}</Label>
                  </div>
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      {t("间隔(秒)")}
                    </span>
                    <Input
                      className="h-8 w-20"
                      type="number"
                      value={
                        backgroundTaskDraft[task.intervalKey] ||
                        stringifyNumber(
                          snapshot.backgroundTasks[
                            task.intervalKey as keyof BackgroundTaskSettings
                          ] as number,
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
                          1,
                        )
                      }
                    />
                  </div>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card className="glass-card shadow-sm">
            <CardHeader>
              <div className="flex items-center gap-2">
                <SettingsIcon className="h-4 w-4 text-primary" />
                <CardTitle className="text-base">{t("运行模式")}</CardTitle>
              </div>
              <CardDescription>
                {t(
                  "普通用户选择一个模式即可，系统会自动按档位调整并发。需要更细的控制时，再打开高级参数。",
                )}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Card size="sm">
                <CardContent>
                <div className="grid gap-4 lg:grid-cols-[minmax(280px,380px)_minmax(0,1fr)] lg:items-end">
                  <div className="space-y-2">
                    <Label>{t("运行模式")}</Label>
                    <Select
                      value={activeWorkerModeValue}
                      onValueChange={(value) => {
                        const selectedPreset = WORKER_PRESETS.find(
                          (preset) => preset.key === value,
                        );
                        if (!selectedPreset) {
                          return;
                        }
                        if (selectedPreset.key === "recommended") {
                          deriveConcurrencyRecommendation.mutate();
                          return;
                        }
                        applyWorkerPreset(selectedPreset);
                      }}
                    >
                      <SelectTrigger
                        className="h-10 w-full bg-background/80"
                        disabled={deriveConcurrencyRecommendation.isPending}
                      >
                        <SelectValue placeholder={t("选择运行模式")}>
                          {(value) => {
                            const selectedPreset = WORKER_PRESETS.find(
                              (preset) =>
                                preset.key === String(value || "").trim(),
                            );
                            return selectedPreset
                              ? t(selectedPreset.simpleLabel)
                              : t("自定义（来自高级参数）");
                          }}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                    <SelectGroup>
                        {WORKER_PRESETS.map((preset) => (
                          <SelectItem key={preset.key} value={preset.key}>
                            {t(preset.simpleLabel)}
                          </SelectItem>
                        ))}
                        {!activeWorkerPreset ? (
                          <SelectItem value={CUSTOM_WORKER_MODE_VALUE} disabled>
                            {t("自定义（来自高级参数）")}
                          </SelectItem>
                        ) : null}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="flex min-h-10 flex-wrap items-center gap-2 lg:justify-start lg:self-end">
                    <span className="text-sm font-medium">{t("当前档位")}</span>
                    <Badge
                      variant={activeWorkerPreset ? "default" : "secondary"}
                      className="h-5 px-2"
                    >
                      {activeWorkerPreset
                        ? t(activeWorkerPreset.simpleLabel)
                        : t("自定义")}
                    </Badge>
                  </div>
                </div>

                <Separator className="my-4" />
                <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                  <p className="text-xs leading-6 text-muted-foreground">
                    {activeWorkerSummary}
                  </p>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-8 w-fit gap-2 px-2"
                    onClick={() => setWorkerAdvancedDialogOpen(true)}
                  >
                    <SettingsIcon className="h-4 w-4" />
                    {t("高级参数")}
                  </Button>
                </div>
                </CardContent>
              </Card>
            </CardContent>
          </Card>
          <Dialog
            open={workerAdvancedDialogOpen}
            onOpenChange={setWorkerAdvancedDialogOpen}
          >
            <DialogContent className="glass-card sm:max-w-2xl">
              <DialogHeader>
                <DialogTitle>{t("高级参数")}</DialogTitle>
                <DialogDescription>
                  {t(
                    "只有在你明确知道这些参数含义时再调整。改动会直接影响并发和资源占用。",
                  )}
                </DialogDescription>
              </DialogHeader>
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                {[
                  {
                    label: "后台巡检并发",
                    helper:
                      "控制用量刷新、后台轮询这些任务同时跑多少个。",
                    key: "usageRefreshWorkers",
                  },
                  {
                    label: "普通请求自动并发",
                    helper:
                      "普通 HTTP 请求的自动并发倍率，越大越快，也越吃资源。",
                    key: "httpWorkerFactor",
                  },
                  {
                    label: "普通请求最低保底",
                    helper:
                      "普通 HTTP 请求至少保留多少个处理线程，防止太冷清。",
                    key: "httpWorkerMin",
                  },
                  {
                    label: "流式请求自动并发",
                    helper:
                      "流式请求的自动并发倍率，流式响应多时会更明显。",
                    key: "httpStreamWorkerFactor",
                  },
                  {
                    label: "流式请求最低保底",
                    helper:
                      "流式请求至少保留多少个处理线程，保证长连接不卡住。",
                    key: "httpStreamWorkerMin",
                  },
                  {
                    label: "单账号并发上限",
                    helper:
                      "同一账号同时能处理多少个请求。满了以后会优先换下一个账号；填 0 表示关闭上限。",
                    key: "accountMaxInflight",
                  },
                ].map((worker) => (
                  <div key={worker.key} className="grid gap-1.5">
                    <Label className="text-xs">{t(worker.label)}</Label>
                    <p className="text-[11px] leading-5 text-muted-foreground">
                      {t(worker.helper)}
                    </p>
                    <Input
                      type="number"
                      min={worker.key === "accountMaxInflight" ? 0 : 1}
                      className="h-9"
                      value={
                        backgroundTaskDraft[worker.key] ??
                        stringifyNumber(
                          worker.key === "accountMaxInflight"
                            ? snapshot.accountMaxInflight
                            : (snapshot.backgroundTasks[
                                worker.key as keyof BackgroundTaskSettings
                              ] as number),
                        )
                      }
                      onChange={(event) =>
                        setBackgroundTaskDraft((current) => ({
                          ...current,
                          [worker.key]: event.target.value,
                        }))
                      }
                      onBlur={() =>
                        worker.key === "accountMaxInflight"
                          ? saveAccountMaxInflightField(0)
                          : saveBackgroundTaskField(
                              worker.key as keyof BackgroundTaskSettings,
                              1,
                            )
                      }
                    />
                  </div>
                ))}
              </div>
              <DialogFooter className="gap-2 sm:gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => setWorkerAdvancedDialogOpen(false)}
                >
                  {t("关闭")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </TabsContent>

        <TabsContent value="env" className="space-y-4">
          <div className="flex flex-col gap-3 rounded-xl border border-border/50 bg-muted/20 p-4 sm:flex-row sm:items-center sm:justify-between">
            <div className="space-y-1">
              <h3 className="text-sm font-semibold">{t("环境变量配置")}</h3>
              <p className="text-sm leading-6 text-muted-foreground">
                {t(
                  "这里保留旧版和外部部署环境变量覆盖；普通用户优先使用前面结构化设置，高风险项只建议排障时临时修改。",
                )}
              </p>
            </div>
            <Button
              type="button"
              variant="outline"
              className="gap-2 self-start sm:self-auto"
              disabled={!hasCustomizedEnvOverrides || updateSettings.isPending}
              onClick={() => setResetAllEnvDialogOpen(true)}
            >
              <RotateCcw className="h-4 w-4" />
              {t("全部恢复默认")}
            </Button>
          </div>

          <div className="grid gap-6 md:grid-cols-[300px_1fr]">
            <Card className="glass-card flex h-[500px] flex-col shadow-sm">
              <CardHeader className="pb-3">
                <div className="relative">
                  <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                  <Input
                    placeholder={t("搜索变量...")}
                    className="h-9 pl-9"
                    value={envSearch}
                    onChange={(event) => setEnvSearch(event.target.value)}
                  />
                </div>
              </CardHeader>
              <CardContent className="flex-1 overflow-y-auto p-2">
                <div className="space-y-1">
                  {filteredEnvCatalog.map((item) => (
                    <Button
                      key={item.key}
                      type="button"
                      variant="ghost"
                      onClick={() => setSelectedEnvKey(item.key)}
                      className={cn(
                        "h-auto w-full justify-start rounded-md px-3 py-2 text-left text-sm transition-colors",
                        selectedEnvKey === item.key
                          ? "bg-primary text-primary-foreground"
                          : "hover:bg-accent",
                      )}
                    >
                      <div className="flex items-center gap-2">
                        <span className="min-w-0 flex-1 truncate font-medium">
                          {t(item.label)}
                        </span>
                        <Badge
                          variant="outline"
                          className={cn(
                            "shrink-0 px-1.5 py-0 text-[10px]",
                            selectedEnvKey === item.key
                              ? "border-primary-foreground/30 bg-primary-foreground/10 text-primary-foreground"
                              : ENV_RISK_BADGE_CLASSES[
                                  normalizeEnvRiskLevel(item.riskLevel)
                                ],
                          )}
                        >
                          {t(
                            ENV_RISK_LABELS[
                              normalizeEnvRiskLevel(item.riskLevel)
                            ],
                          )}
                        </Badge>
                      </div>
                      <code className="block truncate text-[10px] opacity-70">
                        {item.key}
                      </code>
                    </Button>
                  ))}
                </div>
              </CardContent>
            </Card>

            <Card className="glass-card min-h-[500px] shadow-sm">
              {selectedEnvKey ? (
                <>
                  <CardHeader>
                    <div className="flex flex-col gap-2">
                      <CardTitle className="text-lg">
                        {selectedEnvItem ? t(selectedEnvItem.label) : null}
                      </CardTitle>
                      <div className="flex flex-wrap items-center gap-2">
                        <code className="w-fit rounded bg-primary/10 px-2 py-0.5 text-xs text-primary">
                          {selectedEnvKey}
                        </code>
                        <Badge
                          variant="outline"
                          className={cn(
                            "px-2 py-0.5",
                            ENV_RISK_BADGE_CLASSES[selectedEnvRiskLevel],
                          )}
                        >
                          {t(ENV_RISK_LABELS[selectedEnvRiskLevel])}
                        </Badge>
                        <Badge variant="secondary" className="px-2 py-0.5">
                          {t(
                            ENV_EFFECT_SCOPE_LABELS[selectedEnvEffectScope] ||
                              selectedEnvEffectScope,
                          )}
                        </Badge>
                      </div>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-6">
                    <Alert>
                      <Info />
                      <AlertDescription>
                        {t(
                          ENV_DESCRIPTION_MAP[selectedEnvKey] ||
                            `${selectedEnvItem?.label} 对应环境变量，修改后会应用到相关模块。`,
                        )}
                      </AlertDescription>
                    </Alert>
                    {selectedEnvRiskLevel === "high" ? (
                      <Alert variant="destructive">
                        <AlertDescription>{t(selectedEnvSafetyNote)}</AlertDescription>
                      </Alert>
                    ) : (
                      <Alert>
                        <AlertDescription>{t(selectedEnvSafetyNote)}</AlertDescription>
                      </Alert>
                    )}

                    <div className="space-y-2">
                      <Label>{t("当前值")}</Label>
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
                        placeholder={t("输入变量值")}
                      />
                      <p className="text-[10px] text-muted-foreground">
                        {t("默认值:")}{" "}
                        <span className="font-mono italic">
                          {selectedEnvItem?.defaultValue || t("空")}
                        </span>
                      </p>
                    </div>

                    <Separator />
                    <div className="flex gap-3">
                      <Button onClick={handleSaveEnv} className="gap-2">
                        <Save className="h-4 w-4" /> {t("保存修改")}
                      </Button>
                      <Button
                        variant="outline"
                        onClick={handleResetEnv}
                        className="gap-2"
                      >
                        <RotateCcw className="h-4 w-4" /> {t("恢复默认")}
                      </Button>
                    </div>
                  </CardContent>
                </>
              ) : (
                <CardContent className="flex h-full flex-col items-center justify-center gap-4 text-muted-foreground">
                  <div className="rounded-full bg-accent/30 p-4">
                    <Variable className="h-12 w-12 opacity-20" />
                  </div>
                  <p>{t("请从左侧列表选择一个环境变量进行配置")}</p>
                  <Button
                    type="button"
                    variant="outline"
                    className="gap-2"
                    disabled={!hasCustomizedEnvOverrides || updateSettings.isPending}
                    onClick={() => setResetAllEnvDialogOpen(true)}
                  >
                    <RotateCcw className="h-4 w-4" />
                    {t("全部恢复默认")}
                  </Button>
                </CardContent>
              )}
            </Card>
          </div>
        </TabsContent>
      </Tabs>

      <Dialog
        open={updateDialogOpen}
        onOpenChange={(open) => {
          if (prepareUpdate.isPending || applyPreparedUpdate.isPending) {
            return;
          }
          setUpdateDialogOpen(open);
        }}
      >
        <DialogContent
          showCloseButton={false}
          className="glass-card p-6 sm:max-w-[480px]"
        >
          <DialogHeader>
            <DialogTitle>
              {preparedUpdate ? t("替换更新") : t("发现新版本")}
            </DialogTitle>
            <DialogDescription>
              {preparedUpdate
                ? preparedUpdate.isPortable
                  ? t("更新包已下载完成。确认后将重启应用并替换当前程序。")
                  : t("更新包已下载完成。确认后会开始替换流程。")
                : `${t("当前版本")} ${updateDialogCheck?.currentVersion || t("未知")}，${t("发现新版本")} ${
                    updateDialogCheck?.latestVersion ||
                    updateDialogCheck?.releaseTag ||
                    t("可用")
                  }。`}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3 text-sm">
            <Card size="sm">
              <CardContent>
              <div className="flex items-center justify-between gap-4">
                <span className="text-muted-foreground">{t("当前版本")}</span>
                <span className="font-medium">
                  {updateDialogCheck?.currentVersion || t("未知")}
                </span>
              </div>
              <Separator className="my-2" />
              <div className="mt-2 flex items-center justify-between gap-4">
                <span className="text-muted-foreground">{t("目标版本")}</span>
                <span className="font-medium">
                  {preparedUpdate?.latestVersion ||
                    updateDialogCheck?.latestVersion ||
                    updateDialogCheck?.releaseTag ||
                    t("未知")}
                </span>
              </div>
              <Separator className="my-2" />
              <div className="mt-2 flex items-center justify-between gap-4">
                <span className="text-muted-foreground">{t("更新模式")}</span>
                <span className="font-medium">
                  {(preparedUpdate?.isPortable ?? updateDialogCheck?.isPortable)
                    ? t("便携包更新")
                    : t("安装包更新")}
                </span>
              </div>
              {preparedUpdate?.assetName ? (
                <>
                <Separator className="my-2" />
                <div className="mt-2 flex items-center justify-between gap-4">
                  <span className="text-muted-foreground">{t("更新文件")}</span>
                  <span className="max-w-[240px] truncate font-mono text-xs">
                    {preparedUpdate.assetName}
                  </span>
                </div>
                </>
              ) : null}
              </CardContent>
            </Card>

            {preparedUpdate ? null : updateDialogCheck?.reason ? (
              <Alert>
                <AlertDescription className="text-xs leading-5">
                  {updateDialogCheck.reason}
                </AlertDescription>
              </Alert>
            ) : (
              <Alert>
                <AlertDescription className="text-xs leading-5">
                  {t("建议先下载更新包，下载完成后再执行安装或重启更新。")}
                </AlertDescription>
              </Alert>
            )}
          </div>

          <DialogFooter className="gap-2 sm:gap-2">
            <Button
              variant="outline"
              disabled={
                prepareUpdate.isPending || applyPreparedUpdate.isPending
              }
              onClick={() => setUpdateDialogOpen(false)}
            >
              {t("稍后")}
            </Button>
            {preparedUpdate ? (
              <Button
                className="gap-2"
                disabled={applyPreparedUpdate.isPending}
                onClick={() =>
                  applyPreparedUpdate.mutate({
                    isPortable: preparedUpdate.isPortable,
                  })
                }
              >
                <Download className="h-4 w-4" />
                {applyPreparedUpdate.isPending
                  ? preparedUpdate.isPortable
                    ? t("正在替换更新...")
                    : t("正在启动替换...")
                  : t("替换更新")}
              </Button>
            ) : updateDialogCheck?.canPrepare ? (
              <Button
                className="gap-2"
                disabled={prepareUpdate.isPending}
                onClick={() => prepareUpdate.mutate()}
              >
                <Download className="h-4 w-4" />
                {prepareUpdate.isPending ? t("正在下载更新...") : t("下载更新")}
              </Button>
            ) : (
              <Button className="gap-2" onClick={handleOpenReleasePage}>
                <ExternalLink className="h-4 w-4" />
                {t("打开发布页")}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {showAccessControlSettings ? (
        <WebPasswordModal
          open={webPasswordModalOpen}
          onOpenChange={setWebPasswordModalOpen}
        />
      ) : null}

      <ConfirmDialog
        open={resetAllEnvDialogOpen}
        onOpenChange={setResetAllEnvDialogOpen}
        title={t("恢复全部环境默认值？")}
        description={t("会把环境页里所有可配置变量恢复为默认值，并清空你当前尚未保存的环境草稿。")}
        confirmText={t("确认恢复")}
        cancelText={t("取消")}
        onConfirm={handleResetAllEnv}
      />
    </div>
  );
}

export default function SettingsPage() {
  const { data: session, isLoading } = useAppSession();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const role = resolveSessionRole(session, isLoading, isDesktopRuntime);
  const { t } = useI18n();
  if (isLoading || !session) {
    return (
      <div className="flex h-64 items-center justify-center text-muted-foreground">
        {t("加载配置中...")}
      </div>
    );
  }
  if (role === "member") {
    return <MemberSettingsPage />;
  }
  return <AdminSettingsPage />;
}
