"use client";

import type { MessageCatalog } from "../types";

export const EN_MODELS_MESSAGES: MessageCatalog = {
  模型与路由: "Models and routing",
  "配置 CodexManager 本地网关目录中的模型、价格、路由和指令策略。":
    "Configure models, prices, routes, and instruction policies in the local CodexManager gateway catalog.",
  刷新本地目录: "Refresh local catalog",
  "本地网关模型目录已刷新": "Local gateway model catalog refreshed",
  "导入到本地网关目录": "Import into local gateway catalog",
  "新增网关自定义模型": "Add custom gateway model",
  "当前 Codex 模型来源": "Current Codex model source",
  当前生效: "Currently active",
  当前不生效: "Not active for current Codex",
  本地目录: "Local catalog",
  "本地目录是否影响当前 Codex": "Does the local catalog affect current Codex?",
  "当前平台密钥使用本地网关目录；下方模型、路由和可见性设置会影响当前 Codex。":
    "The current platform key uses the local gateway catalog. The model, route, and visibility settings below affect the current Codex session.",
  "当前 Codex 跟随 OpenAI 官方目录；下方设置仅供使用本地目录的平台密钥，不会改变当前模型列表。":
    "The current Codex session follows the official OpenAI catalog. The settings below apply only to platform keys that use the local catalog and will not change the current model list.",
  "尚未确认当前 Codex 的目录来源；请先在 Codex 接入方式页面检查配置。":
    "The current Codex catalog source is not confirmed. Check the configuration on the Codex Connection page.",
  本地网关模型目录: "Local gateway model catalog",
  "从本地网关目录隐藏模型 {slug}": "Hide model {slug} from local gateway catalog",
  "从本地网关目录删除模型 {slug}": "Delete model {slug} from local gateway catalog",
  "内置模型 {slug} 将从本地网关目录隐藏并禁用，数据不会删除。此操作不影响直接连接 OpenAI 或使用官方目录的账号池。":
    "Built-in model {slug} will be hidden and disabled in the local gateway catalog without deleting its data. This does not affect direct OpenAI connections or account pools that use the official catalog.",
  "确定要从本地网关目录永久删除自定义模型 {slug} 吗？此操作不影响直接连接 OpenAI 或使用官方目录的账号池。":
    "Permanently delete custom model {slug} from the local gateway catalog? This does not affect direct OpenAI connections or account pools that use the official catalog.",
  "将处理本地网关目录中的 {count} 个模型：{builtin} 个内置模型会被隐藏并禁用，其余自定义模型会被删除。此操作不影响直接连接 OpenAI 或使用官方目录的账号池。":
    "Process {count} models in the local gateway catalog: {builtin} built-in models will be hidden and disabled, and the custom models will be deleted. This does not affect direct OpenAI connections or account pools that use the official catalog.",
  从本地网关目录批量移除模型: "Remove models from local gateway catalog",
  从本地网关目录移除模型: "Remove model from local gateway catalog",
  移除: "Remove",
  "支持模型目录导出格式和 Codex catalog 格式；所有导入项都会作为本地网关自定义模型处理，不影响直接连接 OpenAI 或使用官方目录的账号池。":
    "Supports model catalog exports and Codex catalog JSON. Every imported item becomes a custom local gateway model and does not affect direct OpenAI connections or account pools that use the official catalog.",
  可用模型: "Available models",
  模型管理: "Model management",
  "本地模型目录是唯一运行时真相源；价格、路由和 instructions policy 原子保存。":
    "The local model catalog is the only runtime source of truth; prices, routes, and the instructions policy are saved atomically.",
  "本地模型目录是唯一运行时真相源；价格、路由和指令策略会原子保存。":
    "The local model catalog is the only runtime source of truth; prices, routes, and the instructions policy are saved atomically.",
  重新读取: "Reload",
  模型目录已重新读取: "Model catalog reloaded",
  读取模型失败: "Failed to read models",
  "从本地 JSON 导入": "Import local JSON",
  "导出中...": "Exporting...",
  新增自定义模型: "Add custom model",
  总数: "Total",
  已启用: "Enabled",
  已禁用: "Disabled",
  隐藏且启用: "Hidden and enabled",
  隐藏且禁用: "Hidden and disabled",
  内置模型: "Built-in models",
  自定义模型: "Custom models",
  价格缺失: "Price missing",
  路由缺失: "Route missing",
  已隐藏: "Hidden",
  模型目录明细: "Model catalog details",
  "显示 origin、启用状态、价格状态、instructions mode 和 route 状态。":
    "Shows origin, enabled state, price status, instructions mode, and route status.",
  "显示来源、启用状态、价格状态、指令模式和路由状态。":
    "Shows source, enabled state, price status, instruction mode, and route status.",
  "请先勾选一个或多个模型，再使用批量分配路由。":
    "Select one or more models, then use bulk route assignment.",
  搜索模型: "Search models",
  全部模型: "All models",
  批量删除模型: "Delete models",
  "服务未连接，模型目录暂不可用。":
    "The service is disconnected, so the model catalog is unavailable.",
  "没有符合条件的模型。": "No models match the current filters.",
  选择全部模型: "Select all models",
  模型: "Model",
  来源: "Source",
  指令: "Instructions",
  路由: "Routes",
  状态: "Status",
  操作: "Actions",
  模型状态: "Model state",
  "模型状态操作 {slug}": "Change model state for {slug}",
  显示并启用: "Visible and enabled",
  显示但禁用: "Visible but disabled",
  隐藏但启用: "Hidden but enabled",
  隐藏并禁用: "Hidden and disabled",
  恢复并启用: "Restore and enable",
  恢复显示但保持禁用: "Restore visibility and keep disabled",
  内置: "Built-in",
  自定义: "Custom",
  隐藏: "Hidden",
  官方价格: "Official",
  估算价格: "Estimated",
  自定义价格: "Custom",
  默认: "Default",
  "{count} 条路由": "{count} routes",
  "选择模型 {slug}": "Select model {slug}",
  "编辑模型 {slug}": "Edit model {slug}",
  "禁用模型 {slug}": "Disable model {slug}",
  "隐藏模型 {slug}": "Hide model {slug}",
  "删除模型 {slug}": "Delete model {slug}",
  编辑模型: "Edit model",
  删除模型: "Delete model",
  "Builtin 模型 {slug} 将被禁用，数据不会删除。":
    "Builtin model {slug} will be disabled; its data will not be deleted.",
  "内置模型 {slug} 将被禁用，数据不会删除。":
    "Built-in model {slug} will be disabled; its data will not be deleted.",
  "内置模型 {slug} 将被隐藏并禁用，数据不会删除。":
    "Built-in model {slug} will be hidden and disabled; its data will not be deleted.",
  "确定要永久删除自定义模型 {slug} 吗？":
    "Permanently delete custom model {slug}?",
  "将处理 {count} 个模型：{builtin} 个 builtin 会被禁用，其余 custom 会被删除。":
    "Process {count} models: {builtin} builtin models will be disabled and the custom models will be deleted.",
  "将处理 {count} 个模型：{builtin} 个内置模型会被禁用，其余自定义模型会被删除。":
    "Process {count} models: {builtin} built-in models will be disabled and the custom models will be deleted.",
  "将处理 {count} 个模型：{builtin} 个内置模型会被隐藏并禁用，其余自定义模型会被删除。":
    "Process {count} models: {builtin} built-in models will be hidden and disabled, and the custom models will be deleted.",
  批量分配路由: "Assign routes",
  筛选模型: "Filter models",
  批量修改状态: "Set status in bulk",
  "批量修改模型状态 ({count})": "Set model status in bulk ({count})",
  设置选中模型状态: "Set selected models to",
  批量更新模型状态: "Update model status in bulk",
  批量更新模型状态失败: "Failed to update model status in bulk",
  "已更新 {count} 个模型的状态": "Updated the status of {count} models",
  批量分配模型路由: "Assign model routes in bulk",
  "已选择 {count} 个模型；每条路由的上游模型名会自动使用对应模型标识。":
    "Selected {count} models. Each route automatically uses the corresponding model slug as its upstream model name.",
  分配方式: "Assignment mode",
  追加或更新路由: "Add or update routes",
  替换全部现有路由: "Replace all existing routes",
  "同来源路由会更新，其他现有路由保持不变。":
    "Routes with the same source are updated; all other existing routes remain unchanged.",
  "将删除所选模型的其他路由，仅保留下方配置。":
    "Remove the selected models' other routes and keep only the configuration below.",
  要分配的路由: "Routes to assign",
  "请添加至少一条要分配的路由。": "Add at least one route to assign.",
  "请至少选择一个模型": "Select at least one model",
  "请至少配置一条路由": "Configure at least one route",
  "请选择模型并至少配置一条路由":
    "Select models and configure at least one route",
  "请选择聚合 API": "Select an Aggregate API",
  "聚合 API ID": "Aggregate API ID",
  "路由优先级必须是整数": "Route priority must be an integer",
  "路由权重必须是正整数": "Route weight must be a positive integer",
  "不能重复分配同一个路由来源": "The same route source cannot be assigned twice",
  "删除第 {index} 条批量路由": "Delete batch route {index}",
  "应用到 {count} 个模型": "Apply to {count} models",
  "已为 {count} 个模型分配路由": "Assigned routes to {count} models",
  "批量分配完成：成功{success}个，失败{failed}个":
    "Batch assignment completed: {success} succeeded, {failed} failed",
  批量分配路由失败: "Batch route assignment failed",
  模型不存在: "Model does not exist",
  "最新的前沿智能体编程模型。": "Latest frontier agentic coding model.",
  "适合日常工作的均衡型智能体编程模型。":
    "Balanced agentic coding model for everyday work.",
  "快速且经济的智能体编程模型。": "Fast and affordable agentic coding model.",
  "适合复杂编程、研究和真实工作场景的前沿模型。":
    "Frontier model for complex coding, research, and real-world work.",
  "适合日常编程的强大模型。": "Strong model for everyday coding.",
  "适合简单编程任务的小型、快速且高性价比模型。":
    "Small, fast, and cost-efficient model for simpler coding tasks.",
  "针对专业工作和长时间运行智能体优化的模型。":
    "Optimized for professional work and long-running agents.",
  "先进的图像生成和编辑模型。":
    "State-of-the-art image generation and editing model.",
  "用于 Codex 自动审批审查的模型。":
    "Automatic approval review model for Codex.",
  "已删除 {count} 个模型": "Deleted {count} models",
  "已隐藏内置模型 {slug}": "Hidden built-in model {slug}",
  "已删除自定义模型 {slug}": "Deleted custom model {slug}",
  更新模型状态: "Update model state",
  更新模型状态失败: "Failed to update model state",
  "模型 {slug} 已隐藏但保持启用":
    "Model {slug} is hidden but remains enabled",
  "模型 {slug} 已隐藏并禁用": "Model {slug} is hidden and disabled",
  "模型 {slug} 已恢复并启用": "Model {slug} was restored and enabled",
  "模型 {slug} 已启用并显示": "Model {slug} is enabled and visible",
  "模型 {slug} 已恢复显示但保持禁用":
    "Model {slug} was restored to visible but remains disabled",
  "模型 {slug} 已禁用但保留显示":
    "Model {slug} is disabled but remains visible",
  "已隐藏 {count} 个内置模型": "Hidden {count} built-in models",
  "已删除 {count} 个自定义模型": "Deleted {count} custom models",
  "已隐藏 {hidden} 个内置模型，并删除 {deleted} 个自定义模型":
    "Hidden {hidden} built-in models and deleted {deleted} custom models",
  "批量处理完成：隐藏{hidden}个，删除{deleted}个，失败{failed}个":
    "Batch processing completed: {hidden} hidden, {deleted} deleted, {failed} failed",
  "批量删除完成：成功{success}个，失败{failed}个":
    "Batch deletion completed: {success} succeeded, {failed} failed",
  批量删除失败: "Batch deletion failed",
  "支持模型目录导出格式和 Codex catalog 格式；所有导入项都会作为自定义模型处理。":
    "Supports model catalog exports and Codex catalog JSON; every imported item becomes a custom model.",
  "本地 JSON 文件": "Local JSON file",
  冲突策略: "Conflict strategy",
  保留现有模型: "Keep existing models",
  替换自定义模型: "Replace custom models",
  预览导入: "Preview import",
  "处理中...": "Processing...",
  新增: "Added",
  更新: "Updated",
  冲突: "Conflicts",
  跳过: "Skipped",
  错误: "Errors",
  忽略字段: "Ignored fields",
  提交导入: "Commit import",
  "导入中...": "Importing...",
  "请选择或粘贴模型 JSON": "Select or paste model JSON",
  导入预览失败: "Import preview failed",
  导入提交失败: "Import commit failed",
  导入模型: "Import models",
  "已导入 {count} 个模型": "Imported {count} models",
  模型目录为空: "The model catalog is empty",
  当前环境不支持浏览器导出: "Browser export is unavailable in this environment",
  导出失败: "Export failed",
};
