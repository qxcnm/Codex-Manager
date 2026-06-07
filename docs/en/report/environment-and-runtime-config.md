# Environment and Runtime Configuration

## 目标

本文档用于集中说明 CodexManager 的环境变量加载规则、设置页持久化规则，以及常用配置项的推荐改法。

源码中的 `CODEXMANAGER_*` 常量仍是最终准入标准；本文档负责给协作者提供稳定入口，不替代源码校验。

## 配置来源与优先级

当前配置来源主要有四层：

1. 系统环境变量 / 启动命令环境变量
2. 可执行文件同目录环境文件：`codexmanager.env` -> `CodexManager.env` -> `.env`
3. `app_settings` 持久化表
4. 桌面端设置页的专属设置卡片 / 高级环境变量编辑器

实际生效优先级可概括为：

- 当前进程已存在环境变量（包括启动前系统环境变量，以及启动时从 `env` 文件读入的值）
- 专属设置卡片 / 持久化 `envOverrides`
- 代码默认值

补充规则：

- 环境文件只会注入“当前进程尚未定义”的变量
- 进程启动后，设置页保存的 `envOverrides` 只会补齐“当前进程还没有”的变量
- 桌面端当前已改为 `env` 优先：如果进程里已经有同名 `CODEXMANAGER_*` 变量，设置页持久化配置不会再覆盖它
- 支持热更新的 service 运行时配置会立即 reload，但同名 `env` 已存在时会优先沿用 `env`
- bootstrap 级变量仍要求在进程启动前准备好

## env 文件放哪

当前进程会按下面顺序在“可执行文件同目录”查找环境文件：

- `codexmanager.env`
- `CodexManager.env`
- `.env`

也就是说：

- 启动桌面端程序时，`env` 文件要放在桌面端实际可执行文件所在目录
- 启动 `codexmanager-start` / `codexmanager-service` / `codexmanager-web` 时，`env` 文件也要放在对应可执行文件旁边
- 三种文件名只会命中一个，优先推荐统一使用 `codexmanager.env`

## 建议如何修改配置

### 优先走设置页的项目

以下变量已有专属设置卡片，不建议再通过通用环境变量编辑器修改：

- `CODEXMANAGER_SERVICE_ADDR`
- `CODEXMANAGER_ROUTE_STRATEGY`
- `CODEXMANAGER_UPSTREAM_PROXY_URL`
- `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS`
- `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS`
- `CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS`
- 后台任务相关轮询 / worker 变量

补充说明：

- 历史兼容开关已从主线路径移除，后续不再作为设置页或排障入口保留。

### 必须走系统环境或 env 文件的项目

以下变量属于 bootstrap 配置，不能依赖启动后再补：

- `CODEXMANAGER_DB_PATH`
- `CODEXMANAGER_RPC_TOKEN`
- `CODEXMANAGER_RPC_TOKEN_FILE`

## 常用变量分组

### 地址与入口

- `CODEXMANAGER_SERVICE_ADDR`：service 地址，默认 `localhost:48760`
- `CODEXMANAGER_WEB_ADDR`：web 地址，默认 `localhost:48761`
- `CODEXMANAGER_WEB_ROOT`：web 静态资源目录
- `CODEXMANAGER_LOGIN_ADDR`：本地 OAuth 回调监听地址

### Docker containers

- `TZ`: container time zone. Docker images default to `Asia/Shanghai`; compose examples use `${TZ:-Asia/Shanghai}`, so they pass through `TZ` from the deployment environment and fall back to `Asia/Shanghai` when unset. Change it to your own IANA time zone when deploying elsewhere.

### 网关与上游

- `CODEXMANAGER_UPSTREAM_BASE_URL`
- `CODEXMANAGER_UPSTREAM_PROXY_URL`
- `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS`: gateway request total timeout in milliseconds. Default `0` means the service does not cut requests off by total duration.
- `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS`
- `CODEXMANAGER_USE_WEBSOCKET_UPSTREAM`: makes ChatGPT `/v1/responses` streaming upstream requests try WebSocket first. Default `0`. This is experimental; failures fall back to HTTP streaming and the path uses the configured upstream proxy and connect timeout.
- `CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS`
- `CODEXMANAGER_PROXY_LIST`
- `CODEXMANAGER_ROUTE_STRATEGY`

