'use client';

import { AlertTriangle, TerminalSquare } from "lucide-react";
import { useI18n } from "@/lib/i18n/provider";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import {
  AdvancedRecoveryPanel,
  CurrentModeCard,
  DirectAccountCard,
  GatewayModeCard,
} from "./page-sections";
import {
  modeImpact,
  usePlatformModePageState,
} from "./use-platform-mode-state";
import type {
  CodexProfileAccountCandidate,
  CodexProfileApiKeyCandidate,
} from "@/types";

function formatTime(ts: number | null): string {
  if (!ts) return "-";
  return new Date(ts * 1000).toLocaleString();
}

function formatBytes(bytes: number | null | undefined): string {
  const value = typeof bytes === "number" && Number.isFinite(bytes) ? bytes : 0;
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[index]}`;
}

function keyLabel(key: CodexProfileApiKeyCandidate): string {
  return key.name || key.modelSlug || key.id;
}

function accountLabel(account: CodexProfileAccountCandidate): string {
  return account.groupName ? `${account.label} · ${account.groupName}` : account.label;
}

export default function PlatformModePage() {
  const { t } = useI18n();
  const state = usePlatformModePageState(t);

  return (
    <main className="flex w-full flex-col gap-5 px-4 py-4 md:px-6">
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary">
            <TerminalSquare className="size-5" />
          </div>
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">{t("平台模式选择")}</h1>
            <p className="text-sm text-muted-foreground">
              {t("选择 Codex CLI 直连账号，或通过 CodexManager 本地网关接入。")}
            </p>
          </div>
        </div>
      </div>

      <Alert className="border-amber-500/30 bg-amber-500/10">
        <AlertTriangle className="size-4" />
        <AlertTitle>{t("写入位置说明")}</AlertTitle>
        <AlertDescription>
          {t("这里修改的是 codexmanager-service 所在机器的 Codex 配置目录，不一定是当前浏览器所在机器。")}
        </AlertDescription>
      </Alert>

      {state.mode === "web-gateway" ? (
        <Alert className="border-sky-500/30 bg-sky-500/10">
          <AlertTriangle className="size-4" />
          <AlertTitle>{t("Web / Docker 模式")}</AlertTitle>
          <AlertDescription>
            {t("当前页面会通过 /api/rpc 写入 codexmanager-service 进程可访问的 Codex profile；Docker 部署时请确认 CODEX_HOME 或挂载卷指向你希望 Codex CLI 使用的配置目录。")}
          </AlertDescription>
        </Alert>
      ) : null}

      {!state.isServiceReady ? (
        <Alert variant="destructive">
          <AlertTriangle className="size-4" />
          <AlertTitle>{t("服务未连接")}</AlertTitle>
          <AlertDescription>
            {t("当前运行环境无法访问管理 RPC，暂时不能读取或写入 Codex profile。")}
          </AlertDescription>
        </Alert>
      ) : null}

      {state.status?.warnings.length ? (
        <Alert className="border-amber-500/30 bg-amber-500/10">
          <AlertTriangle className="size-4" />
          <AlertTitle>{t("Profile 迁移警告")}</AlertTitle>
          <AlertDescription>{state.status.warnings[0]}</AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-5 lg:grid-cols-2 xl:grid-cols-[minmax(320px,0.9fr)_minmax(0,1.05fr)_minmax(0,1.05fr)]">
        <CurrentModeCard
          t={t}
          status={state.status}
          isGatewayActive={state.isGatewayActive}
          statusFetching={state.statusQuery.isFetching}
          candidatesFetching={state.candidatesQuery.isFetching}
          onRefresh={() => void state.refreshAll()}
          codexHome={state.status?.codexHome || "-"}
          activeAccountValue={state.activeAccountValue}
          activeKeyValue={state.activeKeyValue}
          lastAppliedAtLabel={formatTime(state.status?.lastAppliedAt ?? null)}
          modeDescription={modeImpact(state.status?.mode ?? null, t)}
        />

        <DirectAccountCard
          t={t}
          candidates={state.candidates.accounts}
          isLoading={state.candidatesQuery.isLoading}
          isServiceReady={state.isServiceReady}
          isMutating={state.isMutating}
          isDirectActive={state.isDirectActive}
          selectedAccountId={state.selectedAccountId}
          onSelectAccount={(value) => state.setSelectedAccountIdDraft(String(value || ""))}
          onApply={() => state.applyDirectMutation.mutate()}
          isPending={state.applyDirectMutation.isPending}
          accountLabel={accountLabel}
        />

        <GatewayModeCard
          t={t}
          candidates={state.candidates.apiKeys}
          isLoading={state.candidatesQuery.isLoading}
          isServiceReady={state.isServiceReady}
          isMutating={state.isMutating}
          isGatewayActive={state.isGatewayActive}
          selectedApiKeyId={state.selectedApiKeyId}
          onSelectApiKey={(value) => state.setSelectedApiKeyIdDraft(String(value || ""))}
          gatewayBaseUrl={state.gatewayBaseUrl}
          onApply={() => state.applyGatewayMutation.mutate()}
          isPending={state.applyGatewayMutation.isPending}
          keyLabel={keyLabel}
        />
      </div>

      <AdvancedRecoveryPanel
        t={t}
        status={state.status}
        isServiceReady={state.isServiceReady}
        isMutating={state.isMutating}
        codexHomeInput={state.codexHomeInput}
        latestHistoryRepair={state.latestHistoryRepair}
        formatBytes={formatBytes}
        onRepairHistory={() => state.repairHistoryMutation.mutate()}
        onPruneHistoryBackups={() => state.pruneHistoryBackupsMutation.mutate()}
        onRestore={() => state.restoreMutation.mutate()}
        saveConfigPending={state.saveConfigMutation.isPending}
        restorePending={state.restoreMutation.isPending}
        repairHistoryPending={state.repairHistoryMutation.isPending}
        pruneHistoryBackupsPending={state.pruneHistoryBackupsMutation.isPending}
        codexHomeDraftValue={state.codexHomeInput}
        onCodexHomeChange={(value) => state.setCodexHomeDraft(value)}
        onSaveConfig={() => state.saveConfigMutation.mutate()}
        gatewayBaseUrl={state.gatewayBaseUrl}
        defaultGatewayBaseUrl={state.defaultGatewayBaseUrl}
        onGatewayBaseUrlChange={(value) => state.setGatewayBaseUrlDraft(value)}
        onUseCurrentGateway={() => state.setGatewayBaseUrlDraft(state.defaultGatewayBaseUrl)}
      />
    </main>
  );
}
