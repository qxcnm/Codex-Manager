"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  ExternalLink,
  Github,
  Loader2,
  PackageCheck,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Trash2,
  WandSparkles,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { WorkPanel } from "@/components/layout/page-workspace";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { appClient } from "@/lib/api/app-client";
import {
  CODEX_SKILLS_QUERY_KEY,
  CODEX_SKILLS_REGISTRY_QUERY_KEY,
  CODEX_SKILLS_REPOSITORIES_QUERY_KEY,
  codexSkillsClient,
} from "@/lib/api/codex-skills-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import type {
  CodexSkillCatalogItem,
  CodexSkillSummary,
} from "@/types";
import { SkillRepositoriesDialog } from "./skill-repositories-dialog";

type CatalogTab = "repositories" | "registry" | "installed";
type InstallFilter = "all" | "available" | "installed";

function CatalogSkillCard({
  item,
  sourceLabel,
  installing,
  uninstalling,
  disabled,
  onInstall,
  onUninstall,
  onView,
}: {
  item: CodexSkillCatalogItem;
  sourceLabel: string;
  installing: boolean;
  uninstalling: boolean;
  disabled: boolean;
  onInstall: () => void;
  onUninstall: () => void;
  onView: () => void;
}) {
  const { t } = useI18n();
  return (
    <article className="flex min-w-0 flex-col rounded-xl border border-border/60 bg-background/45 p-4 shadow-sm transition-colors hover:bg-background/65">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold" title={item.name}>
              {item.name}
            </h3>
            {item.installed ? (
              <Badge className="gap-1 border-emerald-500/25 bg-emerald-500/10 text-emerald-700">
                <Check className="size-3" />
                {t("已安装")}
              </Badge>
            ) : (
              <Badge variant="outline">{t("可安装")}</Badge>
            )}
          </div>
          <p
            className="mt-1 truncate font-mono text-[11px] text-muted-foreground"
            title={item.skillId}
          >
            {item.skillId}
          </p>
        </div>
        <WandSparkles className="size-5 shrink-0 text-primary/75" />
      </div>

      <p
        className="mt-3 line-clamp-3 min-h-[3.75rem] break-words text-xs leading-5 text-muted-foreground"
        title={item.description || undefined}
      >
        {item.description || t("暂无描述")}
      </p>

      <div className="mt-3 flex flex-wrap gap-1.5">
        <Badge variant="secondary" className="max-w-full" title={sourceLabel}>
          <span className="truncate">{sourceLabel}</span>
        </Badge>
        {item.category ? (
          <Badge variant="outline" title={item.category}>
            {item.category}
          </Badge>
        ) : null}
        {item.author ? (
          <Badge variant="outline" title={item.author}>
            {item.author}
          </Badge>
        ) : null}
        {item.installs > 0 ? (
          <Badge variant="outline">
            {t("{count} 次安装", { count: item.installs })}
          </Badge>
        ) : null}
      </div>

      <div className="mt-3 min-h-9 rounded-md border border-border/50 bg-muted/20 px-3 py-2">
        <p className="truncate font-mono text-[11px] text-muted-foreground" title={item.path}>
          {item.path || "SKILL.md"}
        </p>
      </div>

      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="gap-1.5"
          disabled={!item.sourceUrl}
          onClick={onView}
        >
          <ExternalLink className="size-3.5" />
          {t("查看")}
        </Button>
        {item.installed ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-1.5 text-destructive hover:text-destructive"
            disabled={disabled || uninstalling || !item.installedDirectoryName}
            onClick={onUninstall}
          >
            {uninstalling ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Trash2 className="size-3.5" />
            )}
            {t("卸载")}
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            className="gap-1.5"
            disabled={disabled || installing}
            onClick={onInstall}
          >
            {installing ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <PackageCheck className="size-3.5" />
            )}
            {t("安装")}
          </Button>
        )}
      </div>
    </article>
  );
}

