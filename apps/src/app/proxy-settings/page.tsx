"use client";

import { useI18n } from "@/lib/i18n/provider";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { ProxySettingsCard } from "@/app/settings/components/proxy-settings-card";

export default function ProxySettingsPage() {
  const { t } = useI18n();
  const { canAccessManagementRpc } = useRuntimeCapabilities();

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">{t("代理设置")}</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("管理可复用的代理配置，为账号或网关提供代理能力。")}
        </p>
      </div>

      <ProxySettingsCard canManage={canAccessManagementRpc} />
    </div>
  );
}