### Codex image generation

- `CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED`: enables the image-generation compatibility path. Default `1`.
- `CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL`: automatically injects the `image_generation` tool into normal `/v1/responses` requests. Default `1` to match the official Codex client behavior; explicit client-provided tools are not duplicated.
- `CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL`: main conversation model used internally by Images API compatibility endpoints. Default `gpt-5.4-mini`.
- `CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL`: image tool model. Default `gpt-image-2`.

Notes:

- `/v1/images/generations` and `/v1/images/edits` are converted internally to `/v1/responses + image_generation tool`.
- When Codex CLI sends `tools[].type = "image_generation"` natively, the gateway forwards it and does not affect the existing text request path.

## 迁移中的弃用项

以下旧兼容路径已经移除，不再作为配置项继续维护：

迁移方向：

- 项目策略：不再推荐、不再依赖、不再围绕旧兼容开关设计排障路径
- 后续方向：以会话绑定、自动切线程和 `Codex-First` 语义作为主路径

相关决策与设计文档：


### Web 与访问控制

- `CODEXMANAGER_WEB_ADDR`
- `CODEXMANAGER_WEB_ROOT`
- `CODEXMANAGER_WEB_NO_OPEN`
- `CODEXMANAGER_WEB_NO_SPAWN_SERVICE`

补充说明：

- Web 访问密码当前由设置页写入 `app_settings` 的 `web.auth.password_hash`，不是公开环境变量。

### 后台任务与并发

- `CODEXMANAGER_USAGE_POLLING_ENABLED`
- `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS`
- `CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED`
- `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS`
- `CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED`
- `CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS`
- `CODEXMANAGER_WARMUP_CRON_ENABLED`
- `CODEXMANAGER_WARMUP_CRON_EXPRESSION`
- `CODEXMANAGER_USAGE_REFRESH_WORKERS`
- `CODEXMANAGER_HTTP_WORKER_FACTOR`
- `CODEXMANAGER_HTTP_WORKER_MIN`
- `CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR`
- `CODEXMANAGER_HTTP_STREAM_WORKER_MIN`

### 存储与鉴权

- `CODEXMANAGER_DB_PATH`
- `CODEXMANAGER_RPC_TOKEN`
- `CODEXMANAGER_RPC_TOKEN_FILE`
- `CODEXMANAGER_NO_SERVICE`

### 更新与发布辅助

- `CODEXMANAGER_UPDATE_PRERELEASE`
- `CODEXMANAGER_UPDATE_REPO`
- `CODEXMANAGER_GITHUB_TOKEN`
- `GITHUB_TOKEN`
- `GH_TOKEN`

## env 文件示例

```dotenv
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_WEB_ADDR=localhost:48761
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS=0
CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED=1
CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL=1
CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL=gpt-image-2
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

## 文件夹示例

### 桌面端

Windows 桌面端最直观的放法如下：

```text
CodexManager/
├─ CodexManager.exe
├─ codexmanager.env
└─ 其他运行库文件...
```

对应 `codexmanager.env` 示例：

```dotenv
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_UPSTREAM_PROXY_URL=http://127.0.0.1:7890
CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS=0
CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS=600000
CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS=15000
```

补充说明：

- 桌面端会读取程序同目录下的 `codexmanager.env`
- 但桌面端默认数据库不放在程序目录，而是放在系统应用数据目录
- 默认数据库位置可参考根说明里的“默认数据目录”： [../README.md](../README.md)

### Service 版

如果你用的是 `codexmanager-start` 一键启动包，更推荐把程序和 `env` 文件放成下面这样：

```text
codexmanager-service-bundle/
├─ codexmanager-start.exe
├─ codexmanager-service.exe
├─ codexmanager-web.exe
├─ codexmanager.env
└─ web/
   └─ ...
