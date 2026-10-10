# 当前网关与 Codex 参数传递对照表

说明：当前工作区里，Codex 出站的直接契约差异已经收完。下面按“参数怎么走 / 最终怎么出站 / 是否和 upstream 对齐”重新整理。

## 1. 参数传递链路

1. 入站 HTTP 请求先进入 `crates/service/src/gateway/request/incoming_headers.rs`，这里只做头快照，不直接改写请求。
2. 会话亲和由 `crates/service/src/gateway/request/session_affinity.rs` 统一计算，产出 `incoming_session_id`、`incoming_client_request_id` 和 `fallback_session_id`。
3. 请求体在 `crates/service/src/gateway/request/request_rewrite.rs` 里进入重写流程，再由 `official_responses_http.rs` 处理 Responses 兼容字段。
4. 最终出站头由 `crates/service/src/gateway/upstream/headers/codex_headers.rs` 组装。
5. 真正发往上游前，由 `crates/service/src/gateway/upstream/attempt_flow/transport.rs` 把 headers + body 交给 reqwest。

## 2. 请求头对照

| 字段 | 当前怎么传 | Codex upstream | 状态 | 备注 |
| --- | --- | --- | --- | --- |
| `Authorization` | 从当前账户 token 组装成 `Bearer <token>` | 同样使用 Bearer token | 已对齐 | 由登录/账号链路提供 token，出站时替换成当前账户值 |
| `originator` | 直接发 `codex_cli_rs` | 同样发 `codex_cli_rs` | 已对齐 | 运行时可配置的原始值会同步到出站头 |
| `User-Agent` | `codex_cli_rs/<运行时版本> (<os/version; arch>) <terminal>` | `codex_cli_rs/<编译时版本> (<os/version; arch>) <terminal>` | 实现级差异 | 版本来源不同，格式已对齐 |
| `x-client-request-id` | 由会话亲和链路优先取 `conversation_id`，没有就不补 | upstream 以 `conversation_id` 作为 request id | 已对齐 | failover 时不会凭空造新值 |
| `session_id` | 由 `conversation_id` / fallback session 算出，failover 时会切到 fallback | upstream 以 `conversation_id` 为主 | 已对齐 | 当前实现保留亲和/回退策略 |
| `x-openai-subagent` | 透传当前请求的 subagent | upstream 也会带相同语义的 subagent | 已对齐 | 仅在有值时发送 |
| `x-codex-beta-features` | 透传入站值 | upstream 会发送 | 已对齐 | 仅在有值时发送 |
| `x-codex-turn-metadata` | 透传入站值 | upstream HTTP 出站同样会带；WS 侧还会包装进 `client_metadata` | 已对齐 | 当前网关未额外构造 WS 同款 `client_metadata` |
| `x-codex-turn-state` | 直连时保留，`strip_session_affinity` 时剥离 | upstream 也依赖该会话态 | 已对齐 | 只在亲和未剥离时发送 |
| `OpenAI-Beta` | 流式请求发 `responses_websockets=2026-02-06` | upstream WebSocket 路径同值 | 已对齐 | 非流式不发 |
| `x-responsesapi-include-timing-metrics` | 流式且开启时发 `true` | upstream 同语义 | 已对齐 | 非流式不发 |
| `ChatGPT-Account-ID` | 不再作为 Codex 出站头发送 | upstream 直接请求里也不作为 Codex 协议头发送 | 已对齐 | 只保留在内部账户/usage 路径，不计入出站契约 |

## 3. 请求参数对照

| 字段 | 当前怎么传 | Codex upstream | 状态 | 备注 |
| --- | --- | --- | --- | --- |
| `model` | 由入站请求或适配层选定后写回 | upstream 由 prompt/model 选择决定 | 已对齐 | 结果值对齐，来源不同 |
| `instructions` | 缺失时在 Codex compat 路径补空字符串 | upstream 由 prompt 基础指令生成 | 已对齐 | compat 路径保留空指令以稳住上游校验 |
| `input` | 字符串/对象会先归一成数组 | upstream 使用 Responses input 数组 | 已对齐 | 兼容旧入参格式 |
| `tools` | dynamic tools 会先映射为 `function` tools | upstream 直接从 prompt tools 构建 | 已对齐 | 名称短化和恢复逻辑由适配层处理 |
| `tool_choice` | 缺失或非 auto 时收敛为 `auto` | upstream 核心路径默认 `auto` | 已对齐 | 目前不再保留项目扩展值 |
| `parallel_tool_calls` | 无 tools 且缺省时补 `false` | upstream 由 prompt 配置显式传入 | 部分一致 | 结果字段已对齐，默认来源不同 |
| `reasoning` | 由请求侧 `reasoning` / `reasoning_effort` 和模型默认值归一后输出 | upstream 由模型默认 reasoning + turn 配置构建 | 已对齐 | 输出形状一致，来源链路不同 |
| `store` | Codex compat 路径统一改成 `false` | upstream 非 Azure Responses 时为 `false` | 已对齐 | Azure 特例当前不纳入这条出站路径 |
| `stream` | 标准 Responses 路径强制 `true`，compact 路径保持 compact 形态 | upstream Responses 主路径固定 `true` | 已对齐 | `stream_passthrough` 只是内部开关 |
| `include` | 只有 `reasoning` 存在时才补 `["reasoning.encrypted_content"]` | upstream 同样只在 reasoning 存在时带 | 已对齐 | 不再补空数组 |
| `service_tier` | `Fast -> priority`，其他值原样保留 | upstream 同样按枚举语义映射 | 已对齐 | 目前只看 Codex 兼容值 |
| `prompt_cache_key` | 由会话锚点生成，必要时可内部强制覆盖 | upstream 直接使用 `conversation_id` | 已对齐 | 当前实现保留覆盖能力，但出站结果已对齐 |
| `text` | 由 verbosity / response_format / schema 归一后输出 | upstream 由 `create_text_param_for_request()` 生成 | 已对齐 | 输出结构一致 |
| `stream_passthrough` | 仅作为内部适配标记，出站前会剥离 | upstream 无此字段 | 内部字段 | 不计入 Codex 协议差异 |

## 4. 结论

1. 当前网关的 Codex 出站请求头和请求参数，已经和 upstream 保持同形。
2. 现在剩下的主要是实现级差异，比如 `User-Agent` 的版本来源，以及 WS 场景里 `x-codex-turn-metadata` 的 `client_metadata` 包装。
3. `stream_passthrough`、内部账户标识、会话亲和回退这些都属于适配层内部逻辑，不再算 Codex 直接请求契约差异。

## 5. 源码依据

- `crates/service/src/gateway/request/incoming_headers.rs`
- `crates/service/src/gateway/request/session_affinity.rs`
- `crates/service/src/gateway/request/request_rewrite.rs`
- `crates/service/src/gateway/request/official_responses_http.rs`
- `crates/service/src/gateway/upstream/headers/codex_headers.rs`
- `crates/service/src/gateway/upstream/attempt_flow/transport.rs`
- `crates/service/src/gateway/core/runtime_config.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\core\src\client.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\common.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\endpoint\responses.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\requests\headers.rs`
