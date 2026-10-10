# 更新日志

本文件用于记录 CodexManager 的对外可见变更，作为版本历史的唯一事实源。
格式参考 Keep a Changelog，并结合当前项目的实际维护方式做最小收敛。

## [Unreleased]

## [0.6.3] - 2026-09-30

### Added

- 设置页和顶部栏的界面语言新增“自动检测”，跟随已支持的系统或浏览器语言，其他语言回退英文；手动选择的语言继续持久化保存。
- 模型目录 revision 10 内置 `gpt-6.1-sol`，同步官方模型介绍、推理档位，以及短/长上下文的输入、缓存输入、缓存写入和输出价格；保留同名 custom、用户编辑及删除记录。
- 模型管理页可为每个聚合或自定义模型选择支持的推理强度档位及默认档位；生成的 Codex 模型目录据此启用思考强度选择器（#481）。

### Changed

- 收紧桌面导航、设置、仪表盘、平台模式和模型管理页面的布局密度与状态呈现，统一支持的多语言界面细节。

### Fixed

- Linux 打包新增最终 AppImage 的 `.DirIcon` 校验与修复，解决 AppImageHub 目录检查因缺少图标元数据而失败的问题。
- 改进流式响应终态投递跟踪和桥接诊断日志，区分已向客户端投递终态、上游未完成和客户端投递失败等情况。

## [0.6.2] - 2026-09-25

### Added

- 模型目录 revision 9 新增 `gpt-6-sol`、`gpt-6-luna`、`gpt-image-2.5-sunburst` 和 `gpt-image-2.5-flare`；fresh 目录现在包含 11 个 builtin，其中 10 个默认可见，`codex-auto-review` 保持隐藏。

### Changed

- 从 builtin 目录移除已于 2026-08-31 从 ChatGPT 登录的 Codex 退役的 `gpt-5.4`、`gpt-5.4-mini`，以及在该登录方式下 deprecated 的 `gpt-5.2`；通用 API Key 仍可通过 custom 模型使用 `gpt-5.2`。revision 9 会删除未定制 builtin 及其默认账号池 route；用户编辑、自定义价格或 tiers、非默认 routes 与 permission/API Key 关联会转为 custom 保留。
- free-account 模型上限选项改为基于当前文本模型目录生成；已持久化但不再属于当前候选目录、图片专用或旧 fallback 的值统一归一化为 `auto`。账号预热和账号文本测试的 fallback 改为 `gpt-6-luna`，观测桥接的缺省模型改为 `gpt-6-sol`。
- 价格同步继续在未勾选时保护自定义价格；明确勾选模型后，允许外部价格覆盖所选模型的自定义价格，未选中的自定义模型不会被触碰。
- 发布版本提升到 `0.6.2`，同步 workspace、前端、Tauri 桌面端和锁文件。

### Fixed

- 修复桌面端开发启动时 pnpm 命令解析和 Next 代理的启动竞态；页面隐藏后会停止页面级轮询和后台任务，账号测试与 SSE 在终态主动清理监听。
- 修复 service、gateway、账号和 usage 路径的资源生命周期、请求取消、队列指标与并发边界，减少旧同步桥接在生产路径中的残留。
- 强化旧 SQLite 模型目录自动迁移：加入迁移锁、在线备份、迁移前检查、大小写冲突兼容和迁移后冒烟检查；迁移失败时自动恢复备份，避免升级过程中破坏原库。
- 为 GHCR Release Compose 增加可选 Docker OTA profile；默认只检查并等待用户确认，用户明确启用 `ota-auto` 后才会自动更新带标记的 service/web 镜像，固定版本标签仍保持不变。

## [0.6.1] - 2026-09-24

### Added

- OpenAI 账号池新增默认开启的“额度重置自动唤醒”：标准 5 小时额度耗尽后，在上游重置时间到达时自动发送预热消息并刷新用量；支持单个和批量启停，持久化每周期尝试记录，跳过停用账号与尚未恢复的周额度。
- Codex 接入方式新增聚合 API 直连，可选择 active 的 Codex / Compatible Responses API-key 条目并沿用其认证请求头与 User-Agent，无需经过本地网关（#475）。

### Changed

- 完成服务端异步化与 SeaORM 存储迁移的生产收尾，补齐远程账号、聚合 API、API Key、用量、日志与网关路径的事务、关闭和生产验收门禁。
- 模型管理页统一状态卡样式；“应用模型”未勾选时默认应用完整目录，勾选后仅应用所选模型，并同步模型价格与目录元数据。
- 模型删除改为实际删除；内置模型删除后记录墓碑，后续启动或补种不会自动恢复。
- 发布版本提升到 `0.6.1`，同步 workspace、前端、Tauri 桌面端和锁文件。

### Fixed

- 修复聚合 API 已成功返回后，Windows 客户端断开被错误记为 502 的问题；`os error 10053/10054/10058` 等下游断连现在按客户端中断处理（#479）。
- 修复异步服务迁移后的账号、聚合 API、额度重置与候选路由边界问题，并加强事务回滚与生产配置校验。

## [0.6.0] - 2026-09-05

### Fixed

- 修复订阅查询新增 User-Agent 后触发网页防护、导致额度刷新失败并进一步显示 Token 授权错误的回归；订阅查询恢复不设置 User-Agent。
- 额度查询和 Token 刷新恢复标准 Codex User-Agent，不再继承自定义网关 User-Agent，覆盖默认代理和账号独立代理路径。

### Changed

- 发布版本提升到 `0.6.0`，同步 workspace、前端、Tauri 桌面端和锁文件；沿用已有账号与设置存储。
- `release-all` 将五个平台的服务端构建与桌面构建并行，保留原有桌面、Service、Web 和 Docker 产物；按 Cargo 工作区隔离构建缓存，推荐从主分支发起固定提交的发布以复用跨版本缓存。

## [0.5.7] - 2026-09-05

### Added

