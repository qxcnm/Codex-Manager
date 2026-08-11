"use client";

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, FileKey2, Loader2, ShieldCheck, Upload, WandSparkles, XCircle } from "lucide-react";
import { toast } from "sonner";
import { PageHeader, PageWorkspace, WorkPanel } from "@/components/layout/page-workspace";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { accountClient } from "@/lib/api/account-client";
import {
  maskAggregateKey,
  parseAggregateKeyImport,
  readAggregateAutoProbe,
} from "@/lib/aggregate-key-import";
import { probeAdapterPool, readAdapterAutoProbe, type AdapterProbePoolId } from "@/lib/adapter-probe";
import { commitGrokImport, listGrokCredentials, previewGrokImport } from "@/lib/api/grok-client";
import { commitKiroImport, listKiroCredentials, previewKiroImport } from "@/lib/api/kiro-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";

type UnifiedImportKind = "codex" | "kiro" | "grok" | "aggregate" | "unknown";

type UnifiedImportSource = {
  id: string;
  name: string;
  content: string;
  kind: UnifiedImportKind;
  confidence: number;
  count: number;
  summary: string;
  issues: string[];
};

const kindLabel: Record<UnifiedImportKind, string> = {
  codex: "GPT / Codex",
  kiro: "Kiro",
  grok: "Grok",
  aggregate: "中转站 KEY",
  unknown: "未识别",
};

function sourceId(name: string, index: number) {
  return `${Date.now()}-${index}-${name}`;
}

function looksLikeJson(value: string): boolean {
  const text = value.trim();
  return (text.startsWith("{") && text.endsWith("}")) || (text.startsWith("[") && text.endsWith("]"));
}

function parseJsonSafely(value: string): unknown | null {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return null;
  }
}

function objectContainsKey(value: unknown, keys: Set<string>, depth = 0): boolean {
  if (depth > 8 || value == null || typeof value !== "object") return false;
  if (Array.isArray(value)) return value.some((item) => objectContainsKey(item, keys, depth + 1));
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    if (keys.has(key)) return true;
    if (objectContainsKey(child, keys, depth + 1)) return true;
  }
  return false;
}

function looksLikeCodexJson(value: string): boolean {
  const parsed = parseJsonSafely(value);
  if (!parsed) return false;
  return objectContainsKey(parsed, new Set([
    "accessToken",
    "access_token",
    "refreshToken",
    "refresh_token",
    "idToken",
    "id_token",
    "chatgptAccountId",
    "accountId",
  ]));
}

const KIRO_STRONG_KEYS = new Set([
  "clientId",
  "client_id",
  "clientSecret",
  "client_secret",
  "authMethod",
  "auth_method",
  "authRegion",
  "auth_region",
  "apiRegion",
  "api_region",
  "machineId",
  "machine_id",
]);

const CODEX_STRONG_KEYS = new Set([
  "accessToken",
  "access_token",
  "idToken",
  "id_token",
  "chatgptAccountId",
  "chatgpt_account_id",
  "agent_runtime_id",
  "agentRuntimeId",
  "agent_private_key",
  "agentPrivateKey",
  "agent_identity",
  "agentIdentity",
]);

function looksLikeAgentIdentityJson(value: string): boolean {
  const parsed = parseJsonSafely(value);
  if (!parsed) return false;
  return objectContainsKey(
    parsed,
    new Set(["agent_runtime_id", "agentRuntimeId", "agent_private_key", "agentPrivateKey"]),
  );
}

function looksLikeGrokText(value: string): boolean {
  return value
    .split(/\r?\n/)
    .some((line) => line.trim().split("----").length >= 3);
}

function detectJsonHint(value: string): "kiro" | "codex" | "ambiguous" | "unknown" {
  const parsed = parseJsonSafely(value);
  if (!parsed) return "unknown";
  if (objectContainsKey(parsed, KIRO_STRONG_KEYS)) return "kiro";
  if (objectContainsKey(parsed, CODEX_STRONG_KEYS)) return "codex";
  if (looksLikeCodexJson(value)) return "ambiguous";
  return "unknown";
}

async function mapWithConcurrency<T, R>(
  items: readonly T[],
  concurrency: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(items.length);
  let cursor = 0;
  const workerCount = Math.min(Math.max(1, concurrency), items.length);
  await Promise.all(
    Array.from({ length: workerCount }, async () => {
      while (cursor < items.length) {
        const index = cursor;
        cursor += 1;
        results[index] = await worker(items[index], index);
      }
    }),
  );
  return results;
}

