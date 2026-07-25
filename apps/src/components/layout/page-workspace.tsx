import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";

type PageWorkspaceProps = {
  children: ReactNode;
  className?: string;
};

type PageHeaderProps = {
  eyebrow?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  meta?: ReactNode;
  className?: string;
};

type MetricCardProps = {
  title: ReactNode;
  value: ReactNode;
  detail?: ReactNode;
  icon?: LucideIcon;
  tone?: "blue" | "emerald" | "amber" | "rose" | "violet" | "slate";
  className?: string;
};

type WorkPanelProps = {
  children: ReactNode;
  className?: string;
};

const metricToneClassName = {
  blue: "border-blue-500/20 bg-blue-500/10 text-blue-600 shadow-sm",
  emerald: "border-emerald-500/20 bg-emerald-500/10 text-emerald-600 shadow-sm",
  amber: "border-amber-500/24 bg-amber-500/10 text-amber-600 shadow-sm",
  rose: "border-rose-500/20 bg-rose-500/10 text-rose-600 shadow-sm",
  violet: "border-violet-500/20 bg-violet-500/10 text-violet-600 shadow-sm",
  slate: "border-slate-500/20 bg-slate-500/10 text-slate-600 shadow-sm",
};

export function PageWorkspace({ children, className }: PageWorkspaceProps) {
  return (
    <div
      className={cn(
        "mx-auto flex w-full max-w-[1680px] flex-col gap-4",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
  meta,
  className,
}: PageHeaderProps) {
  return (
    <section
      className={cn(
        "mission-panel glass-card relative overflow-hidden rounded-lg px-4 py-3 lg:flex lg:items-center lg:justify-between",
        className,
      )}
    >
      <div className="pointer-events-none absolute right-3 top-3 hidden grid-cols-3 gap-0.5 opacity-15 sm:grid">
        {Array.from({ length: 9 }).map((_, index) => (
          <span key={index} className="h-0.5 w-0.5 rounded-full bg-primary/50" />
        ))}
      </div>
      <div className="relative flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1.5">
        <div className="flex min-w-0 items-center gap-2">
          <h1 className="truncate text-xl font-semibold text-foreground">
            {title}
          </h1>
          {eyebrow ? (
            typeof eyebrow === "string" ? (
              <Badge variant="secondary" className="h-5 shrink-0 rounded-md border-primary/20 bg-primary/10 px-2 font-mono text-[10px] uppercase text-primary">
                {eyebrow}
              </Badge>
            ) : (
              <span className="shrink-0">{eyebrow}</span>
            )
          ) : null}
        </div>
        {description ? (
          <p className="line-clamp-2 min-w-0 basis-full flex-1 text-xs leading-5 text-muted-foreground sm:basis-auto sm:min-w-[220px] sm:line-clamp-1">
            {description}
          </p>
        ) : null}
        {meta ? <div className="flex shrink-0 flex-wrap gap-1.5">{meta}</div> : null}
      </div>
      {actions ? (
        <div className="relative mt-3 flex w-full flex-wrap items-center gap-2 sm:w-auto lg:mt-0 lg:ml-4 lg:justify-end">
          {actions}
        </div>
      ) : null}
    </section>
  );
}

export function MetricCard({
  title,
  value,
  detail,
  icon: Icon,
  tone = "blue",
  className,
}: MetricCardProps) {
  return (
    <Card
      className={cn(
        "glass-card console-metric mission-panel overflow-hidden py-0 shadow-sm",
        className,
      )}
    >
      <CardContent className="flex min-h-[52px] items-center justify-between gap-2 px-3 py-2">
        <div className="min-w-0">
          <p className="truncate text-xs font-semibold text-muted-foreground">
            {title}
          </p>
          <div
            className="mt-1 truncate font-mono text-xl font-semibold leading-none tabular-nums text-foreground"
            title={typeof detail === "string" ? detail : undefined}
          >
            {value}
          </div>
        </div>
        {Icon ? (
          <div
            className={cn(
              "flex h-7 w-7 shrink-0 items-center justify-center rounded-md border",
              metricToneClassName[tone],
            )}
          >
            <Icon className="h-3 w-3" />
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

export function WorkPanel({
  children,
  className,
}: WorkPanelProps) {
  return (
    <Card className={cn("glass-card console-panel mission-panel overflow-hidden py-0 shadow-sm", className)}>
      {children}
    </Card>
  );
}
