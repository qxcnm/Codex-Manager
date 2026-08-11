"use client";

import { Boxes, Network } from "lucide-react";
import { AdapterPoolConsole } from "@/components/dashboard/adapter-pool-console";
import { PageHeader, PageWorkspace } from "@/components/layout/page-workspace";
import { Badge } from "@/components/ui/badge";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";

export default function AdaptersPage() {
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/adapters/");
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;

  usePageTransitionReady("/adapters/", true);

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="Adapter Matrix"
        title={t("模型接入中心")}
        description={t(
          "统一查看和管理所有已接入的模型平台；新增平台后会自动以相同的资源池结构出现。",
        )}
        meta={
          <>
            <Badge variant="outline" className="gap-1.5">
              <Network className="h-3 w-3" />
              {t("统一接入")}
            </Badge>
            <Badge variant="outline" className="gap-1.5">
              <Boxes className="h-3 w-3" />
              {t("资源池管理")}
            </Badge>
          </>
        }
      />

      <AdapterPoolConsole
        serviceReady={isServiceReady && isPageActive}
        codexTotal={0}
        codexAvailable={0}
      />
    </PageWorkspace>
  );
}