function InstalledSkillRow({
  item,
  deleting,
  onDelete,
}: {
  item: CodexSkillSummary;
  deleting: boolean;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-3 border-b border-border/60 px-4 py-4 last:border-b-0 sm:flex-row sm:items-start sm:justify-between">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-sm font-semibold" title={item.name}>
            {item.name}
          </span>
          {item.source === "system" ? (
            <Badge variant="secondary" className="gap-1">
              <ShieldCheck className="size-3" />
              {t("系统内置 · 只读")}
            </Badge>
          ) : (
            <Badge variant="outline">{t("用户安装")}</Badge>
          )}
          {!item.valid ? (
            <Badge className="border-amber-500/25 bg-amber-500/10 text-amber-700">
              {t("配置无效")}
            </Badge>
          ) : null}
        </div>
        <p className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
          {item.directoryName}
        </p>
        <p className="mt-2 line-clamp-2 text-sm leading-6 text-muted-foreground">
          {item.description || t("暂无描述")}
        </p>
        {item.error ? (
          <p className="mt-2 text-xs text-amber-700">{item.error}</p>
        ) : null}
      </div>
      {item.deletable ? (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="gap-1.5 text-destructive hover:text-destructive"
          disabled={deleting}
          onClick={onDelete}
        >
          {deleting ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Trash2 className="size-3.5" />
          )}
          {t("卸载")}
        </Button>
      ) : (
        <span className="text-xs text-muted-foreground">{t("由 Codex 管理")}</span>
      )}
    </div>
  );
}

function CatalogLoading() {
  return (
    <div className="grid gap-3 p-4 lg:grid-cols-2 2xl:grid-cols-3">
      {Array.from({ length: 6 }).map((_, index) => (
        <div key={index} className="space-y-3 rounded-xl border p-4">
          <Skeleton className="h-4 w-44" />
          <Skeleton className="h-3 w-full" />
          <Skeleton className="h-3 w-4/5" />
          <Skeleton className="h-9 w-full" />
        </div>
      ))}
    </div>
  );
}

