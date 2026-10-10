import { Info, Globe, ShieldCheck } from "lucide-react";
import { AppSettings } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { inferServiceBindPreview, SERVICE_LISTEN_MODE_LABELS } from "@/app/settings/settings-page-helpers";
import packageInfo from "../../../../package.json";

const APP_VERSION = packageInfo.version || "0.0.0";

export function AboutCodexManagerCard({ t }: { t: (value: string) => string }) {
  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <Info className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("关于 CodexManager")}</CardTitle>
        </div>
        <CardDescription>
          {t("查看软件名称、功能简介和当前版本信息")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-2 text-sm">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-base font-semibold text-foreground">
            CodexManager
          </span>
          <Badge variant="secondary" className="font-mono">
            v{APP_VERSION}
          </Badge>
        </div>
        <p className="leading-6 text-muted-foreground">
          {t(
            "CodexManager 用于统一管理 Codex CLI 账号、本地网关、平台密钥、请求日志和用量统计，让 Codex 接入与运维更集中可控。",
          )}
        </p>
      </CardContent>
    </Card>
  );
}

export function ServiceListenCard({
  t,
  snapshot,
  updateSettings,
}: {
  t: (value: string) => string;
  snapshot: AppSettings;
  updateSettings: {
    mutate: (patch: Partial<AppSettings>) => void;
  };
}) {
  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("服务监听")}</CardTitle>
        </div>
        <CardDescription>
          {t("统一控制 Service 与 Web 的监听模式，决定仅本机访问还是开放给局域网")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="grid gap-2">
          <Label>{t("监听地址")}</Label>
          <Select
            value={snapshot.serviceListenMode || "loopback"}
            onValueChange={(value) => {
              const nextValue = String(value || "").trim() || "loopback";
              if (nextValue === snapshot.serviceListenMode) {
                return;
              }
              updateSettings.mutate({ serviceListenMode: nextValue });
            }}
          >
            <SelectTrigger className="w-full md:w-[320px]">
              <SelectValue placeholder={t("选择监听地址模式")}>
                {(value) =>
                  t(
                    SERVICE_LISTEN_MODE_LABELS[String(value || "").trim()] ||
                      String(value || "").trim() ||
                      "仅本机 (localhost)",
                  )
                }
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {(snapshot.serviceListenModeOptions?.length
                  ? snapshot.serviceListenModeOptions
                  : ["loopback", "all_interfaces"]
                ).map((mode) => (
                  <SelectItem key={mode} value={mode}>
                    {t(SERVICE_LISTEN_MODE_LABELS[mode] || mode)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-2 border-t border-border/60 pt-4 text-sm">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-muted-foreground">{t("当前访问地址")}</span>
            <code className="break-all text-xs text-foreground">{snapshot.serviceAddr}</code>
          </div>
          <Separator />
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-muted-foreground">{t("实际监听地址")}</span>
            <code className="break-all text-xs text-foreground">
              {inferServiceBindPreview(
                snapshot.serviceAddr,
                snapshot.serviceListenMode || "loopback",
              )}
            </code>
          </div>
        </div>

        <p className="text-xs leading-5 text-muted-foreground">
          {t("切换到")} <code>0.0.0.0</code>{" "}
          {t(
            "后，局域网设备可通过当前机器 IP 访问；设置保存后需要重启相关进程才会生效，Web 监听地址会默认跟随这里的模式。",
          )}
        </p>
      </CardContent>
    </Card>
  );
}

export function AccessControlCard({
  t,
  snapshot,
  canAccessManagementRpc,
  showAccessControlSettings,
  webAuthModeLabel,
  onOpen,
}: {
  t: (value: string) => string;
  snapshot: AppSettings;
  canAccessManagementRpc: boolean;
  showAccessControlSettings: boolean;
  webAuthModeLabel: string;
  onOpen: () => void;
}) {
  if (!showAccessControlSettings) {
    return null;
  }

  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("访问控制")}</CardTitle>
        </div>
        <CardDescription>
          {t("统一管理 Web 登录方式、访问密码和团队额度分发。")}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div className="space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <Label>{t("当前访问方式")}</Label>
            <Badge variant="secondary">{t(webAuthModeLabel)}</Badge>
          </div>
          <p className="text-xs text-muted-foreground">
            {snapshot.distributionEnabled
              ? t("额度分发已开启，平台 Key 会按归属钱包扣减额度。")
              : t("额度分发未开启，平台 Key 不会扣减成员钱包额度。")}
          </p>
        </div>
        <Button
          variant="outline"
          className="gap-2 self-start md:self-auto"
          disabled={!canAccessManagementRpc}
          onClick={onOpen}
        >
          <ShieldCheck className="h-4 w-4" />
          {t("访问控制")}
        </Button>
      </CardContent>
    </Card>
  );
}
