import type { ModelRoutePolicyV2 } from "@/types/model-v2";

export function modelRoutePolicy(capabilities: Record<string, unknown>): ModelRoutePolicyV2 {
  const value = capabilities.route_policy;
  if (value === undefined) return "legacy";
  if (value === "legacy" || value === "account_first" || value === "aggregate_first" || value === "weighted") return value;
  throw new Error("无效的模型路由策略");
}

export function withModelRoutePolicy(
  capabilities: Record<string, unknown>, policy: ModelRoutePolicyV2,
): Record<string, unknown> {
  const next = { ...capabilities };
  if (policy === "legacy") delete next.route_policy;
  else next.route_policy = policy;
  return next;
}