function combineKiroJsonSources(sources: UnifiedImportSource[]): string | null {
  const parsed = sources.map((source) => parseJsonSafely(source.content));
  if (parsed.some((item) => item == null)) return null;
  return JSON.stringify(parsed);
}

async function fileListToSources(files: FileList | null): Promise<Array<{ name: string; content: string }>> {
  if (!files) return [];
  const selected = Array.from(files).filter((file) => /\.(json|txt)$/i.test(file.name) || file.type === "application/json" || file.type.startsWith("text/"));
  return Promise.all(selected.map(async (file) => ({ name: file.name, content: await file.text() })));
}

async function detectOne(input: { name: string; content: string }, index: number): Promise<UnifiedImportSource> {
  const content = input.content.trim();
  const base: UnifiedImportSource = {
    id: sourceId(input.name, index),
    name: input.name,
    content: input.content,
    kind: "unknown",
    confidence: 0,
    count: 0,
    summary: "",
    issues: [],
  };
  if (!content) return { ...base, summary: "空内容", issues: ["空内容"] };

  const aggregateKeys = parseAggregateKeyImport(content);
  if (aggregateKeys.length > 0) {
    return {
      ...base,
      kind: "aggregate",
      confidence: 0.98,
      count: aggregateKeys.length,
      summary: aggregateKeys
        .map((item) => `${item.supplierName} / ${maskAggregateKey(item.key)}`)
        .join(" / "),
      issues: [],
    };
  }

  if (looksLikeGrokText(content)) {
    try {
      const grok = await previewGrokImport(content);
      if (grok.items.length > 0) {
        return {
          ...base,
          kind: "grok",
          confidence: Math.max(...grok.items.map((item) => item.confidence || 0.9)),
          count: grok.items.length,
          summary: grok.items.map((item) => item.accountMasked).join(" / "),
          issues: grok.issues.map((item) => item.message),
        };
      }
    } catch {
      return { ...base, summary: "Grok 文本解析失败", issues: ["Grok 文本解析失败"] };
    }
  }

  if (looksLikeJson(content)) {
    const hint = detectJsonHint(content);
    if (hint === "codex") {
      return {
        ...base,
        kind: "codex",
        confidence: 0.96,
        count: 1,
        summary: looksLikeAgentIdentityJson(content)
          ? "Codex Agent Identity JSON"
          : "Codex Token JSON（将后台自动生成 Agent Identity）",
        issues: [],
      };
    }

    if (hint === "kiro" || hint === "ambiguous") {
      try {
        const kiro = await previewKiroImport(content);
        if (kiro.items.length > 0) {
          return {
            ...base,
            kind: "kiro",
            confidence: Math.max(...kiro.items.map((item) => item.confidence || 0.88)),
            count: kiro.items.length,
            summary: kiro.items.map((item) => item.email || item.authMethod).join(" / "),
            issues: kiro.issues.map((item) => item.message),
          };
        }
      } catch {
        // Ambiguous refresh-token-only JSON may still be a Codex credential.
      }
    }

    if (hint === "ambiguous" || looksLikeCodexJson(content)) {
      return {
        ...base,
        kind: "codex",
        confidence: hint === "ambiguous" ? 0.76 : 0.9,
        count: 1,
        summary: "Codex Token JSON",
        issues: [],
      };
    }
  }

  return { ...base, summary: "未能识别平台", issues: ["未能识别平台"] };
}

function kindTone(kind: UnifiedImportKind): string {
  switch (kind) {
    case "codex":
      return "border-blue-500/30 bg-blue-500/10 text-blue-600";
    case "kiro":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-600";
    case "grok":
      return "border-sky-500/30 bg-sky-500/10 text-sky-600";
    case "aggregate":
      return "border-amber-500/30 bg-amber-500/10 text-amber-600";
    default:
      return "border-muted-foreground/25 bg-muted/40 text-muted-foreground";
  }
}