export function SkillsCatalogPanel({
  enabled,
  manualInstallPending,
  onInstallZip,
  onImportDirectory,
}: {
  enabled: boolean;
  manualInstallPending: boolean;
  onInstallZip: () => void;
  onImportDirectory: () => void;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [activeTab, setActiveTab] = useState<CatalogTab>("repositories");
  const [repositorySearch, setRepositorySearch] = useState("");
  const [registrySearch, setRegistrySearch] = useState("");
  const [registryQueryText, setRegistryQueryText] = useState("");
  const [repositoryFilter, setRepositoryFilter] = useState("all");
  const [installFilter, setInstallFilter] = useState<InstallFilter>("all");
  const [installedSearch, setInstalledSearch] = useState("");
  const [repositoriesDialogOpen, setRepositoriesDialogOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<{
    directoryName: string;
    name: string;
  } | null>(null);
  const [installingKey, setInstallingKey] = useState<string | null>(null);
  const initialRefreshAttemptedRef = useRef(false);

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setRegistryQueryText(registrySearch.trim());
    }, 300);
    return () => window.clearTimeout(timeoutId);
  }, [registrySearch]);

  const repositoriesQuery = useQuery({
    queryKey: CODEX_SKILLS_REPOSITORIES_QUERY_KEY,
    queryFn: () => codexSkillsClient.listRepositories(),
    enabled,
    staleTime: 15_000,
    retry: 1,
  });
  const inventoryQuery = useQuery({
    queryKey: CODEX_SKILLS_QUERY_KEY,
    queryFn: () => codexSkillsClient.list(),
    enabled,
    staleTime: 15_000,
    retry: 1,
  });
  const registryQuery = useQuery({
    queryKey: [...CODEX_SKILLS_REGISTRY_QUERY_KEY, registryQueryText],
    queryFn: () =>
      codexSkillsClient.searchRegistry({
        query: registryQueryText,
        limit: 48,
        offset: 0,
      }),
    enabled:
      enabled && activeTab === "registry" && registryQueryText.length >= 2,
    staleTime: 30_000,
    retry: 1,
  });

  const initialRefreshMutation = useMutation({
    mutationFn: () => codexSkillsClient.refreshRepository(),
    onSuccess: (catalog) => {
      queryClient.setQueryData(CODEX_SKILLS_REPOSITORIES_QUERY_KEY, catalog);
    },
    onError: (error) => {
      toast.error(`${t("初始化技能仓库失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  useEffect(() => {
    const repositories = repositoriesQuery.data?.repositories ?? [];
    if (
      !enabled ||
      initialRefreshAttemptedRef.current ||
      repositories.length === 0 ||
      !repositories.every(
        (repository) =>
          repository.skillCount === 0 && repository.lastScannedAt === null,
      )
    ) {
      return;
    }
    initialRefreshAttemptedRef.current = true;
    initialRefreshMutation.mutate();
  }, [enabled, initialRefreshMutation, repositoriesQuery.data?.repositories]);

  const repositoryInstallMutation = useMutation({
    mutationFn: codexSkillsClient.installRepositorySkill,
    onSuccess: (catalog) => {
      queryClient.setQueryData(CODEX_SKILLS_REPOSITORIES_QUERY_KEY, catalog);
      void queryClient.invalidateQueries({ queryKey: CODEX_SKILLS_QUERY_KEY });
      void queryClient.invalidateQueries({
        queryKey: CODEX_SKILLS_REGISTRY_QUERY_KEY,
      });
      toast.success(t("Skill 已安装"));
    },
    onError: (error) => {
      toast.error(`${t("安装失败")}: ${getAppErrorMessage(error)}`);
    },
    onSettled: () => setInstallingKey(null),
  });

  const registryInstallMutation = useMutation({
    mutationFn: codexSkillsClient.installRegistrySkill,
    onSuccess: (inventory) => {
      queryClient.setQueryData(CODEX_SKILLS_QUERY_KEY, inventory);
      void queryClient.invalidateQueries({
        queryKey: CODEX_SKILLS_REGISTRY_QUERY_KEY,
      });
      void queryClient.invalidateQueries({
        queryKey: CODEX_SKILLS_REPOSITORIES_QUERY_KEY,
      });
      toast.success(t("Skill 已安装"));
    },
    onError: (error) => {
      toast.error(`${t("安装失败")}: ${getAppErrorMessage(error)}`);
    },
    onSettled: () => setInstallingKey(null),
  });

  const deleteMutation = useMutation({
    mutationFn: codexSkillsClient.delete,
    onSuccess: (inventory) => {
      queryClient.setQueryData(CODEX_SKILLS_QUERY_KEY, inventory);
      void queryClient.invalidateQueries({
        queryKey: CODEX_SKILLS_REPOSITORIES_QUERY_KEY,
      });
      void queryClient.invalidateQueries({
        queryKey: CODEX_SKILLS_REGISTRY_QUERY_KEY,
      });
      setPendingDelete(null);
      toast.success(t("Skill 已卸载"));
    },
    onError: (error) => {
      toast.error(`${t("卸载失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const catalog = repositoriesQuery.data;
  const repositories = catalog?.repositories ?? [];
  const filteredRepositoryItems = useMemo(() => {
    const needle = repositorySearch.trim().toLocaleLowerCase();
    return (catalog?.items ?? []).filter((item) => {
      if (repositoryFilter !== "all" && item.repositoryId !== repositoryFilter) {
        return false;
      }
      if (installFilter === "installed" && !item.installed) return false;
      if (installFilter === "available" && item.installed) return false;
      if (!needle) return true;
      return [
        item.name,
        item.description,
        item.skillId,
        item.repositoryName,
        item.author,
        item.category,
      ].some((value) => value.toLocaleLowerCase().includes(needle));
    });
  }, [catalog?.items, installFilter, repositoryFilter, repositorySearch]);

  const filteredInstalledItems = useMemo(() => {
    const needle = installedSearch.trim().toLocaleLowerCase();
    if (!needle) return inventoryQuery.data?.items ?? [];
    return (inventoryQuery.data?.items ?? []).filter((item) =>
      [item.name, item.description, item.directoryName].some((value) =>
        value.toLocaleLowerCase().includes(needle),
      ),
    );
  }, [installedSearch, inventoryQuery.data?.items]);

  const busy =
    repositoryInstallMutation.isPending ||
    registryInstallMutation.isPending ||
    deleteMutation.isPending ||
    manualInstallPending;

  const installRepositorySkill = async (item: CodexSkillCatalogItem) => {
    const key = `repository:${item.repositoryId}:${item.skillId}`;
    setInstallingKey(key);
    try {
      await repositoryInstallMutation.mutateAsync({
        repositoryId: item.repositoryId,
        skillId: item.skillId,
      });
    } catch {
      // The mutation displays the normalized error.
    }
  };

  const installRegistrySkill = async (item: CodexSkillCatalogItem) => {
    const key = `registry:${item.sourceUrl}:${item.skillId}`;
    setInstallingKey(key);
    try {
      await registryInstallMutation.mutateAsync({
        source: item.sourceUrl,
        skillId: item.skillId,
      });
    } catch {
      // The mutation displays the normalized error.
    }
  };

  const openSource = async (item: CodexSkillCatalogItem) => {
    if (!item.sourceUrl) return;
    try {
      await appClient.openInBrowser(item.sourceUrl);
    } catch (error) {
      toast.error(`${t("打开来源失败")}: ${getAppErrorMessage(error)}`);
    }
  };

  const renderCatalogError = (error: unknown, retry?: () => void) => (
    <Empty className="min-h-64">
      <EmptyMedia variant="icon">
        <AlertTriangle />
      </EmptyMedia>
      <EmptyHeader>
        <EmptyTitle>{t("技能目录加载失败")}</EmptyTitle>
        <EmptyDescription>{getAppErrorMessage(error)}</EmptyDescription>
      </EmptyHeader>
      {retry ? (
        <Button variant="outline" onClick={retry}>
          {t("重试")}
        </Button>
      ) : null}
    </Empty>
  );

  const renderCards = (
    items: CodexSkillCatalogItem[],
    source: "repository" | "registry",
  ) => {
    if (items.length === 0) {
      return (
        <Empty className="min-h-64">
          <EmptyMedia variant="icon">
            <WandSparkles />
          </EmptyMedia>
          <EmptyHeader>
            <EmptyTitle>{t("没有匹配的 Skill")}</EmptyTitle>
            <EmptyDescription>{t("请调整搜索或筛选条件。")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      );
    }
    return (
      <div className="grid gap-3 p-4 lg:grid-cols-2 2xl:grid-cols-3">
        {items.map((item) => {
          const key =
            source === "repository"
              ? `repository:${item.repositoryId}:${item.skillId}`
              : `registry:${item.sourceUrl}:${item.skillId}`;
          return (
            <CatalogSkillCard
              key={key}
              item={item}
              sourceLabel={
                source === "registry"
                  ? "skills.sh"
                  : item.repositoryName || item.repositoryId
              }
              installing={installingKey === key}
              uninstalling={
                deleteMutation.isPending &&
                pendingDelete?.directoryName === item.installedDirectoryName
              }
              disabled={busy || !enabled}
              onInstall={() =>
                void (source === "registry"
                  ? installRegistrySkill(item)
                  : installRepositorySkill(item))
              }
              onUninstall={() => {
                if (!item.installedDirectoryName) return;
                setPendingDelete({
                  directoryName: item.installedDirectoryName,
                  name: item.name,
                });
              }}
              onView={() => void openSource(item)}
            />
          );
        })}
      </div>
    );
  };

  return (
    <div className="space-y-4" data-testid="skills-install-panel">
      <WorkPanel>
        <CardContent className="flex flex-col gap-3 px-4 py-4 lg:flex-row lg:items-center lg:justify-between">
          <div className="min-w-0">
            <p className="text-sm font-semibold">{t("安装独立 Skills")}</p>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              {t("从技能仓库或 skills.sh 发现并安装，也可以继续使用本地 ZIP 或目录。")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              className="gap-2"
              disabled={!enabled || busy}
              onClick={() => setRepositoriesDialogOpen(true)}
            >
              <Settings2 className="size-4" />
              {t("管理仓库")}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={!enabled || busy}
              onClick={onImportDirectory}
            >
              {t("导入目录")}
            </Button>
            <Button
              type="button"
              disabled={!enabled || busy}
              onClick={onInstallZip}
            >
              {t("安装 ZIP")}
            </Button>
          </div>
        </CardContent>
      </WorkPanel>

      <Tabs
        value={activeTab}
        onValueChange={(value) => setActiveTab((value || "repositories") as CatalogTab)}
        className="gap-4"
      >
        <TabsList className="glass-card mission-panel grid h-auto min-h-11 w-full grid-cols-3 rounded-lg p-1 sm:flex sm:h-11 sm:w-auto">
          <TabsTrigger value="repositories" className="min-w-0 gap-1 px-1 text-xs sm:gap-2 sm:px-4 sm:text-sm">
            <Github className="size-4" />
            {t("技能仓库")}
          </TabsTrigger>
          <TabsTrigger value="registry" className="min-w-0 gap-1 px-1 text-xs sm:gap-2 sm:px-4 sm:text-sm">
            <WandSparkles className="size-4" />
            skills.sh
          </TabsTrigger>
          <TabsTrigger value="installed" className="min-w-0 gap-1 px-1 text-xs sm:gap-2 sm:px-4 sm:text-sm">
            <PackageCheck className="size-4" />
            {t("已安装")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="repositories">
          <WorkPanel>
            <CardContent className="space-y-3 border-b border-border/60 px-4 py-3">
              <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_14rem_10rem_auto] lg:items-center">
                <div className="relative min-w-0 flex-1">
                  <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={repositorySearch}
                    onChange={(event) => setRepositorySearch(event.target.value)}
                    placeholder={t("搜索 Skill、描述或作者")}
                    aria-label={t("搜索 Skill、描述或作者")}
                    className="pl-9"
                  />
                </div>
                <Select
                  value={repositoryFilter}
                  onValueChange={(value) => setRepositoryFilter(value || "all")}
                >
                  <SelectTrigger className="w-full min-w-0">
                    <SelectValue>
                      {(value) => {
                        const selected = String(value || "all");
                        if (selected === "all") return t("全部仓库");
                        return (
                          repositories.find(
                            (repository) => repository.id === selected,
                          )?.name || selected
                        );
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="all">{t("全部仓库")}</SelectItem>
                      {repositories.map((repository) => (
                        <SelectItem key={repository.id} value={repository.id}>
                          {repository.name || repository.id}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Select
                  value={installFilter}
                  onValueChange={(value) =>
                    setInstallFilter((value || "all") as InstallFilter)
                  }
                >
                  <SelectTrigger className="w-full min-w-0">
                    <SelectValue>
                      {(value) => {
                        const selected = String(value || "all");
                        if (selected === "available") return t("可安装");
                        if (selected === "installed") return t("已安装");
                        return t("全部状态");
                      }}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="all">{t("全部状态")}</SelectItem>
                      <SelectItem value="available">{t("可安装")}</SelectItem>
                      <SelectItem value="installed">{t("已安装")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="justify-self-end"
                  title={t("刷新")}
                  aria-label={t("刷新")}
                  disabled={!enabled || repositoriesQuery.isFetching}
                  onClick={() => void repositoriesQuery.refetch()}
                >
                  <RefreshCw
                    className={cn(
                      "size-4",
                      repositoriesQuery.isFetching && "animate-spin",
                    )}
                  />
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("显示 {count} 个 Skills，来自 {repositories} 个仓库", {
                  count: filteredRepositoryItems.length,
                  repositories: repositories.length,
                })}
              </p>
              {initialRefreshMutation.isPending ? (
                <p className="flex items-center gap-2 text-xs text-primary">
                  <Loader2 className="size-3.5 animate-spin" />
                  {t("首次进入，正在后台同步技能仓库…")}
                </p>
              ) : null}
              {catalog?.warnings.map((warning) => (
                <p key={warning} className="text-xs text-amber-700">
                  {warning}
                </p>
              ))}
            </CardContent>
            {!enabled ? (
              renderCatalogError(t("请先连接 codexmanager-service。"))
            ) : repositoriesQuery.isLoading ||
              (initialRefreshMutation.isPending &&
                filteredRepositoryItems.length === 0) ? (
              <CatalogLoading />
            ) : repositoriesQuery.error ? (
              renderCatalogError(repositoriesQuery.error, () => {
                void repositoriesQuery.refetch();
              })
            ) : (
              renderCards(filteredRepositoryItems, "repository")
            )}
          </WorkPanel>
        </TabsContent>

        <TabsContent value="registry" keepMounted>
          <WorkPanel>
            <CardContent className="space-y-3 border-b border-border/60 px-4 py-3">
              <div className="relative max-w-2xl">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={registrySearch}
                  onChange={(event) => setRegistrySearch(event.target.value)}
                  placeholder={t("搜索 skills.sh")}
                  aria-label={t("搜索 skills.sh")}
                  className="pl-9"
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {t("skills.sh 提供公开 Skills 索引；安装前请核对来源。")}
              </p>
              {registryQuery.data?.warnings.map((warning) => (
                <p key={warning} className="text-xs text-amber-700">
                  {warning}
                </p>
              ))}
            </CardContent>
            {!enabled ? (
              renderCatalogError(t("请先连接 codexmanager-service。"))
            ) : registryQueryText.length < 2 ? (
              <Empty className="min-h-64">
                <EmptyMedia variant="icon">
                  <Search />
                </EmptyMedia>
                <EmptyHeader>
                  <EmptyTitle>{t("搜索 skills.sh")}</EmptyTitle>
                  <EmptyDescription>
                    {t("输入至少 2 个字符搜索公开 Skills。")}
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : registryQuery.isLoading || registryQuery.isFetching ? (
              <CatalogLoading />
            ) : registryQuery.error ? (
              renderCatalogError(registryQuery.error, () => {
                void registryQuery.refetch();
              })
            ) : (
              renderCards(registryQuery.data?.items ?? [], "registry")
            )}
          </WorkPanel>
        </TabsContent>

        <TabsContent value="installed">
          <WorkPanel>
            <CardContent className="border-b border-border/60 px-4 py-3">
              <div className="relative max-w-2xl">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={installedSearch}
                  onChange={(event) => setInstalledSearch(event.target.value)}
                  placeholder={t("搜索已安装 Skills")}
                  aria-label={t("搜索已安装 Skills")}
                  className="pl-9"
                />
              </div>
            </CardContent>
            {!enabled ? (
              renderCatalogError(t("请先连接 codexmanager-service。"))
            ) : inventoryQuery.isLoading ? (
              <CatalogLoading />
            ) : inventoryQuery.error ? (
              renderCatalogError(inventoryQuery.error, () => {
                void inventoryQuery.refetch();
              })
            ) : filteredInstalledItems.length === 0 ? (
              <Empty className="min-h-64">
                <EmptyMedia variant="icon">
                  <PackageCheck />
                </EmptyMedia>
                <EmptyHeader>
                  <EmptyTitle>{t("尚未安装 Skill")}</EmptyTitle>
                  <EmptyDescription>
                    {t("从技能仓库、skills.sh、本地 ZIP 或目录安装。")}
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <div>
                {filteredInstalledItems.map((item) => (
                  <InstalledSkillRow
                    key={`${item.source}:${item.directoryName}`}
                    item={item}
                    deleting={
                      deleteMutation.isPending &&
                      pendingDelete?.directoryName === item.directoryName
                    }
                    onDelete={() =>
                      setPendingDelete({
                        directoryName: item.directoryName,
                        name: item.name,
                      })
                    }
                  />
                ))}
              </div>
            )}
          </WorkPanel>
        </TabsContent>
      </Tabs>

      <SkillRepositoriesDialog
        open={repositoriesDialogOpen}
        onOpenChange={setRepositoriesDialogOpen}
        enabled={enabled}
      />

      <ConfirmDialog
        open={Boolean(pendingDelete)}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={t("卸载 Skill")}
        description={t("将从服务主机删除“{name}”。仓库记录仍会保留，可随时重新安装。", {
          name: pendingDelete?.name || "",
        })}
        confirmText={t("确认卸载")}
        confirmVariant="destructive"
        onConfirm={async () => {
          if (!pendingDelete) return false;
          try {
            await deleteMutation.mutateAsync({
              directoryName: pendingDelete.directoryName,
            });
            return true;
          } catch {
            return false;
          }
        }}
      />
    </div>
  );
}
