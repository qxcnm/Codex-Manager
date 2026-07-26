"use client";

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  FolderKanban,
  FolderPlus,
  History,
  Loader2,
  Play,
  RefreshCw,
  SquareTerminal,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import {
  PageHeader,
  PageWorkspace,
  WorkPanel,
} from "@/components/layout/page-workspace";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
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
import { Skeleton } from "@/components/ui/skeleton";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import {
  CODEX_PROJECTS_QUERY_KEY,
  codexProjectsClient,
} from "@/lib/api/codex-projects-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import type {
  CodexProjectLaunchAction,
  CodexProjectSummary,
} from "@/types";

function ProjectRow({
  project,
  launchingAction,
  disabled,
  onLaunch,
  onRemove,
}: {
  project: CodexProjectSummary;
  launchingAction: CodexProjectLaunchAction | null;
  disabled: boolean;
  onLaunch: (action: CodexProjectLaunchAction) => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();

  return (
    <div className="grid grid-cols-1 gap-4 border-b border-border/60 px-4 py-4 last:border-b-0 lg:grid-cols-3 lg:items-center">
      <div className="flex min-w-0 flex-1 items-start gap-3 lg:col-span-2">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-primary/20 bg-primary/10 text-primary">
          <FolderKanban className="size-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="truncate text-sm font-semibold" title={project.name}>
              {project.name}
            </h2>
            {project.available ? (
              <Badge variant="outline">{t("目录可用")}</Badge>
            ) : (
              <Badge className="border-amber-500/25 bg-amber-500/10 text-amber-700">
                {t("目录不可用")}
              </Badge>
            )}
          </div>
          <p
            className="mt-1 break-all font-mono text-xs leading-5 text-muted-foreground"
            title={project.path}
          >
            {project.path}
          </p>
          {!project.available ? (
            <p className="mt-1 text-xs text-amber-700">
              {t("目录可能已移动或删除；你仍可以安全移除这条记录。")}
            </p>
          ) : null}
        </div>
      </div>

      <div className="grid w-full grid-cols-1 gap-2 sm:grid-cols-3 lg:col-span-1">
        <Button
          type="button"
          size="sm"
          className="gap-1.5"
          disabled={disabled || !project.available}
          onClick={() => onLaunch("start")}
        >
          {launchingAction === "start" ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Play className="size-3.5" />
          )}
          {t("启动")}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="gap-1.5"
          disabled={disabled || !project.available}
          onClick={() => onLaunch("resume")}
        >
          {launchingAction === "resume" ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <History className="size-3.5" />
          )}
          {t("会话")}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="gap-1.5 text-destructive hover:text-destructive"
          disabled={disabled}
          onClick={onRemove}
        >
          <Trash2 className="size-3.5" />
          {t("移除")}
        </Button>
      </div>
    </div>
  );
}

