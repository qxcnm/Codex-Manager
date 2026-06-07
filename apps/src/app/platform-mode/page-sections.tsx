import type { ReactNode } from "react";
import { ChevronDown, Loader2, Network, RefreshCw, RotateCcw, ShieldCheck, Trash2, UserRoundCheck, Wrench } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";
import { cn } from "@/lib/utils";
import { CODEX_PROFILE_MODE_LABELS } from "@/hooks/useCodexProfileModeStatus";
import type {
  CodexProfileAccountCandidate,
  CodexProfileApiKeyCandidate,
  CodexProfileHistoryRepairSummary,
  CodexProfileMode,
  CodexProfileStatus,
} from "@/types";

function ModeFact({ label, value }: { label: string; value: string }) {
  const displayValue = value || "-";
  const valueSizeClass =
    Array.from(displayValue).length > 64
      ? "text-[0.68rem] leading-snug"
      : Array.from(displayValue).length > 32
        ? "text-[0.75rem] leading-snug"
        : "text-sm";
  return (
    <div className="min-w-0 rounded-xl border border-border/60 bg-background/35 p-3">
      <p className="min-w-0 text-[11px] text-muted-foreground">{label}</p>
      <p
        className={cn(
          "mt-1 block min-w-0 max-w-full break-all font-semibold [overflow-wrap:anywhere]",
          valueSizeClass,
        )}
        title={displayValue}
      >
        {displayValue}
      </p>
    </div>
  );
}

function platformSelectLabelClassName(value: string): string {
  const length = Array.from(String(value || "")).length;
  if (length > 96) return "text-[10px] leading-tight";
  if (length > 60) return "text-[11px] leading-snug";
  if (length > 36) return "text-xs leading-snug";
  return "text-sm";
}

function PlatformSelectText({ value }: { value: string }) {
  return (
    <span
      className={cn(
        "block min-w-0 max-w-full whitespace-normal break-words text-left [overflow-wrap:anywhere]",
        platformSelectLabelClassName(value),
      )}
      title={value}
    >
      {value}
    </span>
  );
}

function ActionLink({
  href,
  children,
}: {
  href: string;
  children: ReactNode;
}) {
  return (
    <a
      href={buildStaticRouteUrl(href)}
      className="inline-flex h-8 w-fit items-center justify-center rounded-lg border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-muted"
    >
      {children}
    </a>
  );
}

function MetadataItem({
  label,
  value,
  valueClassName,
}: {
  label: string;
  value: ReactNode;
  valueClassName?: string;
}) {
  return (
    <div className="grid gap-1 rounded-lg border border-border/50 bg-background/50 p-3 md:grid-cols-[140px_minmax(0,1fr)] md:items-start">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      <span className={cn("min-w-0 break-all text-xs text-foreground", valueClassName)}>
        {value}
      </span>
    </div>
  );
}

export function CurrentModeCard({
  t,
  status,
  isGatewayActive,
  statusFetching,
  candidatesFetching,
  onRefresh,
  codexHome,
  activeAccountValue,
  activeKeyValue,
  lastAppliedAtLabel,
  modeDescription,
}: {
  t: (value: string, params?: Record<string, string | number>) => string;
  status:
    | {
        mode: CodexProfileMode;
      }
    | null
    | undefined;
  isGatewayActive: boolean;
  statusFetching: boolean;
  candidatesFetching: boolean;
  onRefresh: () => void;
  codexHome: string;
  activeAccountValue: string;
  activeKeyValue: string;
  lastAppliedAtLabel: string;
  modeDescription: string;
}) {
  return (
    <Card className="overflow-hidden border-primary/20 bg-primary/5 shadow-sm lg:col-span-2 xl:col-span-1">
      <CardHeader className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between xl:flex-col 2xl:flex-row">
        <div>
          <CardTitle className="flex flex-wrap items-center gap-2 text-xl">
            {t("当前模式")}
            <Badge variant={isGatewayActive ? "default" : "secondary"}>
              {status ? t(CODEX_PROFILE_MODE_LABELS[status.mode]) : "-"}
            </Badge>
          </CardTitle>
          <CardDescription className="mt-2 text-sm">{modeDescription}</CardDescription>
        </div>
        <Button
          type="button"
          variant="outline"
          onClick={onRefresh}
          className="w-fit"
        >
          <RefreshCw
            className={
              statusFetching || candidatesFetching ? "size-4 animate-spin" : "size-4"
            }
          />
          {t("刷新状态")}
        </Button>
      </CardHeader>
      <CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-1 2xl:grid-cols-2">
        <ModeFact label={t("Codex profile")} value={codexHome || "-"} />
        <ModeFact label={t("当前账号")} value={activeAccountValue} />
        <ModeFact label={t("当前平台 Key")} value={activeKeyValue} />
        <ModeFact label={t("最后应用")} value={lastAppliedAtLabel} />
      </CardContent>
    </Card>
  );
}

