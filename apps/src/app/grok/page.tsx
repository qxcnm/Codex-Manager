"use client";

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FileKey2, KeyRound, Loader2, RefreshCw, ShieldCheck, Sparkles, Trash2, Upload } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { PageHeader, PageWorkspace, WorkPanel } from "@/components/layout/page-workspace";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import {
  commitGrokImport,
  deleteGrokCredential,
  listGrokCredentials,
  probeGrokCredentialModels,
  previewGrokImport,
  type GrokImportPreview,
} from "@/lib/api/grok-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";

function formatTime(seconds: number | null) {
  return seconds ? new Date(seconds * 1000).toLocaleString() : "-";
}

export default function GrokPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<GrokImportPreview | null>(null);
  const [busy, setBusy] = useState<"preview" | "commit" | string | null>(null);
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/grok/");
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const credentialsQuery = useQuery({
    queryKey: ["grok", "credentials"],
    queryFn: listGrokCredentials,
    enabled: isServiceReady && isPageActive,
  });

  usePageTransitionReady("/grok/", !isServiceReady || !credentialsQuery.isLoading);

  const credentials = credentialsQuery.data ?? [];
  const activeCount = credentials.filter((item) => item.status === "active").length;

  const runPreview = async () => {
    const text = draft.trim();
    if (!text) return;
    setBusy("preview");
    try {
      setPreview(await previewGrokImport(text));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
    }
  };

  const runCommit = async () => {
    const text = draft.trim();
    if (!text || !preview?.items.length) return;
    setBusy("commit");
    try {
      const result = await commitGrokImport(text);
      await queryClient.invalidateQueries({ queryKey: ["grok", "credentials"] });
      setDraft("");
      setPreview(null);
      if (result.failed) {
        toast.warning(`${t("导入完成")}: ${result.imported} ${t("成功")}, ${result.failed} ${t("失败")}`);
      } else {
        toast.success(`${t("已加密导入")} ${result.imported} ${t("条 Grok 凭据")}`);
      }
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
    }
  };

  const removeCredential = async (id: string) => {
    if (!window.confirm(t("确定删除该 Grok 凭据？此操作不可撤销。"))) return;
    setBusy(id);
    try {
      await deleteGrokCredential(id);
      await queryClient.invalidateQueries({ queryKey: ["grok", "credentials"] });
      toast.success(t("凭据已删除"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
    }
  };

  const probeModels = async (id: string) => {
    setBusy(id);
    try {
      const result = await probeGrokCredentialModels(id);
      await queryClient.invalidateQueries({ queryKey: ["grok", "credentials"] });
      toast.success(`${t("可用模型")}: ${result.availableModels.length}`);
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
    }
  };

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="Grok Provider"
        title={t("Grok 凭据中心")}
        description={t("独立管理 Grok 网页账号，凭据加密保存并与其他 Provider 隔离。")}
        actions={
          <Button variant="outline" className="gap-2" disabled={!isServiceReady || credentialsQuery.isFetching} onClick={() => credentialsQuery.refetch()}>
            <RefreshCw className={credentialsQuery.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
            {t("刷新")}
          </Button>
        }
      />

      {!isServiceReady ? (
        <Alert>
          <ShieldCheck className="h-4 w-4" />
          <AlertTitle>{t("服务未连接")}</AlertTitle>
          <AlertDescription>{t("连接 CodexManager 服务后才能预览和导入凭据。")}</AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-3 md:grid-cols-3">
        <WorkPanel><CardContent className="flex items-center gap-3 p-4"><KeyRound className="h-5 w-5 text-primary" /><div><p className="text-xs text-muted-foreground">{t("凭据总数")}</p><p className="text-xl font-semibold">{credentials.length}</p></div></CardContent></WorkPanel>
        <WorkPanel><CardContent className="flex items-center gap-3 p-4"><ShieldCheck className="h-5 w-5 text-emerald-500" /><div><p className="text-xs text-muted-foreground">{t("正常可用")}</p><p className="text-xl font-semibold">{activeCount}</p></div></CardContent></WorkPanel>
        <WorkPanel><CardContent className="flex items-center gap-3 p-4"><FileKey2 className="h-5 w-5 text-sky-500" /><div><p className="text-xs text-muted-foreground">{t("待导入")}</p><p className="text-xl font-semibold">{preview?.items.length ?? 0}</p></div></CardContent></WorkPanel>
      </div>

      <WorkPanel>
        <CardContent className="grid gap-4 p-4">
          <div>
            <h2 className="font-semibold">{t("Grok 文本导入")}</h2>
            <p className="text-xs text-muted-foreground">{t("每行一条：account----password----SSO。预览区只显示脱敏账号，不回显密码或 SSO。")}</p>
          </div>
          <Textarea
            value={draft}
            onChange={(event) => { setDraft(event.target.value); setPreview(null); }}
            className="min-h-36 font-mono text-xs"
            placeholder="account@example.com----password----SSO"
            autoComplete="off"
            spellCheck={false}
          />
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={!isServiceReady || !draft.trim() || busy !== null} onClick={() => void runPreview()}>
              {busy === "preview" ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <FileKey2 className="mr-2 h-4 w-4" />}
              {t("安全预览")}
            </Button>
            <Button disabled={!isServiceReady || !preview?.items.length || busy !== null} onClick={() => void runCommit()}>
              {busy === "commit" ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Upload className="mr-2 h-4 w-4" />}
              {t("确认加密导入")}
            </Button>
          </div>

          {preview ? (
            <div className="overflow-hidden rounded-lg border">
              <Table>
                <TableHeader><TableRow><TableHead>#</TableHead><TableHead>{t("脱敏账号")}</TableHead><TableHead>{t("置信度")}</TableHead><TableHead>{t("导入行为")}</TableHead></TableRow></TableHeader>
                <TableBody>
                  {preview.items.map((item) => (
                    <TableRow key={`${item.sourceIndex}-${item.accountMasked}`}>
                      <TableCell>{item.sourceIndex}</TableCell>
                      <TableCell className="font-mono">{item.accountMasked}</TableCell>
                      <TableCell>{Math.round(item.confidence * 100)}%</TableCell>
                      <TableCell><Badge variant="outline">{item.isUpdate ? t("更新") : t("新增")}</Badge></TableCell>
                    </TableRow>
                  ))}
                  {preview.items.length === 0 ? <TableRow><TableCell colSpan={4} className="h-20 text-center text-muted-foreground">{t("没有可导入的 Grok 凭据")}</TableCell></TableRow> : null}
                </TableBody>
              </Table>
              {preview.issues.length ? <div className="border-t bg-destructive/5 p-3 text-xs text-destructive">{preview.issues.map((issue) => <p key={`${issue.sourceIndex}-${issue.message}`}>{t("第")}{issue.sourceIndex}{t("行")}: {issue.message}</p>)}</div> : null}
            </div>
          ) : null}
        </CardContent>
      </WorkPanel>

      <WorkPanel>
        <CardContent className="grid gap-3 p-4 pb-3">
          <div>
            <h2 className="font-semibold">{t("Grok 已加密凭据")}</h2>
            <p className="text-xs text-muted-foreground">{t("列表只返回脱敏账号和运行状态，不返回 password 或 SSO。")}</p>
          </div>
        </CardContent>
        <CardContent className="p-0">
          <div className="overflow-x-auto"><Table>
            <TableHeader><TableRow><TableHead>{t("账号")}</TableHead><TableHead>{t("套餐")}</TableHead><TableHead>{t("可用模型")}</TableHead><TableHead>{t("额度窗口")}</TableHead><TableHead>{t("状态")}</TableHead><TableHead>{t("成功率")}</TableHead><TableHead>{t("延迟")}</TableHead><TableHead>{t("冷却至")}</TableHead><TableHead>{t("创建时间")}</TableHead><TableHead className="text-right">{t("操作")}</TableHead></TableRow></TableHeader>
            <TableBody>
              {credentialsQuery.isLoading ? <TableRow><TableCell colSpan={10} className="h-24 text-center"><Loader2 className="mx-auto h-5 w-5 animate-spin" /></TableCell></TableRow> : credentials.length === 0 ? <TableRow><TableCell colSpan={10} className="h-24 text-center text-muted-foreground">{t("暂无 Grok 凭据")}</TableCell></TableRow> : credentials.map((credential) => (
                <TableRow key={credential.id}>
                  <TableCell className="font-mono">{credential.accountMasked}</TableCell>
                  <TableCell><Badge variant="outline">{credential.webTier || "unknown"}</Badge></TableCell>
                  <TableCell className="min-w-52"><div className="flex max-w-72 flex-wrap gap-1">{credential.availableModels.length ? credential.availableModels.map((model) => <Badge key={model} variant="outline" className="font-mono text-[10px]">{model.replace("grok/", "")}</Badge>) : <span className="text-xs text-muted-foreground">-</span>}</div></TableCell>
                  <TableCell className="min-w-36 font-mono text-xs">{credential.quotaWindows.length ? credential.quotaWindows.map((window) => <div key={window.mode}>{window.mode}: {window.remainingQueries}/{window.totalQueries}</div>) : "-"}</TableCell>
                  <TableCell><Badge variant={credential.status === "active" ? "secondary" : "outline"}>{credential.status}</Badge></TableCell>
                  <TableCell>{credential.requestCount ? `${Math.round(credential.successCount / credential.requestCount * 100)}% (${credential.successCount}/${credential.requestCount})` : "-"}</TableCell>
                  <TableCell>{credential.lastLatencyMs == null ? "-" : `${credential.lastLatencyMs} ms`}</TableCell>
                  <TableCell>{formatTime(credential.cooldownUntil)}</TableCell>
                  <TableCell>{formatTime(credential.createdAt)}</TableCell>
                  <TableCell className="text-right"><div className="flex justify-end gap-1"><Button variant="ghost" size="icon-sm" disabled={busy === credential.id || credential.status !== "active"} title={t("探测可用模型")} onClick={() => void probeModels(credential.id)}>{busy === credential.id ? <Loader2 className="h-4 w-4 animate-spin" /> : <Sparkles className="h-4 w-4" />}</Button><Button variant="ghost" size="icon-sm" className="text-destructive" disabled={busy === credential.id} title={t("删除")} onClick={() => void removeCredential(credential.id)}><Trash2 className="h-4 w-4" /></Button></div></TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table></div>
        </CardContent>
      </WorkPanel>
    </PageWorkspace>
  );
}
