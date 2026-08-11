"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  FileJson2,
  FolderOpen,
  Gauge,
  KeyRound,
  Loader2,
  RefreshCw,
  Settings2,
  Sparkles,
  ShieldCheck,
  Power,
  Trash2,
  Upload,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";
import type { LucideIcon } from "lucide-react";
import { PageHeader, PageWorkspace, WorkPanel } from "@/components/layout/page-workspace";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import {
  commitKiroImport,
  deleteKiroCredential,
  listKiroCredentials,
  previewKiroImport,
  probeKiroCredentialModels,
  queryKiroCredentialQuota,
  refreshKiroCredential,
  setKiroCredentialEnabled,
  updateKiroCredentialRouting,
  type KiroCredentialSummary,
  type KiroImportMapping,
  type KiroImportPreview,
} from "@/lib/api/kiro-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";

type ImportSource = {
  id: string;
  name: string;
  json: string;
  preview?: KiroImportPreview;
  error?: string;
  mapping?: KiroImportMapping;
};

type RoutingDraft = {
  id: string;
  priority: string;
  weight: string;
  authRegion: string;
  apiRegion: string;
  proxyUrl: string;
  proxyUsername: string;
};

type MappingTemplate = {
  name: string;
  mapping: KiroImportMapping;
};

const EMPTY_MAPPING: KiroImportMapping = { refreshToken: "" };
const MAPPING_STORAGE_KEY = "codexmanager.kiro.import-mappings.v1";
const MAPPING_FIELDS: Array<{ key: keyof KiroImportMapping; label: string; required?: boolean }> = [
  { key: "refreshToken", label: "refreshToken", required: true },
  { key: "accessToken", label: "accessToken" },
  { key: "clientId", label: "clientId" },
  { key: "clientSecret", label: "clientSecret" },
  { key: "authMethod", label: "authMethod / provider" },
  { key: "email", label: "email" },
  { key: "region", label: "region (auth + api)" },
  { key: "authRegion", label: "authRegion" },
  { key: "apiRegion", label: "apiRegion" },
  { key: "subscription", label: "subscription" },
  { key: "expiresAt", label: "expiresAt" },
  { key: "proxyUrl", label: "proxyUrl" },
  { key: "proxyUsername", label: "proxyUsername" },
  { key: "proxyPassword", label: "proxyPassword" },
  { key: "creditLimit", label: "creditLimit" },
  { key: "creditUsed", label: "creditUsed" },
  { key: "machineId", label: "machineId" },
];

function sourceId(name: string, index: number) {
  return `${Date.now()}-${index}-${name}`;
}

async function filesToSources(files: FileList | null): Promise<ImportSource[]> {
  if (!files) return [];
  const jsonFiles = Array.from(files).filter((file) => file.name.toLowerCase().endsWith(".json"));
  return Promise.all(
    jsonFiles.map(async (file, index) => ({
      id: sourceId(file.webkitRelativePath || file.name, index),
      name: file.webkitRelativePath || file.name,
      json: await file.text(),
    })),
  );
}

function formatTime(seconds: number | null) {
  if (!seconds) return "-";
  return new Date(seconds * 1000).toLocaleString();
}

function formatSuccessRate(success: number, requests: number) {
  if (requests <= 0) return "-";
  return `${Math.round((success / requests) * 100)}%`;
}

