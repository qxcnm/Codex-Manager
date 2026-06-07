"use client";

import { useCallback, useRef, useState } from "react";
import { 
  Dialog, 
  DialogContent, 
  DialogDescription, 
  DialogHeader, 
  DialogTitle
} from "@/components/ui/dialog";
import { 
  Tabs, 
  TabsContent, 
  TabsList, 
  TabsTrigger 
} from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { accountClient } from "@/lib/api/account-client";
import { CODEX_PROFILE_CANDIDATES_QUERY_KEY } from "@/lib/api/codex-profile-client";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { FileUp, Info, LogIn, Clipboard, ExternalLink, Hash } from "lucide-react";

interface AddAccountModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

/**
 * 函数 `pickImportTokenField`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - record: 参数 record
 * - keys: 参数 keys
 *
 * # 返回
 * 返回函数执行结果
 */
function pickImportTokenField(record: unknown, keys: string[]): string {
  const source = record && typeof record === "object" && !Array.isArray(record)
    ? (record as Record<string, unknown>)
    : null;
  if (!source) return "";
  for (const key of keys) {
    const value = source[key];
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return "";
}

/**
 * 函数 `normalizeSingleImportRecord`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - record: 参数 record
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeSingleImportRecord(record: unknown): unknown {
  if (!record || typeof record !== "object" || Array.isArray(record)) {
    return record;
  }
  const source = record as Record<string, unknown>;
  const tokens = source.tokens;
  if (tokens && typeof tokens === "object" && !Array.isArray(tokens)) {
    return record;
  }

  const accessToken = pickImportTokenField(record, ["access_token", "accessToken"]);
  const idToken = pickImportTokenField(record, ["id_token", "idToken"]);
  const refreshToken = pickImportTokenField(record, ["refresh_token", "refreshToken"]);
  if (!accessToken) {
    return record;
  }

  const accountIdHint = pickImportTokenField(record, [
    "account_id",
    "accountId",
    "chatgpt_account_id",
    "chatgptAccountId",
  ]);
  const normalizedTokens: Record<string, string> = {
    access_token: accessToken,
  };
  if (idToken) {
    normalizedTokens.id_token = idToken;
  }
  if (refreshToken) {
    normalizedTokens.refresh_token = refreshToken;
  }
  if (accountIdHint) {
    normalizedTokens.account_id = accountIdHint;
  }

  return {
    ...source,
    tokens: normalizedTokens,
  };
}

/**
 * 函数 `normalizeImportContentForCompatibility`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - rawContent: 参数 rawContent
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeImportContentForCompatibility(rawContent: string): string {
  const text = String(rawContent || "").trim();
  if (!text) return text;
  try {
    const parsed = JSON.parse(text);
    if (Array.isArray(parsed)) {
      return JSON.stringify(parsed.map(normalizeSingleImportRecord));
    }
    if (parsed && typeof parsed === "object") {
      return JSON.stringify(normalizeSingleImportRecord(parsed));
    }
    return text;
  } catch {
    return text;
  }
}

/**
 * 函数 `buildBulkImportContents`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - rawContent: 参数 rawContent
 *
 * # 返回
 * 返回函数执行结果
 */
function buildBulkImportContents(rawContent: string): string[] {
  const text = String(rawContent || "").trim();
  if (!text) return [];

  if (text.startsWith("{") || text.startsWith("[")) {
    return [normalizeImportContentForCompatibility(text)];
  }

  return text
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean)
    .map((item) => normalizeImportContentForCompatibility(item));
}

/**
 * 函数 `getBulkImportErrorMessage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - error: 参数 error
 *
 * # 返回
 * 返回函数执行结果
 */
function getBulkImportErrorMessage(error: unknown, t: (key: string) => string): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("invalid JSON object stream")) {
    return t("导入内容格式不正确。JSON 账号内容请整段粘贴；普通 Token 才按每行一个导入。");
  }
  if (message.includes("invalid JSON array")) {
    return t("JSON 数组格式不正确，请检查括号和逗号后重试。");
  }
  return message;
}