export function DirectAccountCard({
  t,
  candidates,
  isLoading,
  isServiceReady,
  isMutating,
  isDirectActive,
  selectedAccountId,
  onSelectAccount,
  onApply,
  isPending,
  accountLabel,
}: {
  t: (value: string, params?: Record<string, string | number>) => string;
  candidates: CodexProfileAccountCandidate[];
  isLoading: boolean;
  isServiceReady: boolean;
  isMutating: boolean;
  isDirectActive: boolean;
  selectedAccountId: string;
  onSelectAccount: (value: string | null) => void;
  onApply: () => void;
  isPending: boolean;
  accountLabel: (account: CodexProfileAccountCandidate) => string;
}) {
  return (
    <Card
      className={cn(
        "h-full border-border/70 transition-colors",
        isDirectActive && "border-primary/50 bg-primary/5",
      )}
    >
      <CardHeader>
        <div className="flex flex-wrap items-center gap-2">
          <UserRoundCheck className="size-4 text-primary" />
          <CardTitle>{t("账号直连")}</CardTitle>
          {isDirectActive ? <Badge>{t("正在使用")}</Badge> : null}
        </div>
        <CardDescription>
          {t(
            "直连 OpenAI 官方后端，不经过 CodexManager 网关；不会产生 CodexManager 请求日志，仪表盘用量统计不可用。",
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {candidates.length === 0 && !isLoading ? (
          <div className="grid gap-3 rounded-xl border border-dashed border-border/70 bg-muted/25 p-4 text-sm text-muted-foreground">
            <p>{t("没有可用于账号直连的 active OpenAI 账号。")}</p>
            <ActionLink href="/accounts">{t("去添加 OpenAI 账号")}</ActionLink>
          </div>
        ) : (
          <div className="grid gap-2">
            <Label>{t("OpenAI 账号")}</Label>
            <Select
              value={selectedAccountId}
              onValueChange={onSelectAccount}
              disabled={!isServiceReady || isMutating || candidates.length === 0}
            >
              <SelectTrigger className="h-auto min-h-8 w-full whitespace-normal py-1.5 *:data-[slot=select-value]:line-clamp-none *:data-[slot=select-value]:min-w-0">
                <SelectValue placeholder={t("选择账号")}>
                  {(value) => {
                    const account = candidates.find((item) => item.id === value);
                    return (
                      <PlatformSelectText
                        value={account ? accountLabel(account) : t("选择账号")}
                      />
                    );
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent
                align="start"
                className="w-[min(28rem,calc(100vw-2rem))] min-w-[min(28rem,calc(100vw-2rem))]"
              >
                <SelectGroup>
                  {candidates.map((account) => {
                    const label = accountLabel(account);
                    return (
                      <SelectItem key={account.id} value={account.id} className="items-start py-2">
                        <PlatformSelectText value={label} />
                      </SelectItem>
                    );
                  })}
                </SelectGroup>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {isLoading ? t("正在读取可用账号...") : t("可用账号数：{count}", { count: candidates.length })}
            </p>
          </div>
        )}
        <Button
          type="button"
          onClick={onApply}
          disabled={!isServiceReady || isMutating || !selectedAccountId}
          className="w-fit"
        >
          {isPending ? <Loader2 className="size-4 animate-spin" /> : <ShieldCheck className="size-4" />}
          {isDirectActive ? t("重新应用账号直连") : t("切换到账号直连")}
        </Button>
      </CardContent>
    </Card>
  );
}

export function GatewayModeCard({
  t,
  candidates,
  isLoading,
  isServiceReady,
  isMutating,
  isGatewayActive,
  selectedApiKeyId,
  onSelectApiKey,
  gatewayBaseUrl,
  onApply,
  isPending,
  keyLabel,
}: {
  t: (value: string, params?: Record<string, string | number>) => string;
  candidates: CodexProfileApiKeyCandidate[];
  isLoading: boolean;
  isServiceReady: boolean;
  isMutating: boolean;
  isGatewayActive: boolean;
  selectedApiKeyId: string;
  onSelectApiKey: (value: string | null) => void;
  gatewayBaseUrl: string;
  onApply: () => void;
  isPending: boolean;
  keyLabel: (key: CodexProfileApiKeyCandidate) => string;
}) {
  return (
    <Card
      className={cn(
        "h-full border-border/70 transition-colors",
        isGatewayActive && "border-primary/50 bg-primary/5",
      )}
    >
      <CardHeader>
        <div className="flex flex-wrap items-center gap-2">
          <Network className="size-4 text-primary" />
          <CardTitle>{t("本地网关")}</CardTitle>
          {isGatewayActive ? <Badge>{t("正在使用")}</Badge> : null}
        </div>
        <CardDescription>
          {t(
            "通过 CodexManager 本地网关转发 Codex CLI 请求；请求日志、Token、费用估算和仪表盘统计可用。",
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {candidates.length === 0 && !isLoading ? (
          <div className="grid gap-3 rounded-xl border border-dashed border-border/70 bg-muted/25 p-4 text-sm text-muted-foreground">
            <p>{t("没有可用于本地网关的平台密钥。")}</p>
            <ActionLink href="/apikeys">{t("去创建平台密钥")}</ActionLink>
          </div>
        ) : (
          <div className="grid gap-2">
            <Label>{t("平台密钥")}</Label>
            <Select
              value={selectedApiKeyId}
              onValueChange={onSelectApiKey}
              disabled={!isServiceReady || isMutating || candidates.length === 0}
            >
              <SelectTrigger className="h-auto min-h-8 w-full whitespace-normal py-1.5 *:data-[slot=select-value]:line-clamp-none *:data-[slot=select-value]:min-w-0">
                <SelectValue placeholder={t("选择平台密钥")}>
                  {(value) => {
                    const key = candidates.find((item) => item.id === value);
                    return (
                      <PlatformSelectText
                        value={key ? keyLabel(key) : t("选择平台密钥")}
                      />
                    );
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent
                align="start"
                className="w-[min(28rem,calc(100vw-2rem))] min-w-[min(28rem,calc(100vw-2rem))]"
              >
                <SelectGroup>
                  {candidates.map((key) => {
                    const label = keyLabel(key);
                    return (
                      <SelectItem key={key.id} value={key.id} className="items-start py-2">
                        <PlatformSelectText value={label} />
                      </SelectItem>
                    );
                  })}
                </SelectGroup>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {t("将使用 gateway base_url")}：{gatewayBaseUrl || "-"}
            </p>
          </div>
        )}
        <Button
          type="button"
          onClick={onApply}
          disabled={!isServiceReady || isMutating || !selectedApiKeyId || !gatewayBaseUrl.trim()}
          className="w-fit"
        >
          {isPending ? <Loader2 className="size-4 animate-spin" /> : <Network className="size-4" />}
          {isGatewayActive ? t("重新应用本地网关") : t("切换到本地网关")}
        </Button>
      </CardContent>
    </Card>
  );
}

export function AdvancedRecoveryPanel({
  t,
  status,
  isServiceReady,
  isMutating,
  codexHomeInput,
  latestHistoryRepair,
  formatBytes,
  onRepairHistory,
  onPruneHistoryBackups,
  onRestore,
  saveConfigPending,
  restorePending,
  repairHistoryPending,
  pruneHistoryBackupsPending,
  codexHomeDraftValue,
  onCodexHomeChange,
  onSaveConfig,
  gatewayBaseUrl,
  defaultGatewayBaseUrl,
  onGatewayBaseUrlChange,
  onUseCurrentGateway,
}: {
  t: (value: string, params?: Record<string, string | number>) => string;
  status: CodexProfileStatus | null | undefined;
  isServiceReady: boolean;
  isMutating: boolean;
  codexHomeInput: string;
  latestHistoryRepair: CodexProfileHistoryRepairSummary | null;
  formatBytes: (bytes: number | null | undefined) => string;
  onRepairHistory: () => void;
  onPruneHistoryBackups: () => void;
  onRestore: () => void;
  saveConfigPending: boolean;
  restorePending: boolean;
  repairHistoryPending: boolean;
  pruneHistoryBackupsPending: boolean;
  codexHomeDraftValue: string;
  onCodexHomeChange: (value: string) => void;
  onSaveConfig: () => void;
  gatewayBaseUrl: string;
  defaultGatewayBaseUrl: string;
  onGatewayBaseUrlChange: (value: string) => void;
  onUseCurrentGateway: () => void;
}) {
  return (
    <details className="group rounded-xl border border-border/70 bg-card shadow-sm">
      <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-5 py-4">
        <div>
          <h2 className="text-base font-semibold">{t("高级与恢复")}</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("修改 profile 目录、gateway base_url、修复历史会话或恢复接管前配置。")}
          </p>
        </div>
        <ChevronDown className="size-4 text-muted-foreground transition-transform group-open:rotate-180" />
      </summary>
      <div className="grid gap-5 border-t border-border/60 px-5 py-5">
        <div className="grid items-start gap-5 lg:grid-cols-2">
          <Card className="border-border/70">
            <CardHeader>
              <CardTitle>{t("Profile 目标目录")}</CardTitle>
              <CardDescription>
                {t("默认使用 CODEX_HOME 或 service 用户的 ~/.codex。")}
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="grid gap-2">
                <Label htmlFor="codex-home">{t("Codex profile 目录")}</Label>
                <div className="flex flex-col gap-2 sm:flex-row">
                  <Input
                    id="codex-home"
                    value={codexHomeDraftValue}
                    onChange={(event) => onCodexHomeChange(event.target.value)}
                    placeholder="~/.codex"
                    disabled={!isServiceReady || isMutating}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={onSaveConfig}
                    disabled={!isServiceReady || isMutating || !codexHomeInput.trim()}
                  >
                    {saveConfigPending ? <Loader2 className="size-4 animate-spin" /> : <Wrench className="size-4" />}
                    {t("保存")}
                  </Button>
                </div>
              </div>
              <div className="grid gap-2 rounded-lg border bg-muted/30 p-3">
                <MetadataItem label={t("auth.json")} value={status?.authPath || "-"} />
                <MetadataItem label={t("config.toml")} value={status?.configPath || "-"} />
                <MetadataItem
                  label={t("CodexManager 管理文件")}
                  value={status?.managedStorageRoot || "-"}
                />
                <MetadataItem label={t("管理标记")} value={status?.markerPath || "-"} />
                <MetadataItem
                  label={t("可写")}
                  value={status?.profileWritable ? t("是") : t("否或未知")}
                  valueClassName="break-normal"
                />
              </div>
            </CardContent>
          </Card>

          <Card className="self-start border-border/70">
            <CardHeader>
              <CardTitle>{t("Gateway base_url")}</CardTitle>
              <CardDescription>
                {t("默认使用当前 Web 服务可访问的本地网关地址。")}
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="grid gap-2">
                <Label htmlFor="gateway-base-url">{t("OpenAI gateway base_url")}</Label>
                <div className="flex flex-col gap-2 sm:flex-row">
                  <Input
                    id="gateway-base-url"
                    value={gatewayBaseUrl}
                    onChange={(event) => onGatewayBaseUrlChange(event.target.value)}
                    placeholder={defaultGatewayBaseUrl || "http://localhost:48760/v1"}
                    disabled={!isServiceReady || isMutating}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={onUseCurrentGateway}
                    disabled={!defaultGatewayBaseUrl || isMutating}
                    className="sm:shrink-0"
                  >
                    <Wrench className="size-4" />
                    {t("使用当前网关")}
                  </Button>
                </div>
              </div>
              <div className="grid gap-2 rounded-lg border bg-muted/30 p-3">
                <MetadataItem
                  label={t("当前访问地址")}
                  value={gatewayBaseUrl || defaultGatewayBaseUrl || "-"}
                />
                <MetadataItem
                  label={t("默认网关")}
                  value={defaultGatewayBaseUrl || "-"}
                />
              </div>
            </CardContent>
          </Card>
        </div>

        <Card className="border-border/70">
          <CardHeader>
            <CardTitle>{t("恢复与历史会话")}</CardTitle>
            <CardDescription>
              {t("切换模式时会自动修复历史会话 provider 元数据；Codex 运行中锁库时可手动重试。")}
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            <div className="grid gap-2 rounded-lg border bg-muted/20 p-3 text-xs">
              <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <p className="font-medium text-foreground">{t("历史会话可见性")}</p>
                  <p className="text-muted-foreground">
                    {latestHistoryRepair
                      ? latestHistoryRepair.message
                      : t("切换 direct / gateway 时会自动修复历史会话的 provider 元数据。")}
                  </p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  onClick={onRepairHistory}
                  disabled={!isServiceReady || isMutating || !codexHomeInput.trim()}
                >
                  {repairHistoryPending ? <Loader2 className="size-4 animate-spin" /> : <Wrench className="size-4" />}
                  {t("修复历史可见性")}
                </Button>
              </div>
              {latestHistoryRepair ? (
                <div className="grid gap-1 text-muted-foreground">
                  <span>{t("目标 provider")}：{latestHistoryRepair.targetProvider || "-"}</span>
                  <span>
                    {t("已修复 rollout / SQLite / session_index")}：
                    {latestHistoryRepair.changedRolloutFileCount} / {" "}
                    {latestHistoryRepair.updatedSqliteRowCount} / {" "}
                    {latestHistoryRepair.addedSessionIndexEntryCount}
                  </span>
                  {latestHistoryRepair.backupDir ? (
                    <span className="break-all">{t("备份目录")}：{latestHistoryRepair.backupDir}</span>
                  ) : null}
                  {latestHistoryRepair.warnings.length > 0 ? (
                    <span className="text-amber-600 dark:text-amber-400">{t("警告")}：{latestHistoryRepair.warnings[0]}</span>
                  ) : null}
                </div>
              ) : null}
            </div>
            <div className="grid gap-3 rounded-lg border bg-muted/20 p-3 text-xs">
              <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                <div className="min-w-0">
                  <p className="font-medium text-foreground">{t("历史修复备份")}</p>
                  <p className="mt-1 text-muted-foreground">{t("备份保存在 CodexManager 数据目录，不再写入 Codex profile。")}</p>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  onClick={onPruneHistoryBackups}
                  disabled={!isServiceReady || isMutating || !codexHomeInput.trim()}
                  className="w-fit"
                >
                  {pruneHistoryBackupsPending ? <Loader2 className="size-4 animate-spin" /> : <Trash2 className="size-4" />}
                  {t("清理历史备份")}
                </Button>
              </div>
              <div className="grid gap-2 text-muted-foreground sm:grid-cols-2">
                <span className="break-all sm:col-span-2">{t("备份目录")}：{status?.historyBackupRoot || "-"}</span>
                <span>{t("数量 / 占用")}：{status?.historyBackupCount ?? 0} / {" "}{formatBytes(status?.historyBackupBytes)}</span>
                <span className="sm:col-span-2">
                  {t("保留策略")}：
                  {t("最多 {count} 份，最多 {days} 天，至少保留最新 {min} 份", {
                    count: status?.historyRetention.maxHistoryBackupsPerProfile ?? 3,
                    days: status?.historyRetention.maxHistoryBackupAgeDays ?? 7,
                    min: status?.historyRetention.minHistoryBackupsPerProfile ?? 1,
                  })}
                </span>
              </div>
            </div>
            <Separator />
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="text-xs text-muted-foreground">{t("备份")}：{status?.hasBackup ? t("已保存") : t("暂无")}</div>
              <Button
                type="button"
                variant="destructive"
                onClick={onRestore}
                disabled={!isServiceReady || isMutating || !status?.hasBackup}
              >
                {restorePending ? <Loader2 className="size-4 animate-spin" /> : <RotateCcw className="size-4" />}
                {t("恢复接管前配置")}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </details>
  );
}
