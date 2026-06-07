import type { MessageCatalog } from "../types";

export const EN_RUNTIME_UI_MESSAGES: MessageCatalog = {
  打开: "Open",
  后刷新: " until refresh",
  本地服务已连接: "Local service connected",
  等待本地服务: "Waiting for local service",
  正在同步状态: "Syncing status",
  状态读取失败: "Failed to read status",
  关闭: "Close",
  "关闭 {label}": "Close {label}",
  "请通过 `codexmanager-web` 打开页面，或在反向代理中同时提供 `/api/runtime` 与 `/api/rpc`。":
    "Open this page through `codexmanager-web`, or expose both `/api/runtime` and `/api/rpc` in the reverse proxy.",
  压缩: "Compact",
  转发: "Forward",
  规范来源: "Canonical source",
  大小拒绝阶段: "Size rejection stage",
  原始路径: "Original path",
  "这里保留旧版和外部部署环境变量覆盖；普通用户优先使用前面结构化设置，高风险项只建议排障时临时修改。":
    "Legacy and external deployment environment overrides are kept here. Regular users should prefer the structured settings above; high-risk items are recommended only for temporary troubleshooting.",
  "会影响运行时配置；修改后请观察请求链路是否稳定。":
    "This affects runtime configuration. After changing it, monitor whether the request path remains stable.",
  删除条目: "Delete entry",
  "上游 Originator": "Upstream Originator",
  区域驻留要求: "Residency requirement",
  "后，局域网设备可通过当前机器 IP 访问；设置保存后需要重启相关进程才会生效，Web 监听地址会默认跟随这里的模式。":
    "then LAN devices can access this machine by its current IP. After saving, restart the related processes for changes to take effect. The Web bind address follows this mode by default.",
  "赞助 / 推荐": "Sponsors / Recommendations",
  赞助支持: "Sponsor support",
  持续维护中: "Actively maintained",
  "这里集中展示 README 里的赞助信息、推荐服务，以及作者联系入口。":
    "This page gathers sponsorship information, recommended services, and author contact entry points from the README.",
  赞助商: "Sponsor",
  "沿用 README 的展示内容，并同步星思研邀请链接。":
    "Uses the README presentation and keeps the XingSiyan invitation link in sync.",
  服务器推荐: "Server recommendation",
  "补充一个常用服务器选择，便于直接部署或长期运行服务。":
    "Adds a commonly used server option for direct deployment or long-running services.",
  联系作者: "Contact author",
  "需要反馈问题或进一步沟通时，可以通过微信或 TG 群联系作者。":
    "For issue feedback or further communication, contact the author through WeChat or the TG group.",
  联系方式: "Contact methods",
  微信: "WeChat",
  "扫码可直接添加作者微信，也可以手动搜索上面的微信号。":
    "Scan the code to add the author's WeChat, or manually search the WeChat ID above.",
  "加入 TG 群聊": "Join TG group",
  "README 里维护的官方群链接，打开后即可加入讨论。":
    "The official group link maintained in the README. Open it to join the discussion.",
  "打开链接失败：{message}": "Failed to open link: {message}",
  未知错误: "Unknown error",
  "跟随请求表示使用请求体里的实际 model；请求日志展示的是最终生效模型。":
    "Follow request means using the actual model from the request body; request logs show the final effective model.",
  在图表区域使用鼠标滚轮缩放时间区间:
    "Use the mouse wheel over the chart area to zoom the time range",
  支付宝赞助码: "Alipay sponsor QR code",
  "如果这个项目帮你省了时间，可以请作者喝杯咖啡。":
    "If this project saved you time, you can buy the author a coffee.",
  微信赞助码: "WeChat Pay sponsor QR code",
  "项目持续维护、修问题和做适配，欢迎随缘支持。":
    "The project is continuously maintained, fixed, and adapted. Casual support is welcome.",
  作者微信二维码: "Author WeChat QR code",
  内置精选: "Built-in picks",
  "默认使用官方精选插件，适合开箱即用。":
    "Use the official curated plugin catalog by default; suitable for out-of-the-box usage.",
  自定义源: "Custom source",
  "接入你自己的远程 JSON 市场源。":
    "Connect your own remote JSON marketplace source.",
  已安装: "Installed",
  未安装: "Not installed",
  更新: "Update",
};