export default function KiroPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const directoryInputRef = useRef<HTMLInputElement>(null);
  const [jsonDraft, setJsonDraft] = useState("");
  const [sources, setSources] = useState<ImportSource[]>([]);
  const [isPreviewing, setIsPreviewing] = useState(false);
  const [isCommitting, setIsCommitting] = useState(false);
  const [updatingCredentialId, setUpdatingCredentialId] = useState<string | null>(null);
  const [routingDraft, setRoutingDraft] = useState<RoutingDraft | null>(null);
  const [mappingOpen, setMappingOpen] = useState(false);
  const [mappingDraft, setMappingDraft] = useState<KiroImportMapping>(EMPTY_MAPPING);
  const [mappingTemplateName, setMappingTemplateName] = useState("");
  const [mappingTemplates, setMappingTemplates] = useState<MappingTemplate[]>([]);
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/kiro/");
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;

  useEffect(() => {
    directoryInputRef.current?.setAttribute("webkitdirectory", "");
    directoryInputRef.current?.setAttribute("directory", "");
  }, []);

  useEffect(() => {
    try {
      const parsed = JSON.parse(window.localStorage.getItem(MAPPING_STORAGE_KEY) ?? "[]");
      if (Array.isArray(parsed)) setMappingTemplates(parsed as MappingTemplate[]);
    } catch {
      setMappingTemplates([]);
    }
  }, []);

  const credentialsQuery = useQuery({
    queryKey: ["kiro", "credentials"],
    queryFn: listKiroCredentials,
    enabled: isServiceReady && isPageActive,
  });

  usePageTransitionReady("/kiro/", !isServiceReady || !credentialsQuery.isLoading);

  const autoProbeStartedRef = useRef(new Set<string>());

  useEffect(() => {
    if (!isServiceReady || !credentialsQuery.data) return;
    const pending = credentialsQuery.data.filter(
      (credential) =>
        credential.status === "active" &&
        credential.modelProbeCheckedAt == null &&
        !autoProbeStartedRef.current.has(credential.id),
    );
    if (pending.length === 0) return;
    for (const credential of pending) autoProbeStartedRef.current.add(credential.id);
    void (async () => {
      for (const credential of pending) await probeModels(credential.id, true);
    })();
    // probeModels intentionally uses the latest render; IDs are guarded by the ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [credentialsQuery.data, isServiceReady]);

  const credentials = credentialsQuery.data ?? [];
  const activeCredentials = credentials.filter((item) => item.status === "active").length;
  const previewCount = sources.reduce((sum, source) => sum + (source.preview?.items.length ?? 0), 0);
  const issueCount = sources.reduce(
    (sum, source) => sum + (source.preview?.issues.length ?? 0) + (source.error ? 1 : 0),
    0,
  );

  const previewSources = async (nextSources: ImportSource[], mapping?: KiroImportMapping) => {
    if (!isServiceReady || nextSources.length === 0) return;
    setIsPreviewing(true);
    const resolved = await Promise.all(
      nextSources.map(async (source) => {
        try {
          return {
            ...source,
            mapping,
            preview: await previewKiroImport(source.json, mapping),
            error: undefined,
          };
        } catch (error) {
          return { ...source, preview: undefined, error: getAppErrorMessage(error) };
        }
      }),
    );
    setSources(resolved);
    setIsPreviewing(false);
  };

  const selectFiles = async (files: FileList | null) => {
    const nextSources = await filesToSources(files);
    if (nextSources.length === 0) {
      toast.error(t("未找到 JSON 文件"));
      return;
    }
    setSources(nextSources);
    await previewSources(nextSources);
  };

  const previewDraft = async () => {
    const json = jsonDraft.trim();
    if (!json) return;
    const nextSources = [{ id: sourceId("pasted-content", 0), name: "pasted-content.json", json }];
    setSources(nextSources);
    await previewSources(nextSources);
  };

  const commitAll = async () => {
    const importable = sources.filter((source) => source.preview && source.preview.items.length > 0);
    if (importable.length === 0) return;
    setIsCommitting(true);
    let imported = 0;
    let failed = 0;
    for (const source of importable) {
      try {
        const result = await commitKiroImport(source.json, source.mapping);
        imported += result.imported;
        failed += result.failed;
      } catch {
        failed += source.preview?.items.length ?? 1;
      }
    }
    await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
    setIsCommitting(false);
    if (failed > 0) {
      toast.warning(`${t("导入完成")}: ${imported} ${t("成功")}, ${failed} ${t("失败")}`);
    } else {
      toast.success(`${t("已导入")} ${imported} ${t("条 Kiro 凭据")}`);
    }
  };

  const applyManualMapping = async () => {
    if (!mappingDraft.refreshToken.trim() || sources.length === 0) return;
    setMappingOpen(false);
    await previewSources(sources, mappingDraft);
  };

  const saveMappingTemplate = () => {
    const name = mappingTemplateName.trim();
    if (!name || !mappingDraft.refreshToken.trim()) return;
    const next = [
      ...mappingTemplates.filter((template) => template.name !== name),
      { name, mapping: mappingDraft },
    ];
    setMappingTemplates(next);
    window.localStorage.setItem(MAPPING_STORAGE_KEY, JSON.stringify(next));
    toast.success(t("字段映射模板已保存"));
  };

  const deleteMappingTemplate = (name: string) => {
    const next = mappingTemplates.filter((template) => template.name !== name);
    setMappingTemplates(next);
    window.localStorage.setItem(MAPPING_STORAGE_KEY, JSON.stringify(next));
  };

  const toggleCredential = async (id: string, enabled: boolean) => {
    setUpdatingCredentialId(id);
    try {
      await setKiroCredentialEnabled(id, enabled);
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      toast.success(enabled ? t("凭据已启用") : t("凭据已停用"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const removeCredential = async (id: string) => {
    if (!window.confirm(t("确定删除该 Kiro 凭据？此操作不可撤销。"))) return;
    setUpdatingCredentialId(id);
    try {
      await deleteKiroCredential(id);
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      toast.success(t("凭据已删除"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const openRoutingEditor = (credential: KiroCredentialSummary) => {
    setRoutingDraft({
      id: credential.id,
      priority: String(credential.priority),
      weight: String(credential.weight),
      authRegion: credential.authRegion ?? "",
      apiRegion: credential.apiRegion ?? "",
      proxyUrl: credential.proxyUrl ?? "",
      proxyUsername: credential.proxyUsername ?? "",
    });
  };

  const saveRouting = async () => {
    if (!routingDraft) return;
    const priority = Number(routingDraft.priority);
    const weight = Number(routingDraft.weight);
    if (!Number.isInteger(priority) || priority < 0 || priority > 10_000) {
      toast.error(t("优先级必须是 0 到 10000 的整数"));
      return;
    }
    if (!Number.isFinite(weight) || weight < 0.01 || weight > 100) {
      toast.error(t("权重必须在 0.01 到 100 之间"));
      return;
    }
    setUpdatingCredentialId(routingDraft.id);
    try {
      await updateKiroCredentialRouting({
        id: routingDraft.id,
        priority,
        weight,
        authRegion: routingDraft.authRegion.trim() || null,
        apiRegion: routingDraft.apiRegion.trim() || null,
        proxyUrl: routingDraft.proxyUrl.trim() || null,
        proxyUsername: routingDraft.proxyUsername.trim() || null,
      });
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      setRoutingDraft(null);
      toast.success(t("路由设置已更新"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const refreshCredential = async (id: string) => {
    setUpdatingCredentialId(id);
    try {
      await refreshKiroCredential(id);
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      toast.success(t("Token 刷新成功"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const probeModels = async (id: string, quiet = false) => {
    setUpdatingCredentialId(id);
    try {
      const result = await probeKiroCredentialModels(id);
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      if (!quiet) {
        toast.success(`${t("可用模型")}: ${result.availableModels.length}`);
      }
    } catch (error) {
      if (!quiet) toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const refreshQuota = async (id: string) => {
    setUpdatingCredentialId(id);
    try {
      const quota = await queryKiroCredentialQuota(id);
      await queryClient.invalidateQueries({ queryKey: ["kiro", "credentials"] });
      toast.success(`${t("剩余额度")} ${quota.remaining.toFixed(2)} / ${quota.creditLimit.toFixed(2)}`);
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setUpdatingCredentialId(null);
    }
  };

  const sourceRows = useMemo(
    () =>
      sources.flatMap((source) =>
        (source.preview?.items ?? []).map((item) => ({ source, item })),
      ),
    [sources],
  );

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="Kiro Provider"
        title={t("Kiro 凭据中心")}
        description={t("批量识别 Social / IdC JSON，加密入库后供 OpenAI 统一网关调用。")}
        actions={
          <Button
            variant="outline"
            className="gap-2"
            disabled={!isServiceReady || credentialsQuery.isFetching}
            onClick={() => credentialsQuery.refetch()}
          >
            <RefreshCw className={credentialsQuery.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
            {t("刷新")}
          </Button>
        }
      />

      {!isServiceReady ? (
        <Alert>
          <XCircle className="h-4 w-4" />
          <AlertTitle>{t("服务未连接")}</AlertTitle>
          <AlertDescription>{t("连接 CodexManager 服务后才能预览和导入凭据。")}</AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-3 md:grid-cols-4">
        {([
          { label: t("凭据总数"), value: credentials.length, icon: KeyRound },
          { label: t("正常可用"), value: activeCredentials, icon: ShieldCheck },
          { label: t("待导入"), value: previewCount, icon: FileJson2 },
          { label: t("识别问题"), value: issueCount, icon: XCircle },
        ] satisfies Array<{ label: string; value: number; icon: LucideIcon }>).map(({ label, value, icon: Icon }) => (
          <WorkPanel key={label}>
            <CardContent className="flex items-center justify-between p-4">
              <div><p className="text-xs text-muted-foreground">{label}</p><p className="mt-1 font-mono text-2xl font-semibold">{value}</p></div>
              <Icon className="h-5 w-5 text-primary" />
            </CardContent>
          </WorkPanel>
        ))}
      </div>

      <WorkPanel>
        <CardContent className="space-y-4 p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div><h2 className="font-semibold">{t("JSON 导入中心")}</h2><p className="text-xs text-muted-foreground">{t("支持单对象、数组、多文件和目录递归；单条坏数据不会阻断其他记录。")}</p></div>
            <div className="flex flex-wrap gap-2">
              <label className={cn(buttonVariants({ variant: "outline" }), "cursor-pointer gap-2", !isServiceReady && "pointer-events-none opacity-50")}>
                <Upload className="h-4 w-4" />{t("选择多个 JSON")}<Input className="hidden" type="file" accept="application/json,.json" multiple disabled={!isServiceReady} onChange={(event) => void selectFiles(event.target.files)} />
              </label>
              <label className={cn(buttonVariants({ variant: "outline" }), "cursor-pointer gap-2", !isServiceReady && "pointer-events-none opacity-50")}>
                <FolderOpen className="h-4 w-4" />{t("选择目录")}<Input ref={directoryInputRef} className="hidden" type="file" multiple disabled={!isServiceReady} onChange={(event) => void selectFiles(event.target.files)} />
              </label>
            </div>
          </div>
          <Textarea value={jsonDraft} onChange={(event) => setJsonDraft(event.target.value)} className="min-h-36 font-mono text-xs" placeholder='{"refreshToken":"...","clientId":"...","clientSecret":"..."}' />
          <div className="flex items-center gap-2">
            <Button variant="outline" disabled={!isServiceReady || !jsonDraft.trim() || isPreviewing} onClick={() => void previewDraft()}>{isPreviewing ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}{t("预览粘贴内容")}</Button>
            <Button variant="outline" disabled={!isServiceReady || sources.length === 0 || isPreviewing} onClick={() => setMappingOpen(true)}><Settings2 className="mr-2 h-4 w-4" />{t("手动字段映射")}</Button>
            <Button disabled={!isServiceReady || previewCount === 0 || isCommitting} onClick={() => void commitAll()}>{isCommitting ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <CheckCircle2 className="mr-2 h-4 w-4" />}{t("确认加密导入")}</Button>
            {sources.length > 0 ? <span className="text-xs text-muted-foreground">{sources.length} {t("个来源")} · {previewCount} {t("条凭据")}</span> : null}
          </div>

          {sources.some((source) => source.error || source.preview?.issues.length) ? (
            <div className="space-y-1 rounded-md border border-amber-500/30 bg-amber-500/5 p-3 text-xs">
              {sources.flatMap((source) => [
                ...(source.error ? [`${source.name}: ${source.error}`] : []),
                ...(source.preview?.issues.map((issue) => `${source.name} #${issue.sourceIndex + 1}: ${issue.message}`) ?? []),
              ]).map((message, index) => <p key={`${index}-${message}`} className="text-amber-700 dark:text-amber-300">{message}</p>)}
            </div>
          ) : null}

          {sourceRows.length > 0 ? (
            <div className="max-h-72 overflow-auto rounded-md border">
              <Table><TableHeader><TableRow><TableHead>{t("来源")}</TableHead><TableHead>{t("认证")}</TableHead><TableHead>{t("账号")}</TableHead><TableHead>{t("区域")}</TableHead><TableHead>{t("置信度")}</TableHead><TableHead>{t("字段映射")}</TableHead></TableRow></TableHeader>
                <TableBody>{sourceRows.map(({ source, item }) => <TableRow key={`${source.id}-${item.sourceIndex}`}><TableCell className="max-w-52 truncate text-xs" title={source.name}>{source.name}</TableCell><TableCell><div className="flex gap-1"><Badge variant="outline">{item.authMethod}</Badge>{item.isUpdate ? <Badge variant="secondary">{t("更新已有")}</Badge> : <Badge variant="outline">{t("新增")}</Badge>}</div></TableCell><TableCell>{item.email ?? "-"}</TableCell><TableCell>{item.region ?? "-"}</TableCell><TableCell>{Math.round(item.confidence * 100)}%</TableCell><TableCell className="max-w-72 truncate font-mono text-xs" title={`${item.duplicateHint}: ${item.mappedFields.join(", ")}`}>{item.mappedFields.join(", ")}</TableCell></TableRow>)}</TableBody>
              </Table>
            </div>
          ) : null}
        </CardContent>
      </WorkPanel>

      <Dialog open={mappingOpen} onOpenChange={setMappingOpen}>
        <DialogContent className="glass-card max-h-[90vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("手动字段映射")}</DialogTitle>
            <DialogDescription>{t("为未识别 JSON 填写源字段路径。支持点号嵌套路径，例如 auth.refreshToken。映射只作用于当前待导入来源。")}</DialogDescription>
          </DialogHeader>
          {mappingTemplates.length > 0 ? (
            <div className="space-y-2">
              <Label>{t("已保存模板")}</Label>
              <div className="flex flex-wrap gap-2">
                {mappingTemplates.map((template) => (
                  <div key={template.name} className="flex items-center rounded-md border">
                    <Button variant="ghost" size="sm" onClick={() => { setMappingDraft(template.mapping); setMappingTemplateName(template.name); }}>{template.name}</Button>
                    <Button variant="ghost" size="icon-sm" title={t("删除模板")} onClick={() => deleteMappingTemplate(template.name)}><Trash2 className="h-3.5 w-3.5" /></Button>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
          <div className="grid gap-3 py-2 sm:grid-cols-2">
            {MAPPING_FIELDS.map((field) => (
              <div key={field.key} className="grid gap-1.5">
                <Label>{field.label}{field.required ? " *" : ""}</Label>
                <Input
                  className="font-mono text-xs"
                  value={mappingDraft[field.key] ?? ""}
                  placeholder={field.required ? "tokens.refresh" : "profile.value"}
                  onChange={(event) => setMappingDraft({ ...mappingDraft, [field.key]: event.target.value })}
                />
              </div>
            ))}
          </div>
          <div className="grid gap-2 sm:grid-cols-[1fr_auto]">
            <Input value={mappingTemplateName} placeholder={t("模板名称")} onChange={(event) => setMappingTemplateName(event.target.value)} />
            <Button variant="outline" disabled={!mappingTemplateName.trim() || !mappingDraft.refreshToken.trim()} onClick={saveMappingTemplate}>{t("保存模板")}</Button>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setMappingOpen(false)}>{t("取消")}</Button>
            <Button disabled={!mappingDraft.refreshToken.trim() || isPreviewing} onClick={() => void applyManualMapping()}>{t("应用映射并预览")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <WorkPanel>
        <CardContent className="p-0">
          <div className="border-b px-4 py-3"><h2 className="font-semibold">{t("已加密凭据")}</h2><p className="text-xs text-muted-foreground">{t("列表仅显示非敏感元数据，Token 和 client secret 不会返回前端。")}</p></div>
          <div className="overflow-auto"><Table><TableHeader><TableRow><TableHead>{t("账号")}</TableHead><TableHead>{t("认证")}</TableHead><TableHead>{t("区域")}</TableHead><TableHead>{t("订阅")}</TableHead><TableHead>{t("可用模型")}</TableHead><TableHead>{t("额度")}</TableHead><TableHead>{t("状态")}</TableHead><TableHead>{t("成功率")}</TableHead><TableHead>{t("延迟")}</TableHead><TableHead>{t("冷却至")}</TableHead><TableHead>{t("权重")}</TableHead><TableHead>{t("失败")}</TableHead><TableHead>{t("过期时间")}</TableHead><TableHead className="text-right">{t("操作")}</TableHead></TableRow></TableHeader>
            <TableBody>{credentialsQuery.isLoading ? <TableRow><TableCell colSpan={14} className="h-24 text-center"><Loader2 className="mx-auto h-5 w-5 animate-spin" /></TableCell></TableRow> : credentials.length === 0 ? <TableRow><TableCell colSpan={14} className="h-24 text-center text-muted-foreground">{t("暂无 Kiro 凭据")}</TableCell></TableRow> : credentials.map((credential) => <TableRow key={credential.id}><TableCell>{credential.email ?? credential.id.slice(0, 8)}</TableCell><TableCell><Badge variant="outline">{credential.authMethod}</Badge></TableCell><TableCell>{credential.apiRegion ?? credential.authRegion ?? "-"}</TableCell><TableCell>{credential.subscription ?? "-"}</TableCell><TableCell className="min-w-52"><div className="flex max-w-72 flex-wrap gap-1">{credential.availableModels.length === 0 ? <span className="text-xs text-muted-foreground">-</span> : credential.availableModels.map((model) => <Badge key={model} variant="outline" className="font-mono text-[10px]">{model.replace("kiro/", "")}</Badge>)}</div></TableCell><TableCell className="font-mono text-xs">{credential.creditLimit == null ? "-" : `${Math.max(0, credential.creditLimit - (credential.creditUsed ?? 0)).toFixed(2)} / ${credential.creditLimit.toFixed(2)}`}</TableCell><TableCell><Badge variant={credential.status === "active" ? "secondary" : "outline"}>{credential.status}</Badge></TableCell><TableCell className="font-mono text-xs">{formatSuccessRate(credential.successCount, credential.requestCount)}<span className="ml-1 text-muted-foreground">({credential.successCount}/{credential.requestCount})</span></TableCell><TableCell className="font-mono text-xs">{credential.lastLatencyMs == null ? "-" : `${credential.lastLatencyMs} ms`}</TableCell><TableCell className="text-xs">{formatTime(credential.cooldownUntil)}</TableCell><TableCell>{credential.weight}</TableCell><TableCell>{credential.failureCount}</TableCell><TableCell className="text-xs">{formatTime(credential.expiresAt)}</TableCell><TableCell><div className="flex justify-end gap-1"><Button variant="ghost" size="icon-sm" disabled={updatingCredentialId === credential.id || credential.status !== "active"} title={t("探测可用模型")} onClick={() => void probeModels(credential.id)}><Sparkles className={updatingCredentialId === credential.id ? "h-4 w-4 animate-pulse" : "h-4 w-4"} /></Button><Button variant="ghost" size="icon-sm" disabled={updatingCredentialId === credential.id} title={t("查询额度")} onClick={() => void refreshQuota(credential.id)}><Gauge className="h-4 w-4" /></Button><Button variant="ghost" size="icon-sm" disabled={updatingCredentialId === credential.id} title={t("刷新 Token")} onClick={() => void refreshCredential(credential.id)}><RefreshCw className={updatingCredentialId === credential.id ? "h-4 w-4 animate-spin" : "h-4 w-4"} /></Button><Button variant="ghost" size="icon-sm" disabled={updatingCredentialId === credential.id} title={t("路由设置")} onClick={() => openRoutingEditor(credential)}><Settings2 className="h-4 w-4" /></Button><Button variant="ghost" size="icon-sm" disabled={updatingCredentialId === credential.id} title={credential.status === "active" ? t("停用") : t("启用")} onClick={() => void toggleCredential(credential.id, credential.status !== "active")}><Power className="h-4 w-4" /></Button><Button variant="ghost" size="icon-sm" className="text-destructive" disabled={updatingCredentialId === credential.id} title={t("删除")} onClick={() => void removeCredential(credential.id)}><Trash2 className="h-4 w-4" /></Button></div></TableCell></TableRow>)}</TableBody>
          </Table></div>
        </CardContent>
      </WorkPanel>

      <Dialog open={routingDraft !== null} onOpenChange={(open) => !open && setRoutingDraft(null)}>
        <DialogContent className="glass-card sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("Kiro 路由设置")}</DialogTitle>
            <DialogDescription>{t("设置凭据优先级、权重、区域和独立代理。代理 URL 中的密码会自动拆出并加密保存。")}</DialogDescription>
          </DialogHeader>
          {routingDraft ? <div className="grid gap-4 py-2">
            <section className="grid gap-3 rounded-lg border border-border/60 bg-background/35 p-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("路由设置")}</h3>
              <div className="grid grid-cols-2 gap-3"><div className="grid gap-2"><Label>{t("优先级")}</Label><Input type="number" min={0} max={10000} value={routingDraft.priority} onChange={(event) => setRoutingDraft({ ...routingDraft, priority: event.target.value })} /></div><div className="grid gap-2"><Label>{t("权重")}</Label><Input type="number" min={0.01} max={100} step={0.01} value={routingDraft.weight} onChange={(event) => setRoutingDraft({ ...routingDraft, weight: event.target.value })} /></div></div>
            </section>
            <section className="grid gap-3 rounded-lg border border-border/60 bg-background/35 p-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("区域")}</h3>
              <div className="grid grid-cols-2 gap-3"><div className="grid gap-2"><Label>{t("认证区域")}</Label><Input value={routingDraft.authRegion} placeholder="us-east-1" onChange={(event) => setRoutingDraft({ ...routingDraft, authRegion: event.target.value })} /></div><div className="grid gap-2"><Label>{t("API 区域")}</Label><Input value={routingDraft.apiRegion} placeholder="us-east-1" onChange={(event) => setRoutingDraft({ ...routingDraft, apiRegion: event.target.value })} /></div></div>
            </section>
            <section className="grid gap-3 rounded-lg border border-border/60 bg-background/35 p-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("代理")}</h3>
              <div className="grid gap-2"><Label>{t("代理 URL")}</Label><Input value={routingDraft.proxyUrl} placeholder={t("代理地址示例：http://user:password@127.0.0.1:7897 或 direct")} onChange={(event) => setRoutingDraft({ ...routingDraft, proxyUrl: event.target.value })} /></div>
              <div className="grid gap-2"><Label>{t("代理用户名")}</Label><Input value={routingDraft.proxyUsername} onChange={(event) => setRoutingDraft({ ...routingDraft, proxyUsername: event.target.value })} /></div>
            </section>
          </div> : null}
          <DialogFooter><Button variant="outline" onClick={() => setRoutingDraft(null)}>{t("取消")}</Button><Button disabled={!routingDraft || updatingCredentialId === routingDraft.id} onClick={() => void saveRouting()}>{updatingCredentialId === routingDraft?.id ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}{t("保存")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </PageWorkspace>
  );
}
