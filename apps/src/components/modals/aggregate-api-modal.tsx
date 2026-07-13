"use client";

import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Clipboard, Database, Info, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { accountClient } from "@/lib/api/account-client";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { useAppStore } from "@/lib/store/useAppStore";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { AggregateApi } from "@/types";

const AGGREGATE_API_PROVIDER_LABELS: Record<string, string> = {
  codex: "Codex",
  claude: "Claude",
};

const AGGREGATE_API_URL_PLACEHOLDERS: Record<string, string> = {
  codex: "例如：https://api.openai.com/v1/responses",
  claude: "例如：https://api.anthropic.com/v1/messages",
};

type BalanceQueryTemplate = "generic" | "new_api" | "custom";
type BalanceCustomAuth = "provider_bearer" | "balance_bearer" | "none";

interface BalanceCustomConfig {
  path?: unknown;
  auth?: unknown;
  remainingPath?: unknown;
  unit?: unknown;
  multiplier?: unknown;
  totalPath?: unknown;
  usedPath?: unknown;
  planPath?: unknown;
  validPath?: unknown;
  invalidMessagePath?: unknown;
}

const parseBalanceCustomConfig = (
  value: string | null | undefined
): BalanceCustomConfig => {
  if (!value) return {};
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object"
      ? (parsed as BalanceCustomConfig)
      : {};
  } catch {
    return {};
  }
};

const stringConfigValue = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

const parseModelSlugs = (value: string) =>
  value
    .split(/[,，\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);

const normalizeBalanceCustomAuth = (value: unknown): BalanceCustomAuth =>
  value === "balance_bearer" || value === "none"
    ? value
    : "provider_bearer";

interface AggregateApiModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  aggregateApi?: AggregateApi | null;
  defaultSort?: number;
}

