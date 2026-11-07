# Codex 源码兼容性复核（2026-11-06）

## 基线与结论

本次通过 GitHub Release 和 npm 实时确认稳定版为 **0.167.0**，核对官方 `rust-v0.167.0` 固定提交 [`086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`](https://github.com/openai/codex/tree/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a)，并与上次 `rust-v0.164.0` 的 `6aaac694803004a63c6f393d153c96d22521a6b9` 比较。Release 发布时间为 2026-11-06 09:40（北京时间）。

结论：存在需要修正的兼容性差异，但没有发现这两版之间新增的 Responses HTTP / WebSocket 顶层请求字段。HTTP 15 项、WebSocket 17 项声明保持一致。新增内容主要涉及工作区切换、连接路由、Guardian 元数据及 provider 能力配置；不能把它们全部当作每次推理必传的参数。

核查对象是当前工作区，包含本次开始前已有的 0.164 兼容改动，不等同于已发布安装包。本次保留原有未提交工作，不升级版本宣告，不发布版本。

**兼容性检查不能保证不触发账号风控。** 客户端源码没有公开服务端的完整判定规则；字段差异也不能证明某次 401、403、429 或账号限制的原因。本报告不把可选诊断、审核或授权选择字段伪造成必需凭证。

## 本次修正

| 范围 | 原有差异 | 修正原则 |
| --- | --- | --- |
| WebSocket `stream` | 原生 WS 删除该字段；HTTP 转 WS 设为 false 并省略。官方 0.164 已发送该字段。 | 原生客户端显式值保留，旧客户端缺省使用 true；HTTP 转 WS 使用流式上游，非流式调用仍由原有逻辑聚合。 |
| 原生 WS 会话头 | 上游握手使用 `session_id`，缺少规范 `thread-id`。 | 发送 `session-id` 和 `thread-id`；线程、会话和请求 ID 各按自己的来源取值，保留旧客户端输入兼容，切换账号时仍清除亲和信息。 |
| 窗口标识 | 因窗口 ID 不以 session ID 开头而重写为 `session:0`。 | 官方窗口属于线程，session 可独立用于缓存亲和；保留合法来访窗口 ID，仍校验头值并保留换号重建逻辑。 |
| 轮换令牌刷新 | POST 发送错误后自动重发旧 refresh token，上游可能已经消费首个请求。 | 每次刷新尝试只发送一次；保留已有调用层错误处理和并发刷新协调。 |
| 切换工作区后的身份 | 官方可保留旧 ID token，仅更新 access token 和 `tokens.account_id`；本地可能取回旧工作区，并沿用旧 FedRAMP 标记。 | 只有两份令牌的用户 ID 一致，且所选账号与 access token 声明匹配，才采用新的工作区；导入、候选账号和用量元数据解析使用相同规则。AgentIdentity 同时采用已确认的新工作区 FedRAMP 标记，缓存标记冲突时重新注册身份。 |

其中工作区身份问题与本次新版工作区切换直接相关；其余是本轮发现的既存遗漏，不应描述为 0.167 新加的协议要求。

已被旧逻辑错误导入的历史账号需要重新导入带 `tokens.account_id` 的官方认证文件；本次不会仅凭缺少选择信息的历史记录自动切换工作区。刷新修正仅移除同一次尝试中送达情况不明时的立即重发，后续定时刷新及用户重试仍按现有流程处理。

## 新增内容及处理边界

| 元素 / 参数 | 0.164 → 0.167 情况 | CodexManager 判断 |
| --- | --- | --- |
| `current_workspace_id`、`login_hint`、`connection` | 新增于工作区登录分支。 | 仅适用于主动切换/SSO 登录；普通登录不应无条件添加。本次兼容切换后的导入文件，未新增工作区切换 UI。 |
| `ChatGptRoutingAffinity` | 新增连接独有的路由 Cookie pin，保留握手响应中的 `__oailb` / `__cflb`，限制原安全 origin、路径及有效期。 | 尚未实现。不能直接透传全部 Cookie，也不能跨账号/代理出口混用；需要独立设计 HTTP、WS、换号与重连生命周期。基础设施 Cookie 白名单本身在 0.164 已存在。 |
| `guardian_recorded_items` / Guardian assessment | 新增审核请求元数据与响应证据。 | 现有元数据对象和原生响应透传保留未知内容；不主动构造审核证据或声称支持完整 Guardian 路由。 |
| `capabilities.cyber_access_programs` | 新增 provider 能力开关；自定义 provider 默认关闭。 | 已有 `access_programs` 透传继续保留；不自动启用能力或生成授权选择。此开关不授予访问权限。 |
| `project_id`、`location`、`credential_file` | 新增 GCP provider 配置。 | 属于新增 provider 集成，不是当前 ChatGPT 账号池请求缺失字段；本轮不改配置生成器。 |
| `can_access_with_session`、`sso_connection_name`、`is_deactivated`、`is_fedramp_compliant_workspace` | 账号列表新增工作区状态信息。 | 配合新版切换流程使用；未扩展本地工作区选择界面。 |
| `refresh_reason` | 0.164 已有，可选诊断字段。 | 当前未发送，记录为诊断差异；没有证据表明必填或决定风控。 |
| `x-codex-routing-hint`、`x-codex-guardian`、`x-openai-memgen-request` | 0.164 已有的条件性标识。 | 当前受控头列表未覆盖这些用途。routing hint 需匹配最终模型/等级；Guardian/memory 标识需对应真实请求角色，不能无条件补齐。 |

## 已覆盖和仍有差异的行为

- 工作区原有调整已保留 `stream_options`、`access_programs`、`reasoning.context`、`include` 和 `client_metadata`；连续 WS 消息的每帧元数据优先于初始握手信息。
- 既有 OAuth 登录 scope、JSON refresh 编码、AgentIdentity 核心签名/注册字段及账号/地区头未发现需要因 0.167 全量替换的证据。
- User-Agent 仍由网关设置生成；HTTP 在条件满足时保留来访 originator，原生 WS 仍使用全局 originator。npm 自动同步版本仍可能先于协议验证。本轮未将默认 0.164 版本改成 0.167，不把版本号更新当成完整兼容。
- HTTP 路径目前由 `x-client-request-id` 派生 `thread-id`；官方通常使用同一个线程值，但任意第三方客户端提供不同值时仍有差异。
- system/developer 输入提升为顶层 `instructions`、Chat Completions 转换、模型/等级覆盖、账号池和会话恢复是现有网关策略；请求无法声称与单账号官方客户端逐字相同。
- 0.167 的 API key 资源选择、PAT、完整工作区切换和新增云 provider 不在现有 ChatGPT 账号导入契约的完整实现范围内。
- 本次只校正已实现的 AgentIdentity 工作区标记与缓存身份；普通 OAuth Bearer 回退路径仍未完整实现 FedRAMP 路由，不能将本次修复解释为全模式 FedRAMP 支持。Plan 主路径已优先采用 access token，无需因旧 ID token 再改写。

## 固定源码证据

- [HTTP / WS 请求结构](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/codex-api/src/common.rs)、[客户端请求构造](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/client.rs)、[会话头](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/codex-api/src/requests/headers.rs)
- [工作区切换与身份判定](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/auth/manager/workspace_switching.rs)、[登录参数](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/server.rs)、[令牌刷新请求](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/oauth/client.rs)
- [窗口/会话元数据](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/responses_metadata.rs)、[连接路由亲和](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/http-client/src/chatgpt_routing_affinity.rs)、[基础设施 Cookie 边界](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/http-client/src/chatgpt_cloudflare_cookies.rs)
- [Provider 能力](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/model-provider-info/src/capabilities.rs)、[Provider 配置](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/model-provider-info/src/lib.rs)
- [0.164 → 0.167 比较](https://github.com/openai/codex/compare/rust-v0.164.0...rust-v0.167.0)

## 验证

| 命令 | 最终结果 |
| --- | --- |
| `cargo test -p codexmanager-service --locked --offline --no-fail-fast` | 退出码 0；2,169 项通过、0 项失败、11 项按原配置忽略。其中库测试 1,993 项，网关端到端 75 项，RPC 50 项，其余服务测试 51 项。 |
| `cargo fmt --all -- --check` | 退出码 0。 |
| `git diff --check` | 退出码 0。 |

本次 Rust 改动集中在 service 包，因此执行完整服务包测试；覆盖 Responses / Chat Completions、SSE / 非流式、tools、HTTP / WebSocket、账号切换、认证和用量。首轮发现两项仍要求旧 WebSocket 行为的测试断言，已按固定上游协议同步，以上为最终完整重跑结果。

新增回归使用本地 HTTP / WebSocket fixtures 和临时或内存数据库，包含刷新请求完全送达后丢失响应、双向工作区切换、跨用户保护及冲突缓存身份重新注册。认证与协议改动另经独立静态交叉审查。

本地 fixtures 不能替代在线证据；真实 Free 账号的补充测试见下节。生产 FedRAMP 路由和服务端风控行为尚未验证。前端及安装包构建未运行，本次未修改前端或桌面壳。代码尚未提交或发布。

## 用户授权的 Free 账号在线补测

2026-11-06 22:23–22:27（Asia/Shanghai），使用当前工作区构建的 service，在独立临时数据库、独立 CODEX_HOME、临时 API key 下测试。未使用或修改日常账号数据库，未覆盖用户提供的认证文件。测试二进制 SHA-256：`af88ce2c861e4dfb2830bbd7241dae4d5ff4bb9961a7d0c6ab26599686e344b3`。

| 检查 | 结果 |
| --- | --- |
| 导入及用量查询 | 导入 1 个 Free 账号；最终用量查询成功，存在可用额度。 |
| 当前账号模型目录 | 使用现有 access token 单次 GET 官方目录成功，返回 5 个模型；选择 `gpt-6-luna`。此步骤直接查询上游，未验证应用内 `account/fetchModels` 的 exchange 路径。 |
| Responses 非流式 | HTTP 200，`completed`，精确返回 `CM_RESPONSES_OK`。 |
| Responses SSE 工具调用 | HTTP 200，收到 `response.completed`；函数名和参数符合请求。 |
| Chat Completions 非流式 | HTTP 200，精确返回 `CM_CHAT_OK`。 |
| Chat Completions SSE 工具调用 | HTTP 200，`finish_reason=tool_calls`，收到 `[DONE]`；函数名和参数符合请求。 |
| 原生 WebSocket 连续两轮 | 两轮均收到 `response.completed`，第二轮携带上一轮 `previous_response_id`；分别返回 `CM_WS_OK`、`CM_WS_SECOND_OK`。 |

共 6 次实际生成成功；返回的 usage 合计 480 tokens。工具测试验证工具调用结构，未执行外部业务工具。测试后本地账号状态为 `active`、套餐为 `free`。登录凭据在测试期间发生刷新，最新 access / ID / refresh token 已导出并逐项核对数据库，另存用户下载目录为 `codexmanager-free-verified-20261106.json`；旧文件保持原样，后续应使用新版文件，避免轮换后的旧 refresh token 影响续期。临时 API key 已删除，测试服务已停止。

成功前曾收到地域限制错误，用户更换网络后又出现 TLS 握手失败；在用户确认连接恢复、重建测试服务连接后完成上述测试。这些失败不应解释为账号封禁。本次在线结果说明此时该 Free 账号可通过测试构建完成上述请求，不证明长期账号安全、所有模型可用、完整登录流程或生产环境风控兼容。
