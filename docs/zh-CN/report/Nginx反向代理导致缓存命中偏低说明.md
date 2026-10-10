# Nginx 反向代理导致缓存命中偏低说明

## 结论

如果 CodexManager 部署在 Docker 或独立 Service 前面，再套一层默认配置的 Nginx，最容易出现的隐藏问题不是“模型缓存坏了”，而是 **Nginx 把带下划线的请求头丢掉了**。

CodexManager 依赖这些请求头维持稳定的会话锚点：

- `conversation_id`
- `session_id`
- `x-client-request-id`
- `x-codex-turn-metadata`
- `x-codex-turn-state`

只要其中关键头在反向代理层被吞掉，后端就无法持续复用同一个线程锚点，最终表现出来就是：

- Docker / 域名部署的 `cached_input_tokens` 明显偏低
- 本地桌面端直连 service 的缓存命中明显更高
- 同一会话连续多轮请求，线上缓存增长幅度异常小

这不是 Docker 本身的问题，核心差异是：

- 桌面端通常直连 `codexmanager-service`
- Docker 线上版本通常会多一层 Nginx / Cloudflare / 站点面板反向代理

真正的故障点通常在 Nginx。

## 为什么会这样

Nginx 默认对带下划线的请求头不友好。

如果没有显式开启下面两个开关：

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

像 `conversation_id`、`session_id` 这样的头就可能在进入上游前被当成“无效头”直接丢弃。

对于 CodexManager 来说，这会影响两条关键链路：

1. `crates/service/src/gateway/request/incoming_headers.rs`
   这里负责从入站请求中提取 `conversation_id`、`session_id`、`x-codex-turn-state` 等头。
2. `crates/service/src/gateway/request/session_affinity.rs`
   这里会基于这些头计算稳定的会话锚点。
3. `crates/service/src/gateway/request/official_responses_http.rs`
   这里会把线程锚点写回 `prompt_cache_key`。

一旦代理层把头吞掉，后端就只能退化为“不稳定的 fallback 会话”，自然很难拿到和桌面端相同的缓存命中。

新版本会对一种常见残缺态做代码兜底：如果请求只剩 `x-codex-turn-state`，但 `conversation_id`
和 `session_id` 都已经在代理层丢失，同时请求体仍带有 `prompt_cache_key`，网关会允许
`prompt_cache_key` 只参与本地账号路由，以减少同一缓存前缀被 balanced 轮询打散的概率。
这个兜底不会伪造 `conversation_id`，也不会把内部路由 ID 写给上游。

不过这只是降级保护，不是推荐部署形态。正确修法仍然是完整透传会话头。

## 典型症状

部署出问题时，通常会看到下面这些现象：

- Web 界面的请求日志中，`/v1/responses` 的 `cached_input_tokens` 只有几千
- 本地桌面端同一类请求能稳定达到几万
- 第一轮和后续几轮请求的缓存增长不明显
- 模型、账号、提示词相近，但线上和本地缓存差距异常大

注意：

- 第一轮请求缓存偏低是正常的
- 不同模型、不同账号、不同会话，缓存数值本来就不会完全一样
- 真正值得怀疑的是“连续多轮同会话请求，线上始终明显低于本地”

## 正确修法

### 1. 打开下划线请求头支持

在 `http {}` 里加入：

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

### 2. 显式透传会话相关请求头

在反代到 `codexmanager-service` 的 `location` 里加入：

```nginx
proxy_set_header conversation_id $http_conversation_id;
proxy_set_header session_id $http_session_id;
proxy_set_header x-client-request-id $http_x_client_request_id;
proxy_set_header x-openai-subagent $http_x_openai_subagent;
proxy_set_header x-codex-beta-features $http_x_codex_beta_features;
proxy_set_header x-codex-turn-metadata $http_x_codex_turn_metadata;
proxy_set_header x-codex-turn-state $http_x_codex_turn_state;
```

这一步不是必须“理论上”才需要，而是部署时应该直接做。原因很简单：不同的面板、站点模板、二次代理层行为并不一致，显式透传能避免很多隐性差异。

### 3. 对流式请求关闭代理缓冲

建议对 API 反代同时加上：

```nginx
proxy_buffering off;
proxy_request_buffering off;
proxy_read_timeout 3600s;
proxy_send_timeout 3600s;
```

这不是为了解决缓存命中本身，而是为了避免 Responses / SSE / WebSocket 类请求在代理层被额外干扰。

### 4. 对 `/v1/responses/compact` 单独走更保守的代理配置

