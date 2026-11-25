"use client";

import { Field, FieldLabel, FieldDescription } from "@/components/ui/field";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useI18n } from "@/lib/i18n/provider";
import { modelRoutePolicy, withModelRoutePolicy } from "@/lib/model-route-policy";
import type { ModelRoutePolicyV2 } from "@/types/model-v2";

const LABELS: Record<ModelRoutePolicyV2, string> = {
  legacy: "沿用 Key 策略（旧行为）",
  account_first: "账号池优先（优先级 + 权重）",
  aggregate_first: "聚合优先（优先级 + 权重）",
  weighted: "跨源优先级 + 权重",
};

export function RoutePolicySelect({ capabilities, disabled, onChange }: {
  capabilities: Record<string, unknown> | null;
  disabled?: boolean;
  onChange: (json: string) => void;
}) {
  const { t } = useI18n();
  let value: ModelRoutePolicyV2 = "legacy";
  let invalid = capabilities === null;
  try { if (capabilities) value = modelRoutePolicy(capabilities); } catch { invalid = true; }
  return (
    <Field data-invalid={invalid || undefined}>
      <FieldLabel htmlFor="model-route-policy">{t("模型路由策略")}</FieldLabel>
      <Select value={value} disabled={disabled || !capabilities} onValueChange={(policy) => {
        if (capabilities && policy && Object.hasOwn(LABELS, policy)) {
          onChange(JSON.stringify(withModelRoutePolicy(capabilities, policy as ModelRoutePolicyV2), null, 2));
        }
      }}>
        <SelectTrigger id="model-route-policy" aria-label={t("模型路由策略")} aria-invalid={invalid} className="w-full">
          <SelectValue>{(policy) => t(LABELS[policy as ModelRoutePolicyV2] || LABELS.legacy)}</SelectValue>
        </SelectTrigger>
        <SelectContent><SelectGroup>
          {Object.entries(LABELS).map(([policy, label]) => <SelectItem key={policy} value={policy}>{t(label)}</SelectItem>)}
        </SelectGroup></SelectContent>
      </Select>
      <FieldDescription>
        {t("默认不改变现有调度。启用后优先级从高到低，同级按权重平滑轮询；来源偏好仅作用于混合 Key，专用 Key 和显式聚合绑定仍优先。")}
      </FieldDescription>
      {invalid && <FieldDescription>{t("无效的模型路由策略")}</FieldDescription>}
    </Field>
  );
}
