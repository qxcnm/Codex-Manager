"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  Clock,
  Gauge,
  Globe,
  HelpCircle,
  Loader2,
  MoreVertical,
  PencilLine,
  Plus,
  Search,
  ShieldAlert,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { ProxyProfileModal } from "@/components/modals/proxy-profile-modal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  PROXY_PROFILES_QUERY_KEY,
  PROXY_TEST_PRESETS_QUERY_KEY,
  proxyProfilesClient,
} from "@/lib/api/proxy-profiles";
import { getAppErrorMessage } from "@/lib/api/transport";
import {
  formatProxyGeoLocationParts,
  resolveProxyFlagDisplay,
} from "@/lib/utils/proxy-geo";
import { cn } from "@/lib/utils";
import type { ProxyProfile, ProxyTestJobState } from "@/types";
import { useI18n } from "@/lib/i18n/provider";
import { ProxyFlag } from "@/components/accounts/account-proxy-cell";
import { AccountProxyGeoStatusGrid } from "@/components/accounts/account-proxy-status-grid";

type ProxyFilter =
  | "all"
  | "enabled"
  | "disabled"
  | "healthy"
  | "failed"
  | "unchecked";

const JOB_POLL_INTERVAL_MS = 750;

function isTerminalJobStatus(status: ProxyTestJobState["status"]): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}

function formatTransferredBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = value;
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  const digits = amount >= 100 || unitIndex === 0 ? 0 : 1;
  return `${amount.toFixed(digits)} ${units[unitIndex]}`;
}

function formatJobPhase(
  phase: ProxyTestJobState["phase"],
  t: (key: string) => string,
): string {
  switch (phase) {
    case "preflight":
      return t("预检中");
    case "latency":
      return t("延迟测试中");
    case "download":
      return t("下载测试中");
    case "upload":
      return t("上传测试中");
    case "saving":
      return t("保存结果中");
    case "done":
      return t("已完成");
    case "queued":
    default:
      return t("排队中");
  }
}

function formatJobSummary(
  job: ProxyTestJobState,
  t: (key: string) => string,
): string {
  if (job.status === "cancelled") {
    return t("测试已取消");
  }
  if (job.status === "failed" && job.error) {
    return job.error;
  }
  if (job.kind === "cloudflare_style_speed") {
    if (job.phase === "download") {
      const currentMbps = job.downloadMbps != null ? ` · ↓ ${formatMetric(job.downloadMbps, "Mbps", 1)}` : "";
      return `${formatJobPhase(job.phase, t)} · ${formatTransferredBytes(job.downloadedBytes)}${currentMbps}`;
    }
    if (job.phase === "upload") {
      const currentMbps = job.uploadMbps != null ? ` · ↑ ${formatMetric(job.uploadMbps, "Mbps", 1)}` : "";
      return `${formatJobPhase(job.phase, t)} · ${formatTransferredBytes(job.uploadedBytes)}${currentMbps}`;
    }
    if (job.status === "completed" && job.cfStyleResult) {
      const res = job.cfStyleResult;
      const downloadPart = res.download ? `↓ ${formatMetric(res.download.finalMbps, "Mbps", 1)}` : "";
      const uploadPart = res.upload ? `↑ ${formatMetric(res.upload.finalMbps, "Mbps", 1)}` : "";
      const latencyPart = res.latency ? `latency: ${formatMetric(res.latency.medianMs, "ms")}` : "";
      return [downloadPart, uploadPart, latencyPart].filter(Boolean).join("  ");
    }
  }
  if (job.phase === "download") {
    return `${formatJobPhase(job.phase, t)} · ${formatTransferredBytes(job.downloadedBytes)}`;
  }
  if (job.phase === "upload") {
    return `${formatJobPhase(job.phase, t)} · ${formatTransferredBytes(job.uploadedBytes)}`;
  }
  if (job.kind === "latency" && job.latencyMs != null) {
    return `${formatJobPhase(job.phase, t)} · ${formatMetric(job.latencyMs, "ms")}`;
  }
  if (job.kind === "speed") {
    const metrics = [
      job.downloadMbps != null ? `↓ ${formatMetric(job.downloadMbps, "Mbps", 1)}` : null,
      job.uploadMbps != null ? `↑ ${formatMetric(job.uploadMbps, "Mbps", 1)}` : null,
    ].filter(Boolean);
    if (metrics.length > 0) {
      return `${formatJobPhase(job.phase, t)} · ${metrics.join("  ")}`;
    }
  }
  return formatJobPhase(job.phase, t);
}


function formatType(profile: ProxyProfile): string {
  const scheme = String(profile.scheme || "unknown").toUpperCase().replace(/H$/, "");
  return ["HTTP", "HTTPS", "SOCKS4", "SOCKS5"].includes(scheme) ? scheme : "Unsupported";
}