- 账号编辑器新增“额度耗尽后仍使用账号”开关，持久化为 `force_enabled`，默认关闭；开启后该账号仍可参与网关候选，适合需要人工接管额度判定的场景。
- 模型目录默认新增 `gpt-6-astra`；fresh DB 和升级数据库都会通过 revision 8 数据迁移获得其官方 API 限制与含缓存写入的价格、Codex 运行元数据及默认账号池路由，同时保留同名 custom 模型和用户已编辑的 builtin。
- 账号页新增会记住选择的表格/卡片视图切换；管理员可针对明确选中的 ChatGPT 账号，使用该账号的凭据、账号标识与代理拉取上游模型，并选择性关联到模型目录 V2 的账号池路由，不会在账号池中轮转请求。
- API Key 服务等级新增 Standard 与 Ultrafast，并贯通 Responses HTTP、WebSocket 和请求日志：Standard 使用上游默认速度并记为 `standard`，Fast 映射为 `priority`，Ultrafast 原值透传；继续接受 Flex 以保持兼容，实际可用等级仍由模型与上游决定。
- 新增可配置的网关出站 `User-Agent`：`gateway.user_agent` 全局生效，单个聚合 API 的 `aggregate_apis.user_agent` 在真实 route 转发、模型拉取、连通性测试和余额查询中优先；两者均未设置时动态生成 Codex-compatible 默认值（#459）。

### Changed

- 发布版本提升到 `0.5.7`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

### Fixed

- 修复 Responses WebSocket 的工具输出续接：同一工具调用的重复输出会在上游发送前去重，连接前导事件不再错误消耗终止恢复机会，并可在跨任务续接时有界补回缺失的工具调用上下文（#458）。
- 修复 Luna Reserve 用量在后续刷新或暂态空列表后从账号页消失的问题，并统一兼容附加额度的 snake_case/camelCase 返回结构。
- 修复 Luna Reserve 路由边界：仅显式请求 `gpt-reserve` 时，仍有 Reserve 额度的账号才进入 Reserve 候选池；普通 `gpt-5.6-luna` 仍走常规账号池，不会因 Reserve 可用而绕过已耗尽的标准 5h/7d 窗口；硬性授权或停用状态仍按原规则处理。

## [0.5.6] - 2026-09-02

### Added

- 新增管理员专用的“测试账号”功能，可向真实上游发起文字或图片模型测试，并通过按不可猜测 `testId` 隔离的 Tauri 事件或 Web SSE 实时展示进度、结果和取消状态；账号状态以 `status + updated_at` CAS 安全回写，旧任务不会覆盖新状态，`banned`、`disabled` 和 `inactive` 也不会被自动恢复（#452）。
- 新增“混合轮转（聚合 API 优先）”策略：优先尝试聚合 API，仅在候选全部失败且请求尚未被消费时安全回落账号池；仅聚合模型不会绕过其绑定路由（#454）。
- 聚合 API 管理页支持拉取上游模型并选择性关联到模型目录 V2 route，无需恢复旧模型来源表或自动关联链路。

### Changed

- 发布版本提升到 `0.5.6`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 改进 prompt-cache/cache affinity 路由：按平台 Key、协议和模型隔离 `prompt_cache_key` 或根 session ID，原子收敛并发冷启动绑定，让父子任务及流式请求稳定复用同一账号，并仅在回落成功后重新绑定；管理员仪表盘新增缓存命中率。

### Fixed

- 修复部分强校验供应商的聚合 API 连通性测试误报：probe 默认模拟 Codex 官方客户端标识，也可配置自定义 `User-Agent`，且不改变正常 route 转发（#451）。
- 修复 Codex Device Code 登录生命周期、取消和浏览器回调竞争，并同步托管 gateway profile 的认证与 WebSocket 能力。
- 修复原生 Codex 图片生成请求的识别与路由：扩展请求可直达 Codex 图片端点并保留必要标识，普通 OpenAI-compatible 客户端继续使用兼容适配路径。
- 桌面更新器会清理已完成或废弃的版本目录，同时保留待安装版本和更新日志。

## [0.5.5] - 2026-08-23

### Fixed

- 修复关闭启动时显示主界面后，桌面恢复到前台时服务短暂不可用的问题。
- 启动和恢复流程现在会合并窗口导航，等待首页就绪后再显示窗口，并优先复用已初始化的服务，减少重复启动和预热请求。

## [0.5.4] - 2026-08-23

### Changed

- 发布版本提升到 `0.5.4`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 对齐 Responses WebSocket 官方语义：增加有界心跳与恢复、连接上限续接、大图像帧支持、压缩协商回退、首帧/前导事件安全重放和账号重新选择，并在恢复耗尽后保留 HTTP fallback。
- 提高 CI 的 Cargo Git 依赖获取稳定性：使用 CLI fetch、网络重试、锁定依赖预取和缓存，但不重试实际构建或测试失败。
- 更新 AIXiamo 赞助入口、教程链接和相关说明。

### Fixed

- 修复关闭启动时显示主界面后，桌面主窗口初始化、刷新和服务自动启动不可靠的问题。
- 修复切换到直连 OpenAI 后历史对话仍引用 `cm` provider 的兼容性问题；直连模式现在明确使用内置 `openai` provider，并避免切换时扫描重写整个 Codex 历史目录。
- 兼容 Codex 0.149 的托管网关认证，自动为 `cm` provider 写入 `requires_openai_auth`，避免请求缺少 API key。
- 修复 Responses WebSocket 早期断连、连接时限、首帧失败、前导事件续接、`store=false` 上下文和账号资格变化等场景下的错误恢复；已产生实质输出后不会静默重复请求。
- 修复部分非成功上游响应的账号切换和默认模型组路由语义。

## [0.5.3] - 2026-08-08

### Fixed

- 修复同一团队下添加第二个账号会覆盖已有账号的问题，按登录主体隔离账号身份。
- 流式请求遇到可切换的上游错误时，同一账号最多重试一次，仍失败后再切换账号，减少限频恶化。

