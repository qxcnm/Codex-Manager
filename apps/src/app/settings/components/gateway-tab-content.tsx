import { FileText, ShieldCheck, Workflow } from "lucide-react";
import { AppSettings } from "@/types";
import { toast } from "sonner";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
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
import { ModelForwardRulesEditor } from "@/app/settings/components/model-forward-rules-editor";
import {
  EMPTY_RESIDENCY_OPTION,
  RESIDENCY_REQUIREMENT_LABELS,
  ROUTE_STRATEGY_LABELS,
  ensureModelForwardRuleRows,
  formatFreeAccountMaxModelLabel,
  resolveFreeAccountMaxModelOptions,
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
  gatewayOriginatorInput,
  gatewayOriginatorDraft,
  setGatewayOriginatorDraft,
  gatewayOriginatorDefault,
  gatewayUserAgentInput,
  gatewayUserAgentDraft,
  setGatewayUserAgentDraft,
  gatewayUserAgentDefault,
  upstreamProxyInput,
  upstreamProxyDraft,
  setUpstreamProxyDraft,
  upstreamProxyBypassInput,
  upstreamProxyBypassDraft,
  setUpstreamProxyBypassDraft,
}: {
  t: (value: string) => string;
  snapshot: AppSettings;
  updateSettings: {
    mutate: (patch: Partial<AppSettings>) => void;
    mutateAsync: (patch: Partial<AppSettings>) => Promise<unknown>;
    isPending: boolean;
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
  gatewayOriginatorInput: string;
  gatewayOriginatorDraft: string | null;
  setGatewayOriginatorDraft: React.Dispatch<React.SetStateAction<string | null>>;
  gatewayOriginatorDefault: string;
  gatewayUserAgentInput: string;
  gatewayUserAgentDraft: string | null;
  setGatewayUserAgentDraft: React.Dispatch<React.SetStateAction<string | null>>;
  gatewayUserAgentDefault: string;
  upstreamProxyInput: string;
  upstreamProxyDraft: string | null;
  setUpstreamProxyDraft: React.Dispatch<React.SetStateAction<string | null>>;
  upstreamProxyBypassInput: string;
  upstreamProxyBypassDraft: string | null;
  setUpstreamProxyBypassDraft: React.Dispatch<React.SetStateAction<string | null>>;
}) {
  const upstreamProxyBypassSavedValue = snapshot.upstreamProxyBypassHosts || "";
  const upstreamProxyBypassDirty =
    upstreamProxyBypassDraft != null && upstreamProxyBypassInput !== upstreamProxyBypassSavedValue;
  const saveUpstreamProxyBypassDraft = () => {
    if (upstreamProxyBypassDraft == null) return;
    if (!upstreamProxyBypassDirty) {
      setUpstreamProxyBypassDraft(null);
      return;
    }
    void updateSettings
      .mutateAsync({ upstreamProxyBypassHosts: upstreamProxyBypassInput })
      .then(() => setUpstreamProxyBypassDraft(null))
      .catch(() => undefined);
  };

  return (
    <Card className="glass-card mission-panel shadow-sm">
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

        <div className="flex flex-col gap-3 border-t pt-6 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <Workflow className="h-4 w-4 text-primary" />
              <Label>{t("线程感知账号分配")}</Label>
            </div>
            <p className="text-[10px] text-muted-foreground">
              {t("开启后未绑定的新线程会优先选择当前承载线程更少的可用账号，已有线程仍保持账号粘性。")}
            </p>
          </div>
          <Switch
            checked={snapshot.threadAwareAccountDistributionEnabled}
            onCheckedChange={(checked) =>
              updateSettings.mutate({
                threadAwareAccountDistributionEnabled: checked,
              })
            }
          />
        </div>

        <div className="flex flex-col gap-3 border-t pt-6 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-primary" />
              <Label>{t("请求内容脱敏")}</Label>
            </div>
            <p className="text-[10px] text-muted-foreground">
              {t("开启后，请求日志里保存的请求内容会把密钥、Token、密码等凭据字段替换为 [REDACTED]；关闭后按原样保存，不做任何脱敏。仅影响之后的新请求。")}
            </p>
          </div>
          <Switch
            checked={snapshot.requestLogPayloadRedactionEnabled}
            onCheckedChange={(checked) =>
              updateSettings.mutate({
                requestLogPayloadRedactionEnabled: checked,
              })
            }
          />
        </div>

        <div className="flex flex-col gap-3 border-t pt-6 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <FileText className="h-4 w-4 text-primary" />
              <Label>{t("请求内容仅保留 16 KB 预览")}</Label>
            </div>
            <p className="text-[10px] text-muted-foreground">
              {t("开启后每个请求只保存前 16 KB，超出部分截断；关闭后完整保存请求内容，同一会话的历史消息只存一份，按需拼回完整请求。仅影响之后的新请求。")}
            </p>
          </div>
          <Switch
            checked={snapshot.requestLogPayloadPreviewEnabled}
            onCheckedChange={(checked) =>
              updateSettings.mutate({
                requestLogPayloadPreviewEnabled: checked,
              })
            }
          />
        </div>

        <div className="grid gap-2 border-t pt-6">
          <Label>{t("Free 账号模型上限")}</Label>
          <Select
            value={snapshot.freeAccountMaxModel || "auto"}
            onValueChange={(value) =>
              updateSettings.mutate({ freeAccountMaxModel: value || "auto" })
            }
          >
            <SelectTrigger
              className="w-full md:w-[300px]"
              aria-label={t("Free 账号模型上限")}
            >
              <SelectValue placeholder={t("选择 Free 账号模型上限")}>
                {(value) => t(formatFreeAccountMaxModelLabel(String(value || "")))}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {resolveFreeAccountMaxModelOptions(
                  snapshot.freeAccountMaxModel,
                  snapshot.freeAccountMaxModelOptions,
                ).map((model) => (
                  <SelectItem key={model} value={model}>
                    {t(formatFreeAccountMaxModelLabel(model))}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <p className="text-[10px] text-muted-foreground">
            {t(
              "设为“不限制”时，Free 账号可参与所有模型请求；选择具体模型后，目录中排在该模型之上的请求会跳过 Free 账号，但不会改写请求模型。混合模式会继续尝试其他账号，账号候选耗尽后仍按现有策略转聚合 API。",
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
                {t("低于保留百分比的账号会从网关路由候选中跳过。")}
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
          <Label>{t("模型转发规则")}</Label>
          <ModelForwardRulesEditor
            rows={modelForwardRuleRows}
            sourcePlaceholder={t("例如：spark*")}
            targetPlaceholder={t("例如：gpt-6-sol")}
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

        <div className="grid gap-2 border-t pt-6">
          <Label htmlFor="gateway-user-agent">{t("全局 User-Agent")}</Label>
          <Input
            id="gateway-user-agent"
            className="h-10 max-w-2xl font-mono"
            value={gatewayUserAgentInput}
            maxLength={512}
            placeholder={gatewayUserAgentDefault}
            onChange={(event) => setGatewayUserAgentDraft(event.target.value)}
            onBlur={() => {
              if (gatewayUserAgentDraft == null) return;
              const nextUserAgent = gatewayUserAgentInput.trim();
              if (nextUserAgent === (snapshot.gatewayUserAgent || "")) {
                setGatewayUserAgentDraft(null);
                return;
              }
              void updateSettings
                .mutateAsync({ gatewayUserAgent: nextUserAgent })
                .then(() => setGatewayUserAgentDraft(null))
                .catch(() => undefined);
            }}
          />
          <p className="text-[10px] text-muted-foreground">
            {t("留空时使用 Codex 默认 User-Agent：")} <code>{gatewayUserAgentDefault}</code>
            {t("。所有网关上游请求默认使用此值；聚合 API 单独设置时优先。清空后失焦即可恢复默认值。")}
          </p>
        </div>

        <div className="grid gap-2">
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
              let finalUrl = upstreamProxyInput.trim();
              if (finalUrl) {
                const lowerUrl = finalUrl.toLowerCase();
                if (lowerUrl.startsWith("socks5://")) {
                  finalUrl = "socks5h://" + finalUrl.slice(9);
                } else if (lowerUrl.startsWith("socks4://")) {
                  toast.warning(t("SOCKS4 是已过时的协议，建议使用 SOCKS5"));
                }
              }
              if (finalUrl === (snapshot.upstreamProxyUrl || "")) {
                setUpstreamProxyDraft(null);
                return;
              }
              void updateSettings
                .mutateAsync({ upstreamProxyUrl: finalUrl })
                .then(() => setUpstreamProxyDraft(null))
                .catch(() => undefined);
            }}
          />
          <p className="text-[10px] text-muted-foreground">{t("支持 http/https/socks5，留空表示直连。")}</p>
        </div>

        <div className="grid gap-2">
          <div className="flex max-w-2xl flex-wrap items-center justify-between gap-2">
            <Label>{t("代理 Bypass 域名")}</Label>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={upstreamProxyBypassDraft == null}
                onClick={() => setUpstreamProxyBypassDraft(null)}
              >
                {t("恢复")}
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={!upstreamProxyBypassDirty || updateSettings.isPending}
                onClick={saveUpstreamProxyBypassDraft}
              >
                {t("保存")}
              </Button>
            </div>
          </div>
          <Textarea
            placeholder={t("留空表示不绕过代理")}
            className="min-h-24 max-w-2xl resize-y font-mono text-sm"
            value={upstreamProxyBypassInput}
            onChange={(event) => setUpstreamProxyBypassDraft(event.target.value)}
            onBlur={saveUpstreamProxyBypassDraft}
          />
          <p className="text-[10px] text-muted-foreground">
            {t("一行一个或用逗号分隔；命中的上游域名会绕过全局代理直连。支持精确域名和")} <code>*.</code>
            {t("通配。")}
          </p>
        </div>

        <div className="grid gap-4 border-t pt-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="space-y-1">
              <Label htmlFor="sse-keepalive-enabled">{t("保持连接心跳")}</Label>
              <p className="text-[10px] text-muted-foreground">
                {t("开启后会按下方间隔向客户端发送 SSE 心跳，防止代理或网络链路因空闲断开。")}
              </p>
            </div>
            <Switch
              id="sse-keepalive-enabled"
              checked={snapshot.sseKeepaliveEnabled}
              onCheckedChange={(checked) =>
                updateSettings.mutate({ sseKeepaliveEnabled: checked })
              }
            />
          </div>

          <div className="grid gap-4 md:grid-cols-3">
            <div className="grid gap-2">
              <Label htmlFor="sse-keepalive-interval-ms">
                {t("SSE 保活间隔 (ms)")}
              </Label>
              <Input
                id="sse-keepalive-interval-ms"
                type="number"
                value={transportInputValues.sseKeepaliveIntervalMs}
                onChange={(event) =>
                  setTransportDraft((current) => ({
                    ...current,
                    sseKeepaliveIntervalMs: event.target.value,
                  }))
                }
                onBlur={() => saveTransportField("sseKeepaliveIntervalMs", 1)}
                disabled={!snapshot.sseKeepaliveEnabled}
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
        </div>
      </CardContent>
    </Card>
  );
}
