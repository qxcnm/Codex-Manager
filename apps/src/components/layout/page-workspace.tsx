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

export function PageWorkspace({ children, className }: PageWorkspaceProps) {
  return (
    <div
      className={cn(
        "mx-auto flex w-full min-w-0 max-w-[1680px] flex-col gap-3",
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
      data-slot="page-header"
      className={cn(
        "page-header border-b border-border pb-3",
        className,
      )}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <h2 className="min-w-0 text-xl font-semibold tracking-tight text-foreground">
            {title}
          </h2>
          {eyebrow ? (
            typeof eyebrow === "string" ? (
              <Badge variant="secondary" className="h-5 shrink-0 rounded-md px-2 font-mono text-[10px] uppercase">
                {eyebrow}
              </Badge>
            ) : (
              <span className="shrink-0">{eyebrow}</span>
            )
          ) : null}
        </div>
        {description ? (
          <p className="max-w-3xl text-sm leading-5 text-muted-foreground">
            {description}
          </p>
        ) : null}
        {meta ? <div className="flex flex-wrap gap-1.5">{meta}</div> : null}
      </div>
      {actions ? (
        <div className="page-header-actions">
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
      <CardContent className="flex min-h-16 items-center justify-between gap-3 px-3 py-2.5">
        <div className="flex min-w-0 flex-col gap-1.5">
          <p className="truncate text-xs font-medium text-muted-foreground">
            {title}
          </p>
          <div
            className="truncate font-mono text-xl font-semibold leading-none tabular-nums text-foreground"
          >
            {value}
          </div>
          {detail ? <p className="text-xs leading-5 text-muted-foreground">{detail}</p> : null}
        </div>
        {Icon ? (
          <div
            data-tone={tone}
            className="metric-icon flex size-8 shrink-0 items-center justify-center rounded-lg"
          >
            <Icon className="size-4" />
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
