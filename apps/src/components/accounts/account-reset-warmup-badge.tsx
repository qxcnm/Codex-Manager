"use client";

import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useI18n } from "@/lib/i18n/provider";

export function AccountResetWarmupBadge({ enabled }: { enabled: boolean }) {
  const { t } = useI18n();
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="inline-flex" />}>
        <Badge variant={enabled ? "secondary" : "outline"}>
          {enabled ? t("自动唤醒：开") : t("自动唤醒：关")}
        </Badge>
      </TooltipTrigger>
      <TooltipContent className="max-w-72">
        {t("5 小时额度用尽后，在重置时间到达时自动发送一条预热消息，提前启动下一轮额度周期。默认开启，可单独或批量关闭。")}
      </TooltipContent>
    </Tooltip>
  );
}
