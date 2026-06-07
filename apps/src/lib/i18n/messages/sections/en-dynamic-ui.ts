import type { MessageCatalog } from "../types";

export const EN_DYNAMIC_UI_MESSAGES: MessageCatalog = {
  "· 长周期": " · long window",
  按平台价格规则: "By platform pricing rules",
  薄荷绿: "Mint green",
  保存来源模型: "Save source model",
  保存模型映射: "Save model mapping",
  本地: "Local",
  部署级: "Deployment level",
  查看: "View",
  从未使用: "Never used",
  "单条导入内容过大，请拆分后重试":
    "A single import item is too large. Split it and try again.",
  "当前 Web / Docker 版暂不支持该操作":
    "This operation is not supported in the current Web / Docker version.",
  当前环境不支持打开本地目录:
    "The current environment does not support opening local folders.",
  当前环境不支持打开更新日志目录:
    "The current environment does not support opening the changelog folder.",
  当前环境不支持打开浏览器:
    "The current environment does not support opening the browser.",
  当前环境不支持打开外部链接:
    "The current environment does not support opening external links.",
  "当前环境不支持复制，请手动复制。":
    "Copy is not supported in the current environment. Please copy manually.",
  当前环境不支持浏览器文件选择:
    "The current environment does not support browser file selection.",
  "当前页面缺少 CodexManager Web 运行壳，无法访问管理 RPC。请通过 codexmanager-web 打开，或在反向代理中转发 /api/rpc。":
    "This page is missing the CodexManager Web runtime shell and cannot access management RPC. Open it through codexmanager-web, or proxy /api/rpc in your reverse proxy.",
  导出模型目录: "Export model catalog",
  地址不合法: "Invalid address",
  地址解析失败: "Failed to parse address",
  地址为空: "Address is empty",
  低风险: "Low risk",
  读取模型: "Read models",
  读取模型价格失败: "Failed to read model pricing",
  端口已被占用: "Port is already in use",
  服务返回空响应: "Service returned an empty response",
  "服务返回空响应（可能启动未完成、已异常退出或端口被占用）":
    "Service returned an empty response. Startup may be incomplete, the process may have exited, or the port may be occupied.",
  服务已启动: "Service started",
  高风险: "High risk",
  供应商模型保存结果为空: "Supplier model save result is empty",
  海湾青: "Ocean cyan",
  极光青: "Aurora teal",
  极夜黑: "Polar night",
  简体中文: "Simplified Chinese",
  "控制 compact 请求实际转发到哪个上游路径；默认 /v1/responses/compact，可改成 /v1/chat/completions。":
    "Controls which upstream path compact requests are forwarded to. Default is /v1/responses/compact; it can be changed to /v1/chat/completions.",
  "控制 Images API 兼容入口内部使用的 Codex 主模型；默认 gpt-5.4-mini。":
    "Controls the Codex main model used internally by the Images API compatibility endpoint. Default is gpt-5.4-mini.",
  "控制 Images API 兼容入口注入的图片工具模型；默认 gpt-image-2。":
    "Controls the image tool model injected by the Images API compatibility endpoint. Default is gpt-image-2.",
  "控制 OpenAI Images 兼容入口是否启用；默认 1，填 0 会关闭 /v1/images/generations 和 /v1/images/edits。":
    "Controls whether the OpenAI Images compatibility endpoint is enabled. Default is 1; set 0 to disable /v1/images/generations and /v1/images/edits.",
  "控制普通 Responses 请求是否自动注入 image_generation tool；默认 1，填 0 时仅客户端显式传入 tool 才启用。":
    "Controls whether normal Responses requests auto-inject the image_generation tool. Default is 1; when set to 0, it is enabled only when the client explicitly sends the tool.",
  来源模型保存结果为空: "Source model save result is empty",
  连接被拒绝: "Connection refused",
  连接超时: "Connection timed out",
  "连接中断（可能是网络波动或客户端主动取消）":
    "Connection interrupted, possibly due to network instability or client cancellation.",
  令牌刷新轮询: "Token refresh polling",
  玫瑰粉: "Rose pink",
  模型保存结果为空: "Model save result is empty",
  模型映射保存结果为空: "Model mapping save result is empty",
  葡萄灰紫: "Grape violet",
  企业蓝: "Enterprise blue",
  请求语义: "Request semantics",
  请输入端口或地址: "Enter a port or address",
  "请选择 .json 或 .txt 文件": "Please select a .json or .txt file",
  请选择模型组: "Please select a model group",
  全部账号: "All accounts",
  缺少浏览器跳转地址: "Missing browser navigation URL",
  缺少外部跳转地址: "Missing external navigation URL",
  "若开启全局代理，请将 localhost/127.0.0.1/::1 设为直连":
    "If a global proxy is enabled, set localhost/127.0.0.1/::1 to direct connection.",
  删除模型映射: "Delete model mapping",
  上游流式空闲超时: "Upstream streaming idle timeout",
  "上游中途断开，未返回具体错误信息":
    "Upstream disconnected before completion without a specific error message.",
  深邃黑: "Deep black",
  石板灰: "Slate gray",
  "使用更明显的渐层背景、增强玻璃质感和更强层次感。":
    "Use stronger gradient backgrounds, enhanced glass texture, and clearer depth.",
  "使用更轻的玻璃效果和更简洁的背景表现。":
    "Use lighter glass effects and a simpler background presentation.",
  事务金: "Business gold",
  松林绿: "Pine forest green",
  "所选目录中没有可导入的 .json 或 .txt 文件":
    "The selected folder does not contain importable .json or .txt files.",
  同步来源模型: "Sync source models",
  托管状态未知: "Managed status unknown",
  晚霞橙: "Sunset orange",
  网关保活线程: "Gateway keep-alive worker",
  网络抖动: "Network instability",
  未发现配置: "No configuration found",
  未配置可用命令: "No available command configured",
  未托管: "Unmanaged",
  "未在所选文件中找到可导入内容":
    "No importable content found in the selected file.",
  未知计划: "Unknown plan",
  "无法从 userAgent 解析 Codex CLI 版本":
    "Unable to parse Codex CLI version from userAgent.",
  "响应来源不是 codexmanager 服务":
    "The response source is not a codexmanager service.",
  "疑似非 codexmanager 服务": "Possibly not a codexmanager service",
  已存在成员钱包余额: "Member wallet balances already exist",
  已存在成员账号: "Member accounts already exist",
  已存在模型组成员分配: "Model group member assignments already exist",
  "已存在平台 Key 归属": "Platform key ownership already exists",
  已存在钱包流水: "Wallet ledger entries already exist",
  已存在请求扣费记录: "Request charge records already exist",
  用量轮询线程: "Usage polling worker",
  运行时全局: "Runtime global",
  中风险: "Medium risk",
  "RPC 请求失败": "RPC request failed",
};
