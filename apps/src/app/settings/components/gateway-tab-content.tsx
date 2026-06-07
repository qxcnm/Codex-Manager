import { ShieldCheck } from "lucide-react";
import { AppSettings } from "@/types";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
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
import { ModelForwardRulesEditor } from "@/app/settings/components/model-forward-rules-editor";
import {
  DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS,
  EMPTY_RESIDENCY_OPTION,
  RESIDENCY_REQUIREMENT_LABELS,
  ROUTE_STRATEGY_LABELS,
  ensureModelForwardRuleRows,
  formatFreeAccountModelLabel,
} from "@/app/settings/settings-page-helpers";

export function GatewayTabContent({
  t,
  snapshot,
  updateSettings,
  quotaGuardInputValues,
  setQuotaGuardDraft,
  saveQuotaGuardField,
  transportInputValues,
  setTransportDraft,
  saveTransportField,
  modelForwardRuleRows,
  updateModelForwardRuleRows,
  commitModelForwardRulesDraft,
  compactModelForwardRuleRows,
  updateCompactModelForwardRuleRows,
  commitCompactModelForwardRulesDraft,
  gatewayOriginatorInput,
  gatewayOriginatorDraft,
  setGatewayOriginatorDraft,
  gatewayOriginatorDefault,
  upstreamProxyInput,
  upstreamProxyDraft,
  setUpstreamProxyDraft,
}: {
  t: (value: string) => string;
  snapshot: AppSettings;
  updateSettings: {
    mutate: (patch: Partial<AppSettings>) => void;
    mutateAsync: (patch: Partial<AppSettings>) => Promise<unknown>;
  };
  quotaGuardInputValues: {
    primaryMinRemainingPercent: string;
    secondaryMinRemainingPercent: string;
  };
  setQuotaGuardDraft: React.Dispatch<React.SetStateAction<Record<string, string>>>;
  saveQuotaGuardField: (
    key: "primaryMinRemainingPercent" | "secondaryMinRemainingPercent",
  ) => void;
  transportInputValues: {
    sseKeepaliveIntervalMs: string;
    upstreamStreamTimeoutMs: string;
    upstreamTotalTimeoutMs: string;
  };
  setTransportDraft: React.Dispatch<React.SetStateAction<
    Partial<Record<"sseKeepaliveIntervalMs" | "upstreamStreamTimeoutMs" | "upstreamTotalTimeoutMs", string>>
  >>;
  saveTransportField: (
    key: "sseKeepaliveIntervalMs" | "upstreamStreamTimeoutMs" | "upstreamTotalTimeoutMs",
    minimum: number,
  ) => void;
  modelForwardRuleRows: Array<{ pattern: string; target: string }>;
  updateModelForwardRuleRows: (
    updater: (rows: Array<{ pattern: string; target: string }>) => Array<{ pattern: string; target: string }>,
  ) => void;
  commitModelForwardRulesDraft: () => void;
  compactModelForwardRuleRows: Array<{ pattern: string; target: string }>;
  updateCompactModelForwardRuleRows: (
    updater: (rows: Array<{ pattern: string; target: string }>) => Array<{ pattern: string; target: string }>,
  ) => void;
  commitCompactModelForwardRulesDraft: () => void;
  gatewayOriginatorInput: string;
  gatewayOriginatorDraft: string | null;
  setGatewayOriginatorDraft: React.Dispatch<React.SetStateAction<string | null>>;
  gatewayOriginatorDefault: string;
  upstreamProxyInput: string;
  upstreamProxyDraft: string | null;
  setUpstreamProxyDraft: React.Dispatch<React.SetStateAction<string | null>>;
}) {
  return (
    <Card className="glass-card shadow-sm">
      <CardHeader>
        <CardTitle className="text-base">{t("网关策略")}</CardTitle>
        <CardDescription>{t("配置账号选路和请求头处理方式")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        <div className="grid gap-2">
          <Label>{t("账号选路策略")}</Label>
          <Select
            value={snapshot.routeStrategy || "ordered"}
            onValueChange={(value) =>
              updateSettings.mutate({ routeStrategy: value || "ordered" })
            }
          >
            <SelectTrigger className="w-full md:w-[300px]">
              <SelectValue placeholder={t("选择策略")}>
                {(value) => {
                  const nextValue = String(value || "").trim();
                  if (!nextValue) return t("选择策略");
                  return t(ROUTE_STRATEGY_LABELS[nextValue] || nextValue);
                }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="ordered">{t("顺序优先 (Ordered)")}</SelectItem>
                <SelectItem value="balanced">{t("均衡轮询 (Balanced)")}</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
          <p className="text-[10px] text-muted-foreground">
            {t(
              "顺序优先：按账号候选顺序优先尝试，默认只会在头部小窗口内按健康度做轻微换头；均衡轮询：按“平台密钥 + 模型”维度严格轮询可用账号，默认不做健康度换头。",
            )}
          </p>
        </div>

        <div className="grid gap-4 border-t pt-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <ShieldCheck className="h-4 w-4 text-primary" />
                <Label>{t("额度保护")}</Label>
              </div>
              <p className="text-[10px] text-muted-foreground">
                {t("低于保留百分比的账号会从网关路由和远端模型刷新候选中跳过。")}
              </p>
            </div>
            <Switch
              checked={snapshot.quotaGuard.enabled}
              onCheckedChange={(checked) =>
                updateSettings.mutate({
                  quotaGuard: {
                    ...snapshot.quotaGuard,
                    enabled: checked,
                  },
                })
              }
            />
          </div>

          <div className="grid gap-4 md:grid-cols-3">
            <div className="grid gap-2">
              <Label>{t("5 小时窗口保留 (%)")}</Label>
              <Input
                type="number"
                min={0}
                max={100}
                value={quotaGuardInputValues.primaryMinRemainingPercent}
                onChange={(event) =>
                  setQuotaGuardDraft((current) => ({
                    ...current,
                    primaryMinRemainingPercent: event.target.value,
                  }))
                }
                onBlur={() => saveQuotaGuardField("primaryMinRemainingPercent")}
                disabled={!snapshot.quotaGuard.enabled}
              />
            </div>
            <div className="grid gap-2">
              <Label>{t("周窗口保留 (%)")}</Label>
              <Input
                type="number"
                min={0}
                max={100}
                value={quotaGuardInputValues.secondaryMinRemainingPercent}
                onChange={(event) =>
                  setQuotaGuardDraft((current) => ({
                    ...current,
                    secondaryMinRemainingPercent: event.target.value,
                  }))
                }
                onBlur={() => saveQuotaGuardField("secondaryMinRemainingPercent")}
                disabled={!snapshot.quotaGuard.enabled}
              />
            </div>
            <div className="flex items-center justify-between gap-3 rounded-md border border-border/60 px-3 py-2">
              <div className="space-y-1">
                <Label>{t("全部低额度时兜底")}</Label>
                <p className="text-[10px] text-muted-foreground">
                  {t("关闭后如果所有账号都低于阈值，网关会返回无可用账号。")}
                </p>
              </div>
              <Switch
                checked={snapshot.quotaGuard.allowAllLowQuotaFallback}
                onCheckedChange={(checked) =>
                  updateSettings.mutate({
                    quotaGuard: {
                      ...snapshot.quotaGuard,
                      allowAllLowQuotaFallback: checked,
                    },
                  })
                }
                disabled={!snapshot.quotaGuard.enabled}
              />
            </div>
          </div>
        </div>

        <div className="grid gap-2">
          <Label>{t("Free 账号使用模型")}</Label>
          <Select
            value={snapshot.freeAccountMaxModel || "auto"}
            onValueChange={(value) =>
              updateSettings.mutate({
                freeAccountMaxModel: value || "auto",
              })
            }
          >
            <SelectTrigger className="w-full md:w-[300px]">
              <SelectValue placeholder={t("选择 free 账号使用模型")}>
                {(value) => t(formatFreeAccountModelLabel(String(value || "")))}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {(snapshot.freeAccountMaxModelOptions?.length
                  ? snapshot.freeAccountMaxModelOptions
                  : DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
                ).map((model) => (
                  <SelectItem key={model} value={model}>
                    {t(formatFreeAccountModelLabel(model))}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <p className="text-[10px] text-muted-foreground">
            {t(
              "设为“跟随请求”时，不会额外改写 free / 7天单窗口账号的模型；只有你选了具体模型后，命中这些账号时才会统一改写为该模型。",
            )}
          </p>
        </div>

        <div className="grid gap-2">
          <Label>{t("模型转发规则")}</Label>
          <ModelForwardRulesEditor
            rows={modelForwardRuleRows}
            sourcePlaceholder={t("例如：spark*")}
            targetPlaceholder={t("例如：gpt-5.4-openai-compact")}
            sourceLabel={t("源模型")}
            targetLabel={t("目标模型")}
            addButtonLabel={t("新增规则")}
            deleteButtonLabel={t("删除条目")}
            onRowsChange={(updater) =>
              updateModelForwardRuleRows((rows) => ensureModelForwardRuleRows(updater(rows)))
            }
            onCommit={commitModelForwardRulesDraft}
          />
          <p className="text-[10px] text-muted-foreground">
            {t("左边匹配请求模型，右边填写转发目标；支持")} <code>*</code>{" "}
            {t("通配。平台 Key 没有强绑模型时，会先按这里把请求模型改写，再进入账号路由。")}
          </p>
        </div>

        <div className="grid gap-2">
          <Label>{t("压缩模型转发规则")}</Label>
          <ModelForwardRulesEditor
            rows={compactModelForwardRuleRows}
            sourcePlaceholder={t("例如：gpt-5.4")}
            targetPlaceholder={t("例如：gpt-5.4-openai-compact")}
            sourceLabel={t("源模型")}
            targetLabel={t("目标模型")}
            addButtonLabel={t("新增规则")}
            deleteButtonLabel={t("删除条目")}
            onRowsChange={(updater) =>
              updateCompactModelForwardRuleRows((rows) => ensureModelForwardRuleRows(updater(rows)))
            }
            onCommit={commitCompactModelForwardRulesDraft}
          />
          <p className="text-[10px] text-muted-foreground">
            {t("仅对 /v1/responses/compact 生效；命中后会在 compact 请求里优先改写模型。")}
          </p>
        </div>

        <div className="grid gap-2 border-t pt-6">
          <Label>{t("上游 Originator")}</Label>
          <Input
            className="h-10 max-w-md font-mono"
            value={gatewayOriginatorInput}
            onChange={(event) => setGatewayOriginatorDraft(event.target.value)}
            onBlur={() => {
              if (gatewayOriginatorDraft == null) return;
              if (gatewayOriginatorInput === (snapshot.gatewayOriginator || gatewayOriginatorDefault)) {
                setGatewayOriginatorDraft(null);
                return;
              }
              void updateSettings
                .mutateAsync({ gatewayOriginator: gatewayOriginatorInput })
                .then(() => setGatewayOriginatorDraft(null))
                .catch(() => undefined);
            }}
          />
          <p className="text-[10px] text-muted-foreground">
            {t("对齐官方 Codex 的上游 Originator。默认值为")} <code>{gatewayOriginatorDefault}</code>
            {t("，会同步影响登录和网关上游请求头。")}
          </p>
        </div>

        <div className="grid gap-2">
          <Label>{t("区域驻留要求")}</Label>
          <Select
            value={(snapshot.gatewayResidencyRequirement ?? "") || EMPTY_RESIDENCY_OPTION}
            onValueChange={(value) =>
              updateSettings.mutate({
                gatewayResidencyRequirement: value === EMPTY_RESIDENCY_OPTION ? "" : (value ?? ""),
              })
            }
          >
            <SelectTrigger className="w-full md:w-[300px]">
              <SelectValue placeholder={t("选择地域约束")}>
                {(value) => {
                  const nextValue = String(value || "") === EMPTY_RESIDENCY_OPTION ? "" : String(value || "");
                  return t(RESIDENCY_REQUIREMENT_LABELS[nextValue] || nextValue);
                }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {(snapshot.gatewayResidencyRequirementOptions?.length ? snapshot.gatewayResidencyRequirementOptions : ["", "us"]).map((value) => (
                  <SelectItem key={value || EMPTY_RESIDENCY_OPTION} value={value || EMPTY_RESIDENCY_OPTION}>
                    {t(RESIDENCY_REQUIREMENT_LABELS[value] || value)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <p className="text-[10px] text-muted-foreground">
            {t("对齐官方 Codex 的")} <code>x-openai-internal-codex-residency</code>
            {t("头。")} {t("当前只支持留空或")} <code>us</code>
            {t("。")} 
          </p>
        </div>

        <div className="grid gap-2 pt-2">
          <Label>{t("上游代理 (Proxy)")}</Label>
          <Input
            placeholder="http://127.0.0.1:7890"
            className="h-10 max-w-md font-mono"
            value={upstreamProxyInput}
            onChange={(event) => setUpstreamProxyDraft(event.target.value)}
            onBlur={() => {
              if (upstreamProxyDraft == null) return;
              if (upstreamProxyInput === (snapshot.upstreamProxyUrl || "")) {
                setUpstreamProxyDraft(null);
                return;
              }
              void updateSettings
                .mutateAsync({ upstreamProxyUrl: upstreamProxyInput })
                .then(() => setUpstreamProxyDraft(null))
                .catch(() => undefined);
            }}
          />
          <p className="text-[10px] text-muted-foreground">{t("支持 http/https/socks5，留空表示直连。")}</p>
        </div>

        <div className="grid gap-4 border-t pt-6 md:grid-cols-3">
          <div className="grid gap-2">
            <Label>{t("SSE 保活间隔 (ms)")}</Label>
            <Input
              type="number"
              value={transportInputValues.sseKeepaliveIntervalMs}
              onChange={(event) =>
                setTransportDraft((current) => ({
                  ...current,
                  sseKeepaliveIntervalMs: event.target.value,
                }))
              }
              onBlur={() => saveTransportField("sseKeepaliveIntervalMs", 1)}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("上游总超时 (ms，0 为关闭)")}</Label>
            <Input
              type="number"
              value={transportInputValues.upstreamTotalTimeoutMs}
              onChange={(event) =>
                setTransportDraft((current) => ({
                  ...current,
                  upstreamTotalTimeoutMs: event.target.value,
                }))
              }
              onBlur={() => saveTransportField("upstreamTotalTimeoutMs", 0)}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("上游流式空闲超时 (ms)")}</Label>
            <Input
              type="number"
              value={transportInputValues.upstreamStreamTimeoutMs}
              onChange={(event) =>
                setTransportDraft((current) => ({
                  ...current,
                  upstreamStreamTimeoutMs: event.target.value,
                }))
              }
              onBlur={() => saveTransportField("upstreamStreamTimeoutMs", 0)}
            />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