## [0.5.2] - 2026-07-31

### Changed

- 发布版本提升到 `0.5.2`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 模型目录与请求计费支持缓存写入 Token 价格，并补齐 Anthropic、Gemini 与 OpenAI 风格响应中的缓存写入用量采集。
- 适当增大宽屏侧栏菜单的纵向间距，低高度窗口继续使用紧凑布局以避免溢出。

### Fixed

- 非 `2xx` 失败请求不再计入 Token 与预估费用汇总；客户端取消的 `499` 仍保留可能已产生的实际钱包扣费，但不会进入成功用量统计。

## [0.5.1] - 2026-07-26

### Added

- 恢复“Free 账号模型上限”设置；可选择 `auto`（不限制）或模型目录中的具体上限。

### Changed

- 发布版本提升到 `0.5.1`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 清理低风险 Rust 告警，并仅在 Windows MSVC 目标上抑制 `link.exe` 正常建库信息产生的 `linker_messages` 噪声，其他编译告警仍保持可见。

### Fixed

- Free 账号模型上限现在只过滤超过上限的 Free 账号候选，不会改写请求模型；Plus / Pro 账号不受影响，额度保护仍在过滤后执行，混合账号优先模式在账号耗尽后仍按现有策略回退到聚合 API。

## [0.5.0] - 2026-07-23

### Added

- 账号授权新增 `chatgpt.com` Device Code 登录，支持展示/复制验证码、打开验证页、自动轮询、取消会话和过期处理；浏览器授权及其手动回调解析保持可用（#370）。
- `/skills/` 现按“Skills 安装 / Codex 插件安装”分栏：Skills 可从内置或自定义 GitHub 技能仓库、skills.sh 搜索结果、ZIP 和已有目录单独安装，并支持仓库后台刷新与安全卸载；Codex 原生 Marketplace 继续提供标准插件整包安装，`.system` 内置 Skill 只读（#126）。
- 新增 `gpt-image-2`（快照 `gpt-image-2-2026-04-21`），作为第 9 个 builtin、普通列表中的第 8 个模型。该目录项仅用于 `/v1/images/generations` 与 `/v1/images/edits`，不会作为文本生成、Chat Completions 或直接 Responses 主模型；目录价格元数据记录官方每 1M image input / cached input / output tokens 8 / 2 / 30 USD。

### Changed

- 发布版本提升到 `0.5.0`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 清理前端 lint 错误与警告，恢复干净的发布前质量检查。

### Fixed

- 修复混合轮转下模型仅配置聚合 API 路由时仍优先请求本地账号池的问题；现在仅聚合路由会直接走已绑定的聚合 API，双路由保持账号优先、聚合兜底，仅账号池路由也不会误用聚合 API（#381）。
- 修复 Windows 托盘预览窗口未可靠应用最新尺寸的问题；窗口创建或复用时都会重新应用 `360 × 430` 的逻辑尺寸和固定约束，避免内容裁切或沿用旧窗口大小（#380）。
- 修复 Responses WebSocket 在上游先发送 `response.created`、随后以嵌套 `response.failed` 报告额度耗尽时无法自动换号的问题；现在会暂存无内容的生命周期事件，正确识别嵌套错误，立即将耗尽账号标记为受限，并在下一可用账号上静默重放当前请求。
- 聚合 API 新增 `compatible` 通用兼容类型：同一条 URL 和密钥可同时承接 Codex/OpenAI 与 Claude 原生协议请求，按客户端请求路径原样转发，无需重复创建供应商记录（#359）。
- 修复仅从 ChatGPT `/api/auth/session` 导入 `accessToken` 的账号调用 Codex 上游时返回 401：HTTP、Responses WebSocket 与账号预热会按需注册和持久化 AgentIdentity，校验账号绑定后使用 AgentAssertion，并兼容官方嵌套 snake_case / camelCase AgentIdentity 格式（#376）。
- 修复 `/api/auth/session` 账号显示为“导入账号000x”的问题；现在会从 accessToken 的 OpenAI profile 或会话用户信息解析显示名，并在不覆盖手工名称的前提下修复旧占位显示名。
- 按 OpenAI 官方 API 价格修正 GPT-5.6 Sol、Terra、Luna 计费：缓存输入恢复 90% 折扣，并在 272K 输入阈值切换到 2 倍输入、1.5 倍输出的长上下文费率；旧版自动估算价格会迁移为官方价格，自定义价格保持不变。

## [0.4.4] - 2026-07-20

### Changed

- 发布版本提升到 `0.4.4`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- `release-all` 工作流支持推送 `v*` 标签自动触发发布，便于直接通过版本标签发版。

### Fixed

- 修复账号池“刷新用量”在账号缺少 token 或无可刷新目标时仍直接提示成功的问题；服务端现在返回结构化执行结果，前端会区分未执行、部分刷新和成功，并刷新账号列表与用量缓存。
- 修复后台任务线程保存更短间隔或关闭开关后要等待旧休眠周期结束才生效的问题；用量轮询、网关保活和令牌刷新轮询会在等待中重新读取动态配置。

## [0.4.3] - 2026-07-19

### Added

- 新增账号级代理管理（#314）：支持创建可复用的代理配置、为单个账号选择代理，并在账号请求、令牌刷新和用量刷新链路中使用对应代理。
- 新增代理连通性、延迟、下载及上传测试，统一展示运行状态、出口 IP、国家/地区与测速结果；桌面端和 Web 服务模式均可配置和测试。
- 模型目录新增多选批量路由分配：可将账号池或聚合 API 路由追加/更新到多个模型，也可选择替换所选模型的全部现有路由；上游模型名会按各模型标识自动填写。

### Changed

- 精简 AIXiamo 赞助说明，突出国内无海外银行卡用户的 ChatGPT、Claude、Codex 充值与售后保障信息。
- 收紧 Codex 网关识别规则并优先使用当前供应商网关地址，避免将普通 OpenAI 兼容端点误判为 Codex；同时保留 compact、Gemini 及多候选回退行为（#346）。
- 完善原生 Web Search、Image Generation、`/v1/models` 和 Responses WebSocket 的协议兼容，并支持有界解压 zstd 编码内容（#363）。
- 发布版本提升到 `0.4.3`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

