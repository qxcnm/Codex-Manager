"use client";

import {
  Activity,
  BrainCircuit,
  CheckCircle2,
  Database,
  DollarSign,
  PieChart,
  Users,
  XCircle,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardStats } from "@/hooks/useDashboardStats";
import { cn } from "@/lib/utils";
import { formatCompactNumber } from "@/lib/utils/usage";

interface StatProgressCardProps {
  title: string;
  value: number;
  total: number;
  icon: LucideIcon;
  color: string;
  sub: string;
}

interface PercentBarProps {
  label: string;
  value: number | null | undefined;
  tone?: "default" | "green" | "blue";
}

interface AccountHighlightCardProps {
  title: string;
  name: string;
  subtitle: string;
  tone?: "green" | "blue";
  progressLabel?: string;
  progressValue?: number | null | undefined;
}

function formatPercent(value: number | null | undefined): string {
  return value == null ? "--" : `${Math.max(0, Math.round(value))}%`;
}

function PercentBar({ label, value, tone = "default" }: PercentBarProps) {
  const normalized = value == null ? 0 : Math.max(0, Math.min(100, Math.round(value)));
  const colorClass =
    tone === "green"
      ? "bg-green-500"
      : tone === "blue"
        ? "bg-blue-500"
        : "bg-primary";

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-[10px]">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-semibold">{formatPercent(value)}</span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted/60">
        <div
          className={cn("h-full rounded-full transition-all", colorClass)}
          style={{ width: `${normalized}%` }}
        />
      </div>
    </div>
  );
}

function AccountHighlightCard({
  title,
  name,
  subtitle,
  tone = "green",
  progressLabel,
  progressValue,
}: AccountHighlightCardProps) {
  const iconToneClass =
    tone === "blue"
      ? "bg-blue-500/20 text-blue-500"
      : "bg-green-500/20 text-green-500";

  return (
    <div className="rounded-2xl border border-border/40 bg-accent/20 p-4 shadow-sm">
      <div className="flex items-center gap-4">
        <div
          className={cn(
            "flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl",
            iconToneClass,
          )}
        >
          <CheckCircle2 className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] font-medium text-muted-foreground">{title}</p>
          <p className="truncate text-sm font-semibold leading-5">{name}</p>
          <p className="truncate text-xs text-muted-foreground">{subtitle}</p>
        </div>
      </div>
      {progressLabel ? (
        <div className="mt-3 border-t border-border/40 pt-3">
          <PercentBar label={progressLabel} value={progressValue} tone={tone} />
        </div>
      ) : null}
    </div>
  );
}

function StatProgressCard({
  title,
  value,
  total,
  icon: Icon,
  color,
  sub,
}: StatProgressCardProps) {
  const percentage = total > 0 ? Math.min(Math.round((value / total) * 100), 100) : 0;

  return (
    <Card className="glass-card overflow-hidden border-none shadow-md backdrop-blur-md transition-all hover:scale-[1.02]">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Icon className={cn("h-4 w-4", color)} />
      </CardHeader>
      <CardContent className="space-y-3">
        <div>
          <div className="text-2xl font-bold">{value}</div>
          <p className="mt-1 text-[10px] text-muted-foreground">{sub}</p>
        </div>
        <div className="space-y-1">
          <div className="flex items-center justify-between text-[10px]">
            <span className="text-muted-foreground">占比</span>
            <span className="font-mono font-medium">{percentage}%</span>
          </div>
          <Progress value={percentage} className="h-1.5" />
        </div>
      </CardContent>
    </Card>
  );
}

