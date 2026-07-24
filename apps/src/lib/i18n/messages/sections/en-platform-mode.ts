"use client";

import type { MessageCatalog } from "../types";

export const EN_PLATFORM_MODE_MESSAGES: MessageCatalog = {
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
  "选择接入方式后，CodexManager 会接管该 Codex profile 的 auth.json / config.toml。":
    "After you choose a connection, CodexManager manages auth.json and config.toml for this Codex profile.",
  "请统一在 Codex 接入方式页面切换连接，避免 provider、模型目录和运行时重载配置彼此不一致。":
    "Switch connections from the Codex Connection page so the provider, model catalog, and runtime reload settings stay consistent.",
  "打开 Codex 接入方式": "Open Codex Connection",
  "先确认 CodexManager 本地服务可用，再选择 Codex 接入方式。":
    "Confirm that the local CodexManager service is available, then choose how Codex connects.",
  "直接连接 OpenAI 需要 active 账号；通过 CodexManager 需要可用的平台密钥。":
    "A direct OpenAI connection requires an active account; CodexManager routing requires an available platform key.",
  "第三步：应用 Codex 接入方式": "Step 3: Apply the Codex connection",
  "选择接入方式与目标后点击应用，页面会调用现有 profile 接口写入配置。":
    "Choose the connection and target, then apply it. The page writes the configuration through the existing profile API.",
  "选择直接连接 OpenAI 或通过 CodexManager 后，CodexManager 会接管该 Codex profile 的 auth.json / config.toml。":
    "After you choose direct OpenAI or CodexManager, CodexManager manages auth.json and config.toml for this Codex profile.",
  "无需手动编辑 auth.json 或 config.toml。CodexManager 会通过 Codex 接入方式页面安全写入并备份 Codex profile。":
    "You do not need to edit auth.json or config.toml manually. CodexManager safely writes and backs up the Codex profile from the Codex Connection page.",
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
  "直连 OpenAI 官方后端，不经过 CodexManager 网关；不会产生 CodexManager 请求日志，仪表盘用量统计不可用。":
    "Connect directly to the official OpenAI backend without going through the CodexManager gateway. CodexManager request logs and dashboard usage analytics will not be available.",
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
};
