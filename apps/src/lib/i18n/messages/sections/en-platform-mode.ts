"use client";

import type { MessageCatalog } from "../types";

export const EN_PLATFORM_MODE_MESSAGES: MessageCatalog = {
  "认证模式": "Authentication mode",
  "纯 API Key 模式": "API key only",
  "保留账号信息": "Preserve account information",
  "通过 CodexManager 接入只需可用的平台密钥。选择认证模式后，点击下方按钮应用。": "Connecting through CodexManager only requires an available platform key. Choose an authentication mode, then apply it below.",
  "保留账号信息；从纯 API Key 模式切回时恢复原认证，并保留外部修改。认证切换前请关闭对应 Codex 桌面端。": "Preserve account information. Switching back from API-key-only mode restores the original credentials and preserves external changes. Close the affected Codex Desktop before changing authentication.",
  "仅使用所选平台密钥；备份原认证信息后重写 auth.json，切回保留模式可恢复。切换前请关闭使用此配置的 Codex 桌面端。": "Use only the selected platform key. The original credentials are backed up before auth.json is rewritten and can be restored by switching back. Close Codex Desktop instances using this profile before switching.",
  "选择接入方式后应用配置；账号直连会更新 auth.json，聚合 API 直连保留认证，网关可选择保留账号信息或纯 API Key 模式。": "Apply your chosen connection. Direct account setup updates auth.json; direct aggregate API connections preserve credentials. Gateway connections offer preserved account information or API-key-only authentication.",
  "账号直连会更新 auth.json；聚合 API 直连保留认证；网关默认保留账号信息，也可选择纯 API Key 模式。": "Direct account setup updates auth.json; direct aggregate API setup preserves credentials. Gateway setup preserves account information by default and also offers an API-key-only mode.",
  "通过本地网关接入只需可用的平台密钥，默认保留现有认证；可在 Codex 接入方式页面选择纯 API Key 模式。": "The local gateway only requires an available platform key and preserves existing credentials by default. Choose API-key-only mode on the Codex Connection page.",
  "Codex 接入方式": "Codex Connection",
  "选择 Codex 直接连接 OpenAI，或通过 CodexManager 进行转发与管理。":
    "Choose whether Codex connects directly to OpenAI or is routed and managed through CodexManager.",
  "直接连接 OpenAI": "Connect directly to OpenAI",
  "通过 CodexManager": "Use CodexManager",
  "当前 Codex 接入": "Current Codex connection",
  接入方式: "Connection",
  请求路由: "Request routing",
  模型来源: "Model source",
  "日志与统计": "Logs and analytics",
  重载行为: "Reload behavior",
  应用后: "After applying",
  应用接入配置: "Apply connection settings",
  确认应用: "Confirm and apply",
  "OpenAI 官方目录": "Official OpenAI catalog",
  "CodexManager 本地目录": "CodexManager local catalog",
  "聚合 API": "Aggregated API",
  混合路由: "Hybrid routing",
  "所选 OpenAI 账号": "Selected OpenAI account",
  "CodexManager 可记录": "Recorded by CodexManager",
  "CodexManager 不记录": "Not recorded by CodexManager",
  "切换后尝试重载后台": "Attempt a background reload after switching",
  "下次启动时生效": "Takes effect on the next start",
  无法确认: "Unable to determine",
  请选择平台密钥: "Select a platform key",
  "没有可用于直接连接 OpenAI 的 active 账号。":
    "No active OpenAI account is available for a direct connection.",
  "没有可用于 CodexManager 转发的平台密钥。":
    "No platform key is available for CodexManager routing.",
  "重新应用直接连接": "Reapply direct connection",
  "切换为直接连接 OpenAI": "Connect directly to OpenAI",
  "重新应用 CodexManager 接入": "Reapply CodexManager connection",
  "切换为通过 CodexManager": "Switch to CodexManager",
  "已切换为直接连接 OpenAI": "Now connecting directly to OpenAI",
  "已切换为通过 CodexManager": "Now using CodexManager",
  "Codex 直接连接 OpenAI，并跟随 OpenAI 官方模型目录；CodexManager 不参与请求转发或模型目录管理。":
    "Codex connects directly to OpenAI and follows the official OpenAI model catalog. CodexManager does not route requests or manage the catalog.",
  "Codex 请求由 CodexManager 转发；实际路由和模型目录取决于当前平台密钥的配置。":
    "Codex requests are routed by CodexManager. The selected platform key determines the routing and model catalog.",
  "选择接入方式后应用配置；账号直连会更新 auth.json，网关和聚合 API 接入保留 auth.json，仅更新 config.toml。":
    "Apply your chosen connection. Direct account setup updates auth.json; gateway and aggregated API connections preserve auth.json and only update config.toml.",
  "通过 CodexManager 接入只需可用的平台密钥。应用网关配置会保留 auth.json，兼容现有 ChatGPT 或 API key 认证，仅更新 config.toml。":
    "Connecting through CodexManager only requires an available platform key. Applying gateway settings preserves auth.json, supports existing ChatGPT or API key authentication, and only updates config.toml.",
  "请统一在 Codex 接入方式页面切换连接，避免 provider、模型目录和运行时重载配置彼此不一致。":
    "Switch connections from the Codex Connection page so the provider, model catalog, and runtime reload settings stay consistent.",
  "打开 Codex 接入方式": "Open Codex Connection",
  "先确认 CodexManager 本地服务可用，再选择 Codex 接入方式。":
    "Confirm that the local CodexManager service is available, then choose how Codex connects.",
  "直接连接 OpenAI 需要 active 账号；通过 CodexManager 只需可用的平台密钥。":
    "A direct OpenAI connection requires an active account. CodexManager routing only requires an available platform key.",
  "第三步：应用 Codex 接入方式": "Step 3: Apply the Codex connection",
  "选择接入方式与目标后点击应用，页面会调用现有 profile 接口写入配置。":
    "Choose the connection and target, then apply it. The page writes the configuration through the existing profile API.",
  "直接连接 OpenAI 会更新 auth.json 以切换所选账号；网关和聚合 API 接入保留 auth.json，仅更新 config.toml。":
    "Connecting directly to OpenAI updates auth.json to switch to the selected account; gateway and aggregated API connections preserve auth.json and only update config.toml.",
  "应用后会更新 auth.json，切换为所选 OpenAI 账号。":
    "Applying updates auth.json to switch to the selected OpenAI account.",
  "仅更新 config.toml，聚合 API 密钥也保存在该文件；保留现有 auth.json。":
    "Only config.toml is updated, including the aggregated API key; the existing auth.json is preserved.",
  "通过本地网关接入只需可用的平台密钥。应用网关配置会保留 auth.json 中现有的 ChatGPT 或 API key 认证，仅更新 config.toml。":
    "Connecting through the local gateway only requires an available platform key. Applying gateway settings preserves the existing ChatGPT or API key authentication in auth.json and only updates config.toml.",
  平台模式选择: "Platform Mode",
  "选择 Codex CLI 直连账号，或通过 CodexManager 本地网关接入。":
    "Choose a direct Codex CLI account connection or route through the CodexManager local gateway.",
  写入位置说明: "Where changes are written",
  "这里修改的是 codexmanager-service 所在机器的 Codex 配置目录，不一定是当前浏览器所在机器。":
    "These changes affect the Codex profile directory on the machine running codexmanager-service, which may be different from the machine running this browser.",
  "Web / Docker 模式": "Web / Docker mode",
  "当前页面会通过 /api/rpc 写入 codexmanager-service 进程可访问的 Codex profile；Docker 部署时请确认 CODEX_HOME 或挂载卷指向你希望 Codex CLI 使用的配置目录。":
    "This page writes through /api/rpc to the Codex profile accessible by the codexmanager-service process. In Docker deployments, make sure CODEX_HOME or the mounted volume points to the configuration directory you expect Codex CLI to use.",
  "当前运行环境无法访问管理 RPC，暂时不能读取或写入 Codex profile。":
    "The current runtime cannot access management RPC, so it cannot read or write the Codex profile right now.",
  "Profile 迁移警告": "Profile migration warning",
  "当前模式": "Current mode",
  "Codex profile": "Codex profile",
  当前账号: "Current account",
  "当前平台 Key": "Current platform key",
  "最后应用": "Last applied",
  刷新状态: "Refresh status",
  "正在使用": "Active",
  账号直连: "Direct account",
  "OpenAI 账号": "OpenAI account",
  选择账号: "Select account",
  "直连 OpenAI 官方后端，不经过 CodexManager 网关；本机 Codex 的直连请求不会写入网关日志，但仪表盘仍会展示已记录的其他网关流量。":
    "Connect directly to the official OpenAI backend without using the CodexManager gateway. Direct requests from Codex on this machine are not written to gateway logs, but the dashboard still shows other recorded gateway traffic.",
  "没有可用于账号直连的 active OpenAI 账号。":
    "No active OpenAI account is available for direct account mode.",
  "去添加 OpenAI 账号": "Add OpenAI account",
  "正在读取可用账号...": "Loading available accounts...",
  "可用账号数：{count}": "Available accounts: {count}",
  "重新应用账号直连": "Reapply direct account",
  "切换到账号直连": "Switch to direct account",
  本地网关: "Local gateway",
  "通过 CodexManager 本地网关转发 Codex CLI 请求；请求日志、Token、费用估算和仪表盘统计可用。":
    "Route Codex CLI requests through the CodexManager local gateway. Request logs, tokens, cost estimates, and dashboard analytics will be available.",
  "没有可用于本地网关的平台密钥。":
    "No platform key is available for local gateway mode.",
  "去创建平台密钥": "Create platform key",
  "选择平台密钥": "Select platform key",
  "将使用 gateway base_url": "Gateway base_url in use",
  "启用 Responses WebSocket": "Enable Responses WebSocket",
  "关闭时 Codex 使用普通 Responses HTTP 流；开启时使用 Responses WebSocket。仅在当前平台密钥支持时开启。":
    "When disabled, Codex uses the regular Responses HTTP stream. When enabled, it uses Responses WebSocket. Enable it only when the selected platform key supports it.",
  "重新应用本地网关": "Reapply local gateway",
  "切换到本地网关": "Switch to local gateway",
  "保存失败": "Save failed",
  "切换失败": "Switch failed",
  "修复失败": "Repair failed",
  "恢复失败": "Restore failed",
  "清理完成但有警告": "Cleanup completed with warnings",
  "历史修复完成但有警告": "History repair completed with warnings",
  "历史会话可见性已修复": "History visibility repaired",
  "历史会话已与当前模式一致": "Historical sessions already match the current mode",
  "Codex profile 路径已保存": "Codex profile path saved",
  "已切换到账号直连": "Switched to direct account",
  "已切换到本地网关": "Switched to local gateway",
  "已恢复接管前的 Codex 配置": "Restored the pre-managed Codex configuration",
  "已清理 {count} 份历史备份，释放 {bytes}":
    "Cleaned {count} history backups and freed {bytes}",
  "高级与恢复": "Advanced and recovery",
  "修改 profile 目录、gateway base_url、修复历史会话或恢复接管前配置。":
    "Adjust the profile directory, gateway base_url, repair history visibility, or restore the original managed configuration.",
  "Profile 目标目录": "Target profile directory",
  "默认使用 CODEX_HOME 或 service 用户的 ~/.codex。":
    "By default, CODEX_HOME or the service user's ~/.codex is used.",
  "Codex profile 目录": "Codex profile directory",
  "OpenAI gateway base_url": "OpenAI gateway base_url",
  "Gateway base_url": "Gateway base_url",
  "auth.json": "auth.json",
  "config.toml": "config.toml",
  "CodexManager 管理文件": "CodexManager managed files",
  管理标记: "Management marker",
  可写: "Writable",
  是: "Yes",
  "否或未知": "No or unknown",
  "默认使用当前 Web 服务可访问的本地网关地址。":
    "By default, use the local gateway address reachable from the current Web service.",
  "使用当前网关": "Use current gateway",
  "恢复与历史会话": "Restore and history",
  "切换模式时会自动修复历史会话 provider 元数据；Codex 运行中锁库时可手动重试。":
    "Switching modes automatically repairs provider metadata for historical sessions; if Codex is holding the database lock, retry manually after closing it.",
  "历史会话可见性": "History visibility",
  "切换 direct / gateway 时会自动修复历史会话的 provider 元数据。":
    "Switching between direct and gateway modes automatically repairs provider metadata for historical sessions.",
  "修复历史可见性": "Repair history visibility",
  "目标 provider": "Target provider",
  "已修复 rollout / SQLite / session_index": "Repaired rollout / SQLite / session_index",
  备份目录: "Backup directory",
  警告: "Warning",
  "历史修复备份": "History repair backups",
  "备份保存在 CodexManager 数据目录，不再写入 Codex profile。":
    "Backups are stored in the CodexManager data directory and are no longer written into the Codex profile.",
  "清理历史备份": "Clean history backups",
  "数量 / 占用": "Count / size",
  保留策略: "Retention policy",
  备份: "Backup",
  已保存: "Saved",
  暂无: "None",
  "最多 {count} 份，最多 {days} 天，至少保留最新 {min} 份":
    "Keep up to {count} backups, keep them for up to {days} days, and always retain the latest {min} backups.",
  "恢复接管前配置": "Restore pre-managed configuration",
  "切换后重载 Codex 后台": "Reload Codex background services after switching",
  "开启后只向使用当前 Codex profile 的 app-server 发送重载信号，不会终止前台 Codex CLI；关闭后，现有进程会在下次启动时读取新配置。":
    "When enabled, only app-server processes using the current Codex profile receive a reload signal; foreground Codex CLI sessions are not terminated. When disabled, running processes read the new configuration on their next start.",
  "配置已切换；现有 Codex 进程将在下次启动时生效":
    "Configuration switched; running Codex processes will pick it up on their next start",
  "配置已切换，但 Codex 后台重载有警告":
    "Configuration switched, but the Codex background reload reported a warning",
  "已请求重载 {count} 个 Codex 后台进程":
    "Requested reload for {count} Codex background process(es)",
  "未发现需要重载的 Codex 后台进程":
    "No Codex background process needed reloading",
  "选择 Codex 直接连接 OpenAI、直连聚合 API，或通过 CodexManager 进行转发与管理。":
    "Choose direct OpenAI, a direct aggregated API connection, or CodexManager routing and management.",
  "直连聚合 API": "Connect directly to an aggregated API",
  "当前聚合 API": "Current aggregated API",
  "已切换为直连聚合 API": "Now connecting directly to the aggregated API",
  "Codex 直接连接所选聚合 API；CodexManager 只负责写入和备份配置，不参与请求转发或日志统计。":
    "Codex connects directly to the selected aggregated API. CodexManager only writes and backs up the configuration and does not route or log requests.",
  "Codex 直接请求所选聚合 API，不经过 CodexManager 网关；切换时会自动写入对应的 Responses base_url 与认证配置。":
    "Codex sends requests directly to the selected aggregated API without using the CodexManager gateway. Switching writes the matching Responses base_url and authentication settings.",
  "没有可直连的聚合 API；仅支持 active 的 Codex / Compatible Responses API-key 条目。":
    "No aggregated API can be connected directly. Only active Codex or Compatible Responses API-key entries are supported.",
  "去配置聚合 API": "Configure aggregated APIs",
  "选择聚合 API": "Select an aggregated API",
  "请选择聚合 API": "Select an aggregated API",
  "所选聚合 API": "Selected aggregated API",
  "由聚合 API 提供": "Provided by the aggregated API",
  "该条目的模型覆盖会作为直连默认模型；网关模式中用于上游重写。":
    "This entry's model override becomes the direct-mode default model and rewrites the upstream model in gateway mode.",
  "直连 base_url：{baseUrl}": "Direct base_url: {baseUrl}",
  "重新应用直连聚合 API": "Reapply direct aggregated API",
  "切换为直连聚合 API": "Switch to direct aggregated API",
};