### Fixed

- 模型管理页的“批量分配路由”入口现在对管理员始终可见；未选择模型时会显示明确提示并保持禁用，避免功能被误认为缺失。
- 修复部分 Responses 请求收到 400 后无法继续候选重试的问题；重试时会移除不适用的 session/thread 请求头，并正确保留加密内容容器。
- 修复 hosted image tool 与 Codex image generation 命名空间冲突，以及旧 Responses 路径抑制范围过宽的问题。
- 修复 Windows 托盘右键菜单可能立即关闭、点击时刷新引发回归，以及用量更新后菜单文案未及时刷新的问题（#362）。
- 修复 Windows 玻璃效果下顶栏徽标文字模糊（#360），以及侧栏 Logo 和选中图标出现白色填充的问题（#361）。
- 补全模型目录简体中文展示，统计项、筛选器、表头、来源、价格/指令/路由状态及内置模型说明不再混用英文。
- 修复合并功能后的 release 编译、前端 lint/effect、端到端选择器及 Service 并行测试隔离问题，恢复完整发版验证。

## [0.4.2] - 2026-07-19

### Changed

- 管理员仪表盘的用量指标卡合并展示今日总用量、缓存输入与推理输出用量。
- 发布版本提升到 `0.4.2`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

### Fixed

- 修复已清空的请求日志重新出现的问题；计费记录会从操作日志查询中隐藏，同时保留不可变的用量与账单审计数据。
- 修正聚合 API 的 Responses 连通性探针，改为发送 `input_text` 内容，保留上游错误详情，并在界面中展示测试失败详情。
- 修复 `codexmanager-web` 的 `/v1/responses` WebSocket 握手被降级为普通 GET 并返回 405 的问题；Web 网关现在会在保留鉴权与 Codex 请求头的同时双向转发 WebSocket 帧，自动接管和首次引导配置也会显式启用 Responses WebSocket。
- 将 README 的 Star History 远程曲线嵌入替换为仓库内图片，避免曲线无法显示。

## [0.4.1] - 2026-07-14

### Changed

- 模型目录一次性切换到 V2：fresh DB 内置 8 个 builtin（普通列表显示 7 个），模型、整数价格阶梯、routes、权限组和 instructions policy 由单个事务保存。
- Gateway、本地 `/v1/models`、模型组和模型选择器只读取 V2；删除远端目录 merge/prune、供应商 `/models` 发现、请求时 bootstrap 和编译期价格 fallback。
- 模型管理页新增本地 JSON preview/commit、price missing、route/instructions 编辑；builtin 只能禁用，custom 可以删除。
- `models_cache.json` 改为桌面/Web 都只由用户主动导出，且导出不包含模型人格 prompt。
- Codex 缓存导出补齐 GPT-5.6 的 Max/Ultra、`multi_agent_version`、`tool_mode`、Responses Lite、最大上下文和 comp hash 等运行元数据；显式 Ultra 请求按“客户端 Ultra、上游 Max”分别记录。
- 钱包扣费改为整数价格 tier、不可变 request charge snapshot 与幂等 ledger 同事务更新。
- 为 GPT-5.6 Sol、Terra、Luna 写入 5/30、2.5/15、1/6 USD / 1M tokens 的默认 input/output 价格；缓存输入暂按 input 同价保守计费，并只迁移仍为 missing 的未编辑 builtin。
- 发布版本提升到 `0.4.1`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

### Fixed

- 修复旧 V2 builtin 下划线 ID 在启动迁移时触发 SQLite 外键错误 787，导致桌面端拒绝启动的问题。
- 修复模型目录编辑器打开既有模型时出现空白草稿，并补齐中文界面的字段名、控件标题和可访问名称。
- 修复管理员深链接在桌面会话初始化时被重置到首页的问题。
- 修复 Codex 请求改写在候选上游之间串扰，确保每个候选使用隔离的请求体。

## [0.4.0] - 2026-06-24

### Changed
- 简化 Codex 首次接入引导，只保留 `auth.json` 与 `config.toml` 的最小必要配置。
- 发布版本提升到 `0.4.0`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

## [0.3.9] - 2026-06-11

### Added
- 按状态清理账号新增“未知状态”删除选项。
- 聚合 API 禁用后保留已发现模型池与模型映射，重新启用时可继续复用。

### Fixed
- 修复新导入账号在刷新用量状态处理中未覆盖不可用状态的场景。
- 固定同一会话的 `prompt_cache_key`，避免同一会话被重新分配缓存 key。
- 覆盖会话结束后刷新令牌失效的清理场景。
- `CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL` 默认改为 `0`，普通 Responses 请求默认不再自动注入 `image_generation` tool。

### Changed
- 增强上游中转兼容观测信息。
- 发布版本提升到 `0.3.9`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

## [0.3.8] - 2026-05-30

### Fixed
- 修复代理只保留 `x-codex-turn-state` 且请求仍带 `prompt_cache_key` 时，本地 prompt cache 账号绑定无法复用的问题，并补齐 `anchor_mode` 诊断日志。
- 修复 `/v1/messages/count_tokens` 可能被误转发成真实 Responses 生成的问题，改为本地 token 估算。
- 增强 Anthropic-native / Claude Code 兼容路径对 Cloudflare challenge 的处理：禁用兼容流 `zstd`、加入 challenge 冷却与候选切换，并让低额度账号只作为尾部兜底。
- 将 Claude Code 2.x 会话中段的 `system` message 归一为 Responses `developer` role，降低 ChatGPT Codex 后端触发 Cloudflare challenge 的概率。

### Changed
- 发布版本提升到 `0.3.8`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

## [0.3.7] - 2026-05-27

### Added
- `release-all.yml` 新增 Linux arm64 发布目标，统一进入多平台发版流水线。

