# gateway 目录说明

## 目标

`crates/service/src/gateway/` 是 CodexManager 最复杂的服务端域之一，负责本地网关、上游转发、协议适配、可观测性与选路逻辑。

本文档帮助协作者快速判断改动应该落在哪个子目录，避免把不同职责继续堆进单个大文件。

## 子目录职责

### `auth/`

负责：

- 上游鉴权补全
- token exchange
- OpenAI fallback 认证相关逻辑

### `core/`

负责：

- 网关运行时核心配置
- 与 gateway 自身行为直接相关的底层状态

### `local_validation/`

负责：

- 本地预校验
- 请求/鉴权/输入前置检查

### `model_picker/`

负责：

- 模型选择与解析
- 与请求模型决策相关的轻量逻辑

### `observability/`

负责：

- HTTP bridge
- trace log
- request log
- metrics

高风险文件：

- `observability/http_bridge.rs`

### `protocol_adapter/`

负责：

- OpenAI/Codex 输入输出适配
- request mapping
- response conversion
- prompt cache
- tools / `tool_calls` 聚合与还原

高风险文件：

- `protocol_adapter/request_mapping.rs`
- `protocol_adapter/response_conversion.rs`
- `protocol_adapter/response_conversion/sse_conversion.rs`

### `request/`

负责：

- 进入 gateway 前的请求规范化
- 本地能力请求（模型、计数等）
- chat / responses 请求改写

### `routing/`

负责：

- 选路
- cooldown
- failover
- request gate
- route quality / hint

关键文件：

- `routing/selection.rs`
- `routing/route_hint.rs`
- `routing/route_quality.rs`

### `upstream/`

负责：

- 上游候选管理
- 超时、重试、退避
- 代理配置
- transport 发送
- 不同上游协议实现

## 核心链路

典型链路：

1. `request/` 处理传入请求
2. `routing/` 选择候选账号与策略
3. `auth/` / `upstream/` 组装并发送上游请求
4. `protocol_adapter/` 转换输入输出
5. `observability/` 写入 trace、日志和指标

## 账号选路策略

设置入口：

- 前端 `appSettings.routeStrategy`
- 持久化键 `gateway.route_strategy`
- 环境变量 `CODEXMANAGER_ROUTE_STRATEGY`

后端接受的规范值只有两个：

- `ordered`
- `balanced`

兼容别名：

- `round_robin`
- `round-robin`
- `rr`

注意：

- 后端会把以上轮询别名统一归一化为 `balanced`
- 如果未配置 `CODEXMANAGER_ROUTE_STRATEGY`，默认策略是 `balanced`

候选池基础顺序：

- 候选账号先由 `Storage::list_gateway_candidates()` 选出
- 初始顺序按 `account.sort ASC, account.updated_at DESC` 排列
- 也就是说，`ordered` 的“顺序”首先来自账号排序值，而不是随机顺序

额外覆盖规则：

- 如果设置了手动指定账号（manual preferred account），会先把该账号旋转到队首
- 只要该账号仍在可用候选池内，就会覆盖普通 `ordered / balanced` 轮转逻辑
- 如果该账号已不可用或不在候选池，会自动回退到常规策略

### Free 账号模型上限

设置入口：

- 前端 `appSettings.freeAccountMaxModel`
- 持久化键 `gateway.free_account_max_model`

行为：

- 默认值是 `gpt-5.2`
- 当请求模型高于该上限时，free 账号会在候选预检阶段被直接排除
- 这样可以避免自动 failover 到 free 账号后，再被上游以“模型不支持”拒绝

### 单账号并发上限

设置入口：

- 环境变量 `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT`

行为：

- 默认值是 `1`
- 含义是同一账号默认只承载一个正在进行中的 gateway 上游请求
- 当并发 Codex 会话较多时，候选预检会优先跳过已满载账号，避免多个长连接同时压到同一账号上
- 如果你明确需要更高吞吐，可以显式调大；设置为 `0` 表示关闭该保护

### `ordered`

行为：

- 不做 round-robin 轮转，直接沿用候选池当前顺序
- 默认启用健康度 P2C 小窗口换头，窗口默认值为 `3`
- 也就是说，头部候选仍可能被“更健康”的前几个候选之一替换到第一位

适用理解：

- 更接近“按账号优先级优先尝试”
- 不是“永远固定命中第一个账号”

### `balanced`

行为：

- 以 `key_id + model` 作为维度维护独立轮询状态
- 每次请求会推进该维度的起始索引，实现严格 round-robin
- 默认健康度窗口为 `1`，因此默认不会发生健康度换头
- 只有显式调大 `CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW` 时，才会在轮询头部附近引入健康度挑战者

适用理解：

- 更接近“同一平台密钥、同一模型下的均衡轮询”
- 不同 key、不同模型之间的轮询状态互相隔离

### 可观测性

- 候选池最终顺序会在错误 trace 中以 `CANDIDATE_POOL` 事件记录
- 记录字段包含 `strategy` 与 `ordered_candidates`
- 关键入口见 `upstream/proxy_pipeline/request_setup.rs` 和 `observability/trace_log.rs`

## 修改建议

### 改请求字段映射

优先查看：

- `protocol_adapter/request_mapping.rs`
- `request/request_rewrite_*.rs`

### 改 tools / `tool_calls`

优先查看：

- `protocol_adapter/response_conversion/tool_mapping.rs`
- `protocol_adapter/response_conversion.rs`
- `protocol_adapter/response_conversion/sse_conversion.rs`

### 改日志/错误头/trace

优先查看：

- `observability/http_bridge.rs`
- `request_log.rs`
- `trace_log.rs`
- `error_response.rs`

### 改代理/重试/超时

优先查看：

- `upstream/config.rs`
- `upstream/proxy.rs`
- `upstream/retry.rs`
- `upstream/transport.rs`
- `upstream/deadline.rs`

## 测试入口

- 单元/模块测试：各子目录 `tests/`
- service 兼容测试：`crates/service/tests/gateway/`
- 手工探针：`scripts/tests/gateway_regression_suite.ps1`

## 当前治理重点

- 持续拆小 `http_bridge.rs`
- 持续拆小 `request_mapping.rs`
- 持续拆小 `response_conversion.rs` / `sse_conversion.rs`
- 把协议兼容回归固定到脚本与 Rust 测试双路径
