"use client";

import { Check, Palette } from "lucide-react";
import { APPEARANCE_PRESETS } from "@/lib/appearance";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { THEMES } from "@/app/settings/settings-page-helpers";
import { ThemePreviewSwatch } from "@/app/settings/components/theme-preview-swatch";

type TranslateFn = (key: string) => string;

interface AppearanceTabContentProps {
  t: TranslateFn;
  theme: string | undefined;
  appearancePreset: string | null | undefined;
  onThemeChange: (nextTheme: string) => void;
  onAppearancePresetChange: (nextPreset: string) => void;
}

export function AppearanceTabContent({
  t,
  theme,
  appearancePreset,
  onThemeChange,
  onAppearancePresetChange,
}: AppearanceTabContentProps) {
  return (
    <>
      <Card className="glass-card mission-panel shadow-sm">
        <CardHeader>
          <div className="flex items-center gap-2">
            <Palette className="h-4 w-4 text-primary" />
            <CardTitle className="text-base">{t("样式版本")}</CardTitle>
          </div>
          <CardDescription>{t("在渐变版本和默认版本之间切换")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 md:grid-cols-2">
            {APPEARANCE_PRESETS.map((item) => {
              const isActive = appearancePreset === item.id;
              return (
                <Button
                  key={item.id}
                  type="button"
                  variant="outline"
                  onClick={() => onAppearancePresetChange(item.id)}
                  className={cn(
                    "group relative h-auto justify-start rounded-lg p-4 text-left transition-all duration-200",
                    isActive
                      ? "border-primary/30 bg-primary/[0.055] shadow-[0_12px_28px_-24px_rgb(var(--primary-rgb)/0.28)] ring-1 ring-primary/10"
                      : "border-border/50 bg-background/45 hover:border-primary/20 hover:bg-accent/20",
                  )}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="space-y-1.5">
                      <div className="text-sm font-semibold">{t(item.name)}</div>
                      <p className="text-xs leading-5 text-muted-foreground">
                        {t(item.description)}
                      </p>
                    </div>
                    {isActive ? (
                      <div className="rounded-full bg-primary p-1 text-primary-foreground shadow-sm">
                        <Check className="h-3 w-3" />
                      </div>
                    ) : null}
                  </div>
                  <div className="mt-3 flex items-end gap-2.5">
                    <div
                      className={cn(
                        "h-14 flex-1 rounded-lg border",
                        item.id === "modern"
                          ? "border-primary/10 bg-primary/[0.035]"
                          : "border-border/70 bg-muted/60",
                      )}
                    />
                    <div className="flex w-16 flex-col gap-1.5">
                      <div
                        className={cn(
                          "h-4 rounded-lg border",
                          item.id === "modern"
                            ? "border-primary/10 bg-card/75 shadow-[0_8px_18px_-16px_rgb(var(--primary-rgb)/0.16)]"
                            : "border-border/70 bg-card",
                        )}
                      />
                      <div
                        className={cn(
                          "h-4 rounded-lg border",
                          item.id === "modern"
                            ? "border-primary/10 bg-card/65 shadow-[0_8px_18px_-16px_rgb(var(--primary-rgb)/0.12)]"
                            : "border-border/70 bg-card/80",
                        )}
                      />
                    </div>
                  </div>
                </Button>
              );
            })}
          </div>
        </CardContent>
      </Card>

      <Card className="glass-card mission-panel shadow-sm">
        <CardHeader>
          <div className="flex items-center gap-2">
            <Palette className="h-4 w-4 text-primary" />
            <CardTitle className="text-base">{t("界面主题")}</CardTitle>
          </div>
          <CardDescription>
            {t("选择您喜爱的配色方案，适配不同工作心情")}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {THEMES.map((item) => {
              const isActive = theme === item.id;
              return (
                <Button
                  key={item.id}
                  type="button"
                  variant="outline"
                  onClick={() => onThemeChange(item.id)}
                  className={cn(
                    "group relative h-auto justify-start rounded-lg p-3 text-left transition-all duration-200",
                    isActive
                      ? "border-primary/45 bg-primary/10 shadow-sm ring-1 ring-primary/20"
                      : "border-border/60 bg-background/55 hover:border-primary/25 hover:bg-accent/30",
                  )}
                >
                  <div className="flex w-full items-center gap-3">
                    <ThemePreviewSwatch id={item.id} color={item.color} />
                    <div className="min-w-0 flex-1">
                      <div
                        className={cn(
                          "truncate text-sm font-semibold transition-colors",
                          isActive
                            ? "text-primary"
                            : "text-foreground group-hover:text-primary",
                        )}
                      >
                        {t(item.name)}
                      </div>
                      <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-muted">
                        <div
                          className="h-full rounded-full"
                          style={{ backgroundColor: item.color }}
                        />
                      </div>
                    </div>
                    {isActive ? (
                      <div className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
                        <Check className="h-3 w-3" />
                      </div>
                    ) : null}
                  </div>
                </Button>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </>
  );
}