function formatMetric(
  value: number | null,
  suffix: string,
  digits = 0,
): string {
  if (value == null || !Number.isFinite(value)) return "—";
  return `${value.toFixed(digits)} ${suffix}`;
}

function getActiveSpeed(samples: any[] | undefined): number | null {
  if (!samples || samples.length === 0) return null;
  const maxPayload = Math.max(...samples.map((s) => s.payloadBytes || 0));
  if (maxPayload <= 0) return null;
  const currentSamples = samples.filter((s) => s.payloadBytes === maxPayload);
  return currentSamples.length > 0
    ? currentSamples.reduce((sum, s) => sum + s.mbps, 0) / currentSamples.length
    : null;
}

function formatLastTested(value: number | null, t: (key: string) => string): string {
  if (!value) return t("从不");
  return new Date(value * 1000).toLocaleString();
}

function parseTags(tagsJson?: string | null): string[] {
  if (!tagsJson) return [];
  try {
    const parsed = JSON.parse(tagsJson);
    return Array.isArray(parsed)
      ? parsed
          .map((item) => String(item || "").trim())
          .filter(Boolean)
      : [];
  } catch {
    return [];
  }
}

function isFailedStatus(status: string): boolean {
  return ["failed", "runtime_error", "invalid_url"].includes(status);
}

function formatStatusLabel(status: string, t: (key: string) => string): string {
  switch (status) {
    case "ok":
      return t("健康");
    case "checking":
      return t("检查中");
    case "failed":
      return t("失败");
    case "runtime_error":
      return t("运行错误");
    case "invalid_url":
      return t("无效URL");
    case "unchecked":
    default:
      return t("未检查");
  }
}

function statusBadgeClassName(status: string): string {
  switch (status) {
    case "ok":
      return "border-emerald-500/20 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "checking":
      return "border-sky-500/20 bg-sky-500/10 text-sky-700 dark:text-sky-300";
    case "unchecked":
      return "border-amber-500/20 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "failed":
    case "runtime_error":
    case "invalid_url":
      return "border-destructive/20 bg-destructive/10 text-destructive";
    default:
      return "border-border text-muted-foreground";
  }
}

function matchesFilter(profile: ProxyProfile, filter: ProxyFilter): boolean {
  switch (filter) {
    case "enabled":
      return profile.enabled;
    case "disabled":
      return !profile.enabled;
    case "healthy":
      return profile.status === "ok";
    case "failed":
      return isFailedStatus(profile.status);
    case "unchecked":
      return profile.status === "unchecked";
    case "all":
    default:
      return true;
  }
}