export default function UnifiedImportPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState("");
  const [sources, setSources] = useState<UnifiedImportSource[]>([]);
  const [busy, setBusy] = useState<"preview" | "commit" | null>(null);
  const [progressText, setProgressText] = useState("");
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/import/");
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;

  usePageTransitionReady("/import/", isPageActive);

  const importable = sources.filter((source) => source.kind !== "unknown" && source.count > 0);
  const stats = useMemo(() => ({
    codex: sources.filter((source) => source.kind === "codex").reduce((sum, source) => sum + source.count, 0),
    kiro: sources.filter((source) => source.kind === "kiro").reduce((sum, source) => sum + source.count, 0),
    grok: sources.filter((source) => source.kind === "grok").reduce((sum, source) => sum + source.count, 0),
    aggregate: sources.filter((source) => source.kind === "aggregate").reduce((sum, source) => sum + source.count, 0),
    unknown: sources.filter((source) => source.kind === "unknown").length,
  }), [sources]);

  const runDetect = async (items: Array<{ name: string; content: string }>) => {
    if (items.length === 0) return;
    setBusy("preview");
    try {
      let completed = 0;
      setProgressText(`${t("正在识别")} 0/${items.length}`);
      const detected = await mapWithConcurrency(items, 6, async (item, index) => {
        const result = await detectOne(item, index);
        completed += 1;
        setProgressText(`${t("正在识别")} ${completed}/${items.length}`);
        return result;
      });
      setSources(detected);
      const ok = detected.filter((item) => item.kind !== "unknown").length;
      if (ok > 0) toast.success(`${t("已识别")} ${ok} ${t("个来源")}`);
      else toast.warning(t("没有识别到可导入凭据"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
      setProgressText("");
    }
  };

  const previewDraft = async () => {
    const text = draft.trim();
    if (!text) return;
    await runDetect([{ name: "pasted-content", content: text }]);
  };

  const selectFiles = async (files: FileList | null) => {
    const next = await fileListToSources(files);
    if (next.length === 0) {
      toast.warning(t("未找到可导入文件"));
      return;
    }
    setDraft("");
    await runDetect(next);
  };

  const commitAll = async () => {
    if (importable.length === 0) return;
    setBusy("commit");
    setProgressText(t("正在写入凭据"));
    try {
      let imported = 0;
      let failed = 0;
      const autoProbePools = new Set<AdapterProbePoolId>(
        (["codex", "kiro", "grok"] as const).filter(
          (poolId) =>
            importable.some((source) => source.kind === poolId) &&
            readAdapterAutoProbe(poolId),
        ),
      );
      const [kiroBefore, grokBefore] = await Promise.all([
        autoProbePools.has("kiro") ? listKiroCredentials() : Promise.resolve([]),
        autoProbePools.has("grok") ? listGrokCredentials() : Promise.resolve([]),
      ]);
      const importedCodexIds: string[] = [];
      const codexSources = importable.filter((source) => source.kind === "codex");
      const kiroSources = importable.filter((source) => source.kind === "kiro");
      const grokSources = importable.filter((source) => source.kind === "grok");
      const aggregateSources = importable.filter((source) => source.kind === "aggregate");
      const autoProbeAggregate = readAggregateAutoProbe();
      const combinedKiroJson = combineKiroJsonSources(kiroSources);

      const [codexResult, kiroResults, grokResult, aggregateResults] = await Promise.all([
        codexSources.length > 0
          ? accountClient.import(codexSources.map((source) => source.content))
          : Promise.resolve(null),
        kiroSources.length === 0
          ? Promise.resolve([])
          : combinedKiroJson
            ? commitKiroImport(combinedKiroJson).then((result) => [result])
            : mapWithConcurrency(kiroSources, 3, (source) => commitKiroImport(source.content)),
        grokSources.length > 0
          ? commitGrokImport(grokSources.map((source) => source.content).join("\n"))
          : Promise.resolve(null),
        mapWithConcurrency(
          aggregateSources.flatMap((source) => parseAggregateKeyImport(source.content)),
          3,
          async (credential) => {
            let createdId: string | null = null;
            try {
              const created = await accountClient.createAggregateApi({
                providerType: "codex",
                supplierName: credential.supplierName,
                url: credential.url,
                key: credential.key,
                authType: "apikey",
                authCustomEnabled: false,
                actionCustomEnabled: false,
                balanceQueryEnabled: false,
              });
              createdId = created.id;
              if (!autoProbeAggregate) {
                await accountClient.updateAggregateApi(created.id, {
                  supplierName: credential.supplierName,
                  status: "disabled",
                });
                return { created: true, probeOk: null };
              }
              const probe = await accountClient.testAggregateApiConnection(created.id);
              if (!probe.ok) {
                await accountClient.updateAggregateApi(created.id, {
                  supplierName: credential.supplierName,
                  status: "disabled",
                });
              } else {
                try {
                  await accountClient.syncManagedModelSourceModels({
                    sourceKind: "aggregate_api",
                    sourceId: created.id,
                  });
                } catch {
                  // 渠道真实连通已经成立；模型目录稍后可在中转站池重新同步。
                }
              }
              return { created: true, probeOk: probe.ok };
            } catch {
              if (createdId) {
                try {
                  await accountClient.updateAggregateApi(createdId, {
                    supplierName: credential.supplierName,
                    status: "disabled",
                  });
                } catch {
                  // 创建已经成功；即使停用写回失败，也按探测失败向用户报告。
                }
                return { created: true, probeOk: false };
              }
              return { created: false, probeOk: false };
            }
          },
        ),
      ]);

      if (codexResult) {
        imported += (codexResult.created || 0) + (codexResult.updated || 0);
        failed += codexResult.failed || 0;
        importedCodexIds.push(...(codexResult.importedAccountIds || []));
        if (importedCodexIds.length > 0) {
          toast.info(
            `${importedCodexIds.length} ${t("个 Codex 账号已进入 Agent Identity 自动生成队列")}`,
          );
        }
      }
      for (const result of kiroResults) {
        imported += result.imported;
        failed += result.failed;
      }
      if (grokResult) {
        imported += grokResult.imported;
        failed += grokResult.failed;
      }
      imported += aggregateResults.filter((item) => item.created).length;
      const aggregateProbeFailed = aggregateResults.filter((item) => item.created && item.probeOk === false).length;
      const aggregateImportFailed = aggregateResults.filter((item) => !item.created).length;
      failed += aggregateProbeFailed;
      failed += aggregateImportFailed;

      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["resource-pools"] }),
        queryClient.invalidateQueries({ queryKey: ["accounts"] }),
        queryClient.invalidateQueries({ queryKey: ["kiro"] }),
        queryClient.invalidateQueries({ queryKey: ["grok"] }),
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
        queryClient.invalidateQueries({ queryKey: ["managed-model-catalog"] }),
      ]);

      const probeJobs: Array<Promise<{ poolId: AdapterProbePoolId; succeeded: number; failed: number }>> = [];
      if (autoProbePools.has("codex") && importedCodexIds.length > 0) {
        probeJobs.push(
          probeAdapterPool("codex", importedCodexIds).then((result) => ({ poolId: "codex", ...result })),
        );
      }
      if (autoProbePools.has("kiro")) {
        const previousIds = new Set(kiroBefore.map((item) => item.id));
        const current = await listKiroCredentials();
        const targetIds = current
          .filter(
            (item) =>
              item.status === "active" &&
              (!previousIds.has(item.id) || item.modelProbeCheckedAt == null),
          )
          .map((item) => item.id);
        if (targetIds.length > 0) {
          probeJobs.push(
            probeAdapterPool("kiro", targetIds).then((result) => ({ poolId: "kiro", ...result })),
          );
        }
      }
      if (autoProbePools.has("grok")) {
        const previousIds = new Set(grokBefore.map((item) => item.id));
        const current = await listGrokCredentials();
        const targetIds = current
          .filter(
            (item) =>
              item.status === "active" &&
              (!previousIds.has(item.id) || item.availableModels.length === 0),
          )
          .map((item) => item.id);
        if (targetIds.length > 0) {
          probeJobs.push(
            probeAdapterPool("grok", targetIds).then((result) => ({ poolId: "grok", ...result })),
          );
        }
      }
      if (probeJobs.length > 0) {
        toast.info(t("已开始自动探测新导入凭据"));
        void Promise.allSettled(probeJobs).then(async (results) => {
          const completed = results
            .filter((item): item is PromiseFulfilledResult<{ poolId: AdapterProbePoolId; succeeded: number; failed: number }> => item.status === "fulfilled")
            .map((item) => item.value);
          const succeeded = completed.reduce((sum, item) => sum + item.succeeded, 0);
          const probeFailed = completed.reduce((sum, item) => sum + item.failed, 0) + results.filter((item) => item.status === "rejected").length;
          await queryClient.invalidateQueries({ queryKey: ["resource-pools"] });
          if (probeFailed > 0) {
            toast.warning(`${t("自动探测完成")}: ${succeeded} ${t("成功")}, ${probeFailed} ${t("失败")}`);
          } else {
            toast.success(`${t("自动探测完成")}: ${succeeded} ${t("条凭据")}`);
          }
        });
      }
      setSources([]);
      setDraft("");
      if (failed > 0) toast.warning(`${t("导入完成")}: ${imported} ${t("成功")}, ${failed} ${t("失败")}`);
      else toast.success(`${t("已加密导入")} ${imported} ${t("条凭据")}`);
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setBusy(null);
      setProgressText("");
    }
  };

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="Unified Import"
        title={t("统一导入凭据")}
        description={t("一个入口自动识别 Codex Token JSON、Kiro JSON、Grok 文本卡密和中转站 KEY。")}
        actions={
          <Button disabled={!isServiceReady || importable.length === 0 || busy !== null} onClick={() => void commitAll()}>
            {busy === "commit" ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <CheckCircle2 className="mr-2 h-4 w-4" />}
            {t("确认导入全部")}
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

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
        {([
          ["GPT / Codex", stats.codex, "codex"],
          ["Kiro", stats.kiro, "kiro"],
          ["Grok", stats.grok, "grok"],
          [t("中转站 KEY"), stats.aggregate, "aggregate"],
          [t("未识别"), stats.unknown, "unknown"],
        ] as const).map(([label, value, kind]) => (
          <WorkPanel key={label}>
            <CardContent className="flex items-center justify-between gap-3 p-4">
              <div>
                <p className="text-xs text-muted-foreground">{label}</p>
                <p className="mt-1 text-2xl font-semibold">{value}</p>
              </div>
              <Badge variant="outline" className={cn("h-7", kindTone(kind))}>{kindLabel[kind]}</Badge>
            </CardContent>
          </WorkPanel>
        ))}
      </div>

      <WorkPanel>
        <CardContent className="grid gap-4 p-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="font-semibold">{t("导入入口")}</h2>
              <p className="text-xs text-muted-foreground">{t("可以选择多个 JSON/TXT 文件，也可以直接粘贴内容；系统会自动分流到对应资源池。")}</p>
            </div>
            <label className={cn("inline-flex h-9 cursor-pointer items-center justify-center rounded-md border border-border bg-background px-4 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground", (!isServiceReady || busy !== null) && "pointer-events-none opacity-50")}>
              {busy === "preview" ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Upload className="mr-2 h-4 w-4" />}
              {t("选择文件")}
              <Input className="hidden" type="file" multiple accept=".json,.txt,application/json,text/plain" disabled={!isServiceReady || busy !== null} onChange={(event) => void selectFiles(event.target.files)} />
            </label>
          </div>
          <Textarea
            value={draft}
            onChange={(event) => { setDraft(event.target.value); setSources([]); }}
            className="min-h-44 font-mono text-xs"
            placeholder={t("粘贴 Codex / Kiro JSON、Grok account----password----SSO，或中转站 URL + sk-KEY")}
            autoComplete="off"
            spellCheck={false}
          />
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={!isServiceReady || !draft.trim() || busy !== null} onClick={() => void previewDraft()}>
              {busy === "preview" ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <WandSparkles className="mr-2 h-4 w-4" />}
              {t("自动识别")}
            </Button>
            <Button disabled={!isServiceReady || importable.length === 0 || busy !== null} onClick={() => void commitAll()}>
              <FileKey2 className="mr-2 h-4 w-4" />{t("确认导入全部")}
            </Button>
            {progressText ? <span className="self-center text-xs text-muted-foreground">{progressText}</span> : null}
          </div>
        </CardContent>
      </WorkPanel>

      <WorkPanel>
        <CardContent className="border-b px-4 py-3">
          <h2 className="font-semibold">{t("识别结果")}</h2>
          <p className="text-xs text-muted-foreground">{t("只展示平台、数量、置信度和脱敏摘要；不会回显 Token、密码或 SSO。")}</p>
        </CardContent>
        <CardContent className="p-0">
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("来源")}</TableHead>
                  <TableHead>{t("识别平台")}</TableHead>
                  <TableHead>{t("数量")}</TableHead>
                  <TableHead>{t("置信度")}</TableHead>
                  <TableHead>{t("摘要")}</TableHead>
                  <TableHead>{t("状态")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {sources.length === 0 ? (
                  <TableRow><TableCell colSpan={6} className="h-24 text-center text-muted-foreground">{t("等待选择文件或粘贴内容")}</TableCell></TableRow>
                ) : sources.map((source) => (
                  <TableRow key={source.id}>
                    <TableCell className="max-w-64 truncate font-mono text-xs" title={source.name}>{source.name}</TableCell>
                    <TableCell><Badge variant="outline" className={kindTone(source.kind)}>{kindLabel[source.kind]}</Badge></TableCell>
                    <TableCell>{source.count}</TableCell>
                    <TableCell>{Math.round(source.confidence * 100)}%</TableCell>
                    <TableCell className="max-w-80 truncate text-xs" title={source.summary}>{source.summary || "-"}</TableCell>
                    <TableCell>
                      {source.kind === "unknown" ? <span className="inline-flex items-center gap-1 text-xs text-destructive"><XCircle className="h-3 w-3" />{t("不可导入")}</span> : <span className="inline-flex items-center gap-1 text-xs text-emerald-600"><CheckCircle2 className="h-3 w-3" />{t("可导入")}</span>}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </CardContent>
      </WorkPanel>
    </PageWorkspace>
  );
}
