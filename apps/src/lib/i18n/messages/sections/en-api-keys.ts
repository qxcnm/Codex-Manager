import type { MessageCatalog } from "../types";

export const EN_API_KEYS_MESSAGES: MessageCatalog = {
  平台限制: "Platform restriction",
  "Codex + Kiro": "Codex + Kiro",
  "智能别名也只会在允许的平台中选择上游。":
    "Smart aliases only select upstreams from the allowed platforms.",
  "并发限制 (可选)": "Concurrency limit (optional)",
  "达到上限时新请求返回 429，流式请求完成后自动释放。":
    "New requests return 429 at the limit; streaming slots are released when complete.",
  "模型白名单 (可选)": "Model allowlist (optional)",
  "每行或逗号分隔；留空允许该平台范围内的全部模型。":
    "Separate with lines or commas; leave blank to allow every model in the platform scope.",
  "过期时间 (可选)": "Expiration time (optional)",
  "到期后网关立即拒绝该 Key 的新请求。":
    "The gateway rejects new requests for this key immediately after expiration.",
  "Gateway access": "Gateway access",
  项目: "Project",
  "Token / 金额": "Token / Amount",
  已花费: "Spent",
  不限额: "Unlimited",
  已达上限: "Limit reached",
  管理员视图: "Admin view",
  成员视图: "Member view",
  "请选择平台 Key 归属成员": "Select the member owner for this platform key",
  账号组筛选: "Account group filter",
  "额度分发开启时，平台 Key 必须归属到一个成员钱包。":
    "When quota distribution is enabled, the platform key must belong to a member wallet.",
  "未开启额度分发时可先不分配，开启后再补齐归属。":
    "When quota distribution is not enabled, you may leave this unassigned and fill in ownership later.",
  "总额度限制 (Token，可选)": "Total quota limit (tokens, optional)",
  不填表示不限制: "Leave blank for no limit",
  K: "K",
  M: "M",
  "达到上限后，这把平台密钥的新请求会被拒绝；已在途请求会按完成后的真实用量继续统计。":
    "After the limit is reached, new requests using this platform key will be rejected. In-flight requests continue to be counted by their final actual usage.",
  按: "By",
  参考估算: "Reference estimate",
};