### Fixed
- 修复 OpenAI 兼容 `/v1/responses` 的 `prompt_cache_key` 本地账号路由不稳定问题；相同客户端 key 会绑定复用同一上游账号，并且上游请求继续保留客户端原始 key。
- 修复短 `prompt_cache_key`（如 `pc_1`）未参与本地账号绑定的问题，避免 balanced 轮询把同一缓存前缀分散到不同账号。
- 修复 ChatGPT 上游传输错误未按候选账号继续 failover 的问题。
- 修复 Responses WebSocket 上游握手头缺失 beta 标记的问题。
- 修复 `/v1/messages` 兼容流对 `message_stop` 终止事件识别不一致的问题。
- 修复账号 upsert 时已有 token 可能被清空的问题。

### Changed
- 发布版本提升到 `0.3.7`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

## [0.3.6] - 2026-05-25

### Added
- 新增定时账号预热功能，可按配置周期主动预热账号，降低首次请求时的冷启动失败概率。
- 新增实验性 ChatGPT `/v1/responses` WebSocket 上游传输，设置 `CODEXMANAGER_USE_WEBSOCKET_UPSTREAM=1` 后会优先尝试 WebSocket，失败时回退 HTTP 流式路径，并沿用上游代理、连接超时和协议帧校验。

### Changed
- 请求日志“密钥”列改为展示密钥名称而不是 ID，便于排查调用来源。
- 发布版本提升到 `0.3.6`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

## [0.3.5] - 2026-05-24

### Added
- 设置页新增 compact 模型转发规则配置，支持仅对 `/v1/responses/compact` 请求按规则改写模型。
- 网关 trace/error 日志补齐 request gate 等待、首包耗时和慢请求 stdout 诊断，便于定位高并发流式请求延迟。

### Fixed
- 修复 Anthropic SSE 在工具调用后重复回放 completed 快照，导致客户端重复显示助手文本的问题。
- 修复 Chat Completions 走 Responses 后 reasoning summary 丢失的问题，流式和非流式响应都会回填兼容 reasoning 字段。
- 修复 region-blocked token refresh 会被后台轮询反复重试的问题，区域受限账号会暂停自动刷新。
- 修复 Aggregate API / 模型同步后陈旧 source routes 和孤儿自动目录模型未清理的问题，避免已删除模型重新出现。
- 为 request gate 提供可选等待上限，设置 `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS=5000` 后可避免同 key/path/model 高并发流式请求长时间队头阻塞。

### Changed
- 发布版本提升到 `0.3.5`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。

### Added
- 聚合 API 余额检测现在会跟随账号池用量轮询周期自动刷新，刷新间隔和设置页“用量轮询线程”保持一致。
- 新增 Codex 图片生成兼容链路：默认按官方 Codex 行为为 `/v1/responses` 自动注入 `image_generation` tool，支持显式 tool 透传，并提供 `/v1/images/generations` 与 `/v1/images/edits` 兼容入口，默认图片工具模型为 `gpt-image-2`。
- Codex CLI 首次接入引导新增 `auth.json` 配置步骤，明确平台 Key、`auth.json` 与 `config.toml` 的关系。

### Fixed
- 修复官方返回的 Spark 专属额度未展示的问题，附加额度会按 `additional_rate_limits[].rate_limit` 继续解析并显示。
- 调整额度详情弹窗布局，附加额度较多时可按两列展示并滚动查看。

### Changed
- 发布版本提升到 `0.3.2`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- 发布版本提升到 `0.2.6`，同步更新 workspace、前端包、Tauri 桌面端与锁文件。
- README 不再展示最近提交块，首页只保留稳定的功能与文档入口。
- 设置页恢复“上游总超时”入口，`CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` 可通过网关传输设置直接查看和修改，默认 `0` 表示不按总时长截断。
- Nginx 示例配置新增 `/v1/images/` 专用代理块，覆盖图片上传、大体积 `b64_json` 响应与长耗时图片生成场景。
- 请求日志费用估算同步官方 `gpt-5.5`、`gpt-5.5-pro` 与 `gpt-image-*` 价格；图片模型因当前 usage 无 modality 分桶，按官方 Image token 单价保守估算。

## [0.2.3] - 2026-04-15

### Fixed
- 修复原生 Codex 在线程续接场景下的锚点优先级回归：当请求已携带 `conversation_id` 或 `x-codex-turn-state` 时，不再让请求体里的 `prompt_cache_key` 抢占主导权，减少兼容模式下 resume / 上下文续接异常。
- 补齐原生锚点、显式 `prompt_cache_key`、冲突锚点与 Anthropic 原生链路的专项回归测试，避免后续再次把兼容层字段提升到高于原生 Codex 语义的优先级。

### Changed
- 发布版本提升到 `0.2.3`，同步更新 workspace、前端包、Tauri 桌面端与版本锁文件。

## [0.2.0] - 2026-04-12

### Added
- 主页面导航新增后台 keep-alive 缓存与更显眼的整区加载遮罩，桌面端、Web 版与 Docker 部署在页面切换和空闲后回访时的体感更稳定。
- 请求链路排查补齐针对 `service_tier`、Cloudflare challenge 与兼容转发的专项回归测试，覆盖原生 Codex、Claude Code 与 Gemini CLI 关键路径。

### Fixed
- 收敛 Codex 原生直通路径，默认保持官方请求形状，只保留账号选择、认证替换、路由、会话亲和与必要的内部字段处理；Claude / Gemini 继续走协议适配。
- 对齐官方 Codex 出站行为：发往 `chatgpt.com/backend-api/codex/responses` 的 `service_tier=fast` 现会正确映射为上游 `priority`，`/responses/compact` 不再错误携带 `service_tier`。
- 修复 Claude 兼容链路里 `fast` 服务等级映射与错误流模型回显不稳定的问题，减少 `403` 排查时的误导信息。

### Changed
- 发布版本提升到 `0.2.0`，同步更新 workspace、前端包、Tauri 桌面端与对外版本说明。