/**
 * 函数 `AddAccountModal`
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
export function AddAccountModal({ open, onOpenChange }: AddAccountModalProps) {
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const [activeTab, setActiveTab] = useState("login");
  const [isLoading, setIsLoading] = useState(false);
  const [isPollingLogin, setIsPollingLogin] = useState(false);
  const [loginHint, setLoginHint] = useState("");
  const queryClient = useQueryClient();
  const loginPollTokenRef = useRef(0);
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;

  // Login Form
  const [tags, setTags] = useState("");
  const [note, setNote] = useState("");
  const [loginUrl, setLoginUrl] = useState("");
  const [manualCallback, setManualCallback] = useState("");

  // Bulk Import
  const [bulkContent, setBulkContent] = useState("");
  const unavailableMessage = canAccessManagementRpc
    ? t("服务未连接，账号授权与导入暂不可用；连接恢复后可继续操作。")
    : t("当前运行环境暂不支持账号管理。");

  const resetModalState = useCallback(() => {
    loginPollTokenRef.current += 1;
    setActiveTab("login");
    setIsLoading(false);
    setIsPollingLogin(false);
    setLoginHint("");
    setTags("");
    setNote("");
    setLoginUrl("");
    setManualCallback("");
    setBulkContent("");
  }, []);

  /**
   * 函数 `invalidateLoginQueries`
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
  const invalidateLoginQueries = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["usage"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
    ]);
  };

  /**
   * 函数 `handleDialogOpenChange`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - nextOpen: 参数 nextOpen
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleDialogOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      resetModalState();
    }
    onOpenChange(nextOpen);
  };

  /**
   * 函数 `completeLoginSuccess`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - message: 参数 message
   *
   * # 返回
   * 返回函数执行结果
   */
  const syncLoggedInAccountToList = async (): Promise<string> => {
    let lastError = "";
    for (let attempt = 0; attempt < 4; attempt += 1) {
      try {
        const [currentAccountResult, latestAccounts] = await Promise.all([
          accountClient.readCurrentAccessTokenAccount(false),
          accountClient.list(),
        ]);
        queryClient.setQueryData(["accounts", "list"], latestAccounts);
        const currentAccountId = currentAccountResult.account?.accountId || "";
        if (
          currentAccountId &&
          latestAccounts.items.some((account) => account.id === currentAccountId)
        ) {
          return currentAccountId;
        }
        lastError = currentAccountId
          ? t("授权已完成，但账号列表暂未出现该账号")
          : t("授权已完成，但当前服务没有返回已登录账号");
      } catch (error: unknown) {
        lastError = error instanceof Error ? error.message : String(error);
      }
      if (attempt < 3) {
        await delay(800);
      }
    }
    throw new Error(lastError || t("授权已完成，但账号列表暂未同步成功"));
  };

  const completeLoginSuccess = async (message: string) => {
    await syncLoggedInAccountToList();
    await invalidateLoginQueries();
    toast.success(message);
    resetModalState();
    onOpenChange(false);
  };

  /**
   * 函数 `ensureServiceReady`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - actionLabel: 参数 actionLabel
   *
   * # 返回
   * 返回函数执行结果
   */
  const ensureServiceReady = (actionLabel: string) => {
    if (isServiceReady) {
      return true;
    }
    toast.info(
      canAccessManagementRpc
        ? `${t("服务未连接，暂时无法")} ${t(actionLabel)}`
        : t("当前运行环境暂不支持账号管理")
    );
    return false;
  };

  /**
   * 函数 `waitForLogin`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - loginId: 参数 loginId
   * - pendingHint?: 参数 pendingHint?
   *
   * # 返回
   * 返回函数执行结果
   */
  const waitForLogin = async (loginId: string, pendingHint?: string) => {
    const pollToken = loginPollTokenRef.current + 1;
    loginPollTokenRef.current = pollToken;
    setIsPollingLogin(true);
    setLoginHint(pendingHint || t("已生成登录链接，正在等待授权完成..."));

    const deadline = Date.now() + 2 * 60 * 1000;
    while (pollToken === loginPollTokenRef.current && Date.now() < deadline) {
      try {
        const result = await accountClient.getLoginStatus(loginId);
        if (pollToken !== loginPollTokenRef.current) {
          return;
        }

        const status = String(result.status || "").trim().toLowerCase();
        if (status === "success") {
          setLoginHint(t("授权完成，正在同步账号列表..."));
          try {
            await completeLoginSuccess(t("登录成功"));
          } catch (error: unknown) {
            const message = error instanceof Error ? error.message : String(error);
            setIsPollingLogin(false);
            setLoginHint(`${t("登录成功，但账号同步失败")}：${message}`);
            toast.error(`${t("登录成功，但账号同步失败")}：${message}`);
          }
          return;
        }
        if (status === "failed") {
          const message = result.error || t("登录失败，请重试");
          setIsPollingLogin(false);
          setLoginHint(`${t("登录失败")}：${message}`);
          toast.error(message);
          return;
        }
      } catch {
        if (pollToken !== loginPollTokenRef.current) {
          return;
        }
      }

      await new Promise<void>((resolve) => window.setTimeout(resolve, 1500));
    }

    if (pollToken === loginPollTokenRef.current) {
      setIsPollingLogin(false);
      setLoginHint(t("登录超时，请重试或使用下方手动解析回调。"));
    }
  };

  /**
   * 函数 `handleStartLogin`
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
  const handleStartLogin = async () => {
    if (!ensureServiceReady("开始登录授权")) {
      return;
    }
    setIsLoading(true);
    setLoginHint("");
    try {
      const result = await accountClient.startLogin({
        tags: tags.split(",").map(t => t.trim()).filter(Boolean),
        note,
      });
      setLoginUrl(result.authUrl || result.verificationUrl || "");
      const pendingHint = result.userCode
        ? `${t("设备验证码")}：${result.userCode}，${t("正在等待授权完成...")}`
        : t("已生成登录链接，正在等待授权完成...");
      if (result.userCode) {
        setLoginHint(`${t("设备验证码")}：${result.userCode}`);
      }
      toast.success(
        result.userCode
          ? t("已生成设备登录信息，请按提示完成授权")
          : t("已生成登录链接，请在浏览器中完成授权")
      );
      if (result.loginId) {
        void waitForLogin(result.loginId, pendingHint);
      } else {
        setLoginHint(t("未返回登录任务编号，请完成授权后使用手动解析。"));
      }
    } catch (err: unknown) {
      toast.error(`${t("启动登录失败")}: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `handleManualCallback`
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
  const handleManualCallback = async () => {
    if (!ensureServiceReady("解析登录回调")) {
      return;
    }
    if (!manualCallback) {
      toast.error(t("请先粘贴回调链接"));
      return;
    }
    setIsLoading(true);
    setLoginHint(t("正在解析回调..."));
    try {
      const url = new URL(manualCallback);
      const state = url.searchParams.get("state") || "";
      const code = url.searchParams.get("code") || "";
      const redirectUri = `${url.origin}${url.pathname}`;
      
      await accountClient.completeLogin(state, code, redirectUri);
      setLoginHint(t("授权完成，正在同步账号列表..."));
      await completeLoginSuccess(t("登录成功"));
    } catch (err: unknown) {
      setLoginHint(`${t("解析失败")}: ${err instanceof Error ? err.message : String(err)}`);
      toast.error(`${t("解析失败")}: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `handleBulkImport`
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
  const handleBulkImport = async () => {
    if (!ensureServiceReady("导入账号")) {
      return;
    }
    if (!bulkContent.trim()) return;
    setIsLoading(true);
    try {
      const contents = buildBulkImportContents(bulkContent);
      const result = await accountClient.import(contents);
      const total = Number(result?.total || 0);
      const created = Number(result?.created || 0);
      const updated = Number(result?.updated || 0);
      const failed = Number(result?.failed || 0);
      toast.success(`${t("导入完成")}：${t("共")}${total}，${t("新增")}${created}，${t("更新")}${updated}，${t("失败")}${failed}`);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["accounts"] }),
        queryClient.invalidateQueries({ queryKey: ["usage"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
      ]);
      resetModalState();
      onOpenChange(false);
    } catch (err: unknown) {
      toast.error(`${t("导入失败")}: ${getBulkImportErrorMessage(err, t)}`);
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `copyUrl`
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
  const copyUrl = async () => {
    if (!loginUrl) return;
    try {
      await copyTextToClipboard(loginUrl);
      toast.success(t("链接已复制"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleDialogOpenChange}>
      <DialogContent className="glass-card max-h-[85vh] overflow-hidden p-0 sm:max-w-[640px]">
        <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
          <div className="shrink-0 bg-muted/20 px-6 pt-6">
            <DialogHeader className="mb-4">
              <DialogTitle className="flex items-center gap-2">
                <LogIn className="h-5 w-5 text-primary" />
                {t("新增账号")}
              </DialogTitle>
              <DialogDescription>
                {t("通过登录授权或批量导入文本内容来添加账号。")}
              </DialogDescription>
            </DialogHeader>
            <TabsList className="grid w-full grid-cols-2 h-10 mb-0">
              <TabsTrigger value="login" className="gap-2">
                <LogIn className="h-3.5 w-3.5" /> {t("登录授权")}
              </TabsTrigger>
              <TabsTrigger value="bulk" className="gap-2">
                <FileUp className="h-3.5 w-3.5" /> {t("批量导入")}
              </TabsTrigger>
            </TabsList>
          </div>

          <div className="max-h-[calc(85vh-154px)] overflow-y-auto p-6">
            <TabsContent value="login" className="mt-0 space-y-4">
              {!isServiceReady ? (
                <Alert>
                  <Info />
                  <AlertDescription>
                    {canAccessManagementRpc
                      ? t("服务未连接，账号授权与回调解析暂不可用；连接恢复后可继续操作。")
                      : unavailableMessage}
                  </AlertDescription>
                </Alert>
              ) : null}
              <div className="space-y-2">
                <Label>{t("标签（逗号分隔）")}</Label>
                <Input
                  placeholder={t("例如：高频, 团队A")}
                  value={tags}
                  disabled={!isServiceReady}
                  onChange={e => setTags(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label>{t("备注/描述")}</Label>
                <Input
                  placeholder={t("例如：主号 / 测试号")}
                  value={note}
                  disabled={!isServiceReady}
                  onChange={e => setNote(e.target.value)}
                />
              </div>

              <div className="pt-2">
                <Button
                  onClick={handleStartLogin}
                  disabled={!isServiceReady || isLoading || isPollingLogin}
                  className="w-full gap-2"
                >
                  <ExternalLink className="h-4 w-4" /> {t("登录授权")}
                </Button>
                {loginUrl && (
                  <div className="mt-3 p-2 rounded-lg bg-primary/5 border border-primary/10 flex items-center gap-2 animate-in fade-in zoom-in duration-300">
                    <Input value={loginUrl} readOnly className="font-mono text-[10px] h-8 bg-transparent" />
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void copyUrl()}
                      disabled={!loginUrl}
                      className="h-8 w-8 p-0"
                    >
                      <Clipboard className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                )}
                {loginHint ? (
                  <p className="mt-2 text-xs text-muted-foreground">{loginHint}</p>
                ) : null}
              </div>

              <Separator />
              <div className="space-y-3">
                <div className="space-y-2">
                  <Label className="text-xs flex items-center gap-1.5 text-muted-foreground">
                    <Hash className="h-3 w-3" /> {t("手动解析回调（当本地 48760 端口占用时）")}
                  </Label>
                  <div className="flex gap-2">
                    <Input 
                      placeholder={t("粘贴浏览器跳转后的完整回调 URL（包含 state 和 code）")} 
                      value={manualCallback}
                      disabled={!isServiceReady}
                      onChange={e => setManualCallback(e.target.value)}
                      className="font-mono text-[10px] h-9" 
                    />
                    <Button 
                      variant="secondary" 
                      onClick={handleManualCallback} 
                      disabled={!isServiceReady || isLoading} 
                      className="h-9 px-4 shrink-0"
                    >
                      {t("解析")}
                    </Button>
                  </div>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="bulk" className="mt-0 space-y-4">
              {!isServiceReady ? (
                <Alert>
                  <Info />
                  <AlertDescription>{unavailableMessage}</AlertDescription>
                </Alert>
              ) : null}
              <div className="space-y-2">
                <Label>{t("账号数据（Token 可每行一个，JSON 可整段粘贴）")}</Label>
                <Textarea 
                  placeholder={t("粘贴账号数据。普通 Token 可每行一个；完整 JSON / JSON 数组请整段粘贴。")}
                  className="min-h-[250px] resize-none overflow-auto whitespace-pre-wrap break-all [overflow-wrap:anywhere] font-mono text-[10px] leading-4"
                  value={bulkContent}
                  disabled={!isServiceReady}
                  onChange={(e) => setBulkContent(e.target.value)}
                />
              </div>
              <Alert>
                <Info />
                <AlertDescription className="text-xs leading-relaxed">
                  {t("支持格式：ChatGPT 账号（Refresh Token）、Claude Session 等。系统将自动识别格式并导入。")}
                </AlertDescription>
              </Alert>
              <Button
                onClick={handleBulkImport}
                disabled={!isServiceReady || isLoading}
                className="w-full"
              >
                {t("开始导入")}
              </Button>
            </TabsContent>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