```

对应 `codexmanager.env` 示例：

```dotenv
CODEXMANAGER_SERVICE_ADDR=0.0.0.0:48760
CODEXMANAGER_WEB_ADDR=0.0.0.0:48761
CODEXMANAGER_DB_PATH=./data/codexmanager.db
CODEXMANAGER_RPC_TOKEN_FILE=./data/codexmanager.rpc-token
CODEXMANAGER_UPSTREAM_PROXY_URL=http://127.0.0.1:7890
```

这时目录通常会长成这样：

```text
codexmanager-service-bundle/
├─ codexmanager-start.exe
├─ codexmanager-service.exe
├─ codexmanager-web.exe
├─ codexmanager.env
├─ data/
│  ├─ codexmanager.db
│  └─ codexmanager.rpc-token
└─ web/
   └─ ...
```

补充说明：

- `CODEXMANAGER_DB_PATH=./data/codexmanager.db` 这种相对路径，会按“可执行文件所在目录”解析
- `CODEXMANAGER_RPC_TOKEN_FILE` 也是同样规则
- 如果你不写 `CODEXMANAGER_DB_PATH`，Service 版默认会把数据库放到程序目录下的 `codexmanager.db`

## 排障建议

### OpenAI 上游代理到底作用在哪

`CODEXMANAGER_UPSTREAM_PROXY_URL` / 设置页“OpenAI 上游代理”当前主要接管的是：

- 网关向上游平台发起的请求
- ChatGPT upstream WebSocket requests when `CODEXMANAGER_USE_WEBSOCKET_UPSTREAM=1` is enabled
- 用量查询请求
- `refresh_token` 刷新 access token 的请求

Safety note:

- Login-state refresh paths such as `auth.openai.com` / `oauth/token` must exit from an OpenAI-supported region; do not include restricted locations such as Hong Kong in automatic rotation for this path.
- If the refresh request returns `unsupported_country_region_territory`, Service marks the account as `refresh_token_region_blocked` and pauses subsequent automatic refresh attempts. Fix the proxy route, then manually restore the account.

默认不接管的是：

- 你本机浏览器 / 桌面端访问 `localhost` / `127.0.0.1` 的本地回环请求
- 本地 service 与 web 之间的回环通信
- 登录回调成功后的 OAuth `code -> token` 兑换链路

补充规则：

- 如果同时设置了 `CODEXMANAGER_UPSTREAM_PROXY_URL` 和 `CODEXMANAGER_PROXY_LIST`，单个上游代理优先，代理池会被旁路。
- `CODEXMANAGER_PROXY_LIST` 更适合“按账号稳定分流多个出口”；`CODEXMANAGER_UPSTREAM_PROXY_URL` 更适合“全局统一走一个出口”。

### 改了 env 文件但没生效

优先检查：

1. 是否重启了对应进程
2. 是否已有同名系统环境变量
3. 是否又被设置页持久化配置覆盖

### 设置页改了但服务行为没变

优先检查：

1. 该配置是否属于“重启后生效”
2. 当前改的是专属设置卡片还是通用环境变量编辑器
3. service 日志里是否出现 reload / apply 失败

### 设置了网关传输参数但长流还是异常

优先检查：

1. Check whether `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS` is too short and ends long streaming requests early.
2. Check whether `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` is set to a short non-zero value and cuts requests off by total duration.
3. Check whether `CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS` is too long and lets an intermediate proxy treat SSE as idle.
4. Check client-side timeout or proxy idle limits. In request logs, `120s/1.8s` means total duration / first-response duration; it does not necessarily mean the service total-timeout setting is `120s`.

### 本地回环请求异常

优先检查：

- 是否把系统代理错误地应用到了 `localhost/127.0.0.1`
- 地址是否被写成非预期大小写或非 loopback 主机名

## 相关文档

- 根说明：[README.md](../README.md)
- 架构说明：[ARCHITECTURE.md](../ARCHITECTURE.md)
- 发布说明：[release-and-artifacts.md](../release/release-and-artifacts.md)