export default function DashboardPage() {
  const { stats, currentAccount, recommendations, requestLogs, isLoading } = useDashboardStats();
  const poolPrimary = stats.poolRemain?.primary ?? 0;
  const poolSecondary = stats.poolRemain?.secondary ?? 0;

  return (
    <div className="space-y-6 animate-in fade-in duration-700">
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {isLoading ? (
          Array.from({ length: 4 }).map((_, index) => (
            <Skeleton key={index} className="h-36 w-full rounded-2xl" />
          ))
        ) : (
          <>
            <Card className="glass-card overflow-hidden border-none shadow-md backdrop-blur-md transition-all hover:scale-[1.02]">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">总账号数</CardTitle>
                <Users className="h-4 w-4 text-blue-500" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{stats.total}</div>
                <p className="mt-1 text-[10px] text-muted-foreground">池中所有配置账号</p>
                <div className="mt-4 flex w-fit items-center gap-2 rounded-full bg-blue-500/10 px-2 py-0.5 text-[10px] text-blue-600 dark:text-blue-400">
                  <Activity className="h-3 w-3" />
                  最近日志 {requestLogs.length} 条
                </div>
              </CardContent>
            </Card>

            <StatProgressCard
              title="可用账号"
              value={stats.available}
              total={stats.total}
              icon={CheckCircle2}
              color="text-green-500"
              sub="当前健康可调用的账号"
            />

            <StatProgressCard
              title="不可用账号"
              value={stats.unavailable}
              total={stats.total}
              icon={XCircle}
              color="text-red-500"
              sub={
                stats.expired > 0
                  ? `额度耗尽 ${stats.unavailable}，已失效 ${stats.expired}`
                  : "当前额度耗尽的账号"
              }
            />

            <Card className="overflow-hidden border-none bg-primary/10 shadow-md backdrop-blur-md transition-all hover:scale-[1.02]">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-primary">账号池剩余</CardTitle>
                <PieChart className="h-4 w-4 text-primary" />
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">5小时内</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.primary)}</span>
                  </div>
                  <Progress value={poolPrimary} className="h-1.5 bg-primary/20" />
                </div>
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">7天内</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.secondary)}</span>
                  </div>
                  <Progress value={poolSecondary} className="h-1.5 bg-primary/20" />
                </div>
              </CardContent>
            </Card>
          </>
        )}
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {[
          {
            title: "今日令牌",
            value: formatCompactNumber(stats.todayTokens, "0"),
            icon: Zap,
            color: "text-yellow-500",
            sub: "输入 + 输出合计",
          },
          {
            title: "缓存令牌",
            value: formatCompactNumber(stats.cachedTokens, "0"),
            icon: Database,
            color: "text-indigo-500",
            sub: "上下文缓存命中",
          },
          {
            title: "推理令牌",
            value: formatCompactNumber(stats.reasoningTokens, "0"),
            icon: BrainCircuit,
            color: "text-purple-500",
            sub: "大模型思考过程",
          },
          {
            title: "预计费用",
            value: `$${Number(stats.todayCost || 0).toFixed(2)}`,
            icon: DollarSign,
            color: "text-emerald-500",
            sub: "按官价估算",
          },
        ].map((card) => (
          <Card
            key={card.title}
            className="glass-card overflow-hidden border-none shadow-md backdrop-blur-md transition-all hover:scale-[1.02]"
          >
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium">{card.title}</CardTitle>
              <card.icon className={cn("h-4 w-4", card.color)} />
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold">{card.value}</div>
              <p className="mt-1 text-[10px] text-muted-foreground">{card.sub}</p>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="grid gap-6 md:grid-cols-2">
        <Card className="glass-card min-h-[300px] border-none shadow-md">
          <CardHeader className="flex flex-row items-center justify-between">
            <CardTitle className="text-base font-semibold">当前活跃账号</CardTitle>
          </CardHeader>
          <CardContent className="flex min-h-[200px] flex-col justify-start">
            {currentAccount ? (
              <div className="space-y-4">
                <AccountHighlightCard
                  title="当前活跃账号"
                  name={currentAccount.name}
                  subtitle={currentAccount.id}
                  tone="green"
                />
                <div className="grid grid-cols-2 gap-4 text-sm">
                  <div className="space-y-3 rounded-xl bg-muted/30 p-4">
                    <p className="text-xs text-muted-foreground">5小时剩余</p>
                    <p className="text-lg font-bold">{formatPercent(currentAccount.primaryRemainPercent)}</p>
                    <PercentBar label="剩余额度" value={currentAccount.primaryRemainPercent} tone="green" />
                  </div>
                  <div className="space-y-3 rounded-xl bg-muted/30 p-4">
                    <p className="text-xs text-muted-foreground">7天剩余</p>
                    <p className="text-lg font-bold">{formatPercent(currentAccount.secondaryRemainPercent)}</p>
                    <PercentBar label="剩余额度" value={currentAccount.secondaryRemainPercent} tone="blue" />
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
                <div className="rounded-full bg-accent/30 p-4 animate-pulse">
                  <Activity className="h-8 w-8 opacity-20" />
                </div>
                <p>暂无可识别的活跃账号</p>
              </div>
            )}
          </CardContent>
        </Card>

        <Card className="glass-card min-h-[300px] border-none shadow-md">
          <CardHeader>
            <CardTitle className="text-base font-semibold">智能推荐</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <p className="text-xs text-muted-foreground">
              基于当前配额，系统会优先推荐剩余额度更高且仍可参与路由的账号。
            </p>
            {recommendations.primaryPick || recommendations.secondaryPick ? (
              <>
                {recommendations.primaryPick ? (
                  <AccountHighlightCard
                    title="5小时优先账号"
                    name={recommendations.primaryPick.name}
                    subtitle={recommendations.primaryPick.id}
                    tone="green"
                    progressLabel="剩余额度"
                    progressValue={recommendations.primaryPick.primaryRemainPercent}
                  />
                ) : null}
                {recommendations.secondaryPick ? (
                  <AccountHighlightCard
                    title="7天优先账号"
                    name={recommendations.secondaryPick.name}
                    subtitle={recommendations.secondaryPick.id}
                    tone="blue"
                    progressLabel="剩余额度"
                    progressValue={recommendations.secondaryPick.secondaryRemainPercent}
                  />
                ) : null}
              </>
            ) : (
              <div className="rounded-xl bg-accent/20 p-4 text-sm text-muted-foreground">
                当前没有可推荐的可用账号。
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
