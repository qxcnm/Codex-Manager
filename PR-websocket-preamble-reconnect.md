# fix(gateway): 修复 Responses WebSocket 大图像首帧 Broken pipe 与失败账号重复恢复

## Summary

本 PR 在 PR #430 已有的心跳、连接上限、大图像帧和有界恢复基础上，继续修复新线程或图像上下文线程在上游 WebSocket 首帧发送阶段出现 `Broken pipe (os error 32)`，随后连续 502 并降级 HTTP 的问题。

本次修复遵循官方 Responses WebSocket / Codex 客户端的传输行为：

- 使用官方 Codex 当前固定的 `tokio-tungstenite` / `tungstenite` fork 版本；
- 为上游 WebSocket 启用 `permessage-deflate` 协商，降低大图像上下文单帧发送压力；
- 首帧发送失败后的恢复仍然有界，不无限重试；
- 如果存在其他可选账号，恢复尝试会排除已经首帧发送失败的账号，避免同一账号重复触发相同的 `Broken pipe`；
- 只有完整收到 `response.completed` 才算 WebSocket 请求成功；恢复预算耗尽后仍保留官方客户端的 HTTP fallback。

## Official behavior baseline

实现以官方 [Responses WebSocket Mode](https://developers.openai.com/api/docs/guides/websocket-mode) 和官方 [Codex Responses WebSocket 客户端](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/responses_websocket.rs) 为基准：

- 每一轮通过一个 `response.create` 消息开始；
- 请求体仍是 Responses create 形状，图像上下文可以作为输入的一部分发送；
- 连接关闭、连接缓存不可用或连接达到时限后，需要建立新连接，并按上下文情况使用 `previous_response_id` 或完整输入；
- 连接内的请求按顺序处理，当前代理路径继续使用官方顺序语义的兼容子集；
- 只有 `response.completed` 才确认该轮完成；
- WebSocket 恢复失败后由客户端按 session 级策略进入 HTTPS/SSE fallback，Manager 不强制已经回退的下游 session 重新升级 WebSocket。

## Problems and root causes

### 1. 大图像上下文首帧发送阶段的传输不一致

Codex 会将本轮完整上下文序列化为一个 `response.create` 文本消息。包含多张内联图像时，单帧可能达到数十 MiB。此前 Manager 使用 crates.io 的 WebSocket 传输配置，没有按官方 Codex 配置启用 `permessage-deflate`，大帧更容易在上游连接刚建立后暴露为：

```text
IO error: Broken pipe (os error 32)
```

这发生在响应完成前，因此下游看不到 `response.completed`，会继续执行有限恢复并最终降级 HTTP。

### 2. 首帧恢复可能重复使用同一失败账号

此前首帧发送失败后，恢复逻辑仍可能从 conversation-bound 候选列表头部重新选择原账号。只要该账号的上游连接策略持续关闭 socket，就会得到连续相同的 `Broken pipe`，其他可选账号没有机会接收首帧。

### 3. 测试 mock 没有声明可处理扩展

接入官方压缩协商后，测试用 upstream 也必须显式提供 WebSocket 扩展配置；否则严格的 WebSocket handshake 会把 `Sec-WebSocket-Extensions` 当作无效请求。测试 fixture 已同步到官方协商语义。

## Changes

### 官方 WebSocket 传输对齐

- 固定官方 Codex 使用的 `tokio-tungstenite` fork revision；
- 固定官方 Codex 使用的 `tungstenite` fork revision；
- 启用 `deflate` / `proxy` 相关 feature；
- 上游 WebSocket `WebSocketConfig` 保留 256 MiB 的 message/frame 硬上限；
- 在连接握手中启用 `permessage-deflate`，服务端支持时协商压缩，服务端不返回扩展时仍按未压缩 WebSocket 继续工作。

### 有界首帧恢复与账号轮换

- 为恢复函数维护本轮已失败账号集合；
- 首次恢复优先排除导致首帧发送失败的账号；
- 每次恢复发送失败后将对应账号加入排除集合；
- 在存在其他候选时按原有线程/会话/优先级顺序选择下一个可选账号；
- 只有所有候选都已尝试过时，才允许在原有有界预算内再次使用候选池；
- 不改变禁用、冷却、限流账号的候选过滤，也不覆盖线程感知账号分配策略；
- 账号切换时继续清理跨账号 session affinity，并按当前候选重建请求上下文。

### 安全终态与 fallback

- `response.completed` 仍是唯一成功终态；
- `response.done`、`response.failed`、`response.incomplete`、TCP reset 和发送错误不会清除恢复状态；
- 已转发实质模型输出、工具事件或二进制内容后不透明重放，避免重复输出或工具副作用；
- 恢复预算耗尽后返回 502，由官方 Codex 客户端继续其 HTTP fallback；
- 不强制同一 session 从 HTTP 再次切回 WebSocket。

## Reproduction and regression coverage

### 大图像上下文

隔离测试构造约 34 MiB 的单个 `response.create` 文本帧，其中包含内联 `input_image` data URL：

1. 建立下游 Responses WebSocket；
2. 发送大图像上下文；
3. 验证 Manager→upstream 的帧完整到达；
4. 验证下游收到 `response.completed`；
5. 验证同一连接的 follow-up 仍可继续。

### 首帧失败后的账号轮换

1. 将首个候选账号设置为握手后 reset；
2. 验证恢复发送不重复使用该失败账号；
3. 验证下一个候选账号收到完整 `response.create`；
4. 验证下游收到 `response.completed`，不收到恢复错误；
5. 验证单账号场景仍遵守原有有限重试预算。

## Validation

已通过：

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo test -p codexmanager-service --lib official_responses_websocket_ --no-fail-fast` — 16 passed
- `cargo test -p codexmanager-service --lib send_websocket_upstream_request_ --no-fail-fast` — 5 passed
- `cargo test -p codexmanager-service --lib official_responses_websocket_accepts_large_image_context_frame --no-fail-fast` — 1 passed（约 34 MiB 图像上下文）
- `cargo test -p codexmanager-web --no-fail-fast` — 26 passed
- `pnpm -C apps run build:desktop` — passed
- `cargo-tauri build --bundles app` — passed
- 生成的 App 通过 ad-hoc code-sign verification

## Scope / Compatibility

- 不改变公开 `/v1/responses` endpoint 形状；
- 不改变 Codex 同一 session 内 HTTP fallback 的粘性；
- 不强制已经降级 HTTP 的下游 session 重新升级 WebSocket；
- 不在已经转发实质模型/工具内容后静默复制请求；
- 不新增配置项，不改变账号优先级、线程感知分配或禁用/冷却过滤策略；
- 心跳仍只发送 WebSocket 协议层 Ping，不注入 Responses JSON 事件；
- 大帧仍使用 256 MiB 硬上限，不开放无限消息；
- 上游确实不可用或恢复预算耗尽时，仍会按官方策略降级 HTTP。
