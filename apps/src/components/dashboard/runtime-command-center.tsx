"use client";

import { useQuery } from "@tanstack/react-query";
import {
  ArrowUpRight,
  Boxes,
  Braces,
  Cable,
  Cpu,
  KeyRound,
  Orbit,
  Radio,
  Route,
  ServerCog,
  Sparkles,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { listKiroCredentials } from "@/lib/api/kiro-client";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";

interface RuntimeCommandCenterProps {
  serviceReady: boolean;
  directAccountMode: boolean;
  codexTotal: number;
  codexAvailable: number;
}

function RuntimeLink({
  href,
  label,
  icon: Icon,
}: {
  href: string;
  label: string;
  icon: typeof Cpu;
}) {
  const navigateShellPath = useAppStore((state) => state.navigateShellPath);
  return (
    <button
      type="button"
      onClick={() => navigateShellPath(href)}
      className="group inline-flex h-8 items-center gap-2 rounded-md border border-border/60 bg-background/35 px-3 text-[11px] font-medium text-muted-foreground transition-colors hover:border-primary/35 hover:bg-primary/[0.06] hover:text-foreground"
    >
      <Icon className="h-3.5 w-3.5" />
      {label}
      <ArrowUpRight className="h-3 w-3 opacity-40 transition-opacity group-hover:opacity-100" />
    </button>
  );
}

function AdapterNode({
  name,
  status,
  detail,
  accent,
  icon: Icon,
}: {
  name: string;
  status: string;
  detail: string;
  accent: "cyan" | "emerald";
  icon: typeof Cpu;
}) {
  return (
    <div className={cn("openruntime-adapter-node", `openruntime-adapter-${accent}`)}>
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-current/20 bg-background/55">
        <Icon className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-mono text-[11px] font-semibold text-foreground">
            {name}
          </span>
          <span className="h-1.5 w-1.5 rounded-full bg-current shadow-[0_0_8px_currentColor]" />
        </div>
        <div className="truncate text-[9px] text-muted-foreground">{detail}</div>
      </div>
      <span className="font-mono text-sm font-semibold tabular-nums text-foreground">
        {status}
      </span>
    </div>
  );
}

function RuntimeRail() {
  return (
    <div className="openruntime-rail" aria-hidden="true">
      <span />
      <span />
      <span />
    </div>
  );
}

export function RuntimeCommandCenter({
  serviceReady,
  directAccountMode,
  codexTotal,
  codexAvailable,
}: RuntimeCommandCenterProps) {
  const { t } = useI18n();
  const kiroQuery = useQuery({
    queryKey: ["kiro", "credentials", "runtime-command-center"],
    queryFn: listKiroCredentials,
    enabled: serviceReady,
    staleTime: 15_000,
  });
  const credentials = kiroQuery.data ?? [];
  const activeKiro = credentials.filter((credential) => credential.status === "active");
  const availableKiroModels = Array.from(
    new Set(activeKiro.flatMap((credential) => credential.availableModels ?? [])),
  ).sort();
  const runtimeLive = serviceReady && !directAccountMode;

  return (
    <section className="openruntime-console" aria-labelledby="openruntime-title">
      <div className="openruntime-grid" aria-hidden="true" />
      <div className="openruntime-glow" aria-hidden="true" />

      <header className="relative z-10 flex flex-col gap-5 px-5 pb-5 pt-5 lg:flex-row lg:items-start lg:justify-between lg:px-6">
        <div className="flex min-w-0 items-start gap-4">
          <div className="openruntime-mark" aria-hidden="true">
            <Orbit className="h-5 w-5" />
            <span className="openruntime-mark-core" />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2.5">
              <h2
                id="openruntime-title"
                className="font-mono text-xl font-bold tracking-[-0.04em] text-foreground sm:text-2xl"
              >
                Open<span className="text-primary">Runtime</span>
              </h2>
              <Badge
                variant="outline"
                className={cn(
                  "h-5 gap-1.5 rounded-sm px-2 font-mono text-[9px] tracking-[0.14em]",
                  runtimeLive
                    ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-500"
                    : "border-amber-500/30 bg-amber-500/10 text-amber-500",
                )}
              >
                <span className="h-1.5 w-1.5 rounded-full bg-current shadow-[0_0_7px_currentColor]" />
                {runtimeLive ? "RUNTIME ONLINE" : "RUNTIME STANDBY"}
              </Badge>
            </div>
            <p className="mt-2 text-sm font-semibold tracking-[-0.01em] text-foreground/90 sm:text-base">
              One Runtime. Every AI.
            </p>
            <p className="mt-1 max-w-xl text-[11px] leading-5 text-muted-foreground sm:text-xs">
              {t("Build Once. Connect Every AI. 上层只对接一种协议，底层平台通过 Adapter 即插即用。")}
            </p>
          </div>
        </div>

        <div className="flex flex-wrap gap-2">
          <RuntimeLink href="/import" label={t("接入 Adapter")} icon={Boxes} />
          <RuntimeLink href="/routing" label={t("编排路由")} icon={Route} />
          <RuntimeLink href="/apikeys" label={t("Runtime Key")} icon={KeyRound} />
        </div>
      </header>

      <div className="relative z-10 border-y border-border/55 bg-background/[0.16] px-5 py-5 lg:px-6">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2 font-mono text-[9px] uppercase tracking-[0.18em] text-muted-foreground">
            <Radio className="h-3 w-3 text-primary" />
            Live protocol topology
          </div>
          <div className="font-mono text-[9px] text-muted-foreground/70">
            APP → CANONICAL IR → ADAPTER
          </div>
        </div>

        <div className="grid items-stretch gap-3 lg:grid-cols-[minmax(0,1fr)_42px_minmax(220px,0.72fr)_42px_minmax(0,1fr)]">
          <div className="openruntime-zone">
            <div className="openruntime-zone-label">01 · ADAPTER MESH</div>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-1 xl:grid-cols-2">
              <AdapterNode
                name="codex.adapter"
                status={directAccountMode ? "--" : `${codexAvailable}/${codexTotal}`}
                detail={directAccountMode ? t("账号直连") : t("健康账号 / 总账号")}
                accent="cyan"
                icon={Cpu}
              />
              <AdapterNode
                name="kiro.adapter"
                status={String(activeKiro.length)}
                detail={`${availableKiroModels.length} ${t("个已验证模型")}`}
                accent="emerald"
                icon={ServerCog}
              />
            </div>
          </div>

          <RuntimeRail />

          <div className="openruntime-kernel">
            <div className="openruntime-zone-label text-center">02 · PROTOCOL KERNEL</div>
            <div className="openruntime-kernel-orbit">
              <Braces className="h-5 w-5" />
              <span>Canonical IR</span>
            </div>
            <div className="mt-3 flex flex-wrap justify-center gap-1.5">
              {["normalize", "route", "fallback"].map((item) => (
                <span key={item} className="openruntime-chip">{item}</span>
              ))}
            </div>
          </div>

          <RuntimeRail />

          <div className="openruntime-zone">
            <div className="openruntime-zone-label">03 · OPEN INTERFACE</div>
            <div className="grid gap-2">
              <div className="openruntime-endpoint">
                <Cable className="h-3.5 w-3.5 text-primary" />
                <span>/v1/chat/completions</span>
                <span className="ml-auto">OPENAI</span>
              </div>
              <div className="openruntime-endpoint">
                <Braces className="h-3.5 w-3.5 text-primary" />
                <span>/v1/responses</span>
                <span className="ml-auto">OPENAI</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <footer className="relative z-10 flex min-w-0 flex-wrap items-center gap-2 px-5 py-3 lg:px-6">
        <span className="mr-1 inline-flex items-center gap-1.5 font-mono text-[9px] uppercase tracking-[0.16em] text-muted-foreground">
          <Sparkles className="h-3 w-3 text-amber-500" />
          Discovered capabilities
        </span>
        {availableKiroModels.length > 0 ? (
          availableKiroModels.map((model) => (
            <Badge key={model} variant="outline" className="h-5 rounded-sm font-mono text-[9px]">
              {model.replace("kiro/", "")}
            </Badge>
          ))
        ) : (
          <span className="text-[10px] text-muted-foreground">
            {kiroQuery.isLoading ? t("正在读取 Adapter 状态") : t("接入凭据后由 Adapter 自动发现能力")}
          </span>
        )}
      </footer>
    </section>
  );
}