## [0.1.19] - 2026-04-08

### Added
- 聚合 API 现已支持多认证类型与自定义 `action` 配置，透传路由会按认证方式与动作正确命中对应上游。
- 国际化新增语言持久化与拆分后的消息目录，补齐了仪表盘、模态窗、侧边栏和用量标签等剩余多语言文案。

### Fixed
- 修复聚合 API 透传流里对 Anthropic `message_stop` 事件的识别问题，减少流式响应提前终止或状态误判。
- 修复网关继续把不受支持的 `service_tier` 发往上游的问题；标准 Responses 请求现在只保留受支持值，避免上游拒绝。
- 调整协作与安全入口页为默认中文正文，并整理多语言文档入口，减少发版后从根文档跳转时的断层感。

### Changed
- 正式文档已按语言目录重组，根入口文档与多语言首页文案同步整理。
- 发布版本提升到 `0.1.19`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.18] - 2026-04-06

### Added
- 新增账号列表直接排序控制与“低额度优先”排序，排查额度紧张账号时更直接。
- 新增 Gemini CLI 基础兼容，补齐 `tools` 流式、MCP 工具名、SSE `tool_call` 等关键链路。

### Fixed
- 修复账号页“额度详情”悬浮卡位置偏移问题，右侧浮层改为按额度概览卡片中线对齐。
- 修复 Gemini completed 工具结果误当正文、流式缓存词元日志采集、请求适配兼容与 token 刷新边界等问题。

### Changed
- 对齐 Gemini → Codex / Responses 请求链路到 CPA 兼容方向，请求会补齐 developer message、tool name 映射、FIFO `call_id`、`reasoning`、`include`、`parallel_tool_calls` 等字段。
- 清理 Gemini 路线未使用代码，并补充 CPA 鸣谢与版本文档说明。
- 发布版本提升到 `0.1.18`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.17] - 2026-04-05

### Added
- 请求日志新增“最终生效服务等级”口径，HTTP / WS 日志现在会同时保留客户端显式 `service_tier` 和请求改写后的最终值，方便核对平台 Key 默认 `Fast` 是否真正发往上游。
- 设置页新增全局“模型转发规则”，支持使用 `pattern=target` 形式做模型名改写，并在运行时请求改写阶段生效。

### Changed
- 普通平台 Key 的协议类型收敛为“通配兼容 (Codex / Claude Code)”，默认按请求路径自动选择 Claude 或 Codex / OpenAI 语义，减少重复维护多套 Key 的成本。
- 发布版本提升到 `0.1.17`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.16] - 2026-04-05

### Added
- 新增 `/v1/responses` WebSocket 请求支持，补齐传输类型识别、请求头归一化、代理运行时与请求日志链路。
- 账号页与用量弹窗新增附加额度窗口展示；刷新后会统一显示标准额度与 Code Review / Spark 等额外额度的剩余额度和重置时间。
- 网关 trace 新增 `CLIENT_SERVICE_TIER` 事件，记录 HTTP / WS 原始请求是否显式携带 `service_tier`、原始值以及日志归一化值，便于快速区分客户端显式 `fast` 与平台 Key 默认服务等级。

### Fixed
- 修复 HTTP 与 WS 请求日志中 `service_tier` 口径不一致的问题；现在仅当客户端请求自己显式携带 `service_tier` 时才记录 `fast`，不再把平台 Key 默认值误记成请求显式开启。
- 修复日志页服务等级展示与网关 on-wire 值不一致的问题；`priority` 会统一展示为 `fast`，未显式携带服务等级的请求继续显示为 `auto`。

### Changed
- 发布版本提升到 `0.1.16`，同步更新 workspace、前端包、Tauri 桌面端、锁文件、README 与 CHANGELOG 的版本说明。

## [0.1.15] - 2026-04-03

### Changed
- 发布版本提升到 `0.1.15`，同步更新 workspace、前端包、Tauri 桌面端、运行文档与 README 的版本说明。

## [0.1.14] - 2026-03-30

### Added
- 设置页新增“系统推导”按钮和“单账号并发上限”，可以按当前机器资源一键回填并立即生效。
- 入口层新增短队列等待与超载快速退化，避免高并发直接拖死服务进程。

### Changed
- README、workspace、前端包、Tauri 桌面端与版本一致性校验脚本统一提升到 `0.1.14`。

## [0.1.13] - 2026-03-25

### Added
- 新增“聚合 API”管理页，支持供应商名称、顺序优先级、按 `Codex / Claude` 分类、连通性测试与最小转发上游管理。
- 平台密钥新增 `账号轮转 / 聚合 API 轮转` 策略，聚合 API 轮转会按顺序优先命中对应供应商，再继续下一个渠道。

### Fixed
- 修复桌面端服务启动与页面切换时的自动恢复行为，避免关停后被切页重新拉起，也避免断连时仪表盘误清空数据。

### Changed
- README、workspace、前端包、Tauri 桌面端与版本一致性校验脚本统一提升到 `0.1.13`。

## [0.1.12] - 2026-03-20

### Fixed
- 修复平台密钥名称编辑链路在桌面端未完整透传的问题；现在 Web 与桌面端都能正确保存并回显名称，且支持中文名称。
- 修复平台密钥列表中密钥 ID 默认被截断的问题；现在会直接完整显示，便于核对与排查。

### Changed
- README 新增赞助支持入口与赞助区跳转，方便从文档顶部直接定位到赞助说明。
- 发布版本提升到 `0.1.12`，同步更新 workspace、前端包、Tauri 桌面端、版本一致性校验脚本与 README 最新版本说明。

## [0.1.11] - 2026-03-20

