# fix(gateway): 对齐 Responses WebSocket 压缩协商与首帧恢复

## 摘要

本 PR 在 PR #430 的心跳、连接上限、大图像帧和有界恢复基础上，继续修复 Responses WebSocket 在大上下文首帧阶段断开、失败账号重复恢复，以及部分上游拒绝压缩协商的问题。

本次改动覆盖：

- 使用官方 Codex 当前固定的 `tokio-tungstenite` / `tungstenite` fork revision；
- 默认按官方客户端协商 `permessage-deflate`，并保留 256 MiB message/frame 上限；
- 上游明确以握手 `400`/`426` 或扩展拒绝信息拒绝压缩时，只重新建立一次不带压缩扩展的连接；
- 首帧发送失败后的恢复排除已经失败的账号，优先尝试其他仍符合线程感知、优先级、禁用和限流过滤的账号；
- 只有完整收到 `response.completed` 才确认 WebSocket 成功，恢复预算耗尽后继续由客户端进入 HTTP fallback；
- 增加根 workspace、Web 测试、前端构建和 Tauri 目标构建的 CI 验证，以及 pinned fork 的维护文档。

## 官方行为基线

实现以官方 [Responses WebSocket Mode](https://developers.openai.com/api/docs/guides/websocket-mode) 和官方 [Codex Responses WebSocket 客户端](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/responses_websocket.rs) 为基准：

- 每一轮通过一个 `response.create` 消息开始；
- 图像上下文随 Responses create 请求发送；
- WebSocket 连接在达到服务端时限、断开或不可用后重新建立；
- 连接内的请求按顺序处理；
- 只有 `response.completed` 才确认该轮完成；
- 已经产生实质输出后不透明重放，避免重复输出或工具副作用；
- WebSocket 恢复失败后继续使用官方客户端的 HTTPS/SSE fallback，不强制已经回退的下游 session 再次升级 WebSocket。

## 问题与根因

### 1. 大图像上下文首帧发送阶段的兼容性不足

包含多张内联图像时，完整 `response.create` 文本帧可能达到数十 MiB。此前传输层没有完整对齐官方 fork 和扩展协商配置，首帧发送阶段更容易出现：

```text
IO error: Broken pipe (os error 32)
```

此前的账号恢复逻辑已经处理了大帧发送失败，但如果上游连接策略本身拒绝 `permessage-deflate`，仍会在连接阶段失败。

### 2. 首帧失败后的恢复可能重复选择原账号

首帧发送失败后，conversation-bound 候选列表可能再次把原账号放在头部，导致同一失败 socket 被重复使用，其他可选账号无法接收首帧。

### 3. 依赖与独立 Tauri workspace 容易漂移

根 workspace 与 `apps/src-tauri` 是两个 Cargo workspace。只在其中一个 workspace 固定 fork，或只生成其中一个 lockfile，会使桌面构建和服务构建采用不同的 WebSocket 行为。

## 修改内容

### 官方 WebSocket 传输对齐

- 根 workspace 与 `apps/src-tauri` 同步固定：
  - `tokio-tungstenite` fork revision `0e5b2d73aa18dd9f0a50ee9ff199d5aef7594186`；
  - `tungstenite` fork revision `4fffad30fe373adbdcffab9545e9e9bf4f2fc19f`。
- 启用 `deflate` / `proxy` feature，并保持官方压缩配置；
- message/frame 上限保持 256 MiB，不开放无限制消息；
- 上游不返回扩展时，正常按未压缩 WebSocket 继续工作；
- 如果上游以 `400`/`426` 或明确的 `permessage-deflate` / WebSocket 扩展拒绝信息拒绝压缩，只重新握手一次且不发送扩展；
- 非压缩协商错误不触发该回退，避免把认证、代理和普通网关错误误判为压缩问题。

### 有界首帧恢复与账号轮换

- 为本轮恢复维护已失败账号集合；
- 首次恢复排除导致首帧发送失败的账号；
- 每次恢复发送失败后将该账号加入排除集合；
- 有其他候选时保持现有线程感知、会话、手动优先级、禁用、冷却和限流过滤；
- 只有候选全部尝试过时，才在既有有限预算内复用候选池；
- 账号切换时继续清理跨账号 session affinity，并按当前候选重建请求上下文。

### CI 与依赖维护

- 新增 `scripts/ci/check-websocket-pins.sh`，校验两个 workspace 的 manifest、lockfile 和 fork revision 同步；
- 新增 `docs/zh-CN/WEBSOCKET_DEPENDENCY_MAINTENANCE.md`，记录 fork 的来源、升级步骤、压缩回退条件和验证要求；
- 新增 `.github/workflows/ci.yml`：
  - 根 workspace 格式检查与 `cargo check --workspace --all-targets`；
  - service WebSocket 回归测试，避免现有跨平台 `codex_skills` 测试失败阻塞本 PR 的传输验证；
  - 先构建 `apps/out`，再运行 `codexmanager-web` 测试；
  - macOS arm64 Tauri app bundle 构建；
  - WebSocket pinned fork 同步检查。

## 回归覆盖

新增 `official_responses_websocket_retries_without_compression_after_upstream_rejection`：

1. mock upstream 首次握手确认收到 `permessage-deflate`；
2. 返回 `400 unsupported extension: permessage-deflate`；
3. 验证第二次握手不再携带 `Sec-WebSocket-Extensions`；
4. 验证原始 `response.create` 完整转发；
5. 验证下游收到 `response.completed`。

同时保留并继续验证：

- 约 34 MiB 图像上下文单帧；
- 首帧 socket reset 后的有界恢复；
- 重连 socket 在发送前再次断开的恢复；
- 首帧失败后的账号切换；
- 前导事件阶段重放；
- 已有实质输出后的不重放策略；
- 连接上限、心跳、follow-up 和账号绑定语义。

## 验证结果

已通过：

- `bash scripts/ci/check-websocket-pins.sh`；
- `cargo fmt --all -- --check`；
- `git diff --check`；
- `cargo test -p codexmanager-service --lib official_responses_websocket_ --no-fail-fast` — 17 passed；
- `cargo test -p codexmanager-service --lib send_websocket_upstream_request_ --no-fail-fast` — 5 passed；
- WebSocket 连接错误分类测试 — 5 passed；
- `pnpm -C apps run build:desktop`；
- `cargo test -p codexmanager-web --no-fail-fast` — 26 passed；
- `cargo tauri build --bundles app`；
- 生成的 macOS app 通过 ad-hoc code-sign verification。

完整 service lib 串行验证结果为 `1421 passed / 1 failed / 3 ignored`。唯一失败是现有 `codex_skills::tests::directory_import_detects_same_size_file_replacement_and_fifo_entries` 的跨平台文件替换断言（Linux CI 中表现为返回了仍可读的文件句柄），单独运行也可复现，与本 PR 修改文件无关；因此 CI 保留 workspace target 编译检查，并将服务测试聚焦于本 PR 的 WebSocket 回归集合。

## 兼容性与边界

- 不改变公开 `/v1/responses` endpoint 形状；
- 不改变同一 session 已进入 HTTP fallback 后的粘性；
- 不强制 HTTP session 再次升级 WebSocket；
- 不在已转发实质模型/工具内容后静默复制请求；
- 不新增配置项，不覆盖线程感知账号分配或禁用/冷却/限流过滤；
- 心跳仍只发送 WebSocket 协议层 Ping；
- 上游仍不可用或恢复预算耗尽时，仍按官方策略返回失败并允许客户端降级 HTTP。