export default function ProjectsPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/projects/");
  const isReady = useDeferredDesktopActivation(
    isDesktopRuntime && isPageActive,
  );
  const [pendingRemove, setPendingRemove] =
    useState<CodexProjectSummary | null>(null);

  const projectsQuery = useQuery({
    queryKey: CODEX_PROJECTS_QUERY_KEY,
    queryFn: codexProjectsClient.list,
    enabled: isReady,
    staleTime: 10_000,
    retry: 1,
  });
  usePageTransitionReady(
    "/projects/",
    !isDesktopRuntime || !projectsQuery.isLoading,
  );

  const refreshProjects = async () => {
    await queryClient.invalidateQueries({ queryKey: CODEX_PROJECTS_QUERY_KEY });
  };

  const addMutation = useMutation({
    mutationFn: codexProjectsClient.add,
    onSuccess: async (result) => {
      if (result.canceled) return;
      await refreshProjects();
      if (result.added) {
        toast.success(t("项目目录已添加"));
      } else {
        toast.info(t("该目录已在项目列表中"));
      }
    },
    onError: (error) => {
      toast.error(`${t("添加目录失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const removeMutation = useMutation({
    mutationFn: codexProjectsClient.remove,
    onSuccess: async (result) => {
      await refreshProjects();
      setPendingRemove(null);
      if (result.removed) {
        toast.success(t("项目记录已移除"));
      } else {
        toast.info(t("项目记录已不存在"));
      }
    },
    onError: (error) => {
      toast.error(`${t("移除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const launchMutation = useMutation({
    mutationFn: codexProjectsClient.launch,
    onSuccess: (result) => {
      toast.success(
        result.target === "app"
          ? t("已在 ChatGPT Codex App 中打开项目")
          : result.action === "resume"
          ? t("已请求打开 Codex 会话选择器")
          : t("已请求在新终端中启动 Codex"),
      );
    },
    onError: (error) => {
      toast.error(`${t("启动 Codex 失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const anyMutationPending =
    addMutation.isPending ||
    removeMutation.isPending ||
    launchMutation.isPending;
  const projects = projectsQuery.data?.items ?? [];

  const launchProject = (
    project: CodexProjectSummary,
    action: CodexProjectLaunchAction,
  ) => {
    if (anyMutationPending || !project.available) return;
    launchMutation.mutate({ path: project.path, action });
  };

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="DESKTOP"
        title={t("项目启动")}
        description={t(
          "收藏常用目录，在 Windows 与 macOS 上直接用 ChatGPT Codex App 打开项目。",
        )}
        meta={
          <Badge variant="outline">
            {t("{count} 个项目", { count: projects.length })}
          </Badge>
        }
        actions={
          <>
            <Button
              type="button"
              className="w-full gap-2 sm:w-auto"
              disabled={!isReady || anyMutationPending}
              onClick={() => addMutation.mutate()}
            >
              {addMutation.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <FolderPlus className="size-4" />
              )}
              {t("添加目录")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              disabled={!isReady || projectsQuery.isFetching}
              title={t("刷新")}
              aria-label={t("刷新")}
              onClick={() => void projectsQuery.refetch()}
            >
              <RefreshCw
                className={cn(
                  "size-4",
                  projectsQuery.isFetching && "animate-spin",
                )}
              />
            </Button>
          </>
        }
      />

      <WorkPanel>
        <CardContent className="flex items-start gap-3 px-4 py-4">
          <SquareTerminal className="mt-0.5 size-4 shrink-0 text-primary" />
          <div className="space-y-1 text-xs leading-5 text-muted-foreground">
            <p className="font-medium text-foreground">{t("Codex App 与本机 CLI")}</p>
            <p>
              {t(
                "Windows 与 macOS 的“启动”会在 ChatGPT Codex App 中打开项目；“会话”继续使用本机 Codex CLI 会话选择器。",
              )}
            </p>
            <p>
              {t(
                "远程服务上的 Codex profile 不会复制到本机，也不会作为本机启动路径使用。",
              )}
            </p>
            <p>
              {t(
                "“会话”会打开 Codex CLI 自带的当前项目会话选择器，不会由 CodexManager 读取或修改会话文件。",
              )}
            </p>
          </div>
        </CardContent>
      </WorkPanel>

      <WorkPanel>
        {!isDesktopRuntime ? (
          <Empty className="min-h-72">
            <EmptyMedia variant="icon">
              <AlertTriangle />
            </EmptyMedia>
            <EmptyHeader>
              <EmptyTitle>{t("项目启动仅支持桌面端")}</EmptyTitle>
              <EmptyDescription>
                {t("Web / Docker 无法安全打开你设备上的目录和交互式终端。")}
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : projectsQuery.isLoading ? (
          <div className="space-y-3 p-4">
            {Array.from({ length: 3 }).map((_, index) => (
              <div key={index} className="flex items-center gap-3 rounded-md border p-4">
                <Skeleton className="size-10 rounded-lg" />
                <div className="min-w-0 flex-1 space-y-2">
                  <Skeleton className="h-4 w-40 max-w-full" />
                  <Skeleton className="h-3 w-96 max-w-full" />
                </div>
              </div>
            ))}
          </div>
        ) : projectsQuery.error ? (
          <Empty className="min-h-72">
            <EmptyMedia variant="icon">
              <AlertTriangle />
            </EmptyMedia>
            <EmptyHeader>
              <EmptyTitle>{t("项目列表加载失败")}</EmptyTitle>
              <EmptyDescription>
                {getAppErrorMessage(projectsQuery.error)}
              </EmptyDescription>
            </EmptyHeader>
            <Button variant="outline" onClick={() => void projectsQuery.refetch()}>
              {t("重试")}
            </Button>
          </Empty>
        ) : projects.length === 0 ? (
          <Empty className="min-h-72">
            <EmptyMedia variant="icon">
              <FolderKanban />
            </EmptyMedia>
            <EmptyHeader>
              <EmptyTitle>{t("还没有项目目录")}</EmptyTitle>
              <EmptyDescription>
                {t("添加一个本机目录，即可从这里启动 Codex 或继续该项目的会话。")}
              </EmptyDescription>
            </EmptyHeader>
            <Button
              type="button"
              className="gap-2"
              disabled={anyMutationPending}
              onClick={() => addMutation.mutate()}
            >
              {addMutation.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <FolderPlus className="size-4" />
              )}
              {t("添加目录")}
            </Button>
          </Empty>
        ) : (
          <div>
            {projects.map((project) => {
              const isLaunching =
                launchMutation.isPending &&
                launchMutation.variables?.path === project.path;
              return (
                <ProjectRow
                  key={project.path}
                  project={project}
                  launchingAction={
                    isLaunching ? launchMutation.variables.action : null
                  }
                  disabled={anyMutationPending}
                  onLaunch={(action) => launchProject(project, action)}
                  onRemove={() => setPendingRemove(project)}
                />
              );
            })}
          </div>
        )}
      </WorkPanel>

      <ConfirmDialog
        open={Boolean(pendingRemove)}
        onOpenChange={(open) => {
          if (!open && !removeMutation.isPending) setPendingRemove(null);
        }}
        title={t("移除项目记录")}
        description={t(
          "只会从 CodexManager 中移除“{name}”，不会删除项目目录或其中的文件。",
          { name: pendingRemove?.name || "" },
        )}
        confirmText={t("确认移除")}
        confirmVariant="destructive"
        onConfirm={async () => {
          if (!pendingRemove) return;
          try {
            await removeMutation.mutateAsync(pendingRemove.path);
          } catch {
            return false;
          }
        }}
      />
    </PageWorkspace>
  );
}
