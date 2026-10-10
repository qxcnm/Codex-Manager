"use client";

import type { MessageCatalog } from "../types";

export const EN_MODEL_CATALOG_MESSAGES: MessageCatalog = {
  "模型、价格、路由和指令策略将通过一次原子保存提交。":
    "The model, prices, routes, and instructions policy are committed by one atomic save.",
  "模型、价格、路由和 instructions policy 将通过一次 V2 原子保存提交。":
    "The model, prices, routes, and instructions policy are committed by one atomic V2 save.",
  基本信息: "Basic information",
  价格: "Pricing",
  路由: "Routes",
  指令策略: "Instructions policy",
  "模型标识（Slug）": "Model identifier (slug)",
  描述: "Description",
  提供方: "Provider",
  模型系列: "Model family",
  模型分类: "Model category",
  "例如：编程, 推理": "For example: coding, reasoning",
  排序: "Sort order",
  支持的推理强度: "Supported reasoning efforts",
  默认推理强度: "Default reasoning effort",
  不指定: "Not specified",
  未在支持档位中: "not supported",
  默认推理强度必须属于支持的档位:
    "Default reasoning effort must be one of the supported efforts",
  启用模型: "Enable model",
  "可用于 API": "Available through API",
  可见性: "Visibility",
  列表显示: "Show in list",
  隐藏: "Hidden",
  "关键能力 JSON": "Capabilities JSON",
  "关键能力 JSON 必须是对象": "Capabilities JSON must be an object",
  "基础价格（美元 / 百万令牌）": "Base prices (USD / 1M tokens)",
  输入价格: "Input price",
  缓存输入价格: "Cached-input price",
  缓存读取价格: "Cache-read price",
  缓存写入价格: "Cache-write price",
  输出价格: "Output price",
  留空表示价格缺失: "Leave blank for price missing",
  留空按普通输入价格计费: "Leave blank to use the regular input price",
  "价格必须是非负十进制数": "Price must be a non-negative decimal number",
  "价格最多支持 6 位有效小数":
    "Price supports at most six significant decimal places",
  "价格超出安全整数范围": "Price exceeds the safe integer range",
  "无效的 micro-USD 价格": "Invalid micro-USD price",
  "输入、缓存输入和输出价格必须同时填写":
    "Input, cached-input, and output prices must all be filled together",
  "配置长上下文价格前必须先填写基础三价":
    "Fill all three base prices before configuring long-context prices",
  "配置缓存写入或长上下文价格前必须先填写基础三价":
    "Fill all three base prices before configuring cache-write or long-context prices",
  可选长上下文阶梯价: "Optional long-context price tier",
  输入令牌阈值: "Input-token threshold",
  长上下文阈值: "Long-context threshold",
  "长上下文阈值和三项价格必须完整填写":
    "The long-context threshold and all three prices must be filled",
  "价格按十进制字符串无损转换为整数 micro-USD；三个基础价格必须同时存在或同时留空。":
    "Prices are converted losslessly from decimal strings to integer micro-USD; all three base prices must be filled or left blank together.",
  "上游模型名始终手填；这里不会访问供应商 `/models`。":
    "Upstream model names are always entered manually; this screen never accesses a supplier `/models` endpoint.",
  账号池: "Account pool",
  添加账号池路由: "Add account-pool route",
  添加聚合路由: "Add aggregate route",
  "当前模型没有 route，启用后将显示 missing route。":
    "This model has no route and will show as missing route when enabled.",
  来源类型: "Source type",
  "来源 ID": "Source ID",
  来源: "Source",
  默认账号池: "Default account pool",
  "未命名聚合 API": "Unnamed aggregate API",
  "未知聚合 API": "Unknown aggregate API",
  "选择聚合 API": "Select aggregate API",
  上游模型: "Upstream model",
  优先级: "Priority",
  权重: "Weight",
  路由优先级: "Route priority",
  路由权重: "Route weight",
  "每条路由都必须填写来源和上游模型":
    "Every route must specify a source and upstream model",
  "启用 route": "Enable route",
  启用路由: "Enable route",
  删除路由: "Delete route",
  指令模式: "Instructions mode",
  指令文本: "Instructions text",
  透传: "Pass through",
  兜底: "Fallback",
  覆盖: "Override",
  "Instructions 模式": "Instructions mode",
  "客户端 instructions 原样传递，模型文本不参与请求。":
    "Client instructions pass through unchanged; model text is not used.",
  "仅当所有客户端 instruction channel 都为空时使用模型文本。":
    "Use the model text only when every client instruction channel is empty.",
  "模型文本替换顶层及连续 leading system/developer instructions；文本不能为空。":
    "Model text replaces top-level and consecutive leading system/developer instructions; it cannot be empty.",
  "override 模式必须填写 instructions text":
    "Override mode requires instructions text",
  "模型 slug 不能为空": "Model slug cannot be empty",
  "缺失价格的模型不能保留在计费权限组中":
    "A model with missing prices cannot remain in a billing permission group",
  上下文窗口: "Context window",
  最大上下文窗口: "Maximum context window",
  正: "positive ",
  保存模型: "Save model",
  保存中: "Saving",
  "保存中...": "Saving...",
  保存模型失败: "Failed to save model",
  模型已保存: "Model saved",
  删除模型失败: "Failed to delete model",
  模型已删除: "Model deleted",
};