如果你线上主要报的是：

- `stream disconnected before completion`
- 本地 Codex 反复重试 compact
- 服务端日志却已经出现多条 `200` 的 `/v1/responses/compact`

那更像是 **compact 成功响应在代理返回给客户端的途中被截断了**。

这种情况下，建议给 `/v1/responses/compact` 单独加一条 location，至少补齐：

```nginx
proxy_set_header Connection "";
proxy_buffering off;
proxy_request_buffering off;
gzip off;
add_header X-Accel-Buffering no;
proxy_read_timeout 600s;
proxy_send_timeout 600s;
send_timeout 600s;
```

当前仓库里的 `docker/nginx/nginx.conf` 已经内置了这一条专用配置，可直接作为部署基线。

### 5. 给 `/v1/images/` 图片生成入口单独保守配置

CodexManager 已支持 `/v1/images/generations` 与 `/v1/images/edits` 兼容入口。这个链路和普通文本请求不同，常见风险是：

- `/v1/images/edits` 可能上传 multipart 图片，body 明显更大
- 图片生成耗时可能长于普通文本首包
- `b64_json` 响应体可能很大，默认代理缓冲容易造成额外截断或延迟

建议给 `/v1/images/` 单独加一条 `location`，至少保留：

```nginx
client_max_body_size 0;
proxy_buffering off;
proxy_request_buffering off;
gzip off;
add_header X-Accel-Buffering no;
proxy_read_timeout 3600s;
proxy_send_timeout 3600s;
send_timeout 3600s;
```

当前仓库里的 `docker/nginx/nginx.conf` 已在直连 service 与 Web 代理两个 server 块中都包含 `location ^~ /v1/images/`，两个公网入口都可直接作为图片生成部署基线。Web 入口还为精确 `/v1/responses` 配置了同样的 3600 秒超时，用于覆盖非流式 `image_generation` 工具调用。

## 推荐示例配置

可直接参考：

- [`docker/nginx/nginx.conf`](../../../docker/nginx/nginx.conf)

这份示例同时覆盖了：

- `manager.example.com -> codexmanager-service:48760`
- `web.example.com -> codexmanager-web:48761`
- HTTPS 跳转
- 下划线请求头支持
- 会话相关头透传
- API 流式代理建议
- `/v1/images/generations` 与 `/v1/images/edits` 图片生成代理建议

## 部署后如何验证

至少验证 1 条关键路径：

1. 用同一个客户端、同一个账号、同一个会话，连续发 3 到 5 次请求到线上域名。
2. 观察最新几条 `/v1/responses` 日志中的 `cached_input_tokens` 是否明显抬升。
3. 与本地桌面端做同账号、同模型、同提示词对比。

如果修复生效，常见表现是：

- 第一条缓存一般仍然不高
- 第二条开始缓存显著增加
- 后续多轮请求会更接近桌面端表现

## 常见误判

### 误判 1：是 Docker 把缓存做坏了

不是。Docker 只是运行方式，真正影响缓存命中的通常是它前面的反向代理。

### 误判 2：是 Cloudflare 在改请求

Cloudflare 会增加代理链路复杂度，但这类问题最常见的直接原因仍然是 Nginx 默认不接受带下划线的请求头。

### 误判 3：日志里缓存低就是后端逻辑错了

也不一定。CodexManager 日志展示的是上游返回的缓存统计结果，如果前面的线程锚点已经丢了，后端只能如实记录“低缓存命中”。

## 排障清单

如果线上缓存仍然偏低，按这个顺序排：

1. 检查 `http {}` 里是否有 `underscores_in_headers on;`
2. 检查 `http {}` 里是否有 `ignore_invalid_headers off;`
3. 检查 API 反代块是否显式透传了 `conversation_id` 和 `session_id`
4. 检查是否还有第二层代理或面板改写了请求头
5. 检查对比样本是否真的是“同账号 + 同模型 + 同会话”
6. 检查是否拿“第一轮请求”去和“多轮复用后的请求”做了错误比较

## 源码依据

- [`crates/service/src/gateway/request/incoming_headers.rs`](../../../crates/service/src/gateway/request/incoming_headers.rs)
- [`crates/service/src/gateway/request/session_affinity.rs`](../../../crates/service/src/gateway/request/session_affinity.rs)
- [`crates/service/src/gateway/request/official_responses_http.rs`](../../../crates/service/src/gateway/request/official_responses_http.rs)
- [`crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs`](../../../crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs)
- [`crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs`](../../../crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs)