### Added
- 账号管理新增封禁识别、封禁筛选与“一键清理封禁账号”入口；`account_deactivated` 与 `workspace_deactivated` 会被自动识别为不可用信号，并可在列表中直接筛选和清理。
- 账号列表的 5 小时 / 7 天额度列现在会展示各自窗口的重置时间；仅返回 7 天窗口的 free 账号也会把重置时间显示到 7 天列。
- 平台密钥新增服务等级配置：`跟随请求`、`Fast`、`Flex`，其中 `Fast` 会映射为上游 `priority`，`Flex` 会直传为 `flex`。

### Fixed
- 修复桌面端平台密钥创建 / 编辑时 `serviceTier` 未透传导致“服务等级”保存后不生效、不回显的问题。
- 修复 Web 端在非首页刷新时偶发下载错误文件的问题，并修复部分运行环境下复制 API Key / 登录链接时 `navigator.clipboard.writeText` 不可用导致的复制失败。
- 修复设置页“检查更新”按钮在自动静默检查更新时持续错误转圈的问题；现在只有手动点击时才显示加载状态。

### Changed
- 网关主链路继续向 Codex-first 收口：会话绑定、自动切号即切线程、`originator` / `User-Agent` / 请求压缩等出站语义已进一步对齐，并移除了旧兼容路径遗留的 upstream cookie 链路。
- 设置页补回服务监听地址切换，可在 `localhost` 与 `0.0.0.0` 之间切换；README 与文档也已同步收口到当前主线路径。
- 发布版本提升到 `0.1.11`，同步更新 workspace、前端包、Tauri 桌面端、版本一致性校验脚本与 README 最新版本说明。

## [0.1.10] - 2026-03-18

### Fixed
- 修复 Web / Docker 版误走桌面专属命令分支、账号启用 / 禁用缺少 `sort` 参数导致无法切换状态，以及账号详情刷新失败后状态列不及时回刷的问题。
- 修复禁用账号仍参与手动批量刷新与后台用量轮询的问题；批量刷新与后台轮询现已跳过手动禁用账号，并按并发 worker 执行。
- 修复账号状态语义混乱问题：手动禁用统一为 `disabled`，额度用尽与 `usage endpoint 401` 统一为 `unavailable`，`refresh token 401` 相关链路也统一落成 `unavailable`，前端状态展示同步收口为“已禁用 / 不可用”。
- 修复 Windows 本地 Web 启动器关闭控制台窗口后 `codexmanager-service` / `codexmanager-web` 仍残留后台的问题；启动器现在会通过 Job Object 一并回收子进程。

### Changed
- 发布版本提升到 `0.1.10`，同步更新 workspace、Tauri 桌面端、前端包版本、README 最新版本说明和版本一致性测试。

## [0.1.9] - 2026-03-18

### Added
- 请求日志现在支持后端分页、后端统计、首尝试账号和尝试链路展示，便于区分实际命中账号与 failover 后的最终账号。
- 设置页新增 free / 7 天单窗口账号使用模型配置，free 类账号会统一按设置模型发起请求。

### Fixed
- 修复桌面端启动误判、`/rpc` 空响应、`spawn_blocking` 缺失导致的刷新失败、用量弹窗刷新不同步、首次切页卡顿、Hydration 不一致等稳定性问题。
- 修复 refresh token 误摘号、free 账号请求模型未正确改写、优先账号行为不稳定，以及 `503 no available account` 缺少上下文诊断的问题。
- 修复 release workflow 中 pnpm 版本与当前锁文件不匹配导致的 verify 失败问题。

### Changed
- 旧前端已移除，桌面端与 Web 管理界面统一收口到新的 `apps` 前端；账号管理、平台密钥、请求日志、设置页和导航布局都做了整轮桌面优先重构。
- Codex 请求链路继续按实际 on-wire 行为收口：登录 / callback / workspace 校验、refresh 语义、`/v1/responses` 与 `/v1/responses/compact` 重写、线程锚点、请求压缩、错误摘要和 fallback 诊断均已继续对齐。
- 网关失败诊断和磁盘日志继续收敛，compact 假成功体、HTML/challenge 页、`401 refresh` 子类和 exhausted 候选链路都会输出更明确的摘要。
- 统一将发版版本提升到 `0.1.9`，同步更新 workspace、Tauri 桌面端、`tauri.conf.json` 与前端包版本。
- GitHub Release workflow 中固定的 Tauri CLI 版本已对齐到当前 Rust 侧实际使用版本，减少打包阶段的 CLI / crate 漂移风险。
- 发布文档与 README 已同步更新到 `v0.1.9`，并修正前端静态导出目录说明为 `apps/out`。

## [0.1.8] - 2026-03-11

### Fixed
- Removed the default `https://api.openai.com/v1` fallback path for ChatGPT-backed requests; upstream `challenge` and `403` outcomes are now returned from the primary login-account path instead of being rewritten into local fallback errors.
- ChatGPT login-account requests now recover from `401` by refreshing the local `access_token` with the stored `refresh_token` and retrying the current request once.

### Changed
- ChatGPT login-account turns now use `access_token` directly on the primary upstream path and no longer mix in `api_key_access_token` semantics.
- Synthetic gateway terminal failures now return structured OpenAI-style `error.message / error.type / error.code` payloads while keeping the existing trace and error-code headers.

## [0.1.7] - 2026-03-11

### Added
- 设置页新增网关传输参数：支持直接配置上游流式超时与 SSE keepalive 间隔，并在 service 运行时热生效。
- 桌面端启动快照补齐：仪表盘统计、账号用量状态、请求日志首屏会优先恢复最近一次快照，减少源码运行或服务重启后的全 0 / 未知状态。

