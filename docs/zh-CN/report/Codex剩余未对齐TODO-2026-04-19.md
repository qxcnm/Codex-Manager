# Codex剩余未对齐TODO - 2026-04-19

## 1. 当前结论

如果只看原生 `/v1/responses -> Codex backend` 这条主链路：

- 请求头和请求体 shape：已经高度对齐
- SSE 解析组件：已经对齐到 `eventsource-stream`
- 上游 transport 执行模型：还没有完全对齐
- bridge 事件模型：还没有完全对齐

如果把 `chat/completions` / `completions` 兼容入口也算进去：

- 还不能说“和官方完全一样”
- 因为这条链路天然要先做协议映射，再转成内部 `/v1/responses`

## 2. 当前状态

按这份清单最初定义的口径，这一轮已经收口完毕：

- P0：2 项已完成
- P1：2 项已完成
- P2：2 项已评估并明确保持现状

其中：

- `已完成` = 已经落代码并通过关键回归
- `已评估并保持现状` = 继续改会明显增加对账号管理、自动切换、转发主链路的风险，当前不再推进

## 3. TODO 清单

### P0

- [x] 让可控调用方尽量直接走原生 `/v1/responses`
  - 结果：运行时前端调用已基本收口到原生 `/v1/responses`，这轮再次全仓复核后，剩余 `/v1/chat/completions` / `/v1/completions` 主要只存在于协议兼容入口、验证逻辑、观测统计和测试覆盖中
  - 保留原因：这些剩余路径属于我们对外兼容能力的一部分，不是主功能内部调用
  - 关键代码：
    - `crates/service/src/gateway/protocol_adapter/request_router.rs`
    - `crates/service/src/gateway/protocol_adapter/codex_adapter.rs`

- [x] 统一 `response.incomplete` / `idle timeout` / body error 的归因和终态文案
  - 结果：
    - `response.incomplete` 无详细错误时统一为“连接中断（可能是网络波动或客户端主动取消）”
    - `stream_timeout` / `stream idle timeout` 统一为“上游流式空闲超时”
    - `request or response body error` / `stream read failed` 统一为“上游中途断开，未返回具体错误信息”
  - 同步范围：
    - `crates/service/src/gateway/observability/http_bridge/stream_readers/common.rs`
    - `crates/service/src/gateway/observability/http_bridge/stream_readers/openai_responses.rs`
    - `crates/service/src/errors/mod.rs`
    - `apps/src/lib/api/transport-errors.ts`

### P1

- [x] 把 `/v1/responses` 观测链路从 SSE frame inspector 继续收成 typed `ResponseEvent`
  - 结果：`responses` 终态、usage、error hint 已抽成专用 typed event 模块，不再把这部分逻辑继续堆在 `sse_frame.rs` 里
  - 关键代码：
    - `crates/service/src/gateway/observability/http_bridge/aggregate/openai_responses_event.rs`
    - `crates/service/src/gateway/observability/http_bridge/aggregate/sse_frame.rs`
    - `crates/service/src/gateway/observability/http_bridge/stream_readers/openai_responses.rs`

- [x] 继续收窄 OpenAI compat 映射默认值和官方原生 `responses` 的差异
  - 结果：
    - 不再无条件补 `instructions`
    - 没有显式 reasoning 时不再补默认 `reasoning`
    - 没有 tools 时不再补 `tool_choice` / `parallel_tool_calls`
    - 没有 reasoning 时不再补 `include`
    - 官方 `/v1/responses` allowlist 不再把 `stream_passthrough` 当成官方字段
  - 关键代码：
    - `crates/service/src/gateway/protocol_adapter/request_router.rs`
    - `crates/service/src/gateway/request/official_responses_http.rs`

### P2

- [x] 评估是否把 `/v1/responses` 的上游请求执行从 `reqwest::blocking::Client` 继续往 async transport 靠
  - 结论：当前不继续推进
  - 原因：这会穿透 candidate / retry / failover / postprocess 整条执行链，已经超出“协议继续对齐”的安全边界，容易影响账号管理、自动切换和转发主功能
  - 当前保留状态：已有 `GatewayStreamResponse` / `GatewayByteStream` 抽象，暂时停在这层

- [x] 评估是否把 bridge 的 keepalive 注入与 SSE 重编码改成“原始字节透传 + sidecar 观测”
  - 结论：当前不继续推进
  - 原因：这会改动 bridge 的时序和下游兼容行为，收益主要是“更像官方”，但对主功能稳定性的风险高于收益
  - 当前保留状态：继续使用现在的 keepalive 注入与 bridge 回放语义

## 4. 暂时不建议继续折腾的项

- [x] `User-Agent` 版本来源
  - 这是实现级差异，不是主故障源

- [x] token/account 的真实值差异
  - 网关本来就不可能用官方账号上下文，属于运行环境差异，不是协议对齐问题

- [x] 其它 provider 一并 async 化
  - 当前最大收益点仍然是 `/v1/responses`

## 5. 建议顺序

1. 当前这份清单已经收口完成
2. 后续如果再继续推进，应单独立项做“全链路 async transport 重构”
3. 那会是稳定性风险较高的大改，不应混在常规协议对齐里

## 6. 一句话判断

现在不是“还有高优先级尾差没收”，而是：

- 原生 `/v1/responses`：请求 shape、解析组件、终态归因、typed event 都已经收口到当前安全边界
- 兼容入口：默认值尾差已经明显收窄，剩余的是兼容能力本身，不是缺陷
- 真正还没继续做的，只剩高风险的大改造项，而且已经明确评估为当前不推进
