"use client";

import { FormEvent, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  MoreVertical,
  PencilLine,
  Plus,
  RefreshCw,
  Save,
  Settings2,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import {
  isAdminRole,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { managedModelsV2Client } from "@/lib/api/managed-models-v2";
import {
  buildManagedModelListQueryKey,
  buildModelGroupListQueryKey,
  buildModelGroupUsersQueryKey,
  normalizeQueryServiceAddress,
} from "@/lib/api/account-query-keys";
import { appClient } from "@/lib/api/app-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { AppUser, ManagedModelV2, ModelGroup, ModelGroupModel } from "@/types";

type ManageTab = "base" | "models" | "users";

type ModelDraft = {
  enabled: boolean;
  rateMultiplier: string;
  note: string;
};

function multiplierToText(value?: number | null): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "";
  return (value / 1000).toFixed(2).replace(/\.?0+$/, "");
}

function parseMultiplier(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number.parseFloat(trimmed);
  if (!Number.isFinite(parsed) || parsed < 0) return null;
  return Math.round(parsed * 1000);
}

function activeMemberUsers(users: AppUser[]): AppUser[] {
  return users.filter((user) => user.role === "member" && user.status === "active");
}

function modelDraftFromEntry(entry?: ModelGroupModel): ModelDraft {
  return {
    enabled: Boolean(entry),
    rateMultiplier: multiplierToText(entry?.rateMultiplierMillis),
    note: entry?.note || "",
  };
}

function defaultGroupModelCount(models: ManagedModelV2[]): number {
  return models.filter((model) => model.enabled && model.supportedInApi).length;
}

function groupModelCount(
  group: ModelGroup,
  models: ModelGroupModel[],
  catalogModels: ManagedModelV2[],
): number {
  if (group.isDefault) return defaultGroupModelCount(catalogModels);
  return models.filter((item) => item.groupId === group.id && item.enabled).length;
}

function groupUserCount(groupId: string, assignments: { groupId: string; status?: string }[]): number {
  return assignments.filter((item) => item.groupId === groupId && item.status !== "disabled").length;
}

function groupDraftFromGroup(group: ModelGroup | null, sort: number) {
  return {
    name: group?.name ?? "",
    description: group?.description ?? "",
    status: group?.status || "active",
    sort: String(group?.sort ?? sort),
    rateMultiplier: multiplierToText(group?.rateMultiplierMillis) || "1",
    isDefault: group?.isDefault ?? false,
  };
}

export default function ModelGroupsPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const serviceAddr = normalizeQueryServiceAddress(serviceStatus.addr);
  const groupListQueryKey = buildModelGroupListQueryKey(serviceAddr);
  const modelListQueryKey = buildManagedModelListQueryKey(serviceAddr, false);
  const groupUsersQueryKey = buildModelGroupUsersQueryKey(serviceAddr);
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const isAdminMode = isAdminRole(role);
  const isPageActive = useDesktopPageActive("/model-groups/");
  const shouldQuery = isAdminMode && serviceStatus.connected && isPageActive;
  const [groupDialogOpen, setGroupDialogOpen] = useState(false);
  const [manageTab, setManageTab] = useState<ManageTab>("base");
  const [editingGroup, setEditingGroup] = useState<ModelGroup | null>(null);
  const [groupDraft, setGroupDraft] = useState(groupDraftFromGroup(null, 0));
  const [modelDrafts, setModelDrafts] = useState<Record<string, ModelDraft>>({});
  const [selectedUserIds, setSelectedUserIds] = useState<string[]>([]);

  const groupsQuery = useQuery({
    queryKey: groupListQueryKey,
    queryFn: () => appClient.listModelGroups(serviceAddr),
    enabled: shouldQuery,
  });
  const modelsQuery = useQuery({
    queryKey: modelListQueryKey,
    queryFn: () => managedModelsV2Client.list(false, serviceAddr),
    enabled: shouldQuery,
  });
  const usersQuery = useQuery({
    queryKey: groupUsersQueryKey,
    queryFn: () => appClient.listAppUsers(serviceAddr),
    enabled: shouldQuery,
  });

  usePageTransitionReady(
    "/model-groups/",
    !shouldQuery || groupsQuery.isFetched || groupsQuery.isError,
  );

  const groups = useMemo(() => groupsQuery.data?.groups ?? [], [groupsQuery.data?.groups]);
  const groupModels = useMemo(
    () => groupsQuery.data?.models ?? [],
    [groupsQuery.data?.models],
  );
  const userAssignments = useMemo(
    () => groupsQuery.data?.userAssignments ?? [],
    [groupsQuery.data?.userAssignments],
  );
  const catalogModels = useMemo(
    () => modelsQuery.data?.items ?? [],
    [modelsQuery.data?.items],
  );
  const memberUsers = useMemo(() => activeMemberUsers(usersQuery.data ?? []), [usersQuery.data]);
  const refreshedEditingGroup = editingGroup
    ? groups.find((group) => group.id === editingGroup.id)
    : null;
  const activeGroup = refreshedEditingGroup ?? editingGroup;
  const activeGroupId = activeGroup?.id ?? "";
  const activeGroupIsDefault = activeGroup?.isDefault === true;

  useEffect(() => {
    let active = true;
    queueMicrotask(() => {
      if (!active) return;
      setGroupDialogOpen(false);
      setEditingGroup(null);
      setManageTab("base");
      setModelDrafts({});
      setSelectedUserIds([]);
    });
    return () => {
      active = false;
    };
  }, [serviceAddr]);

  useEffect(() => {
    if (!groupDialogOpen || !activeGroupId) return;
    let active = true;
    const bySlug = new Map(
      groupModels
        .filter((item) => item.groupId === activeGroupId)
        .map((item) => [item.platformModelSlug, item]),
    );
    const nextDrafts: Record<string, ModelDraft> = {};
    for (const model of catalogModels) {
      nextDrafts[model.slug] = activeGroupIsDefault
        ? {
            enabled: model.enabled && model.supportedInApi,
            rateMultiplier: "",
            note: "",
          }
        : modelDraftFromEntry(bySlug.get(model.slug));
    }
    const nextSelectedUserIds = userAssignments
      .filter((item) => item.groupId === activeGroupId && item.status === "active")
      .map((item) => item.userId);
    queueMicrotask(() => {
      if (!active) return;
      setModelDrafts(nextDrafts);
      setSelectedUserIds(nextSelectedUserIds);
    });
    return () => {
      active = false;
    };
  }, [
    activeGroupId,
    activeGroupIsDefault,
    catalogModels,
    groupDialogOpen,
    groupModels,
    userAssignments,
  ]);

  const refreshAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: groupListQueryKey }),
      queryClient.invalidateQueries({ queryKey: modelListQueryKey }),
      queryClient.invalidateQueries({ queryKey: groupUsersQueryKey }),
    ]);
  };

  const saveGroup = useMutation({
    mutationFn: async () =>
      appClient.saveModelGroup(
        {
          id: editingGroup?.id ?? null,
          name: groupDraft.name.trim(),
          description: groupDraft.description.trim() || null,
          status: groupDraft.status,
          sort: Number.parseInt(groupDraft.sort, 10) || 0,
          isDefault: groupDraft.isDefault,
          rateMultiplierMillis: parseMultiplier(groupDraft.rateMultiplier) ?? 1000,
        },
        serviceAddr,
      ),
    onSuccess: async (group) => {
      const wasCreating = !editingGroup;
      setEditingGroup(group);
      if (wasCreating) {
        setManageTab("models");
      }
      await refreshAll();
      toast.success(t("模型组已保存"));
    },
    onError: (error) => toast.error(`${t("保存失败")}: ${getAppErrorMessage(error)}`),
  });

  const deleteGroup = useMutation({
    mutationFn: (id: string) => appClient.deleteModelGroup(id, serviceAddr),
    onSuccess: async () => {
      setGroupDialogOpen(false);
      setEditingGroup(null);
      await refreshAll();
      toast.success(t("模型组已删除"));
    },
    onError: (error) => toast.error(`${t("删除失败")}: ${getAppErrorMessage(error)}`),
  });

  const saveModels = useMutation({
    mutationFn: async () => {
      if (!activeGroup) throw new Error(t("请选择模型组"));
      return appClient.setModelGroupModels(
        {
          groupId: activeGroup.id,
          models: catalogModels
            .map((model) => {
              const draft = modelDrafts[model.slug] ?? modelDraftFromEntry();
              if (!draft.enabled) return null;
              return {
                platformModelSlug: model.slug,
                enabled: true,
                rateMultiplierMillis: parseMultiplier(draft.rateMultiplier),
                note: draft.note.trim() || null,
              };
            })
            .filter(Boolean) as Array<{
            platformModelSlug: string;
            enabled: boolean;
            rateMultiplierMillis: number | null;
            note: string | null;
          }>,
        },
        serviceAddr,
      );
    },
    onSuccess: async () => {
      await refreshAll();
      toast.success(
        activeGroup?.isDefault
          ? t("默认模型组自动包含所有启用模型")
          : t("模型权限已保存"),
      );
    },
    onError: (error) => toast.error(`${t("保存失败")}: ${getAppErrorMessage(error)}`),
  });

  const saveUsers = useMutation({
    mutationFn: async () => {
      if (!activeGroup) throw new Error(t("请选择模型组"));
      return appClient.setModelGroupUsers(
        {
          groupId: activeGroup.id,
          userIds: selectedUserIds,
        },
        serviceAddr,
      );
    },
    onSuccess: async () => {
      await refreshAll();
      toast.success(t("成员分配已保存"));
    },
    onError: (error) => toast.error(`${t("保存失败")}: ${getAppErrorMessage(error)}`),
  });

  const openCreateDialog = () => {
    setEditingGroup(null);
    setManageTab("base");
    setGroupDraft(groupDraftFromGroup(null, groups.length));
    setModelDrafts({});
    setSelectedUserIds([]);
    setGroupDialogOpen(true);
  };

  const openGroupDialog = (group: ModelGroup, tab: ManageTab = "base") => {
    setEditingGroup(group);
    setManageTab(tab);
    setGroupDraft(groupDraftFromGroup(group, groups.length));
    setGroupDialogOpen(true);
  };

  const toggleUser = (userId: string, checked: boolean) => {
    setSelectedUserIds((current) =>
      checked ? Array.from(new Set(current.concat(userId))) : current.filter((id) => id !== userId),
    );
  };

  const confirmDeleteGroup = (group: ModelGroup) => {
    if (group.isDefault) return;
    if (window.confirm(t("确认删除该模型组？"))) {
      deleteGroup.mutate(group.id);
    }
  };

  if (!isAdminMode) {
    return (
      <div className="container mx-auto p-3">
        <Card className="glass-card mission-panel">
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            {t("只有管理员可以管理模型组")}
          </CardContent>
        </Card>
      </div>
    );
  }

  const isRefreshing = groupsQuery.isFetching || modelsQuery.isFetching || usersQuery.isFetching;
  const activeModelCount = activeGroup
    ? groupModelCount(activeGroup, groupModels, catalogModels)
    : 0;
  const activeModelSummary = activeGroup?.isDefault
    ? t("全部启用模型")
    : `${activeModelCount} ${t("个模型")}`;
  const activeUserCount = activeGroup ? groupUserCount(activeGroup.id, userAssignments) : 0;

  return (
    <div className="container mx-auto flex flex-col gap-3 p-3">
      <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
        <div className="flex flex-col gap-1">
          <h1 className="text-lg font-semibold">{t("模型组")}</h1>
          <p className="text-sm text-muted-foreground">
            {t("按用户分配可用平台模型，并为不同订阅层配置扣费倍率。")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            className="glass-card mission-panel h-10 gap-2 rounded-xl px-3 shadow-sm"
            disabled={isRefreshing}
            onClick={() => void refreshAll()}
          >
            <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
            {t("刷新")}
          </Button>
          <Button className="h-10 gap-2 rounded-xl px-3" onClick={openCreateDialog}>
            <Plus className="h-4 w-4" />
            {t("新建模型组")}
          </Button>
        </div>
      </div>

      <Card className="glass-card mission-panel">
        <CardHeader>
          <CardTitle>{t("模型组列表")}</CardTitle>
          <CardDescription>
            {t("在列表中查看订阅层配置，具体模型权限和成员分配通过弹窗维护。")}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-x-auto rounded-lg border border-border/60">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="min-w-[240px]">{t("模型组")}</TableHead>
                  <TableHead className="w-[130px]">{t("状态")}</TableHead>
                  <TableHead className="w-[130px]">{t("模型权限")}</TableHead>
                  <TableHead className="w-[110px]">{t("成员")}</TableHead>
                  <TableHead className="w-[110px]">{t("倍率")}</TableHead>
                  <TableHead className="w-[180px]">{t("更新时间")}</TableHead>
                  <TableHead className="w-[160px] text-right">{t("操作")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groupsQuery.isLoading ? (
                  <TableRow>
                    <TableCell colSpan={7} className="py-10 text-center text-sm text-muted-foreground">
                      {t("加载中...")}
                    </TableCell>
                  </TableRow>
                ) : groups.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7} className="py-10 text-center text-sm text-muted-foreground">
                      {t("暂无模型组")}
                    </TableCell>
                  </TableRow>
                ) : (
                  groups.map((group) => {
                    const modelCount = groupModelCount(group, groupModels, catalogModels);
                    const userCount = groupUserCount(group.id, userAssignments);
                    const multiplier = multiplierToText(group.rateMultiplierMillis) || "1";
                    return (
                      <TableRow key={group.id}>
                        <TableCell>
                          <div className="flex min-w-0 flex-col gap-1">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="font-medium">{group.name}</span>
                              {group.isDefault ? <Badge variant="secondary">{t("默认")}</Badge> : null}
                            </div>
                            <span className="line-clamp-2 text-xs text-muted-foreground">
                              {group.description || t("未填写描述")}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell>
                          <Badge variant={group.status === "active" ? "default" : "outline"}>
                            {group.status === "active" ? t("启用") : t("禁用")}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-sm">
                          {group.isDefault
                            ? t("全部启用")
                            : `${modelCount} / ${catalogModels.length || "-"}`}
                        </TableCell>
                        <TableCell className="text-sm">{userCount}</TableCell>
                        <TableCell className="font-mono text-sm">{multiplier}x</TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatTsFromSeconds(group.updatedAt, t("未知时间"))}
                        </TableCell>
                        <TableCell className="text-right">
                          <div className="flex justify-end gap-1">
                            <Button
                              variant="outline"
                              size="sm"
                              className="gap-2"
                              onClick={() => openGroupDialog(group, "base")}
                            >
                              <Settings2 className="h-4 w-4" />
                              {t("管理")}
                            </Button>
                            <DropdownMenu>
                              <DropdownMenuTrigger
                                render={
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={t("模型组操作")}
                                    title={t("模型组操作")}
                                    render={<span />}
                                    nativeButton={false}
                                  />
                                }
                                nativeButton={false}
                              >
                                <MoreVertical className="h-4 w-4" />
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end" className="w-40">
                                  <DropdownMenuGroup>
                                  <DropdownMenuItem onClick={() => openGroupDialog(group, "base")}>
                                    <PencilLine className="h-4 w-4" />
                                    {t("基础信息")}
                                  </DropdownMenuItem>
                                  <DropdownMenuItem onClick={() => openGroupDialog(group, "models")}>
                                    <Save className="h-4 w-4" />
                                    {t("模型权限")}
                                  </DropdownMenuItem>
                                  <DropdownMenuItem onClick={() => openGroupDialog(group, "users")}>
                                    <Check className="h-4 w-4" />
                                    {t("成员分配")}
                                  </DropdownMenuItem>
                                  <DropdownMenuItem
                                    variant="destructive"
                                    disabled={group.isDefault || deleteGroup.isPending}
                                    onClick={() => confirmDeleteGroup(group)}
                                  >
                                    <Trash2 className="h-4 w-4" />
                                    {t("删除")}
                                  </DropdownMenuItem>
                                </DropdownMenuGroup>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
          </div>
        </CardContent>
      </Card>

      <Dialog open={groupDialogOpen} onOpenChange={setGroupDialogOpen}>
        <DialogContent className="max-h-[calc(100vh-2rem)] overflow-hidden md:max-w-5xl">
          <DialogHeader>
            <DialogTitle>{editingGroup ? t("管理模型组") : t("新建模型组")}</DialogTitle>
            <p className="text-sm text-muted-foreground">
              {editingGroup
                ? `${activeGroup?.name ?? editingGroup.name} · ${activeModelSummary} · ${activeUserCount} ${t("个成员")}`
                : t("先保存基础信息，再继续配置模型权限和成员。")}
            </p>
          </DialogHeader>

          <Tabs
            value={manageTab}
            onValueChange={(value) => setManageTab(value as ManageTab)}
            className="min-h-0"
          >
            <TabsList className="grid w-full grid-cols-3 sm:w-[360px]">
              <TabsTrigger value="base">{t("基础信息")}</TabsTrigger>
              <TabsTrigger value="models" disabled={!activeGroup}>
                {t("模型权限")}
              </TabsTrigger>
              <TabsTrigger value="users" disabled={!activeGroup}>
                {t("成员分配")}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="base" className="min-h-0">
              <form
                className="flex max-h-[62vh] flex-col gap-4 overflow-y-auto pr-1"
                onSubmit={(event: FormEvent<HTMLFormElement>) => {
                  event.preventDefault();
                  saveGroup.mutate();
                }}
              >
                <div className="flex flex-col gap-2">
                  <Label htmlFor="model-group-name">{t("名称")}</Label>
                  <Input
                    id="model-group-name"
                    value={groupDraft.name}
                    onChange={(event) =>
                      setGroupDraft((current) => ({ ...current, name: event.target.value }))
                    }
                  />
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="model-group-description">{t("描述")}</Label>
                  <Textarea
                    id="model-group-description"
                    value={groupDraft.description}
                    onChange={(event) =>
                      setGroupDraft((current) => ({
                        ...current,
                        description: event.target.value,
                      }))
                    }
                  />
                </div>
                <div className="grid gap-3 sm:grid-cols-3">
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="model-group-rate">{t("默认倍率")}</Label>
                    <Input
                      id="model-group-rate"
                      value={groupDraft.rateMultiplier}
                      onChange={(event) =>
                        setGroupDraft((current) => ({
                          ...current,
                          rateMultiplier: event.target.value,
                        }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="model-group-sort">{t("排序")}</Label>
                    <Input
                      id="model-group-sort"
                      value={groupDraft.sort}
                      onChange={(event) =>
                        setGroupDraft((current) => ({ ...current, sort: event.target.value }))
                      }
                    />
                  </div>
                  <div className="flex flex-col gap-2">
                    <Label htmlFor="model-group-status">{t("状态")}</Label>
                    <Select
                      value={groupDraft.status}
                      onValueChange={(value) =>
                        setGroupDraft((current) => ({
                          ...current,
                          status: value || "active",
                        }))
                      }
                    >
                      <SelectTrigger id="model-group-status" className="h-10 w-full">
                        <SelectValue>
                          {groupDraft.status === "active" ? t("启用") : t("禁用")}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                    <SelectGroup>
                        <SelectItem value="active">{t("启用")}</SelectItem>
                        <SelectItem value="disabled">{t("禁用")}</SelectItem>
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={groupDraft.isDefault}
                    onCheckedChange={(checked) =>
                      setGroupDraft((current) => ({ ...current, isDefault: checked === true }))
                    }
                  />
                  {t("设为新成员默认模型组")}
                </label>
                <DialogFooter className="gap-2 sm:justify-end">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => setGroupDialogOpen(false)}
                  >
                    {t("取消")}
                  </Button>
                  <Button type="submit" disabled={saveGroup.isPending || !groupDraft.name.trim()}>
                    {editingGroup ? t("保存基础信息") : t("保存并继续")}
                  </Button>
                </DialogFooter>
              </form>
            </TabsContent>

            <TabsContent value="models" className="min-h-0">
              <div className="flex max-h-[62vh] flex-col gap-4 overflow-hidden">
                <div className="flex flex-col gap-2 rounded-lg border border-border/60 bg-background/35 p-3 sm:flex-row sm:items-center sm:justify-between">
                  <div className="text-sm text-muted-foreground">
                    {activeGroup?.isDefault
                      ? t("默认模型组自动包含模型目录中所有启用且支持 API 的模型，不保存显式模型列表。")
                      : t("启用后，该组成员才能调用对应平台模型；倍率为空时使用模型组默认倍率。")}
                  </div>
                  <Button
                    size="sm"
                    className="gap-2"
                    disabled={!activeGroup || activeGroup.isDefault || saveModels.isPending}
                    onClick={() => saveModels.mutate()}
                  >
                    <Save className="h-4 w-4" />
                    {t("保存模型")}
                  </Button>
                </div>
                <div className="min-h-0 overflow-auto rounded-lg border border-border/60">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-[72px]">{t("启用")}</TableHead>
                        <TableHead className="min-w-[220px]">{t("平台模型")}</TableHead>
                        <TableHead className="w-[140px]">{t("倍率")}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {catalogModels.length === 0 ? (
                        <TableRow>
                          <TableCell colSpan={3} className="py-10 text-center text-sm text-muted-foreground">
                            {t("暂无平台模型")}
                          </TableCell>
                        </TableRow>
                      ) : (
                        catalogModels.map((model: ManagedModelV2) => {
                          const draft = modelDrafts[model.slug] ?? modelDraftFromEntry();
                          return (
                            <TableRow key={model.slug}>
                              <TableCell>
                                <Checkbox
                                  checked={draft.enabled}
                                  disabled={activeGroup?.isDefault}
                                  onCheckedChange={(checked) =>
                                    setModelDrafts((current) => ({
                                      ...current,
                                      [model.slug]: {
                                        ...draft,
                                        enabled: checked === true,
                                      },
                                    }))
                                  }
                                />
                              </TableCell>
                              <TableCell>
                                <div className="font-mono text-sm">{model.slug}</div>
                                <div className="text-xs text-muted-foreground">
                                  {model.displayName}
                                </div>
                              </TableCell>
                              <TableCell>
                                <Input
                                  value={draft.rateMultiplier}
                                  placeholder={multiplierToText(activeGroup?.rateMultiplierMillis) || "1"}
                                  disabled={activeGroup?.isDefault}
                                  onChange={(event) =>
                                    setModelDrafts((current) => ({
                                      ...current,
                                      [model.slug]: {
                                        ...draft,
                                        rateMultiplier: event.target.value,
                                      },
                                    }))
                                  }
                                />
                              </TableCell>
                            </TableRow>
                          );
                        })
                      )}
                    </TableBody>
                  </Table>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="users" className="min-h-0">
              <div className="flex max-h-[62vh] flex-col gap-4 overflow-hidden">
                <div className="flex flex-col gap-2 rounded-lg border border-border/60 bg-background/35 p-3 sm:flex-row sm:items-center sm:justify-between">
                  <div className="text-sm text-muted-foreground">
                    {t("成员可同时持有多个模型组，最终按可用模型集合和最低有效倍率生效。")}
                  </div>
                  <Button
                    size="sm"
                    className="gap-2"
                    disabled={!activeGroup || saveUsers.isPending}
                    onClick={() => saveUsers.mutate()}
                  >
                    <Check className="h-4 w-4" />
                    {t("保存成员")}
                  </Button>
                </div>
                <div className="min-h-0 overflow-y-auto rounded-lg border border-border/60 p-3">
                  {memberUsers.length === 0 ? (
                    <div className="py-8 text-center text-sm text-muted-foreground">
                      {t("暂无可分配成员")}
                    </div>
                  ) : (
                    <div className="grid gap-2 md:grid-cols-2">
                      {memberUsers.map((user) => (
                        <label
                          key={user.id}
                          className="flex cursor-pointer items-center justify-between gap-3 rounded-lg border border-border/60 bg-background/40 px-3 py-2"
                        >
                          <div className="min-w-0">
                            <div className="truncate text-sm font-medium">
                              {user.displayName || user.username}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">
                              {user.username}
                            </div>
                          </div>
                          <Checkbox
                            checked={selectedUserIds.includes(user.id)}
                            onCheckedChange={(checked) => toggleUser(user.id, checked === true)}
                          />
                        </label>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            </TabsContent>
          </Tabs>
        </DialogContent>
      </Dialog>
    </div>
  );
}
