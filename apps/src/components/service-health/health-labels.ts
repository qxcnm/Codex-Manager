import type { ServiceHealthProvider, ServiceHealthRegion, ServiceHealthSymptom } from "@/types/service-health";

export const PROVIDER_LABELS: Record<ServiceHealthProvider, string> = {
  codex: "Codex", claude: "Claude", unknown: "其他或未知",
};
export const REGION_LABELS: Record<ServiceHealthRegion, string> = {
  unknown: "未提供", apac: "亚太", europe: "欧洲", north_america: "北美",
  south_america: "南美", middle_east: "中东", africa: "非洲", oceania: "大洋洲",
};
export const SYMPTOM_LABELS: Record<ServiceHealthSymptom, string> = {
  unavailable: "无法使用", slow: "响应缓慢", rate_limited: "请求受限", errors: "请求报错",
};
