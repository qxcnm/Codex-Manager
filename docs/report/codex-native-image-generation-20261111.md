# Codex 原生图片生成源码对照

核对日期：2026-11-11。官方源码固定为稳定版 `rust-v0.167.1`，提交 `0a94398fa60c4fac0e7659e4213a0a36e613dd46`。本轮只使用源码、单测及本地 mock，不包含真实账号的图片生成验证。

## 两条不同的图片链路

- 原生 `image_gen.imagegen` 是 Codex 扩展工具，无需 Skill。它在生成或编辑时单独发送 Images API JSON 请求；当前官方扩展将图片模型固定为 `gpt-image-2`，并发送 `x-codex-image-turn-id`。旧客户端可能固定为其他图片模型，不能用当前版本的默认值推断旧客户端请求。
- 普通 `/v1/images/generations`、`/v1/images/edits` 兼容入口可在 CM 内转换为 `/v1/responses + image_generation`。顶层是文本主模型，图片模型放在工具内；这与原生扩展直接使用 Images API 不同。

源码：[原生工具及固定模型](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/ext/image-generation/src/tool.rs)、[原生请求头](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/ext/image-generation/src/backend.rs)、[Images 请求端点](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/codex-api/src/endpoint/images.rs)。Responses 模型层级见 [OpenAI Docs](https://developers.openai.com/api/docs/guides/tools-image-generation)。

## CM 的处理

`CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL` 现在同时用于：

1. 带非空 `x-codex-image-turn-id` 的原生 Images JSON 请求：在账号、聚合、混合路由下，用配置值替换客户端固定的图片模型。CCSwitch 或手写 provider 配置无需附带 CM 本地扩展标记。
2. 普通 Images 兼容请求：仅在调用方省略 `model` 时作为工具模型默认值，显式模型仍优先。

第一种请求仍直接走 Images API，不改成 Responses。模型改写发生在策略检查前，实际请求体、有效模型、权限、路由及计费使用一致的最终模型；客户端原始模型保留在日志中。目标模型必须在模型目录中启用并允许当前 Key 使用，不能借助扩展标记跳过校验。已知文本模型配置会在本地被拒绝。未带图片 turn 请求头的普通 API 请求不应用此覆盖；第三方若复用该请求头，也会使用配置的图片工具模型。

图片工具模型应选 `gpt-image-2`、`gpt-image-2.5-sunburst` 或 `gpt-image-2.5-flare` 等有效图片型号。`gpt-image-2.5` 不是完整的官方型号；文本模型也不能作为独立 Images 请求的图片模型。`CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL` 只控制第二条兼容链路的文本主模型，不能切换原生扩展的图片模型。

## 配置与生效

运行包含修复的 CM 后，进入「设置 → 环境」，搜索「图」，选择「Codex 图片工具模型」。该字段使用下拉框，只提供上述三个完整型号；旧配置若是不完整型号、文本模型或其他不支持的值，会显示原值并禁止保存，需重新选择或恢复默认，不会静默替换。

保存成功后，下一次图片请求读取新的运行时配置，改型号无需重启 CM 或 Codex，正在执行的请求保持原型号。保存或重置期间锁定编辑控件，避免响应返回时清除新草稿。其他环境变量继续使用原输入方式。原生图片请求仍需 Codex 客户端已注册图片工具，且目标型号在 CM 中启用、当前平台 Key 获准使用，上游具备对应能力。

普通 API 或 `cm-imagegen` 请求若显式指定 `model`，该显式值仍优先；切换 Skill 型号需用 `--model` 或 `CODEXMANAGER_IMAGE_MODEL`。已安装的 Skill 副本还需更新，才能使用新的凭据读取逻辑。

## 客户端限制

原生工具是否出现由 Codex 决定，服务端不能从一个尚未注册工具的客户端收到原生图片请求。上述版本要求图片功能开启、当前认证套餐不是 Free、provider 图片能力开启、会话模型支持图片输入，并满足 actor authorization 或 Codex backend 认证条件；仅修改 CM 的工具模型不能消除这些门槛。

官方 actor authorization 分支要求 `requires_openai_auth=false`。但这一设置会使 Codex 的 `account_state` 返回空账户，影响 `account/read` 和桌面身份；仅保留 `auth.json` 不足以消除这类状态变化。因此本轮保留当前认证及项目保护逻辑，不为打开图片工具而改写登录模式。

源码：[工具注册条件](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/core/src/tools/spec_plan.rs)、[actor authorization 条件](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/model-provider-info/src/lib.rs)、[provider 账户状态](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/model-provider/src/provider.rs)、[account/read](https://github.com/openai/codex/blob/0a94398fa60c4fac0e7659e4213a0a36e613dd46/codex-rs/app-server/src/request_processors/account_processor/workspace_routing.rs)。

如果原生图片请求已到上游，仍返回模型容量、账号权限或额度错误，源码及 mock 不能确认其真实原因；本轮不将这类错误标为已解决。

## 本地验证

- `cargo test -p codexmanager-service --lib --test gateway_logs image`：81 项图片单测、8 项本地模拟上游测试通过，覆盖原生生成/编辑、兼容请求、Responses 工具及 SSE。
- `cargo test -p codexmanager-service --lib images_policy_tests`：补充流式矩阵后 8 项通过；原生覆盖含 4 种路由策略 × 生成/编辑 × 流式/非流式，且验证无 CM actor 标记的手写 provider 请求。
- `python -m unittest discover -s cm-skills/cm-imagegen/tests -v`：使用本机 Python 3.14 绝对路径执行，15 项通过；包含临时配置及本地 HTTP 生成/编辑，不读取真实账号。
- `pnpm -C apps run build`、`pnpm -C apps run test:runtime`：静态构建通过，运行时 337 项通过。首次并行重负载运行有一项既有超时断言失败，无源码调整重跑后通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。

昨日 service 全量测试为 2217 通过、1 失败、8 忽略；失败项是 `account/bps_tests.rs:223` 的 `no such column: group_name`，单独重跑通过，该次全量测试不记为全绿。未连接真实图片上游，未调用任何 Luna 模型，无法据此确认截图中的容量错误已恢复。

2026-11-12 发布前再次执行 `cargo test -p codexmanager-service --lib -- --test-threads=1`，最终为 2223 通过、0 失败、8 忽略；包含此前失败的 BPS 用例，以及 Codex Desktop 跨账号本地项目保护、Companion Page HTTP/WebSocket 页面上下文等昨日修复的回归。该结果是本次串行运行的新证据，不代表已经修复此前偶发数据库初始化竞争的根因，也不替代真实账号的桌面切换与生图验收。

同日追加验证：图片模拟上游集成测试 8 项、Python 测试 15 项、前端运行时测试 337 项均通过。使用本机 Chrome 执行 `pnpm -C apps exec playwright test tests/gateway-heartbeat-settings.spec.ts`，该命令先执行 `build:desktop` 静态构建，最终 4 项页面交互测试通过；覆盖下拉三项候选、非法旧值、切换标签及变量时保留草稿、保存期间锁定控件、实际 RPC 保存与恢复默认。
