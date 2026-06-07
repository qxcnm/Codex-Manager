"use client";

import { Info, RotateCcw, Save, Search, Variable } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import type { EnvOverrideCatalogItem } from "@/types";

type TranslateFn = (key: string) => string;

interface EnvTabContentProps {
  t: TranslateFn;
  envSearch: string;
  selectedEnvKey: string | null;
  selectedEnvItem: EnvOverrideCatalogItem | null;
  selectedEnvValue: string;
  selectedEnvRiskLevel: string;
  selectedEnvEffectScope: string;
  selectedEnvSafetyNote: string;
  hasCustomizedEnvOverrides: boolean;
  isSaving: boolean;
  filteredEnvCatalog: EnvOverrideCatalogItem[];
  descriptionMap: Record<string, string>;
  riskBadgeClasses: Record<string, string>;
  riskLabels: Record<string, string>;
  effectScopeLabels: Record<string, string>;
  onSearchChange: (value: string) => void;
  onSelectEnvKey: (key: string) => void;
  onSelectedEnvValueChange: (value: string) => void;
  onSaveEnv: () => void;
  onResetEnv: () => void;
  onResetAllEnv: () => void;
}

export function EnvTabContent({
  t,
  envSearch,
  selectedEnvKey,
  selectedEnvItem,
  selectedEnvValue,
  selectedEnvRiskLevel,
  selectedEnvEffectScope,
  selectedEnvSafetyNote,
  hasCustomizedEnvOverrides,
  isSaving,
  filteredEnvCatalog,
  descriptionMap,
  riskBadgeClasses,
  riskLabels,
  effectScopeLabels,
  onSearchChange,
  onSelectEnvKey,
  onSelectedEnvValueChange,
  onSaveEnv,
  onResetEnv,
  onResetAllEnv,
}: EnvTabContentProps) {
  return (
    <>
      <div className="flex flex-col gap-3 rounded-xl border border-border/50 bg-muted/20 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold">{t("环境变量配置")}</h3>
          <p className="text-sm leading-6 text-muted-foreground">
            {t(
              "这里保留旧版和外部部署环境变量覆盖；普通用户优先使用前面结构化设置，高风险项只建议排障时临时修改。",
            )}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          className="gap-2 self-start sm:self-auto"
          disabled={!hasCustomizedEnvOverrides || isSaving}
          onClick={onResetAllEnv}
        >
          <RotateCcw className="h-4 w-4" />
          {t("全部恢复默认")}
        </Button>
      </div>

      <div className="grid gap-6 md:grid-cols-[300px_1fr]">
        <Card className="glass-card flex h-[500px] flex-col shadow-sm">
          <CardHeader className="pb-3">
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder={t("搜索变量...")}
                className="h-9 pl-9"
                value={envSearch}
                onChange={(event) => onSearchChange(event.target.value)}
              />
            </div>
          </CardHeader>
          <CardContent className="flex-1 overflow-y-auto p-2">
            <div className="space-y-1">
              {filteredEnvCatalog.map((item) => (
                <Button
                  key={item.key}
                  type="button"
                  variant="ghost"
                  onClick={() => onSelectEnvKey(item.key)}
                  className={cn(
                    "h-auto w-full justify-start rounded-md px-3 py-2 text-left text-sm transition-colors",
                    selectedEnvKey === item.key
                      ? "bg-primary text-primary-foreground"
                      : "hover:bg-accent",
                  )}
                >
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate font-medium">
                      {t(item.label)}
                    </span>
                    <Badge
                      variant="outline"
                      className={cn(
                        "shrink-0 px-1.5 py-0 text-[10px]",
                        selectedEnvKey === item.key
                          ? "border-primary-foreground/30 bg-primary-foreground/10 text-primary-foreground"
                          : riskBadgeClasses[item.riskLevel] ??
                              riskBadgeClasses.medium,
                      )}
                    >
                      {t(riskLabels[item.riskLevel] || riskLabels.medium)}
                    </Badge>
                  </div>
                  <code className="block truncate text-[10px] opacity-70">
                    {item.key}
                  </code>
                </Button>
              ))}
            </div>
          </CardContent>
        </Card>

        <Card className="glass-card min-h-[500px] shadow-sm">
          {selectedEnvKey ? (
            <>
              <CardHeader>
                <div className="flex flex-col gap-2">
                  <CardTitle className="text-lg">
                    {selectedEnvItem ? t(selectedEnvItem.label) : null}
                  </CardTitle>
                  <div className="flex flex-wrap items-center gap-2">
                    <code className="w-fit rounded bg-primary/10 px-2 py-0.5 text-xs text-primary">
                      {selectedEnvKey}
                    </code>
                    <Badge
                      variant="outline"
                      className={cn(
                        "px-2 py-0.5",
                        riskBadgeClasses[selectedEnvRiskLevel] ??
                          riskBadgeClasses.medium,
                      )}
                    >
                      {t(riskLabels[selectedEnvRiskLevel] || riskLabels.medium)}
                    </Badge>
                    <Badge variant="secondary" className="px-2 py-0.5">
                      {t(
                        effectScopeLabels[selectedEnvEffectScope] ||
                          selectedEnvEffectScope,
                      )}
                    </Badge>
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-6">
                <Alert>
                  <Info />
                  <AlertDescription>
                    {descriptionMap[selectedEnvKey]
                      ? t(descriptionMap[selectedEnvKey])
                      : (
                          <>
                            {selectedEnvItem ? t(selectedEnvItem.label) : null}{" "}
                            {t("对应环境变量，修改后会应用到相关模块。")}
                          </>
                        )}
                  </AlertDescription>
                </Alert>
                {selectedEnvRiskLevel === "high" ? (
                  <Alert variant="destructive">
                    <AlertDescription>{t(selectedEnvSafetyNote)}</AlertDescription>
                  </Alert>
                ) : (
                  <Alert>
                    <AlertDescription>{t(selectedEnvSafetyNote)}</AlertDescription>
                  </Alert>
                )}

                <div className="space-y-2">
                  <Label>{t("当前值")}</Label>
                  <Input
                    value={selectedEnvValue}
                    onChange={(event) =>
                      onSelectedEnvValueChange(event.target.value)
                    }
                    className="h-11 font-mono"
                    placeholder={t("输入变量值")}
                  />
                  <p className="text-[10px] text-muted-foreground">
                    {t("默认值:")}{" "}
                    <span className="font-mono italic">
                      {selectedEnvItem?.defaultValue || t("空")}
                    </span>
                  </p>
                </div>

                <Separator />
                <div className="flex gap-3">
                  <Button onClick={onSaveEnv} className="gap-2">
                    <Save className="h-4 w-4" /> {t("保存修改")}
                  </Button>
                  <Button variant="outline" onClick={onResetEnv} className="gap-2">
                    <RotateCcw className="h-4 w-4" /> {t("恢复默认")}
                  </Button>
                </div>
              </CardContent>
            </>
          ) : (
            <CardContent className="flex h-full flex-col items-center justify-center gap-4 text-muted-foreground">
              <div className="rounded-full bg-accent/30 p-4">
                <Variable className="h-12 w-12 opacity-20" />
              </div>
              <p>{t("请从左侧列表选择一个环境变量进行配置")}</p>
              <Button
                type="button"
                variant="outline"
                className="gap-2"
                disabled={!hasCustomizedEnvOverrides || isSaving}
                onClick={onResetAllEnv}
              >
                <RotateCcw className="h-4 w-4" />
                {t("全部恢复默认")}
              </Button>
            </CardContent>
          )}
        </Card>
      </div>
    </>
  );
}