function matchesSearch(profile: ProxyProfile, search: string): boolean {
  const query = search.trim().toLowerCase();
  if (!query) return true;
  const haystack = [
    profile.name,
    profile.host,
    profile.proxyUrlRedacted,
    profile.scheme,
    profile.notes,
    ...parseTags(profile.tagsJson),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(query);
}

export function ProxySettingsCard({
  canManage,
}: {
  canManage: boolean;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ProxyFilter>("all");
  const [modalOpen, setModalOpen] = useState(false);
  const [editingProfile, setEditingProfile] = useState<ProxyProfile | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProxyProfile | null>(null);
  const [cfDownloadPreset, setCfDownloadPreset] = useState<"all" | "100kb" | "1mb" | "10mb" | "25mb">("all");
  const [cfUploadPreset, setCfUploadPreset] = useState<"all" | "100kb" | "1mb" | "10mb" | "25mb" | "50mb">("all");
  const [activeJobs, setActiveJobs] = useState<Record<string, ProxyTestJobState>>({});
  const [selectedDetailProfile, setSelectedDetailProfile] = useState<ProxyProfile | null>(null);

  const isMountedRef = useRef(true);
  const trackedJobIdsRef = useRef<Record<string, string>>({});

  const { data, isLoading, isError, error, refetch, isFetching } = useQuery({
    queryKey: PROXY_PROFILES_QUERY_KEY,
    queryFn: () => proxyProfilesClient.listProxyProfiles(),
    enabled: canManage,
  });
  const {
    data: presetsData,
    isLoading: isLoadingPresets,
    isError: isPresetsError,
    error: presetsError,
    refetch: refetchPresets,
  } = useQuery({
    queryKey: PROXY_TEST_PRESETS_QUERY_KEY,
    queryFn: () => proxyProfilesClient.listProxyTestPresets(),
    enabled: canManage,
  });

  const items = data?.items ?? [];
  const speedProviders = presetsData?.speedProviders ?? [];
  const fileSizes = presetsData?.fileSizes ?? [];

  const speedControlsDisabled =
    !canManage ||
    isLoadingPresets ||
    isPresetsError;

  const counts = useMemo(
    () => ({
      all: items.length,
      enabled: items.filter((item) => item.enabled).length,
      disabled: items.filter((item) => !item.enabled).length,
      healthy: items.filter((item) => item.status === "ok").length,
      failed: items.filter((item) => isFailedStatus(item.status)).length,
      unchecked: items.filter((item) => item.status === "unchecked").length,
    }),
    [items],
  );

  const filteredItems = useMemo(
    () =>
      items.filter(
        (item) => matchesFilter(item, filter) && matchesSearch(item, search),
      ),
    [filter, items, search],
  );

  const invalidateProfiles = async () => {
    await queryClient.invalidateQueries({
      queryKey: PROXY_PROFILES_QUERY_KEY,
    });
  };

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const setActiveJob = (profileId: string, job: ProxyTestJobState) => {
    setActiveJobs((current) => ({
      ...current,
      [profileId]: job,
    }));
  };

  const clearActiveJob = (profileId: string, jobId?: string) => {
    setActiveJobs((current) => {
      if (!(profileId in current)) return current;
      const next = { ...current };
      delete next[profileId];
      return next;
    });
    if (!jobId || trackedJobIdsRef.current[profileId] === jobId) {
      delete trackedJobIdsRef.current[profileId];
    }
  };

  const finishTrackedJob = async (profileId: string, job: ProxyTestJobState) => {
    clearActiveJob(profileId, job.jobId);
    await invalidateProfiles();
    if (!isMountedRef.current) return;
    if (job.status === "completed") {
      toast.success(job.kind === "speed" ? t("速度测试通过") : t("代理测试通过"));
      return;
    }
    if (job.status === "cancelled") {
      toast(t("测试已取消"));
      return;
    }
    if (job.error) {
      toast.warning(`${t("测试失败")}: ${job.error}`);
      return;
    }
    toast.warning(job.kind === "speed" ? t("速度测试未通过") : t("代理测试未通过"));
  };

  const pollProxyTestJob = async (profileId: string, initialJob: ProxyTestJobState) => {
    trackedJobIdsRef.current[profileId] = initialJob.jobId;
    setActiveJob(profileId, initialJob);

    let currentJob = initialJob;
    while (!isTerminalJobStatus(currentJob.status)) {
      await new Promise((resolve) => window.setTimeout(resolve, JOB_POLL_INTERVAL_MS));
      if (!isMountedRef.current) return;
      if (trackedJobIdsRef.current[profileId] !== initialJob.jobId) return;
      try {
        currentJob = await proxyProfilesClient.getProxyTestJob({
          jobId: initialJob.jobId,
        });
      } catch (pollError) {
        clearActiveJob(profileId, initialJob.jobId);
        await invalidateProfiles();
        if (isMountedRef.current) {
          toast.error(`${t("读取测试状态失败")}: ${getAppErrorMessage(pollError)}`);
        }
        return;
      }
      if (!isMountedRef.current) return;
      if (trackedJobIdsRef.current[profileId] !== initialJob.jobId) return;
      setActiveJob(profileId, currentJob);
    }

    if (trackedJobIdsRef.current[profileId] !== initialJob.jobId) return;
    await finishTrackedJob(profileId, currentJob);
  };

  const toggleMutation = useMutation({
    mutationFn: (profile: ProxyProfile) =>
      proxyProfilesClient.updateProxyProfile({
        id: profile.id,
        enabled: !profile.enabled,
      }),
    onSuccess: async (profile) => {
      await invalidateProfiles();
      toast.success(profile.enabled ? t("代理已启用") : t("代理已禁用"));
    },
    onError: (mutationError: unknown) => {
      toast.error(getAppErrorMessage(mutationError));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => proxyProfilesClient.deleteProxyProfile(id),
    onSuccess: async () => {
      await invalidateProfiles();
      toast.success(t("代理已删除"));
    },
    onError: (mutationError: unknown) => {
      toast.error(getAppErrorMessage(mutationError));
    },
  });

  const testMutation = useMutation({
    mutationFn: ({
      id,
    }: {
      id: string;
    }) =>
      proxyProfilesClient.testProxyProfileLatency({ id }),
    onSuccess: (result, variables) => {
      void pollProxyTestJob(result.proxyProfileId ?? variables.id, result);
    },
    onError: (mutationError: unknown) => {
      toast.error(`${t("测试失败")}: ${getAppErrorMessage(mutationError)}`);
    },
  });

  const runLatencyTest = (profileId: string) => {
    testMutation.mutate({
      id: profileId,
    });
  };

  const cloudflareSpeedTestMutation = useMutation({
    mutationFn: ({
      id,
      config,
    }: {
      id: string;
      config: {
        downloadPreset?: "all" | "100kb" | "1mb" | "10mb" | "25mb" | null;
        uploadPreset?: "all" | "100kb" | "1mb" | "10mb" | "25mb" | "50mb" | null;
      };
    }) =>
      proxyProfilesClient.testProxyProfileCloudflareSpeed({
        id,
        config,
      }),
    onSuccess: (result, variables) => {
      void pollProxyTestJob(result.proxyProfileId ?? variables.id, result);
    },
    onError: (mutationError: unknown) => {
      toast.error(`${t("测试失败")}: ${getAppErrorMessage(mutationError)}`);
    },
  });

  const cancelTestMutation = useMutation({
    mutationFn: ({
      jobId,
    }: {
      profileId: string;
      jobId: string;
    }) => proxyProfilesClient.cancelProxyTestJob({ jobId }),
    onSuccess: () => {
      toast(t("已请求取消测试"));
    },
    onError: (mutationError: unknown) => {
      toast.error(`${t("取消测试失败")}: ${getAppErrorMessage(mutationError)}`);
    },
  });

  const runSpeedTest = (profileId: string) => {
    cloudflareSpeedTestMutation.mutate({
      id: profileId,
      config: {
        downloadPreset: cfDownloadPreset,
        uploadPreset: cfUploadPreset,
      },
    });
  };


  const filterOptions: Array<{ id: ProxyFilter; label: string; count: number }> = [
    { id: "all", label: t("全部"), count: counts.all },
    { id: "enabled", label: t("已启用"), count: counts.enabled },
    { id: "disabled", label: t("已禁用"), count: counts.disabled },
    { id: "healthy", label: t("健康"), count: counts.healthy },
    { id: "failed", label: t("失败"), count: counts.failed },
    { id: "unchecked", label: t("未检查"), count: counts.unchecked },
  ];

  return (
    <>
      <Card className="glass-card shadow-sm">
        {/*
        <CardHeader>
          <div className="flex items-center gap-2">
            <Globe className="h-4 w-4 text-primary" />
            <CardTitle className="text-base">{t("代理设置")}</CardTitle>
          </div>
          <CardDescription>
            {t("在系统设置内管理可复用的代理配置，为账号或网关提供代理能力。")}
          </CardDescription>
        */}
          <div className="flex justify-between items-center p-6 border-b border-border/50">
          <div className="flex items-center gap-2">
              <Badge variant="outline" className="text-xs">
                {counts.all} {t("个配置")}
              </Badge>
          </div>
          <div className="flex items-center gap-2">
              <Button
                type="button"
                size="sm"
                onClick={() => {
                  setEditingProfile(null);
                  setModalOpen(true);
                }}
                disabled={!canManage}
              >
                <Plus data-icon="inline-start" />
                {t("添加代理")}
              </Button>
            </div>
          </div>
        {/*</CardHeader>*/}
        <CardContent className="flex flex-col gap-4 mt-6">
          {!canManage ? (
            <Card size="sm" className="border border-border/60 bg-muted/20">
              <CardContent className="pt-0 text-sm text-muted-foreground">
                {t("当前运行环境不支持代理配置管理。")}
              </CardContent>
            </Card>
          ) : (
            <>
              <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <div className="relative min-w-0 flex-1 lg:max-w-md">
                  <Search className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("搜索名称、端点、主机或标签")}
                    className="pl-9"
                  />
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  {filterOptions.map((option) => (
                    <Button
                      key={option.id}
                      type="button"
                      size="sm"
                      variant={filter === option.id ? "secondary" : "outline"}
                      className="min-w-[92px] justify-between"
                      onClick={() => setFilter(option.id)}
                    >
                      <span>{option.label}</span>
                      <span className="text-xs text-muted-foreground">
                        {option.count}
                      </span>
                    </Button>
                  ))}
                </div>
              </div>



              <Card size="sm" className="border border-border/60 bg-muted/20">
                <CardContent className="pt-4">
                  {isPresetsError ? (
                    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <div className="text-sm text-destructive">
                        {t("加载测试目标失败:")} {getAppErrorMessage(presetsError)}
                      </div>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => void refetchPresets()}
                      >
                        {t("重试")}
                      </Button>
                    </div>
                  ) : (
                    <div className="grid gap-4">
                      {/* Cloudflare Speed Test Presets */}
                      <div className="grid gap-4 sm:grid-cols-2">
                        {/* Download Preset */}
                        <div className="grid gap-1.5">
                          <div className="flex items-center gap-1.5">
                            <Label htmlFor="cf-download-preset" className="text-sm font-medium">
                              {t("下载预设")}
                            </Label>
                            {cfDownloadPreset === "all" && (
                              <Tooltip>
                                <TooltipTrigger render={<span className="inline-flex cursor-help text-muted-foreground/80 hover:text-foreground transition-colors" />}>
                                  <HelpCircle className="h-3.5 w-3.5" />
                                </TooltipTrigger>
                                <TooltipContent className="max-w-[320px] p-3 text-xs leading-relaxed">
                                  <p>
                                    Download determines how fast your network connection can get data from the test network. This is important when downloading large files such as updates for applications or streaming video services. Download speed is tested by downloading files of various sizes. The number reported represents the 90th percentile of download measurements and not the absolute maximum. Scroll down to view details.
                                  </p>
                                </TooltipContent>
                              </Tooltip>
                            )}
                          </div>
                          <Select
                            value={cfDownloadPreset}
                            onValueChange={(value) => setCfDownloadPreset(value as any)}
                          >
                            <SelectTrigger id="cf-download-preset" className="w-full">
                              <SelectValue placeholder={t("Select preset...")} />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectGroup>
                                <SelectItem value="all">{t("默认（所有大小）")}</SelectItem>
                                <SelectItem value="100kb">100 kB</SelectItem>
                                <SelectItem value="1mb">1 MB</SelectItem>
                                <SelectItem value="10mb">10 MB</SelectItem>
                                <SelectItem value="25mb">25 MB</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>

                        {/* Upload Preset */}
                        <div className="grid gap-1.5">
                          <div className="flex items-center gap-1.5">
                            <Label htmlFor="cf-upload-preset" className="text-sm font-medium">
                              {t("上传预设")}
                            </Label>
                            {cfUploadPreset === "all" && (
                              <Tooltip>
                                <TooltipTrigger render={<span className="inline-flex cursor-help text-muted-foreground/80 hover:text-foreground transition-colors" />}>
                                  <HelpCircle className="h-3.5 w-3.5" />
                                </TooltipTrigger>
                                <TooltipContent className="max-w-[320px] p-3 text-xs leading-relaxed">
                                  <p>
                                    Upload determines how fast your network connection can send data to the test network. This is important when uploading large files such as photos or video to social media or cloud storage. Upload speed is tested by uploading files of various sizes. The number reported represents the 90th percentile of upload measurements and not the absolute maximum. Scroll down to view details.
                                  </p>
                                </TooltipContent>
                              </Tooltip>
                            )}
                          </div>
                          <Select
                            value={cfUploadPreset}
                            onValueChange={(value) => setCfUploadPreset(value as any)}
                          >
                            <SelectTrigger id="cf-upload-preset" className="w-full">
                              <SelectValue placeholder={t("Select preset...")} />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectGroup>
                                <SelectItem value="all">{t("默认（所有大小）")}</SelectItem>
                                <SelectItem value="100kb">100 kB</SelectItem>
                                <SelectItem value="1mb">1 MB</SelectItem>
                                <SelectItem value="10mb">10 MB</SelectItem>
                                <SelectItem value="25mb">25 MB</SelectItem>
                                <SelectItem value="50mb">50 MB</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>

              {isLoading ? (
                <div className="grid gap-2">
                  {Array.from({ length: 5 }).map((_, index) => (
                    <Skeleton key={index} className="h-12 w-full rounded-xl" />
                  ))}
                </div>
              ) : isError ? (
                <Card size="sm" className="border border-destructive/20 bg-destructive/5">
                  <CardContent className="flex flex-col gap-3 pt-0 sm:flex-row sm:items-center sm:justify-between">
                    <div className="text-sm text-destructive">
                      {t("加载失败:")} {getAppErrorMessage(error)}
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => void refetch()}
                    >
                      {t("重试")}
                    </Button>
                  </CardContent>
                </Card>
              ) : filteredItems.length === 0 ? (
                <Empty className="rounded-xl border border-dashed border-border/70 bg-muted/15 py-10">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <ShieldAlert />
                    </EmptyMedia>
                    <EmptyTitle>
                      {items.length === 0 ? t("暂无代理配置") : t("没有匹配的代理")}
                    </EmptyTitle>
                    <EmptyDescription>
                      {items.length === 0
                        ? t("创建第一个代理配置，以便在系统设置中路由和跟踪端点健康状况。")
                        : t("调整当前搜索或过滤选项以查看更多代理配置。")}
                    </EmptyDescription>
                  </EmptyHeader>
                  <EmptyContent>
                    {items.length === 0 ? (
                      <Button
                        type="button"
                        onClick={() => {
                          setEditingProfile(null);
                          setModalOpen(true);
                        }}
                      >
                        <Plus data-icon="inline-start" />
                        {t("添加代理")}
                      </Button>
                    ) : null}
                  </EmptyContent>
                </Empty>
              ) : (
                <div className="overflow-hidden rounded-xl border border-border/60">
                  <Table>
                    <TableHeader className="bg-muted/30">
                      <TableRow className="border-border/60">
                        <TableHead className="min-w-[180px]">{t("名称")}</TableHead>
                        <TableHead className="min-w-[130px]">{t("状态")}</TableHead>
                        <TableHead className="min-w-[92px]">{t("类型")}</TableHead>
                        <TableHead className="min-w-[150px] max-w-[150px] truncate">{t("端点")}</TableHead>
                        <TableHead className="min-w-[180px]">{t("位置")}</TableHead>
                        <TableHead className="min-w-[120px]">{t("连通性延迟")}</TableHead>
                        <TableHead className="min-w-[160px]">{t("下载 / 上传")}</TableHead>
                        <TableHead className="min-w-[90px]">{t("账号数")}</TableHead>
                        <TableHead className="min-w-[170px]">{t("最后测试")}</TableHead>
                        <TableHead className="table-sticky-action-head min-w-[88px] text-right">
                          {t("操作")}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredItems.map((profile) => {
                        const locationParts = formatProxyGeoLocationParts(profile);
                        const tags = parseTags(profile.tagsJson);
                        const activeJob = activeJobs[profile.id];
                        const isActiveJobRunning =
                          !!activeJob && !isTerminalJobStatus(activeJob.status);
                        const isLatencyStartPending =
                          testMutation.isPending && testMutation.variables?.id === profile.id;
                        const isSpeedStartPending =
                          cloudflareSpeedTestMutation.isPending && cloudflareSpeedTestMutation.variables?.id === profile.id;
                        const isCancelPending =
                          cancelTestMutation.isPending &&
                          cancelTestMutation.variables?.profileId === profile.id;
                        const isRowBusy =
                          (toggleMutation.isPending && toggleMutation.variables?.id === profile.id) ||
                          isLatencyStartPending ||
                          isSpeedStartPending ||
                          isCancelPending ||
                          isActiveJobRunning;
                        return (
                          <TableRow key={profile.id} className="border-border/50">
                            <TableCell className="whitespace-normal">
                              <div className="flex min-w-0 flex-col gap-1">
                                <div className="flex items-center gap-2">
                                  <span className="font-medium">{profile.name}</span>
                                  {!profile.enabled ? (
                                    <Badge variant="outline" className="text-[10px]">
                                      {t("已禁用")}
                                    </Badge>
                                  ) : null}
                                </div>
                                {tags.length > 0 ? (
                                  <div className="flex flex-wrap gap-1">
                                    {tags.slice(0, 3).map((tag) => (
                                      <Badge
                                        key={tag}
                                        variant="outline"
                                        className="text-[10px]"
                                      >
                                        {tag}
                                      </Badge>
                                    ))}
                                  </div>
                                ) : null}
                                {profile.notes ? (
                                  <p className="line-clamp-2 text-xs text-muted-foreground">
                                    {profile.notes}
                                  </p>
                                ) : null}
                              </div>
                            </TableCell>
                            <TableCell>
                              <div className="flex flex-col gap-1">
                                <Badge
                                  variant="outline"
                                  className={cn("w-fit", statusBadgeClassName(profile.status))}
                                >
                                  {formatStatusLabel(profile.status, t)}
                                </Badge>
                                {profile.lastError ? (
                                  <span className="line-clamp-1 text-xs text-muted-foreground">
                                    {profile.lastError}
                                  </span>
                                ) : null}
                              </div>
                            </TableCell>
                            <TableCell>{formatType(profile)}</TableCell>
                            <TableCell className="font-mono text-xs max-w-[150px] truncate">
                              <span title={profile.proxyUrlRedacted.replace(/^[a-zA-Z0-9+-.]+:\/\//, '')}>
                                {profile.proxyUrlRedacted.replace(/^[a-zA-Z0-9+-.]+:\/\//, '')}
                              </span>
                            </TableCell>
                            <TableCell className="whitespace-normal">
                              {locationParts.length > 0 ? (
                                <div className="flex items-start gap-2">
                                  <ProxyFlag
                                    countryCode={profile.countryCode}
                                    flagEmoji={profile.flagEmoji}
                                    flagImgUrl={profile.flagImgUrl}
                                    className="shrink-0 mt-1"
                                  />
                                  <span className="text-sm leading-5">
                                    {locationParts.join(", ")}
                                  </span>
                                </div>
                              ) : (
                                <span className="text-muted-foreground">{t("未知")}</span>
                              )}
                            </TableCell>
                            <TableCell>
                              {formatMetric(profile.lastUrlLatencyMs, "ms")}
                            </TableCell>
                            <TableCell className="font-mono text-xs whitespace-nowrap">
                              {isActiveJobRunning && (activeJob?.kind === "speed" || activeJob?.kind === "cloudflare_style_speed") ? (

                                (() => {
                                  if (activeJob.phase === "download") {
                                    const dlMbps = activeJob.downloadMbps ?? getActiveSpeed(activeJob.downloadSamples);
                                    return (
                                      <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400 animate-pulse">
                                        <span>↓</span>
                                        <span>
                                          {dlMbps != null
                                            ? `${dlMbps.toFixed(1)} Mbps`
                                            : formatTransferredBytes(activeJob.downloadedBytes || 0)}
                                        </span>
                                        <span className="text-muted-foreground ml-1.5">↑</span>
                                        <span className="text-muted-foreground">—</span>
                                      </span>
                                    );
                                  }
                                  if (activeJob.phase === "upload") {
                                    const dlMbps = activeJob.downloadMbps ?? activeJob.downloadSummary?.median ?? getActiveSpeed(activeJob.downloadSamples);
                                    const ulMbps = activeJob.uploadMbps ?? getActiveSpeed(activeJob.uploadSamples);
                                    return (
                                      <span className="inline-flex items-center gap-1 text-blue-600 dark:text-blue-400 animate-pulse">
                                        <span>↓</span>
                                        <span>{dlMbps != null ? `${dlMbps.toFixed(1)} Mbps` : "—"}</span>
                                        <span className="text-muted-foreground ml-1.5">↑</span>
                                        <span>
                                          {ulMbps != null
                                            ? `${ulMbps.toFixed(1)} Mbps`
                                            : formatTransferredBytes(activeJob.uploadedBytes || 0)}
                                        </span>
                                      </span>
                                    );
                                  }
                                  return (
                                    <span className="text-muted-foreground animate-pulse text-[11px]">
                                      {formatJobPhase(activeJob.phase, t)}
                                    </span>
                                  );
                                })()
                              ) : (
                                <span className="inline-flex items-center gap-1">
                                  <span className="text-muted-foreground">↓</span>
                                  <span>{formatMetric(profile.lastDownloadMbps, "Mbps", 1)}</span>
                                  <span className="text-muted-foreground ml-1.5">↑</span>
                                  <span>{formatMetric(profile.lastUploadMbps, "Mbps", 1)}</span>
                                </span>
                              )}
                            </TableCell>
                            <TableCell>{profile.accountsCount ?? 0}</TableCell>
                            <TableCell>{formatLastTested(profile.lastTestedAt, t)}</TableCell>
                            <TableCell className="table-sticky-action-cell">
                              <div className="flex flex-col items-end gap-1">
                                <div className="flex items-center justify-end gap-2">
                                <Tooltip>
                                  <TooltipTrigger
                                    render={
                                      <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon-sm"
                                        className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                                        disabled={
                                          isActiveJobRunning ||
                                          !canManage
                                        }
                                        onClick={() => runLatencyTest(profile.id)}
                                        aria-label={t("测试代理")}
                                      />
                                    }
                                  >
                                    {isLatencyStartPending ||
                                    (activeJob?.kind === "latency" && isActiveJobRunning) ? (
                                      <Loader2 className="h-4 w-4 animate-spin text-primary" />
                                    ) : (
                                      <Activity className="h-4 w-4" />
                                    )}
                                  </TooltipTrigger>
                                  <TooltipContent>
                                    {t("测试代理")}
                                  </TooltipContent>
                                </Tooltip>
                                <Tooltip>
                                  <TooltipTrigger
                                    render={
                                      <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon-sm"
                                        className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                                        disabled={
                                          isActiveJobRunning ||
                                          speedControlsDisabled
                                        }
                                        onClick={() => runSpeedTest(profile.id)}
                                        aria-label={t("测试速度")}
                                      />
                                    }
                                  >
                                    {isSpeedStartPending ||
                                     ((activeJob?.kind === "speed" || activeJob?.kind === "cloudflare_style_speed") && isActiveJobRunning) ? (
                                       <Loader2 className="h-4 w-4 animate-spin text-primary" />
                                     ) : (
                                       <Gauge className="h-4 w-4" />
                                     )}
                                  </TooltipTrigger>
                                  <TooltipContent>
                                    {t("测试速度")}
                                  </TooltipContent>
                                </Tooltip>
                                {isActiveJobRunning ? (
                                  <Tooltip>
                                    <TooltipTrigger
                                      render={
                                        <Button
                                          type="button"
                                          variant="ghost"
                                          size="icon-sm"
                                          className="h-8 w-8 text-muted-foreground transition-colors hover:text-destructive"
                                          disabled={isCancelPending}
                                          onClick={() =>
                                            activeJob
                                              ? cancelTestMutation.mutate({
                                                  profileId: profile.id,
                                                  jobId: activeJob.jobId,
                                                })
                                              : undefined
                                          }
                                          aria-label={t("取消测试")}
                                        />
                                      }
                                    >
                                      {isCancelPending ? (
                                        <Loader2 className="h-4 w-4 animate-spin text-destructive" />
                                      ) : (
                                        <X className="h-4 w-4" />
                                      )}
                                    </TooltipTrigger>
                                    <TooltipContent>
                                      {t("取消测试")}
                                    </TooltipContent>
                                  </Tooltip>
                                ) : null}
                                <DropdownMenu>
                                  <DropdownMenuTrigger>
                                    <Button
                                      type="button"
                                      variant="ghost"
                                      size="icon-sm"
                                      aria-label={`Actions for ${profile.name}`}
                                    >
                                      {isRowBusy || isFetching ? (
                                        <Loader2 className="animate-spin" />
                                      ) : (
                                        <MoreVertical />
                                      )}
                                    </Button>
                                  </DropdownMenuTrigger>
                                  <DropdownMenuContent align="end">
                                    <DropdownMenuGroup>
                                      <DropdownMenuItem
                                        disabled={
                                          isActiveJobRunning ||
                                          !canManage
                                        }
                                        onClick={() => runLatencyTest(profile.id)}
                                      >
                                        <Activity />
                                        {t("测试代理")}
                                      </DropdownMenuItem>
                                      <DropdownMenuItem
                                        disabled={
                                          isActiveJobRunning ||
                                          speedControlsDisabled
                                        }
                                        onClick={() => runSpeedTest(profile.id)}
                                      >
                                        <Gauge />
                                        {t("测试速度")}
                                      </DropdownMenuItem>
                                      {isActiveJobRunning && activeJob ? (
                                        <DropdownMenuItem
                                          disabled={isCancelPending}
                                          onClick={() =>
                                            cancelTestMutation.mutate({
                                              profileId: profile.id,
                                              jobId: activeJob.jobId,
                                            })
                                          }
                                        >
                                          <X />
                                          {t("取消测试")}
                                        </DropdownMenuItem>
                                      ) : null}
                                      <DropdownMenuItem
                                        onClick={() => setSelectedDetailProfile(profile)}
                                      >
                                        <Activity className="h-4 w-4" />
                                        {t("详细信息")}
                                      </DropdownMenuItem>
                                      <DropdownMenuItem
                                        disabled={isActiveJobRunning}
                                        onClick={() => {
                                          setEditingProfile(profile);
                                          setModalOpen(true);
                                        }}
                                      >
                                        <PencilLine />
                                        {t("编辑")}
                                      </DropdownMenuItem>
                                      <DropdownMenuItem
                                        disabled={toggleMutation.isPending || isActiveJobRunning}
                                        onClick={() => toggleMutation.mutate(profile)}
                                      >
                                        <Globe />
                                        {profile.enabled ? t("禁用") : t("启用")}
                                      </DropdownMenuItem>
                                      <DropdownMenuItem
                                        variant="destructive"
                                        disabled={deleteMutation.isPending || isActiveJobRunning}
                                        onClick={() => setDeleteTarget(profile)}
                                      >
                                        <Trash2 />
                                        {t("删除")}
                                      </DropdownMenuItem>
                                    </DropdownMenuGroup>
                                  </DropdownMenuContent>
                                </DropdownMenu>
                                </div>
                              </div>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      <ProxyProfileModal
        open={modalOpen}
        onOpenChange={(open) => {
          setModalOpen(open);
          if (!open) {
            setEditingProfile(null);
          }
        }}
        profile={editingProfile}
      />

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteTarget(null);
          }
        }}
        title={t("删除代理配置")}
        description={
          deleteTarget
            ? t("确定要移除 {name} 吗？", { name: deleteTarget.name })
            : ""
        }
        confirmText={t("删除")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (!deleteTarget) return;
          deleteMutation.mutate(deleteTarget.id);
          setDeleteTarget(null);
        }}
      />

      <Dialog
        open={Boolean(selectedDetailProfile)}
        onOpenChange={(open) => {
          if (!open) {
            setSelectedDetailProfile(null);
          }
        }}
      >
        <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-hidden p-0 sm:max-w-[600px]">
          <DialogHeader className="px-6 pt-6">
            <DialogTitle>{t("详细信息")}</DialogTitle>
            <DialogDescription className="flex flex-wrap items-center gap-1.5 mt-1.5 text-muted-foreground break-all text-xs">
              <span className="text-sm font-medium text-foreground">
                {selectedDetailProfile?.name || "--"}
              </span>
            </DialogDescription>
          </DialogHeader>
          <div className="grid max-h-[calc(100vh-13rem)] gap-4 overflow-y-auto px-6 py-4">
            <div className="rounded-xl bg-muted/20 px-4 py-4 text-xs">
              <div className="flex flex-wrap items-start gap-x-12 gap-y-3 border-b border-border/50 pb-4 mb-4">
                <div>
                  <div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
                    {t("测试状态")}
                  </div>
                  <div
                    className={cn(
                      "mt-0.5 text-xs font-normal",
                      selectedDetailProfile ? statusBadgeClassName(selectedDetailProfile.status) : ""
                    )}
                  >
                    {selectedDetailProfile ? formatStatusLabel(selectedDetailProfile.status, t) : ""}
                    {selectedDetailProfile?.status === "ok" && selectedDetailProfile?.lastUrlLatencyMs != null && (
                      <span className="text-muted-foreground font-normal">
                        {" • "}{selectedDetailProfile.lastUrlLatencyMs} {t("ms")}
                      </span>
                    )}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] font-medium text-muted-foreground/80 uppercase tracking-wider">
                    {t("最近检查")}
                  </div>
                  <div className="mt-0.5 text-xs font-normal text-foreground">
                    {selectedDetailProfile ? formatLastTested(selectedDetailProfile.lastTestedAt, t) : ""}
                  </div>
                </div>
              </div>
              <AccountProxyGeoStatusGrid geo={selectedDetailProfile} t={t} />
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