### Fixed
- 修复 `codexmanager-web` 的访问密码会话跨重启仍可继续使用的问题；关闭并重新打开 Web 进程后，旧登录 Cookie 会失效，需要重新验证密码。
- 修复源码运行 `codexmanager-web` 时的启动与根路由兼容问题，减少 Web 静态资源与根路径在 Axum 路由下的不一致行为。
- 修复长输出场景下的 SSE 空闲断流重连问题，降低长时流式响应被误判中断的概率。
- 修复设置页保存上游代理、平台密钥创建弹窗关闭与重复提交、登录成功后账号表格未刷新等桌面交互问题。
- 修复模型拉取默认附加版本参数导致的部分上游兼容性问题，模型请求改为默认不附带版本号。
- 修复账号导入与登录回调两条链路的账号归并逻辑不一致问题，统一按同一身份规则新增或更新账号。
- 修复 Claude / Anthropic `/v1/messages` 适配在多 MCP server 场景下的工具截断问题；不再因前 16 个工具占满而丢失后续 server 的工具。
- 修复 Claude / Anthropic `/v1/messages` 链路缺少长工具名缩短与响应还原的问题，避免 MCP 工具名过长时映射不稳定。

### Changed
- 网关失败响应增加结构化 `errorCode` / `errorDetail` 字段，并同步补充 `X-CodexManager-Error-Code`、`X-CodexManager-Trace-Id` 响应头，便于客户端与日志系统追踪失败链路。
- 协议适配继续对齐 Codex / OpenAI 兼容生态：进一步统一 `/v1/chat/completions`、`/v1/responses`、Claude `/v1/messages` 的转发语义，并稳固 `tools` / `tool_calls`、thinking / reasoning、流式桥接和响应还原链路。
- 设置页与运行时配置继续收敛：背景任务、网关传输、上游代理、Web 安全等高频配置统一由 `app_settings` 持久化并回填到当前进程。
- 桌面与 service 启动链路继续治理，收敛 Web / service / desktop 之间的启动边界与启动顺序，减少源码运行与打包运行的行为分叉。
- 项目内部继续推进长期维护向的重构治理：前端主入口、设置页、请求日志视图、Tauri 命令注册、service 生命周期、gateway protocol adapter、HTTP bridge、upstream attempt flow 等区域已进一步拆分模块边界，减少大文件与根层门面耦合。
- service / gateway 目录结构继续收敛，更多通配导入、跨层直连和超长门面清单已被显式依赖与分层模块替代，后续维护和协议回归定位成本更低。
- 发布链路继续收敛到 `release-all.yml` 单入口，并复用前端构建产物与协议回归基线，减少重复构建与发布时的协议回归风险。

## [0.1.6] - 2026-03-07

### Fixed
- 修复 `release-all.yml` 在手动关闭 `run_verify` 时仍强依赖预构建前端工件的问题；各平台任务缺少 `codexmanager-frontend-dist` 时会自动回退到本地 `pnpm install + build`。

### Changed
- Windows 桌面端发布产物继续收敛，仅保留 `CodexManager-portable.exe` 便携版，不再额外生成 `CodexManager-windows-portable.zip`。
- 完善 SOCKS5 上游代理支持与归一化，并补充设置页中的代理协议提示文案。

## [0.1.5] - 2026-03-06

### Added
- 新增“按文件夹导入”：桌面端可直接选择目录，递归扫描其中 `.json` 文件并批量导入账号。
- 新增 OpenAI 上游代理配置与请求头收敛策略开关，可在设置页直接保存并即时生效。
- 补充 chat tools 命中探针脚本，便于本地验证工具调用是否真正命中与透传。

### Fixed
- 修复 `tool_calls` / `tools` 相关回归：补齐 chat 聚合路径中的工具调用保留、工具名缩短与响应还原链路，避免工具调用在 OpenAI 兼容返回、流式增量和适配转换中丢失或名称错乱。
- 完善 OpenClaw / Anthropic 兼容返回适配，确保工具调用、SSE 增量和非流式 JSON 响应都能按兼容格式正确还原。
- 请求日志追踪增强，补充原始路径、适配路径和更多上下文，便于定位 `/v1/chat/completions -> /v1/responses` 转发与协议适配问题。

### Changed
- 网关协议适配进一步对齐 Codex CLI：`/v1/chat/completions` 与 `/v1/responses` 两条链路统一收敛到 Codex `responses` 语义，上游流式/非流式行为与官方更接近，兼容 Cherry Studio 等客户端的 OpenAI 兼容调用。
- 设置页顶部常用配置改为统一的三列行布局，代理配置与其保持一致；同时支持关闭窗口后隐藏到系统托盘运行。
- 发布流程整合为单一一键多平台 workflow，并收敛桌面端产物形态；Windows 直接提供 portable exe，macOS 统一使用 DMG 分发。

## [0.1.4] - 2026-03-03

### Added
- 新增“一键移除不可用 Free 账号”：批量清理“不可用 + free 计划”账号，并返回扫描/跳过/删除统计。
- 新增“导出用户”：支持选择本地目录并按“一个账号一个 JSON 文件”导出。
- 导入兼容增强：支持 `tokens.*`、顶层 `*_token`、camelCase 字段（如 `accessToken` / `idToken` / `refreshToken`）自动识别。

### Fixed
- 兼容旧 service：前端导入前会自动归一化顶层 token 格式，避免旧版后端报 `missing field: tokens`。

### Changed
- 账号管理页操作区整合为单一“账号操作”下拉菜单，替代右侧多按钮堆叠，界面更简洁。

[Unreleased]: https://github.com/qxcnm/Codex-Manager/compare/v0.6.3...HEAD
[0.6.3]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.3
[0.6.2]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.2
[0.6.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.0
[0.5.4]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.4...v0.5.0
[0.4.4]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.9...v0.4.0
[0.3.9]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.8...v0.3.9
[0.3.8]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.7...v0.3.8
[0.3.7]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.4...v0.3.5
[0.2.6]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.3...v0.2.6
[0.2.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.0...v0.2.3
[0.2.0]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.2.0
[0.1.19]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.19
[0.1.17]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.17
[0.1.16]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.16
[0.1.15]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.15
[0.1.14]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.14
[0.1.13]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.13
[0.1.12]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.12
[0.1.11]: https://github.com/qxcnm/Codex-Manager/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.10
[0.1.9]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.9
[0.1.8]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.8
[0.1.7]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.7
[0.1.6]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.6
[0.1.5]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.5
[0.1.4]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.4
