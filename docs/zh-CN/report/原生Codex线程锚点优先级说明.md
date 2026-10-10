# 原生 Codex 线程锚点优先级说明

## 背景

`Codex App` 与部分官方链路会同时携带两类“线程锚点”信息：

- 原生会话锚点：`conversation_id`、`session_id`、`x-codex-turn-state`
- 请求体锚点：`prompt_cache_key`

这两类信息如果同时存在且不一致，网关必须有明确的优先级；否则会出现线程续接异常、旧 `turn-state` 被清掉、兼容模式下原生链路反而更不稳定等问题。

2026-04-15 的修复目标，就是把这条规则重新收口到“原生 Codex 优先”的方向，并避免后续再回归。

## 固定策略

统一规则：

1. 有原生会话锚点时，优先使用原生会话锚点。
2. 没有原生会话锚点时，才使用客户端显式传入的 `prompt_cache_key`。
3. 两边都没有时，才退回兼容兜底锚点。

换成优先级就是：

`native anchor > explicit prompt_cache_key > sticky fallback`

## 具体解释

### 1. 原生 Codex 请求

当请求里已经有以下任一信息时：

- `conversation_id`
- `x-codex-turn-state`

就认为客户端已经提供了稳定线程语义。此时 `prompt_cache_key` 只能作为附属信息，不能反过来抢主导权。

原因：

- 原生 `Codex App` 的 resume / 会话续接本来就围绕原生头部语义工作。
- 如果让 `prompt_cache_key` 压过原生锚点，最容易造成“请求体像线程 B，请求头像线程 A”的冲突。

### 2. OpenAI 兼容客户端 / CLI

如果客户端没有原生会话锚点，而是只在请求体里显式携带了 `prompt_cache_key`，则应保留它。

原因：

- 很多兼容客户端并不会带 `conversation_id`、`x-codex-turn-state`。
- 此时 `prompt_cache_key` 往往是唯一稳定的线程线索。

### 3. Anthropic 原生链路

Anthropic 适配阶段可能生成临时 `prompt_cache_key`，因此不能把这类值直接当作原始客户端线程锚点使用。

## 测试矩阵

当前已固定以下回归场景：

1. 只有原生锚点：沿用原生锚点。
2. 只有 `prompt_cache_key`：保留客户端显式 `prompt_cache_key`。
3. 两者同时存在且冲突：原生锚点优先，`prompt_cache_key` 不得抢主导权。
4. 两者同时存在且一致：仍按原生锚点语义处理，不让请求体字段改变优先级。
5. Anthropic 原生请求：不把客户端 `prompt_cache_key` 当作原始线程锚点。

## 代码落点

- 决策入口：
  `crates/service/src/gateway/local_validation/request.rs`
- 会话锚点推导：
  `crates/service/src/gateway/request/session_affinity.rs`
- 回归测试：
  `crates/service/src/gateway/local_validation/tests/request_tests.rs`
  `crates/service/tests/gateway_logs/prompt_cache.rs`
  `crates/service/src/http/tests/proxy_runtime_tests.rs`

## 排障建议

如果后续再遇到“原生 Codex 在兼容模式下异常”的反馈，优先检查：

1. 请求头里是否已有 `conversation_id` 或 `x-codex-turn-state`
2. 请求体里是否同时带了不同的 `prompt_cache_key`
3. 最终出站请求里 `prompt_cache_key` 是否被错误地改成了客户端自带值
4. `x-codex-turn-state` 是否因为线程锚点冲突被清掉

如果以上 4 点出现任意异常，优先按本文件的优先级规则回看实现，不要再从“让 `prompt_cache_key` 更强势”这个方向修。
