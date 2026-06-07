"use client";

import type { MessageCatalog } from "../types";

export const EN_MODEL_CATALOG_MESSAGES: MessageCatalog = {
  "Priority": "Priority",
  "Token 价格 (USD / 1M tokens)": "Token prices (USD / 1M tokens)",
  "保留本地覆写": "Keep local override",
  "保存来源模型失败": "Failed to save source model",
  "保存模型价格": "Save model price",
  "保存模型失败": "Failed to save model",
  "保存模型映射失败": "Failed to save model mapping",
  "删除模型失败": "Failed to delete model",
  "删除模型映射失败": "Failed to delete model mapping",
  "可用于 API": "Available for API",
  "在这里维护所有复杂字段，包括 supportedReasoningLevels、truncationPolicy、inputModalities、availableInPlans 以及任意扩展字段。":
    "Maintain all complex fields here, including supportedReasoningLevels, truncationPolicy, inputModalities, availableInPlans, and any extension fields.",
  "同步来源模型失败": "Failed to sync source models",
  "开启后必须至少配置一个启用映射，否则只能保存为草稿。":
    "After enabling this, at least one enabled mapping is required; otherwise it can only be saved as a draft.",
  "开启后，远端刷新不会直接覆盖当前本地版本。":
    "When enabled, remote refreshes will not directly overwrite the current local version.",
  "必须是对象": "must be an object",
  "必须是数字": "must be a number",
  "排序权重": "Sort weight",
  "新增模型": "New model",
  "服务未就绪，无法保存模型价格":
    "Service is not ready, so model prices cannot be saved",
  "来源模型已保存": "Source model saved",
  "来源模型已同步": "Source models synced",
  "来源类型": "Source type",
  "核心字段单独编辑，其余官方 `/models` 参数请直接在高级 JSON 中维护。":
    "Edit core fields separately. Maintain the remaining official /models parameters directly in advanced JSON.",
  "模型 slug 不能为空": "Model slug cannot be empty",
  "模型已保存": "Model saved",
  "模型已保存，但价格保存失败": "Model saved, but price save failed",
  "模型已保存，但同步 Codex 模型缓存失败":
    "Model saved, but syncing the Codex model cache failed",
  "模型已删除": "Model deleted",
  "模型已删除，但同步 Codex 模型缓存失败":
    "Model deleted, but syncing the Codex model cache failed",
  "模型映射已保存": "Model mapping saved",
  "模型映射已删除": "Model mapping deleted",
  "模型目录已刷新": "Model catalog refreshed",
  "模型目录已刷新，但同步 Codex 模型缓存失败":
    "Model catalog refreshed, but syncing the Codex model cache failed",
  "清理远端旧模型失败": "Failed to clean stale remote models",
  "缓存输入价格": "Cached input price",
  "输入价格": "Input price",
  "输入价格和输出价格必须同时填写":
    "Input price and output price must be filled together",
  "输出价格": "Output price",
  "远端同步": "Remote sync",
  "远端旧模型已清理": "Stale remote models cleaned",
  "远端旧模型已清理，但同步 Codex 模型缓存失败":
    "Stale remote models cleaned, but syncing the Codex model cache failed",
  "零表示不计费，价格将用于请求成本估算。":
    "Zero means no billing. Prices are used for request cost estimation.",
  "高级 JSON": "Advanced JSON",
  "价格必须为非负有效数字": "Prices must be valid non-negative numbers",
  "不是有效 JSON 对象": "is not a valid JSON object",
  "读取模型价格": "Read model price",
  "默认推理等级": "Default reasoning level",
};