/**
 * 函数 `AggregateApiModal`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
export function AggregateApiModal({
  open,
  onOpenChange,
  aggregateApi,
  defaultSort = 0,
}: AggregateApiModalProps) {
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const [providerType, setProviderType] = useState("codex");
  const [supplierName, setSupplierName] = useState("");
  const [sortDraft, setSortDraft] = useState("0");
  const [url, setUrl] = useState("");
  const [authType, setAuthType] = useState<"apikey" | "userpass">("apikey");
  const [authCustomEnabled, setAuthCustomEnabled] = useState(false);
  const [apiKeyLocation, setApiKeyLocation] = useState<"header" | "query">(
    "header"
  );
  const [apiKeyName, setApiKeyName] = useState("authorization");
  const [apiKeyHeaderValueFormat, setApiKeyHeaderValueFormat] = useState<
    "bearer" | "raw"
  >("bearer");
  const [userpassMode, setUserpassMode] = useState<
    "basic" | "headerPair" | "queryPair"
  >("basic");
  const [userpassUsernameName, setUserpassUsernameName] = useState("username");
  const [userpassPasswordName, setUserpassPasswordName] = useState("password");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [actionCustomEnabled, setActionCustomEnabled] = useState(false);
  const [action, setAction] = useState("");
  const [modelOverride, setModelOverride] = useState("");
  const [modelWhitelist, setModelWhitelist] = useState("");
  const [balanceQueryEnabled, setBalanceQueryEnabled] = useState(false);
  const [balanceQueryTemplate, setBalanceQueryTemplate] =
    useState<BalanceQueryTemplate>("generic");
  const [balanceQueryBaseUrl, setBalanceQueryBaseUrl] = useState("");
  const [balanceQueryAccessToken, setBalanceQueryAccessToken] = useState("");
  const [balanceQueryUserId, setBalanceQueryUserId] = useState("");
  const [balanceCustomPath, setBalanceCustomPath] = useState("/v1/usage");
  const [balanceCustomAuth, setBalanceCustomAuth] =
    useState<BalanceCustomAuth>("provider_bearer");
  const [balanceCustomRemainingPath, setBalanceCustomRemainingPath] =
    useState("remaining");
  const [balanceCustomUnit, setBalanceCustomUnit] = useState("USD");
  const [balanceCustomMultiplier, setBalanceCustomMultiplier] = useState("1");
  const [balanceCustomTotalPath, setBalanceCustomTotalPath] = useState("");
  const [balanceCustomUsedPath, setBalanceCustomUsedPath] = useState("");
  const [balanceCustomPlanPath, setBalanceCustomPlanPath] = useState("");
  const [balanceCustomValidPath, setBalanceCustomValidPath] = useState("");
  const [
    balanceCustomInvalidMessagePath,
    setBalanceCustomInvalidMessagePath,
  ] = useState("");
  const [key, setKey] = useState("");
  const [generatedKey, setGeneratedKey] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const queryClient = useQueryClient();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const unavailableMessage = canAccessManagementRpc
    ? t("服务未连接，聚合 API 暂不可编辑；连接恢复后可继续操作。")
    : t("当前运行环境暂不支持聚合 API 管理。");

  useEffect(() => {
    if (!open) return;
    const nextProviderType = aggregateApi?.providerType || "codex";
    setProviderType(nextProviderType);
    setSupplierName(aggregateApi?.supplierName || "");
    setSortDraft(String(aggregateApi?.sort ?? defaultSort));
    setUrl(aggregateApi?.url || "");
    const nextAuthType =
      aggregateApi?.authType === "userpass" ? "userpass" : "apikey";
    setAuthType(nextAuthType);
    const authParams =
      aggregateApi?.authParams && typeof aggregateApi.authParams === "object"
        ? aggregateApi.authParams
        : null;
    setAuthCustomEnabled(Boolean(authParams));
    if (nextAuthType === "apikey") {
      const location =
        authParams && authParams["location"] === "query" ? "query" : "header";
      setApiKeyLocation(location);
      const name =
        authParams && typeof authParams["name"] === "string"
          ? String(authParams["name"])
          : location === "query"
            ? "api_key"
            : "authorization";
      setApiKeyName(name);
      const format =
        authParams && typeof authParams["headerValueFormat"] === "string"
          ? String(authParams["headerValueFormat"]).toLowerCase()
          : "bearer";
      setApiKeyHeaderValueFormat(format === "raw" ? "raw" : "bearer");
    } else {
      const mode =
        authParams && typeof authParams["mode"] === "string"
          ? String(authParams["mode"])
          : "basic";
      setUserpassMode(
        mode === "headerPair" || mode === "queryPair" ? mode : "basic"
      );
      setUserpassUsernameName(
        authParams && typeof authParams["usernameName"] === "string"
          ? String(authParams["usernameName"])
          : "username"
      );
      setUserpassPasswordName(
        authParams && typeof authParams["passwordName"] === "string"
          ? String(authParams["passwordName"])
          : "password"
      );
    }
    const nextAction = aggregateApi?.action ?? "";
    setAction(nextAction);
    setActionCustomEnabled(aggregateApi?.action !== null && aggregateApi?.action !== undefined);
    setModelOverride(aggregateApi?.modelOverride || "");
    setModelWhitelist((aggregateApi?.modelSlugs || []).join(", "));
    setBalanceQueryEnabled(Boolean(aggregateApi?.balanceQueryEnabled));
    const nextBalanceQueryTemplate =
      aggregateApi?.balanceQueryTemplate === "new_api"
        ? "new_api"
        : aggregateApi?.balanceQueryTemplate === "custom"
          ? "custom"
          : "generic";
    setBalanceQueryTemplate(nextBalanceQueryTemplate);
    setBalanceQueryBaseUrl(aggregateApi?.balanceQueryBaseUrl || "");
    setBalanceQueryAccessToken("");
    setBalanceQueryUserId(aggregateApi?.balanceQueryUserId || "");
    const customConfig = parseBalanceCustomConfig(
      aggregateApi?.balanceQueryConfigJson
    );
    setBalanceCustomPath(stringConfigValue(customConfig.path, "/v1/usage"));
    setBalanceCustomAuth(normalizeBalanceCustomAuth(customConfig.auth));
    setBalanceCustomRemainingPath(
      stringConfigValue(customConfig.remainingPath, "remaining")
    );
    setBalanceCustomUnit(stringConfigValue(customConfig.unit, "USD"));
    setBalanceCustomMultiplier(
      typeof customConfig.multiplier === "number" &&
        Number.isFinite(customConfig.multiplier)
        ? String(customConfig.multiplier)
        : "1"
    );
    setBalanceCustomTotalPath(stringConfigValue(customConfig.totalPath));
    setBalanceCustomUsedPath(stringConfigValue(customConfig.usedPath));
    setBalanceCustomPlanPath(stringConfigValue(customConfig.planPath));
    setBalanceCustomValidPath(stringConfigValue(customConfig.validPath));
    setBalanceCustomInvalidMessagePath(
      stringConfigValue(customConfig.invalidMessagePath)
    );
    setKey("");
    setUsername("");
    setPassword("");
    setGeneratedKey("");
  }, [aggregateApi, defaultSort, open]);

  /**
   * 函数 `handleSave`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleSave = async () => {
    if (!isServiceReady) {
      toast.info(
        canAccessManagementRpc
          ? t("服务未连接，暂时无法保存聚合 API")
          : t("当前运行环境暂不支持聚合 API 管理")
      );
      return;
    }
    if (!url.trim()) {
      toast.error(t("请输入聚合 API URL"));
      return;
    }
    if (!supplierName.trim()) {
      toast.error(t("请输入供应商名称"));
      return;
    }
    const rawSort = sortDraft.trim();
    if (!rawSort) {
      toast.error(t("请输入顺序值"));
      return;
    }
    const parsedSort = Number(rawSort);
    if (!Number.isFinite(parsedSort)) {
      toast.error(t("顺序必须是数字"));
      return;
    }
    if (!aggregateApi?.id && !key.trim()) {
      if (authType === "apikey") {
        toast.error(t("请输入聚合 API 密钥"));
        return;
      }
    }
    if (!aggregateApi?.id && authType === "userpass") {
      if (!username.trim() || !password.trim()) {
        toast.error(t("请输入账号密码"));
        return;
      }
    }
    if (authType === "userpass" && (username.trim() || password.trim())) {
      if (!username.trim() || !password.trim()) {
        toast.error(t("账号和密码必须同时填写"));
        return;
      }
    }
    if (aggregateApi?.id && aggregateApi.authType !== authType) {
      if (authType === "apikey" && !key.trim()) {
        toast.error(t("切换为 APIKey 认证时必须填写密钥"));
        return;
      }
      if (authType === "userpass" && (!username.trim() || !password.trim())) {
        toast.error(t("切换为账号密码认证时必须填写账号密码"));
        return;
      }
    }

    const authParams =
      authCustomEnabled && authType === "apikey"
        ? {
            location: apiKeyLocation,
            name: apiKeyName.trim(),
            headerValueFormat:
              apiKeyLocation === "header" ? apiKeyHeaderValueFormat : undefined,
          }
        : authCustomEnabled && authType === "userpass"
          ? {
              mode: userpassMode,
              usernameName:
                userpassMode === "headerPair" || userpassMode === "queryPair"
                  ? userpassUsernameName.trim()
                  : undefined,
              passwordName:
                userpassMode === "headerPair" || userpassMode === "queryPair"
                  ? userpassPasswordName.trim()
                  : undefined,
            }
          : null;
    if (authCustomEnabled) {
      if (authType === "apikey") {
        if (!apiKeyName.trim()) {
          toast.error(t("请输入认证参数名称"));
          return;
        }
      } else if (userpassMode !== "basic") {
        if (!userpassUsernameName.trim() || !userpassPasswordName.trim()) {
          toast.error(t("请输入账号密码参数名称"));
          return;
        }
      }
    }
    let balanceQueryConfigJson: string | null = null;
    if (balanceQueryTemplate === "custom") {
      const customPath = balanceCustomPath.trim();
      const remainingPath = balanceCustomRemainingPath.trim();
      const multiplierText = balanceCustomMultiplier.trim() || "1";
      const multiplier = Number(multiplierText);
      if (!customPath) {
        toast.error(t("请输入自定义余额查询路径"));
        return;
      }
      if (!remainingPath) {
        toast.error(t("请输入余额字段路径"));
        return;
      }
      if (!Number.isFinite(multiplier) || multiplier <= 0) {
        toast.error(t("余额倍率必须大于 0"));
        return;
      }
      const config: Record<string, string | number> = {
        method: "GET",
        path: customPath,
        auth: balanceCustomAuth,
        remainingPath,
        unit: balanceCustomUnit.trim() || "USD",
        multiplier,
      };
      const totalPath = balanceCustomTotalPath.trim();
      const usedPath = balanceCustomUsedPath.trim();
      const planPath = balanceCustomPlanPath.trim();
      const validPath = balanceCustomValidPath.trim();
      const invalidMessagePath = balanceCustomInvalidMessagePath.trim();
      if (totalPath) config.totalPath = totalPath;
      if (usedPath) config.usedPath = usedPath;
      if (planPath) config.planPath = planPath;
      if (validPath) config.validPath = validPath;
      if (invalidMessagePath) {
        config.invalidMessagePath = invalidMessagePath;
      }
      balanceQueryConfigJson = JSON.stringify(config);
    }
    setIsLoading(true);
    try {
      const modelSlugs = parseModelSlugs(modelWhitelist);
      if (aggregateApi?.id) {
        await accountClient.updateAggregateApi(aggregateApi.id, {
          providerType,
          supplierName,
          sort: parsedSort,
          url,
          key: authType === "apikey" ? key || null : null,
          authType,
          authCustomEnabled,
          authParams,
          actionCustomEnabled,
          action: actionCustomEnabled ? action.trim() : null,
          modelOverride: modelOverride.trim(),
          username: authType === "userpass" ? username.trim() || null : null,
          password: authType === "userpass" ? password.trim() || null : null,
          balanceQueryEnabled,
          balanceQueryTemplate,
          balanceQueryBaseUrl: balanceQueryBaseUrl.trim(),
          balanceQueryAccessToken: balanceQueryAccessToken.trim() || null,
          balanceQueryUserId: balanceQueryUserId.trim(),
          balanceQueryConfigJson,
          modelSlugs,
        });
        toast.success(t("聚合 API 已更新"));
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
          queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
          queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
          queryClient.invalidateQueries({ queryKey: ["quota"] }),
        ]);
        onOpenChange(false);
        return;
      }

      const result = await accountClient.createAggregateApi({
        providerType,
        supplierName,
        sort: parsedSort,
        url,
        key: authType === "apikey" ? key : null,
        authType,
        authCustomEnabled,
        authParams,
        actionCustomEnabled,
        action: actionCustomEnabled ? action.trim() : null,
        modelOverride: modelOverride.trim(),
        username: authType === "userpass" ? username.trim() : null,
        password: authType === "userpass" ? password.trim() : null,
        balanceQueryEnabled,
        balanceQueryTemplate,
        balanceQueryBaseUrl: balanceQueryBaseUrl.trim(),
        balanceQueryAccessToken: balanceQueryAccessToken.trim() || null,
        balanceQueryUserId: balanceQueryUserId.trim(),
        balanceQueryConfigJson,
        modelSlugs,
      });
      setGeneratedKey(result.key);
      toast.success(t("聚合 API 已创建"));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: ["quota"] }),
      ]);
      onOpenChange(false);
    } catch (error: unknown) {
      toast.error(
        `${t("操作失败")}: ${error instanceof Error ? error.message : String(error)}`
      );
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `copyKey`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const copyKey = async () => {
    try {
      await copyTextToClipboard(generatedKey);
      toast.success(t("密钥已复制"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card w-[calc(100%-2rem)] max-w-[calc(100%-2rem)] overflow-hidden p-0 sm:max-w-[92vw] md:max-w-[640px] lg:max-w-[720px] xl:max-w-[760px]">
        <div className="flex max-h-[92vh] flex-col">
          <div className="border-b border-border/50 px-5 pt-5 pb-3">
            <DialogHeader>
              <div className="mb-2 flex items-center gap-3">
                <div className="rounded-full bg-primary/10 p-2">
                  <Database className="h-5 w-5 text-primary" />
                </div>
                <DialogTitle>
                  {aggregateApi?.id ? t("编辑聚合 API") : t("创建聚合 API")}
                </DialogTitle>
              </div>
              <DialogDescription>
                {t("配置一个最小转发上游，保存 URL 和密钥后即可用于平台密钥轮转。")}
              </DialogDescription>
            </DialogHeader>
          </div>

          <div className="overflow-y-auto px-5 py-3">
            <div className="grid gap-4">
              {!isServiceReady ? (
                <Alert>
                  <Info />
                  <AlertDescription>{unavailableMessage}</AlertDescription>
                </Alert>
              ) : null}

              <div className="grid gap-4 md:grid-cols-2">
                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-supplier-name">{t("供应商名称 *")}</Label>
                  <Input
                    id="aggregate-api-supplier-name"
                    placeholder={t("例如：官方中转、XX 供应商")}
                    value={supplierName}
                    disabled={!isServiceReady}
                    onChange={(event) => setSupplierName(event.target.value)}
                  />
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-sort">{t("顺序值")}</Label>
                  <Input
                    id="aggregate-api-sort"
                    type="number"
                    min={0}
                    step={1}
                    value={sortDraft}
                    disabled={!isServiceReady}
                    onChange={(event) => setSortDraft(event.target.value)}
                  />
                  <p className="text-[11px] leading-4 text-muted-foreground">
                    {t("值越小越靠前，用于聚合 API 轮转优先级")}
                  </p>
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-provider">{t("类型")}</Label>
                  <Select
                    value={providerType}
                    disabled={!isServiceReady}
                    onValueChange={(value) => {
                      if (!value) return;
                      setProviderType(value);
                    }}
                  >
                    <SelectTrigger id="aggregate-api-provider" className="w-full">
                      <SelectValue>
                        {(value) =>
                          AGGREGATE_API_PROVIDER_LABELS[String(value || "")] ||
                          "Codex"
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                    <SelectGroup>
                      <SelectItem value="codex">Codex</SelectItem>
                      <SelectItem value="claude">Claude</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>

                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-auth-type">{t("认证类型")}</Label>
                  <Select
                    value={authType}
                    disabled={!isServiceReady}
                    onValueChange={(value) => {
                      const next = value === "userpass" ? "userpass" : "apikey";
                      setAuthType(next);
                      setGeneratedKey("");
                      setKey("");
                      setUsername("");
                      setPassword("");
                    }}
                  >
                    <SelectTrigger
                      id="aggregate-api-auth-type"
                      className="w-full"
                    >
                      <SelectValue>
                        {(value) =>
                          String(value || "") === "userpass"
                            ? t("账号密码")
                            : "APIKey"
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                    <SelectGroup>
                      <SelectItem value="apikey">APIKey</SelectItem>
                      <SelectItem value="userpass">{t("账号密码")}</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div className="grid gap-2">
                <Label htmlFor="aggregate-api-url">{t("URL")}</Label>
                <Input
                  id="aggregate-api-url"
                  placeholder={
                    t(AGGREGATE_API_URL_PLACEHOLDERS[providerType] || "请输入 URL")
                  }
                  value={url}
                  disabled={!isServiceReady}
                  onChange={(event) => setUrl(event.target.value)}
                />
              </div>

              <div className="grid gap-2">
                <Label htmlFor="aggregate-api-model-override">
                  {t("上游模型覆盖")}
                </Label>
                <Input
                  id="aggregate-api-model-override"
                  placeholder={
                    providerType === "claude"
                      ? "qwen3.5-plus"
                      : t("留空则使用请求中的模型")
                  }
                  value={modelOverride}
                  disabled={!isServiceReady}
                  onChange={(event) => setModelOverride(event.target.value)}
                />
                <p className="text-[11px] leading-4 text-muted-foreground">
                  {t("用于 API 转发服务要求固定模型名的场景；连接测试和真实转发都会使用该模型。")}
                </p>
              </div>

              <div className="grid gap-2">
                <Label htmlFor="aggregate-api-model-whitelist">
                  {t("额度模型白名单")}
                </Label>
                <Input
                  id="aggregate-api-model-whitelist"
                  placeholder="gpt-5.4, gpt-5.4-mini"
                  value={modelWhitelist}
                  disabled={!isServiceReady}
                  onChange={(event) => setModelWhitelist(event.target.value)}
                />
                <p className="text-[11px] leading-4 text-muted-foreground">
                  {t("仅用于额度池归属统计；留空表示该来源对所有 API 可用模型生效。")}
                </p>
              </div>

              {authType === "apikey" ? (
                <div className="grid gap-2">
                  <Label htmlFor="aggregate-api-key">{t("密钥")}</Label>
                  <Input
                    id="aggregate-api-key"
                    type="password"
                    placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入密钥")}
                    value={key}
                    disabled={!isServiceReady}
                    onChange={(event) => setKey(event.target.value)}
                  />
                </div>
              ) : (
                <div className="grid gap-4 lg:grid-cols-2">
                  <div className="grid gap-2">
                    <Label htmlFor="aggregate-api-username">{t("账号")}</Label>
                    <Input
                      id="aggregate-api-username"
                      placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入账号")}
                      value={username}
                      disabled={!isServiceReady}
                      onChange={(event) => setUsername(event.target.value)}
                    />
                  </div>
                  <div className="grid gap-2">
                    <Label htmlFor="aggregate-api-password">{t("密码")}</Label>
                    <Input
                      id="aggregate-api-password"
                      type="password"
                      placeholder={aggregateApi?.id ? t("留空则保持原值") : t("请输入密码")}
                      value={password}
                      disabled={!isServiceReady}
                      onChange={(event) => setPassword(event.target.value)}
                    />
                  </div>
                </div>
              )}

              <div className="grid gap-4 xl:grid-cols-2">
                <Card size="sm">
                  <CardContent className="grid gap-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <Label className="text-sm">{t("自定义认证参数")}</Label>
                      <p className="text-[11px] text-muted-foreground">
                        {t("关闭则按默认规则注入认证（APIKey=Bearer，账号密码=Basic）。")}
                      </p>
                    </div>
                    <Switch
                      checked={authCustomEnabled}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) =>
                        setAuthCustomEnabled(Boolean(checked))
                      }
                    />
                  </div>

                  {authCustomEnabled && authType === "apikey" ? (
                    <div className="grid gap-3">
                      <div className="grid gap-3 md:grid-cols-2">
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("位置")}</Label>
                          <Select
                            value={apiKeyLocation}
                            onValueChange={(value) =>
                              setApiKeyLocation(
                                value === "query" ? "query" : "header"
                              )
                            }
                            disabled={!isServiceReady}
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue>
                                {(value) =>
                                  String(value || "") === "query"
                                    ? "Query"
                                    : "Header"
                                }
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                    <SelectGroup>
                              <SelectItem value="header">Header</SelectItem>
                              <SelectItem value="query">Query</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("参数名")}</Label>
                          <Input
                            value={apiKeyName}
                            disabled={!isServiceReady}
                            placeholder={
                              apiKeyLocation === "query"
                                ? "api_key"
                                : "authorization"
                            }
                            onChange={(e) => setApiKeyName(e.target.value)}
                          />
                        </div>
                      </div>
                      {apiKeyLocation === "header" ? (
                        <div className="grid gap-2">
                          <Label className="text-xs">{t("Header 格式")}</Label>
                          <Select
                            value={apiKeyHeaderValueFormat}
                            onValueChange={(value) =>
                              setApiKeyHeaderValueFormat(
                                value === "raw" ? "raw" : "bearer"
                              )
                            }
                            disabled={!isServiceReady}
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue>
                                {(value) =>
                                  String(value || "") === "raw"
                                    ? "Raw"
                                    : "Bearer"
                                }
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                    <SelectGroup>
                              <SelectItem value="bearer">Bearer</SelectItem>
                              <SelectItem value="raw">Raw</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>
                      ) : null}
                    </div>
                  ) : null}

                  {authCustomEnabled && authType === "userpass" ? (
                    <div className="grid gap-3">
                      <div className="grid gap-2">
                        <Label className="text-xs">{t("发送模式")}</Label>
                        <Select
                          value={userpassMode}
                          onValueChange={(value) => {
                            const next =
                              value === "headerPair" || value === "queryPair"
                                ? value
                                : "basic";
                            setUserpassMode(next);
                          }}
                          disabled={!isServiceReady}
                        >
                          <SelectTrigger className="w-full">
                            <SelectValue>
                              {(value) => {
                                const v = String(value || "");
                                if (v === "headerPair") return t("Header 双字段");
                                if (v === "queryPair") return t("Query 双字段");
                                return t("HTTP Basic");
                              }}
                            </SelectValue>
                          </SelectTrigger>
                          <SelectContent>
                    <SelectGroup>
                            <SelectItem value="basic">{t("HTTP Basic")}</SelectItem>
                            <SelectItem value="headerPair">{t("Header 双字段")}</SelectItem>
                            <SelectItem value="queryPair">{t("Query 双字段")}</SelectItem>
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </div>
                      {userpassMode !== "basic" ? (
                        <div className="grid gap-3 md:grid-cols-2">
                          <div className="grid gap-2">
                            <Label className="text-xs">{t("账号字段名")}</Label>
                            <Input
                              value={userpassUsernameName}
                              disabled={!isServiceReady}
                              onChange={(e) =>
                                setUserpassUsernameName(e.target.value)
                              }
                            />
                          </div>
                          <div className="grid gap-2">
                            <Label className="text-xs">{t("密码字段名")}</Label>
                            <Input
                              value={userpassPasswordName}
                              disabled={!isServiceReady}
                              onChange={(e) =>
                                setUserpassPasswordName(e.target.value)
                              }
                            />
                          </div>
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                  </CardContent>
                </Card>

                <Card size="sm">
                  <CardContent className="grid gap-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <Label className="text-sm">{t("自定义 action")}</Label>
                      <p className="text-[11px] text-muted-foreground">
                        {t(
                          "默认直接使用 URL 原样请求；开启后才在 URL 后使用指定 path。"
                        )}
                      </p>
                    </div>
                    <Switch
                      checked={actionCustomEnabled}
                      disabled={!isServiceReady}
                      onCheckedChange={(checked) =>
                        setActionCustomEnabled(Boolean(checked))
                      }
                    />
                  </div>
                  {actionCustomEnabled ? (
                    <div className="grid gap-2">
                      <Label className="text-xs">{t("action path")}</Label>
                      <Input
                        value={action}
                        disabled={!isServiceReady}
                        placeholder={t("例如：/responses；留空则直接使用 URL")}
                        onChange={(e) => setAction(e.target.value)}
                      />
                    </div>
                  ) : null}
                  </CardContent>
                </Card>
              </div>

              <Card size="sm">
                <CardContent className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <Label className="text-sm">{t("余额检测")}</Label>
                    <p className="text-[11px] text-muted-foreground">
                      {t("开启后可在聚合 API 列表手动刷新并显示余额。")}
                    </p>
                  </div>
                  <Switch
                    checked={balanceQueryEnabled}
                    disabled={!isServiceReady}
                    onCheckedChange={(checked) =>
                      setBalanceQueryEnabled(Boolean(checked))
                    }
                  />
                </div>

                {balanceQueryEnabled ? (
                  <div className="grid gap-3 md:grid-cols-2">
                    <div className="grid gap-2">
                      <Label className="text-xs">{t("查询模板")}</Label>
                      <Select
                        value={balanceQueryTemplate}
                        disabled={!isServiceReady}
                        onValueChange={(value) =>
                          setBalanceQueryTemplate(
                            value === "new_api"
                              ? "new_api"
                              : value === "custom"
                                ? "custom"
                                : "generic"
                          )
                        }
                      >
                        <SelectTrigger className="w-full">
                          <SelectValue>
                            {(value) =>
                              String(value || "") === "new_api"
                                ? "New API"
                                : String(value || "") === "custom"
                                  ? "Custom"
                                : t("通用余额")
                            }
                          </SelectValue>
                        </SelectTrigger>
                        <SelectContent>
                    <SelectGroup>
                          <SelectItem value="generic">{t("通用余额")}</SelectItem>
                          <SelectItem value="new_api">New API</SelectItem>
                          <SelectItem value="custom">Custom</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </div>

                    <div className="grid gap-2">
                      <Label htmlFor="aggregate-api-balance-base-url">
                        {t("余额接口基础地址")}
                      </Label>
                      <Input
                        id="aggregate-api-balance-base-url"
                        value={balanceQueryBaseUrl}
                        disabled={!isServiceReady}
                        placeholder={
                          balanceQueryTemplate === "new_api"
                            ? t("留空则从 URL 推断服务根地址")
                            : t("留空则使用上方 URL")
                        }
                        onChange={(event) =>
                          setBalanceQueryBaseUrl(event.target.value)
                        }
                      />
                    </div>

                    {balanceQueryTemplate === "custom" ? (
                      <>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-custom-path">
                            Custom path
                          </Label>
                          <Input
                            id="aggregate-api-balance-custom-path"
                            value={balanceCustomPath}
                            disabled={!isServiceReady}
                            placeholder="/v1/usage"
                            onChange={(event) =>
                              setBalanceCustomPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label>Auth</Label>
                          <Select
                            value={balanceCustomAuth}
                            disabled={!isServiceReady}
                            onValueChange={(value) =>
                              setBalanceCustomAuth(
                                normalizeBalanceCustomAuth(value)
                              )
                            }
                          >
                            <SelectTrigger className="w-full">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                    <SelectGroup>
                              <SelectItem value="provider_bearer">
                                Provider key
                              </SelectItem>
                              <SelectItem value="balance_bearer">
                                Balance token
                              </SelectItem>
                              <SelectItem value="none">None</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-remaining-path">
                            Remaining path
                          </Label>
                          <Input
                            id="aggregate-api-balance-remaining-path"
                            value={balanceCustomRemainingPath}
                            disabled={!isServiceReady}
                            placeholder="data.remaining"
                            onChange={(event) =>
                              setBalanceCustomRemainingPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-multiplier">
                            Multiplier
                          </Label>
                          <Input
                            id="aggregate-api-balance-multiplier"
                            value={balanceCustomMultiplier}
                            disabled={!isServiceReady}
                            placeholder="1"
                            onChange={(event) =>
                              setBalanceCustomMultiplier(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-unit">
                            Unit
                          </Label>
                          <Input
                            id="aggregate-api-balance-unit"
                            value={balanceCustomUnit}
                            disabled={!isServiceReady}
                            placeholder="USD"
                            onChange={(event) =>
                              setBalanceCustomUnit(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-total-path">
                            Total path
                          </Label>
                          <Input
                            id="aggregate-api-balance-total-path"
                            value={balanceCustomTotalPath}
                            disabled={!isServiceReady}
                            placeholder="data.total"
                            onChange={(event) =>
                              setBalanceCustomTotalPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-used-path">
                            Used path
                          </Label>
                          <Input
                            id="aggregate-api-balance-used-path"
                            value={balanceCustomUsedPath}
                            disabled={!isServiceReady}
                            placeholder="data.used"
                            onChange={(event) =>
                              setBalanceCustomUsedPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-plan-path">
                            Plan path
                          </Label>
                          <Input
                            id="aggregate-api-balance-plan-path"
                            value={balanceCustomPlanPath}
                            disabled={!isServiceReady}
                            placeholder="data.plan"
                            onChange={(event) =>
                              setBalanceCustomPlanPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-valid-path">
                            Valid path
                          </Label>
                          <Input
                            id="aggregate-api-balance-valid-path"
                            value={balanceCustomValidPath}
                            disabled={!isServiceReady}
                            placeholder="data.active"
                            onChange={(event) =>
                              setBalanceCustomValidPath(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-error-path">
                            Error path
                          </Label>
                          <Input
                            id="aggregate-api-balance-error-path"
                            value={balanceCustomInvalidMessagePath}
                            disabled={!isServiceReady}
                            placeholder="message"
                            onChange={(event) =>
                              setBalanceCustomInvalidMessagePath(
                                event.target.value
                              )
                            }
                          />
                        </div>
                        {balanceCustomAuth === "balance_bearer" ? (
                          <div className="grid gap-2">
                            <Label htmlFor="aggregate-api-balance-custom-token">
                              Balance access token
                            </Label>
                            <Input
                              id="aggregate-api-balance-custom-token"
                              type="password"
                              value={balanceQueryAccessToken}
                              disabled={!isServiceReady}
                              placeholder={
                                aggregateApi?.id ? "Keep current" : "Optional"
                              }
                              onChange={(event) =>
                                setBalanceQueryAccessToken(event.target.value)
                              }
                            />
                          </div>
                        ) : null}
                      </>
                    ) : null}

                    {balanceQueryTemplate === "new_api" ? (
                      <>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-access-token">
                            {t("余额 Access Token")}
                          </Label>
                          <Input
                            id="aggregate-api-balance-access-token"
                            type="password"
                            value={balanceQueryAccessToken}
                            disabled={!isServiceReady}
                            placeholder={
                              aggregateApi?.id
                                ? t("留空则保持原值或使用密钥")
                                : t("留空则使用密钥")
                            }
                            onChange={(event) =>
                              setBalanceQueryAccessToken(event.target.value)
                            }
                          />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="aggregate-api-balance-user-id">
                            {t("New API 用户 ID")}
                          </Label>
                          <Input
                            id="aggregate-api-balance-user-id"
                            value={balanceQueryUserId}
                            disabled={!isServiceReady}
                            onChange={(event) =>
                              setBalanceQueryUserId(event.target.value)
                            }
                          />
                        </div>
                      </>
                    ) : null}
                  </div>
                ) : null}
                </CardContent>
              </Card>

              {generatedKey ? (
                <div className="space-y-2 border-t pt-2">
                  <Label className="flex items-center gap-1.5 text-xs text-primary">
                    <ShieldCheck className="h-3.5 w-3.5" /> {t("新密钥已生成")}
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      value={generatedKey}
                      readOnly
                      className="bg-primary/5 font-mono text-sm"
                    />
                    <Button
                      variant="outline"
                      onClick={() => void copyKey()}
                      disabled={!generatedKey}
                    >
                      <Clipboard className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          </div>

          <div className="border-t border-border/50 px-5 py-3">
            <DialogFooter>
              {!generatedKey ? (
                <DialogClose
                  className={buttonVariants({ variant: "ghost" })}
                  type="button"
                >
                  {t("取消")}
                </DialogClose>
              ) : null}
              {!generatedKey ? (
                <Button
                  onClick={() => void handleSave()}
                  disabled={!isServiceReady || isLoading}
                >
                  {isLoading ? t("保存中...") : t("完成")}
                </Button>
              ) : null}
            </DialogFooter>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
