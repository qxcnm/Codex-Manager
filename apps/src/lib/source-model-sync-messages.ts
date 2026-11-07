const SOURCE_MODEL_SYNC_MESSAGES: Record<string, string> = {
  "model discovery completion could not be saved; retry delayed": "同步结果暂时无法保存，已延后重试。",
  "model discovery source no longer exists": "同步来源已删除。",
  "model discovery source is disabled": "同步来源已停用，请先启用账号或聚合 API。",
  "model discovery fetch timed out": "拉取模型超时，将按计划重试。",
  "model discovery fetch failed; check source authorization and connectivity": "拉取模型失败，请检查来源的授权和连接。",
  "model discovery catalog update failed": "保存模型及路由失败，将按计划重试。",
  "previous model discovery was interrupted; retrying": "上次同步被中断，正在重新尝试。",
  "models were saved but the active Codex profile could not be refreshed": "模型已保存，但当前 Codex 配置未能刷新。",
  "source storage unavailable": "暂时无法读取同步来源。",
  "source status could not be read": "暂时无法读取来源状态。",
  "intervalMinutes must be between 5 and 10080": "同步间隔必须为 5–10080 分钟。",
};

export function sourceModelSyncMessage(message: string, translate: (message: string) => string): string {
  return translate(SOURCE_MODEL_SYNC_MESSAGES[message] ?? message);
}
