"use client";

import { useId } from "react";
import { CheckCircle2, Copy, ExternalLink, KeyRound, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { AccountBpsController } from "@/hooks/useAccountBps";
import { appClient } from "@/lib/api/app-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import type { Account } from "@/types";

export function AccountBpsControl({ account, controller }: {
  account: Account;
  controller: AccountBpsController;
}) {
  const { t } = useI18n();
  const labelId = useId();
  const disabled = !controller.available || controller.isUpdating;
  return (
    <div className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border/70 bg-background/50 px-1.5 text-xs">
      <Tooltip>
        <TooltipTrigger render={<span id={labelId} className="whitespace-nowrap" />}>
          {t("BPS 转发：{status}", { status: account.bpsEnabled ? t("开") : t("关") })}
        </TooltipTrigger>
        <TooltipContent className="max-w-xs">
          {t("实验性 Excel/BPS 入口，需要独立授权，不保证改善模型能力。关闭后使用原有请求通道。")}
        </TooltipContent>
      </Tooltip>
      <Switch
        size="sm"
        aria-labelledby={labelId}
        checked={account.bpsEnabled === true}
        disabled={disabled}
        onCheckedChange={(checked) => controller.toggle(account, checked)}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-5 w-5"
        disabled={disabled}
        aria-label={t("BPS 独立授权")}
        title={t("BPS 独立授权")}
        onClick={() => controller.openLogin(account)}
      >
        <KeyRound className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}

export function AccountBpsLoginDialog({ controller }: { controller: AccountBpsController }) {
  const { t } = useI18n();
  const codeId = useId();
  const selection = controller.selection;
  const login = selection?.login;
  const copyCode = async () => {
    if (!login) return;
    try {
      await copyTextToClipboard(login.userCode);
      toast.success(t("验证码已复制"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };
  const openAuthorization = async () => {
    if (!login) return;
    try {
      await appClient.openExternalUrl(login.verificationUri);
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    }
  };
  return (
    <Dialog open={Boolean(selection)} onOpenChange={(open) => { if (!open) controller.closeLogin(); }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("BPS 独立授权")}</DialogTitle>
          <DialogDescription>{selection?.accountName}</DialogDescription>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          {t("请在浏览器中使用此账号完成 BPS 授权。授权单独保存，不替换现有 Codex 登录。")}
        </p>
        <div className="rounded-md border border-border/60 bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          <p className="font-medium text-foreground">{t("先开启授权码登录")}</p>
          <p className="mt-1">
            {t("授权前，请先在 ChatGPT「设置 → 安全」开启“设备代码登录”（授权码登录），再返回这里开始授权。工作区账号可能需要管理员启用；这不保证 BPS 使用权限。")}
          </p>
        </div>
        <p className="text-sm text-muted-foreground">
          {t("实验性 Excel/BPS 入口，需要独立授权，不保证改善模型能力。关闭后使用原有请求通道。")}
        </p>
        {selection?.enableAfterLogin && selection.stage !== "completed" ? (
          <p className="text-sm">{t("授权成功后将为此账号开启 BPS 转发。")}</p>
        ) : null}
        {selection?.error ? (
          <Alert variant="destructive">
            <AlertTitle>{t("BPS 操作失败")}</AlertTitle>
            <AlertDescription>{selection.error}</AlertDescription>
          </Alert>
        ) : null}
        {selection?.stage === "completed" ? (
          <Alert>
            <CheckCircle2 />
            <AlertTitle>{t("BPS 授权已保存")}</AlertTitle>
            <AlertDescription>
              {selection.enableAfterLogin
                ? t("此账号已开启 BPS 转发。授权成功不代表已验证上游生成权限。")
                : t("可使用账号旁的开关开启 BPS 转发。授权成功不代表已验证上游生成权限。")}
            </AlertDescription>
          </Alert>
        ) : login && selection?.stage === "pending" ? (
          <div className="space-y-3">
            <div className="space-y-2">
              <Label htmlFor={codeId}>{t("授权验证码")}</Label>
              <div className="flex gap-2">
                <Input id={codeId} value={login.userCode} readOnly className="font-mono text-lg tracking-wider" />
                <Button variant="outline" size="icon" aria-label={t("复制验证码")} onClick={() => void copyCode()}>
                  <Copy />
                </Button>
              </div>
            </div>
            <Button type="button" onClick={() => void openAuthorization()}>
              <ExternalLink />{t("打开 BPS 授权页面")}
            </Button>
            <p className="break-all text-xs text-muted-foreground">{login.verificationUri}</p>
            <p className="flex items-center gap-2 text-sm" role="status">
              <Loader2 className="h-4 w-4 animate-spin" />{t("等待浏览器授权完成…")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("验证码有效至：{time}", { time: formatTsFromSeconds(login.expiresAt, "—") })}
            </p>
          </div>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={controller.closeLogin}>{t("关闭")}</Button>
          {selection?.stage === "ready" || selection?.stage === "failed" || selection?.stage === "starting" ? (
            <Button type="button" disabled={selection.stage === "starting" || controller.isUpdating} onClick={() => void controller.startLogin()}>
              {selection.stage === "starting" ? <Loader2 className="animate-spin" /> : <KeyRound />}
              {t("开始 BPS 授权")}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AccountBpsRiskDialog({ controller }: { controller: AccountBpsController }) {
  const { t } = useI18n();
  const confirmation = controller.confirmation;
  return (
    <ConfirmDialog
      open={Boolean(confirmation)}
      onOpenChange={(open) => { if (!open) controller.cancelConfirmation(); }}
      title={t("BPS 转发风险提示")}
      description={`${t("BPS 转发为实验性功能，不保证改善模型能力或避免“降智”。可能出现请求失败、功能不兼容及账号使用风险。请自行评估，使用风险由你承担。")} ${t("本次将为 {count} 个账号开启 BPS 转发。", { count: confirmation?.accountIds.length ?? 0 })}`}
      confirmText={confirmation?.needsAuthorization ? t("了解风险并授权") : t("了解风险并开启")}
      cancelText={t("取消")}
      onConfirm={controller.confirmEnable}
    />
  );
}
