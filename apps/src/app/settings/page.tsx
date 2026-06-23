"use client";

import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Cpu,
  Download,
  ExternalLink,
  Globe,
  Palette,
  Save,
  Settings as SettingsIcon,
  UserRound,
  Variable,
  LockKeyhole,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { WebPasswordModal } from "@/components/modals/web-password-modal";
import { useI18n } from "@/lib/i18n/provider";
import { AppearanceTabContent } from "@/app/settings/components/appearance-tab-content";
import { EnvTabContent } from "@/app/settings/components/env-tab-content";
import { GatewayTabContent } from "@/app/settings/components/gateway-tab-content";
import {
  AccessControlCard,
  ServiceListenCard,
} from "@/app/settings/components/general-tab-cards";
import { GeneralBasicsCard } from "@/app/settings/components/general-basics-card";
import { TasksTabContent } from "@/app/settings/components/tasks-tab-content";
import {
  CUSTOM_WORKER_MODE_VALUE,
  ENV_DESCRIPTION_MAP,
  ENV_EFFECT_SCOPE_LABELS,
  ENV_RISK_BADGE_CLASSES,
  ENV_RISK_LABELS,
  SETTINGS_ACTIVE_TAB_KEY,
  SETTINGS_TABS,
  THEMES,
  WORKER_PRESET_KEYS,
  WORKER_PRESETS,
  buildReleaseUrl,
  type CheckUpdateRequest,
  compareEnvOverrideItems,
  ensureModelForwardRuleRows,
  matchesRecommendedWorkerSettings,
  normalizeEnvRiskLevel,
  normalizeWorkerRecommendation,
  parseIntegerInput,
  parseModelForwardRules,
  readInitialSettingsTab,
  serializeModelForwardRules,
  stringifyNumber,
  type SettingsTab,
  type WorkerPreset,
} from "@/app/settings/settings-page-helpers";function MemberSettingsPage() {
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
    let active = true;
    const nextDisplayName = session?.currentUser?.displayName || "";
    queueMicrotask(() => {
      if (active) setDisplayName(nextDisplayName);
    });
    return () => {
      active = false;
    };
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
  const [modelForwardRuleRowsDraft, setModelForwardRuleRowsDraft] = useState<
    ReturnType<typeof parseModelForwardRules> | null
  >(null);
  const [
    compactModelForwardRuleRowsDraft,
    setCompactModelForwardRuleRowsDraft,
  ] = useState<ReturnType<typeof parseModelForwardRules> | null>(null);
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
  const modelForwardRuleRows = ensureModelForwardRuleRows(
    modelForwardRuleRowsDraft ??
      parseModelForwardRules(snapshot?.modelForwardRules || ""),
  );
  const compactModelForwardRuleRows = ensureModelForwardRuleRows(
    compactModelForwardRuleRowsDraft ??
      parseModelForwardRules(snapshot?.compactModelForwardRules || ""),
  );
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
  const updateModelForwardRuleRows = (
    updater: (rows: ReturnType<typeof parseModelForwardRules>) => ReturnType<
      typeof parseModelForwardRules
    >,
  ) => {
    const sourceRows =
      modelForwardRuleRowsDraft ??
      parseModelForwardRules(snapshot?.modelForwardRules || "");
    setModelForwardRuleRowsDraft(updater(ensureModelForwardRuleRows(sourceRows)));
  };
  const commitModelForwardRulesDraft = () => {
    if (modelForwardRuleRowsDraft == null) return;
    const nextSerialized = serializeModelForwardRules(modelForwardRuleRowsDraft);
    if (nextSerialized.trim() === (snapshot?.modelForwardRules || "").trim()) {
      setModelForwardRuleRowsDraft(null);
      return;
    }
    void updateSettings
      .mutateAsync({
        modelForwardRules: nextSerialized,
      })
      .then(() => setModelForwardRuleRowsDraft(null))
      .catch(() => undefined);
  };
  const updateCompactModelForwardRuleRows = (
    updater: (rows: ReturnType<typeof parseModelForwardRules>) => ReturnType<
      typeof parseModelForwardRules
    >,
  ) => {
    const sourceRows =
      compactModelForwardRuleRowsDraft ??
      parseModelForwardRules(snapshot?.compactModelForwardRules || "");
    setCompactModelForwardRuleRowsDraft(
      updater(ensureModelForwardRuleRows(sourceRows)),
    );
  };
  const commitCompactModelForwardRulesDraft = () => {
    if (compactModelForwardRuleRowsDraft == null) return;
    const nextSerialized = serializeModelForwardRules(
      compactModelForwardRuleRowsDraft,
    );
    if (
      nextSerialized.trim() ===
      (snapshot?.compactModelForwardRules || "").trim()
    ) {
      setCompactModelForwardRuleRowsDraft(null);
      return;
    }
    void updateSettings
      .mutateAsync({
        compactModelForwardRules: nextSerialized,
      })
      .then(() => setCompactModelForwardRuleRowsDraft(null))
      .catch(() => undefined);
  };
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

  const saveBackgroundTaskTextField = (
    key: "warmupCronExpression",
  ) => {
    if (!snapshot) return;
    const draftKey = String(key);
    const sourceValue =
      backgroundTaskDraft[draftKey] ??
      String(snapshot.backgroundTasks[key] || "");
    const nextValue = sourceValue.trim();
    const schedules = nextValue
      .split("|")
      .map((item) => item.trim())
      .filter(Boolean);
    if (!nextValue && !snapshot.backgroundTasks.warmupCronEnabled) {
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
      return;
    }
    const allSchedulesValid =
      schedules.length > 0 &&
      schedules.every((item) => {
        const partCount = item.split(/\s+/).filter(Boolean).length;
        return partCount === 5 || partCount === 6;
      });
    if (!allSchedulesValid) {
      toast.error(t("Cron 表达式需要 5 段，或带秒的 6 段"));
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
          <GeneralBasicsCard
            t={t}
            updateActionLabel={updateActionLabel}
            updateActionDescription={updateActionDescription}
            lastUpdateCheck={lastUpdateCheck}
            preparedUpdate={preparedUpdate}
            shouldShowUpdateLogsEntry={shouldShowUpdateLogsEntry}
            handleOpenUpdateLogsDir={handleOpenUpdateLogsDir}
            canSelfUpdate={canSelfUpdate}
            updateActionBusy={updateActionBusy}
            handleUpdateAction={handleUpdateAction}
            manualUpdateCheckPending={manualUpdateCheckPending}
            prepareUpdatePending={prepareUpdate.isPending}
            applyPreparedUpdatePending={applyPreparedUpdate.isPending}
            hasPreparedUpdate={hasPreparedUpdate}
            canDownloadUpdate={canDownloadUpdate}
            updateActionBusyLabel={updateActionBusyLabel}
            snapshot={snapshot}
            canCloseToTray={canCloseToTray}
            updateSettings={updateSettings}
          />
<ServiceListenCard
            t={t}
            snapshot={snapshot}
            updateSettings={updateSettings}
          />

          <AccessControlCard
            t={t}
            snapshot={snapshot}
            canAccessManagementRpc={canAccessManagementRpc}
            showAccessControlSettings={showAccessControlSettings}
            webAuthModeLabel={webAuthModeLabel}
            onOpen={() => setWebPasswordModalOpen(true)}
          />
        </TabsContent>

        <TabsContent value="appearance" className="space-y-6">
          <AppearanceTabContent
            t={t}
            theme={theme}
            appearancePreset={normalizeAppearancePreset(snapshot.appearancePreset)}
            onThemeChange={handleThemeChange}
            onAppearancePresetChange={handleAppearancePresetChange}
          />
        </TabsContent>

        <TabsContent value="gateway" className="space-y-4">
          <GatewayTabContent
            t={t}
            snapshot={snapshot}
            updateSettings={updateSettings}
            quotaGuardInputValues={quotaGuardInputValues}
            setQuotaGuardDraft={setQuotaGuardDraft}
            saveQuotaGuardField={saveQuotaGuardField}
            transportInputValues={transportInputValues}
            setTransportDraft={setTransportDraft}
            saveTransportField={saveTransportField}
            modelForwardRuleRows={modelForwardRuleRows}
            updateModelForwardRuleRows={updateModelForwardRuleRows}
            commitModelForwardRulesDraft={commitModelForwardRulesDraft}
            compactModelForwardRuleRows={compactModelForwardRuleRows}
            updateCompactModelForwardRuleRows={updateCompactModelForwardRuleRows}
            commitCompactModelForwardRulesDraft={commitCompactModelForwardRulesDraft}
            gatewayOriginatorInput={gatewayOriginatorInput}
            gatewayOriginatorDraft={gatewayOriginatorDraft}
            setGatewayOriginatorDraft={setGatewayOriginatorDraft}
            gatewayOriginatorDefault={gatewayOriginatorDefault}
            upstreamProxyInput={upstreamProxyInput}
            upstreamProxyDraft={upstreamProxyDraft}
            setUpstreamProxyDraft={setUpstreamProxyDraft}
          />
        </TabsContent>

        <TabsContent value="tasks" className="space-y-4">
          <TasksTabContent
            t={t}
            snapshot={snapshot}
            backgroundTaskDraft={backgroundTaskDraft}
            setBackgroundTaskDraft={setBackgroundTaskDraft}
            updateBackgroundTasks={updateBackgroundTasks}
            saveBackgroundTaskField={saveBackgroundTaskField}
            saveBackgroundTaskTextField={saveBackgroundTaskTextField}
            activeWorkerModeValue={activeWorkerModeValue}
            activeWorkerPreset={activeWorkerPreset}
            activeWorkerSummary={activeWorkerSummary}
            deriveConcurrencyRecommendationPending={deriveConcurrencyRecommendation.isPending}
            applyWorkerPreset={applyWorkerPreset}
            deriveConcurrencyRecommendation={() => deriveConcurrencyRecommendation.mutate()}
            workerAdvancedDialogOpen={workerAdvancedDialogOpen}
            setWorkerAdvancedDialogOpen={setWorkerAdvancedDialogOpen}
            saveAccountMaxInflightField={saveAccountMaxInflightField}
            onInvalidWarmupCron={() => toast.error(t("请先填写 Cron 表达式"))}
          />
        </TabsContent>
        <TabsContent value="env" className="space-y-4">
          <EnvTabContent
            t={t}
            envSearch={envSearch}
            selectedEnvKey={selectedEnvKey}
            selectedEnvItem={selectedEnvItem}
            selectedEnvValue={selectedEnvValue}
            selectedEnvRiskLevel={selectedEnvRiskLevel}
            selectedEnvEffectScope={selectedEnvEffectScope}
            selectedEnvSafetyNote={selectedEnvSafetyNote}
            hasCustomizedEnvOverrides={hasCustomizedEnvOverrides}
            isSaving={updateSettings.isPending}
            filteredEnvCatalog={filteredEnvCatalog}
            descriptionMap={ENV_DESCRIPTION_MAP}
            riskBadgeClasses={ENV_RISK_BADGE_CLASSES}
            riskLabels={ENV_RISK_LABELS}
            effectScopeLabels={ENV_EFFECT_SCOPE_LABELS}
            onSearchChange={setEnvSearch}
            onSelectEnvKey={setSelectedEnvKey}
            onSelectedEnvValueChange={(value) => {
              if (!selectedEnvKey) return;
              setEnvDrafts((current) => ({
                ...current,
                [selectedEnvKey]: value,
              }));
            }}
            onSaveEnv={handleSaveEnv}
            onResetEnv={handleResetEnv}
            onResetAllEnv={() => setResetAllEnvDialogOpen(true)}
          />
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
