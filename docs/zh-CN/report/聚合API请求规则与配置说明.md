# 聚合 API 请求规则与配置说明

本文按当前源码实现整理聚合 API 的配置方式、请求转发规则、模型映射规则和余额查询规则。源码仍是最终准入标准，本文用于日常配置、排查和交接。

## 适用范围

聚合 API 是平台 Key 的一种上游来源。它不使用本地 OpenAI/Claude/Gemini 账号池，而是把客户端请求转发到第三方 API 供应商。

适合场景：

- 使用 New API、One API、OpenAI-compatible、Anthropic-compatible、Gemini-compatible 这类外部供应商。
- 某些平台 Key 不想使用账号池，直接绑定一个供应商。
- 账号池优先，但账号池不可用时由聚合 API 兜底。
- 某个模型需要固定转发到某个供应商模型。

不适合场景：

- 需要 CodexManager 代理官方账号登录态、RT/AT 刷新、账号健康切换的请求。这类仍应走账号池。
- 需要在聚合 API 内做复杂协议转换的请求。纯聚合 API 路由当前是 passthrough，只做少量请求覆盖和鉴权注入。

## 核心概念

### 平台 Key

平台 Key 是客户端使用的入口密钥。每个 Key 可以配置：

- 协议类型：OpenAI-compatible、Claude native、Gemini native。
- 轮转策略：账号池、聚合 API、账号池优先聚合兜底。
- 绑定模型、推理等级、服务层级等请求默认值。
- 可选绑定一个首选聚合 API。

### 聚合 API

聚合 API 是一个可转发的上游供应商记录，包含：

- 供应商类型：`codex`、`claude`、`gemini`。
- 上游基础地址：例如 `https://api.openai.com` 或带供应商前缀的 `https://open.bigmodel.cn/api/anthropic`。
- 认证方式：API Key 或用户名密码。
- 自定义鉴权参数。
- 自定义 action path。
- 可选 `modelOverride`。
- 可选余额查询配置。
- 可选模型关联。

### 模型映射

模型映射用于把平台模型绑定到具体聚合 API 来源和上游模型。

例如：

- 平台模型：`gpt-5.5`
- 来源：`aggregate_api`
- 来源 ID：`ag_xxx`
- 上游模型：`gpt-5.4-mini`

命中该平台模型时，请求候选源会被过滤到绑定了该模型的聚合 API，并把请求 JSON body 中的 `model` 改写为上游模型。

## 请求入口

Web 网关会把以下请求代理到 service：

- `/v1`
- `/v1/{*path}`
- `/v1alpha/{*path}`
- `/v1beta/{*path}`
- Gemini internal generate/count 路径

Web 到 service 的 body 限制由环境变量控制：

| 配置 | 默认值 | 说明 |
| --- | --- | --- |
| `CODEXMANAGER_GATEWAY_PROXY_MAX_BODY_BYTES` | `0` | `0` 表示不限制；大于 0 时按字节限制 Web 网关代理请求体大小。 |

Service 侧还会按请求路径识别协议：

| 请求路径 | 协议 |
| --- | --- |
| `/v1/messages`、`/v1/messages/*`、`/v1/messages?*` | `anthropic_native` |
| `/v1/models/*:generateContent`、`/v1beta/models/*:generateContent`、`/v1alpha/models/*:generateContent` | `gemini_native` |
| `/v1/models/*:streamGenerateContent`、`/v1beta/models/*:streamGenerateContent`、`/v1alpha/models/*:streamGenerateContent` | `gemini_native` |
| `/v1/models/*:countTokens`、`/v1beta/models/*:countTokens`、`/v1alpha/models/*:countTokens` | `gemini_native` |
| 其他标准 `/v1/*` | `openai_compat` |

## 轮转策略

平台 Key 的 `rotationStrategy` 支持以下值：

| 规范值 | 别名 | 行为 |
| --- | --- | --- |
| `account_rotation` | `account`、`account_rotate`、`账号轮转` | 只走账号池。 |
| `aggregate_api_rotation` | `aggregateapi`、`aggregate_api`、`aggregateapirotation`、`聚合api`、`聚合api轮转` | 只走聚合 API。 |
| `hybrid_rotation` | `hybrid`、`mixed`、`mixed_rotation`、`混合轮转`、`账号优先聚合兜底` | 先走账号池；账号池耗尽或不可用后再走聚合 API。 |

全局候选排序策略由 `CODEXMANAGER_ROUTE_STRATEGY` 或设置页控制：

| 值 | 别名 | 聚合 API 行为 |
| --- | --- | --- |
| `ordered` | `order`、`priority`、`sequential` | 按聚合 API 的 `sort ASC, created_at DESC, id ASC` 顺序尝试。 |
| `balanced` | `round_robin`、`round-robin`、`rr` | 多个候选时按平台 Key + 模型维度轮询；如果平台 Key 绑定了首选聚合 API，首选项保持第一位，其余候选再轮询。 |

默认策略是 `ordered`。

## 候选源选择规则

聚合 API 候选按以下步骤筛选：

1. 按请求协议映射供应商类型。
2. 只取 `status = active` 的聚合 API。
3. 只取供应商类型匹配的聚合 API。
4. 按 `sort ASC, created_at DESC, id ASC` 排序。
5. 如果平台 Key 绑定了 `aggregateApiId`，会把该记录插到候选列表第一位。
6. 如果请求带模型，且存在模型映射，则只保留绑定了该平台模型的聚合 API，并把候选的 `modelOverride` 设置为映射里的上游模型。
7. 如果全局策略是 `balanced`，再按 Key + 模型做轮询排序。

协议到供应商类型的映射：

| 协议 | 聚合 API providerType |
| --- | --- |
| `anthropic_native` | `claude` |
| `gemini_native` | `gemini` |
| 其他 | `codex` |

如果没有可用候选，会返回类似：

- `aggregate api not found for provider codex`
- `model_unavailable: gpt-5.5`

## 聚合 API 字段说明

| 字段 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `providerType` | 否 | `codex` | 供应商类型。 |
| `supplierName` | 是 | 无 | 供应商显示名。 |
| `sort` | 否 | `0` | 候选排序值，越小越靠前。 |
| `url` | 否 | 按 providerType 默认 | 上游请求 URL，只允许 `http` / `https`，尾部 `/` 会被移除。默认会原样作为请求地址使用。 |
| `status` | 否 | `active` | `active` 或 `disabled`。 |
| `authType` | 否 | `apikey` | `apikey` 或 `userpass`。 |
| `key` | API Key 鉴权时是 | 无 | 聚合 API 的上游密钥，单独存储。 |
| `username` / `password` | userpass 鉴权时是 | 无 | 用户名密码，序列化后作为 secret 存储。 |
| `authCustomEnabled` | 否 | 不变/关闭 | 是否启用自定义鉴权参数。 |
| `authParams` | 自定义鉴权时是 | 无 | JSON 对象，见下文。 |
| `actionCustomEnabled` | 否 | 不变/关闭 | 是否启用自定义请求路径。 |
| `action` | 否 | 空路径 | 自定义 action path，只能是相对路径；仅启用自定义 action 时才会追加到 `url` 后。 |
| `modelOverride` | 否 | 无 | 固定上游模型；为空或 `auto` 表示不覆盖。 |
| `balanceQueryEnabled` | 否 | `false` | 是否启用余额查询。 |
| `balanceQueryTemplate` | 否 | `generic` | 余额查询模板。 |
| `balanceQueryBaseUrl` | 否 | 聚合 API `url` | 余额查询 base URL。 |
| `balanceQueryAccessToken` | 否 | provider secret | 余额查询专用 access token，单独存储。 |
| `balanceQueryUserId` | 否 | 无 | New API 查询时作为 `New-Api-User` header。 |
| `balanceQueryConfigJson` | custom 模板时是 | 无 | 自定义余额 JSON 配置。 |
| `modelSlugs` | 否 | 无 | 该聚合 API 关联的平台模型列表。 |

### providerType

支持以下规范值和别名：

| 规范值 | 别名 |
| --- | --- |
| `codex` | `codex`、`openai`、`openai_compat`、`gpt` |
| `claude` | `claude`、`anthropic`、`anthropic_native`、`claude_code` |
| `gemini` | `gemini`、`gemini_native`、`google`、`google_ai`、`google_gemini` |

默认 URL：

| providerType | 默认 URL |
| --- | --- |
| `codex` | `https://api.openai.com/v1` |
| `claude` | `https://api.anthropic.com/v1` |
| `gemini` | `https://generativelanguage.googleapis.com` |

注意：未启用自定义 action 时，请求转发会直接使用 `url` 原样请求，不追加客户端原始路径。生产配置建议：

- 通用 OpenAI-compatible 供应商：`url` 直接写最终接口地址，例如 `https://api.example.com/v1/responses`。
- 供应商必须带固定前缀时：把完整最终路径写进 `url`。
- 需要按供应商固定 path 覆盖时：启用 action，并把 action 写成 `/chat/completions`、`/responses` 这类路径。

### status

支持：

- 启用：`active`、`enabled`、`enable`
- 禁用：`disabled`、`disable`、`inactive`

只有 `active` 会进入候选源。

### modelOverride

`modelOverride` 规则：

- 空字符串、未配置、`auto` 都表示不覆盖。
- 只允许 ASCII 字母、数字、`-`、`_`、`.`、`/`、`:`。
- 纯聚合 API 请求会在转发前尝试解析 JSON body；如果 body 是对象且存在 `model`，会改写为 `modelOverride`。
- 模型发现时，如果配置了 `modelOverride`，会直接返回这个模型，不再请求上游模型列表。

优先级：

1. 模型映射里的上游模型会写入候选的 `modelOverride`。
2. 聚合 API 自身的 `modelOverride` 用于未被映射覆盖的请求。
3. 都没有时保留客户端原始 `model`。

## action 和 URL 拼接规则

聚合 API 最终上游地址默认就是 `url`；只有启用自定义 action 时才由 `url` 和 action path 组成。

### 未启用 action

如果没有启用自定义 action，直接使用 `url` 原样请求：

```text
url=https://api.example.com/v1/responses
client path=/v1/chat/completions
final=https://api.example.com/v1/responses
```

### 启用 action

如果配置了 action，则忽略客户端原始路径，固定使用 action：

```text
url=https://api.example.com
action=/v1/responses
final=https://api.example.com/v1/responses
```

action 规则：

- 只能是路径，不能是完整 URL。
- `responses` 会自动规范成 `/responses`。
- `action` 里可以带 query，例如 `/v1/messages?beta=true`。
- `action` 写成 `@raw`、`raw` 或 `__raw__` 时，也会直接使用 `url` 原样请求。
- 空 action 等价于没有自定义 action，即直接使用 `url`。

### base URL 带路径前缀

base URL 的路径前缀会保留：

```text
url=https://open.bigmodel.cn/api/anthropic
action=/v1/messages
final=https://open.bigmodel.cn/api/anthropic/v1/messages
```

## 鉴权配置

### API Key 默认鉴权

当 `authType = apikey` 且未启用自定义鉴权时，转发时注入：

```http
Authorization: Bearer <key>
```

### API Key 自定义 header

```json
{
  "location": "header",
  "name": "x-api-key",
  "headerValueFormat": "raw"
}
```

字段说明：

| 字段 | 值 | 说明 |
| --- | --- | --- |
| `location` | `header` | 把密钥放到 header。 |
| `name` | 任意合法 header 名 | header 名必填。 |
| `headerValueFormat` | `bearer` 或 `raw` | `bearer` 会注入 `Bearer <key>`；`raw` 只注入原始 key。 |

示例：

```json
{
  "location": "header",
  "name": "Authorization",
  "headerValueFormat": "bearer"
}
```

```json
{
  "location": "header",
  "name": "api-key",
  "headerValueFormat": "raw"
}
```

### API Key 自定义 query

```json
{
  "location": "query",
  "name": "api_key"
}
```

转发时会把 key 写入最终 URL query：

```text
https://api.example.com/v1/chat/completions?api_key=<key>
```

如果 query 中已有同名参数，会先移除旧值再追加新值。

### 用户名密码默认鉴权

当 `authType = userpass` 且未启用自定义鉴权时，转发使用 HTTP Basic Auth。

### 用户名密码自定义 headerPair

```json
{
  "mode": "headerPair",
  "usernameName": "x-user",
  "passwordName": "x-password"
}
```

转发时注入：

```http
x-user: <username>
x-password: <password>
```

### 用户名密码自定义 queryPair

```json
{
  "mode": "queryPair",
  "usernameName": "username",
  "passwordName": "password"
}
```

转发时写入 query：

```text
?username=<username>&password=<password>
```

## 转发 header 规则

聚合 API 会透传大部分客户端 header，但以下 header 不会透传：

- `authorization`
- `x-api-key`
- `api-key`
- `content-length`
- `connection`
- `proxy-authorization`
- `proxy-authenticate`
- `te`
- `trailer`
- `transfer-encoding`
- `upgrade`
- `host`
- 自定义鉴权注入的 header 名

流式请求还会丢弃客户端 `accept`，改为：

```http
Accept: text/event-stream
```

这样可以避免客户端旧鉴权、错误 host、错误 content-length 或重复鉴权污染上游请求。

## 请求体处理规则

纯聚合 API 路由是 passthrough，默认不做账号池协议适配。

仍会执行的处理：

- 平台 Key 默认模型、推理等级、service tier 会写入请求。
- 非原生 Codex 客户端访问 `/v1/responses` 且没有显式 stream 时，会默认补 `stream=true`。
- 如果聚合 API 或模型映射产生了 `modelOverride`，会尝试改写 JSON body 顶层 `model` 字段。
- 会执行文本输入长度检查。

不会做的处理：

- 不把 OpenAI chat 自动深度转换成官方 Codex Responses 账号池请求。
- 不使用账号池的 AT/RT、会话绑定、账号健康预检。
- 不使用账号池计费归属。

## 重试与失败规则

每个聚合 API 候选最多会尝试 4 次：首次请求 + 3 次重试。

失败处理：

| 场景 | 行为 |
| --- | --- |
| 当前候选缺少 secret | 记录 403 失败，尝试下一个候选。 |
| URL 或 authParams 无效 | 记录失败，尝试下一个候选。 |
| 上游超时 | 记录 504 失败，尝试重试或下一个候选。 |
| 上游返回非 2xx | 摘要上游错误体，当前候选重试；最终对客户端按 502 处理。 |
| 所有候选失败 | 返回最后一次失败信息。 |
| 没有候选 | 返回 404 或 `aggregate api not found...`。 |

请求日志会记录：

- trace id
- 原始路径和适配路径
- response adapter
- 平台 Key
- 实际来源类型 `aggregate_api`
- 实际来源 ID
- 聚合 API 供应商名和 URL
- 尝试过的聚合 API ID
- 上游模型
- 状态码、耗时、token、错误摘要

## 余额查询配置

余额查询只影响管理界面展示，不参与实时请求转发。

启用字段：

```json
{
  "balanceQueryEnabled": true,
  "balanceQueryTemplate": "generic"
}
```

支持模板：

| 模板 | 别名 | 说明 |
| --- | --- | --- |
| `generic` | `generic` | 通用余额接口探测。 |
| `new_api` | `newapi`、`new_api` | New API 格式余额查询。 |
| `custom` | `custom`、`custom_json` | 自定义余额接口和 JSON 字段路径。 |

余额请求默认 header：

```http
Accept: application/json
Accept-Encoding: identity
User-Agent: codex-manager/aggregate-api-balance
```

### generic 模板

请求顺序：

1. `GET <base>/user/balance`
2. 如果 404、405、501、非 JSON 或缺少余额字段，则 fallback 到 `GET <usage_base>/v1/usage`

base URL 规则：

- 优先使用 `balanceQueryBaseUrl`。
- 未配置时使用聚合 API `url`。
- fallback `/v1/usage` 时，如果未显式配置 `balanceQueryBaseUrl` 且 `url` 以 `/v1` 结尾，会先去掉 `/v1`。

支持的余额字段：

- `remaining`
- `balance`
- `available`
- `quota.remaining`
- `data.remaining`
- `data.balance`
- `data.available`
- `data.quota.remaining`
- `credits.balance`

有效性字段：

- `success`
- `is_active`
- `active`
- `data.is_active`
- `data.active`
- `isValid`
- `is_valid`
- `data.isValid`
- `data.is_valid`
- `status` 不应为 `expired`、`quota_exhausted`、`disabled`

其他字段：

- 单位：`unit`、`currency`、`data.unit`、`data.currency`，默认 `USD`。
- 套餐：`planName`、`plan_name`、`mode`、`data.planName`、`data.plan_name`、`data.group`、`data.mode`。
- 总额：`total`、`quota.limit`、`data.total`、`data.quota.limit`。
- 已用：`used`、`used_quota`、`quota.used`、`data.used`、`data.used_quota`、`data.quota.used`。

### new_api 模板

请求：

```http
GET <base>/api/user/self
Authorization: Bearer <balanceQueryAccessToken 或 provider key>
New-Api-User: <balanceQueryUserId，可选>
```

base URL 规则：

- 优先使用 `balanceQueryBaseUrl`。
- 未配置时使用聚合 API `url`。
- 如果未配置 `balanceQueryBaseUrl` 且 `url` 以 `/v1` 结尾，会自动去掉 `/v1`。

字段换算：

- `data.quota / 500000` => remaining USD
- `data.used_quota / 500000` => used USD
- `remaining + used` => total USD
- `data.group` 或 `data.plan` => plan

### custom 模板

配置示例：

```json
{
  "method": "GET",
  "path": "/api/user/self",
  "auth": "balance_bearer",
  "remainingPath": "data.quota",
  "unit": "USD",
  "multiplier": 0.000002,
  "totalPath": "data.total",
  "usedPath": "data.used_quota",
  "planPath": "data.group",
  "validPath": "success",
  "invalidMessagePath": "message"
}
```

字段说明：

| 字段 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `method` | 否 | `GET` | 只支持 `GET`、`POST`。 |
| `path` | 是 | 无 | 相对路径，不能是完整 URL。 |
| `auth` | 否 | `provider_bearer` | `provider_bearer`、`balance_bearer`、`none`。 |
| `remainingPath` | 是 | 无 | 剩余额度 JSON 路径。 |
| `unit` | 否 | `USD` | 展示单位，最长 16 字符。 |
| `multiplier` | 否 | `1` | 数值换算倍率，必须大于 0。 |
| `totalPath` | 否 | 无 | 总额度 JSON 路径。 |
| `usedPath` | 否 | 无 | 已用额度 JSON 路径。 |
| `planPath` | 否 | 无 | 套餐名 JSON 路径。 |
| `validPath` | 否 | 无 | 可用状态 JSON 路径。 |
| `invalidMessagePath` | 否 | 无 | 不可用原因 JSON 路径。 |

`auth` 说明：

| auth | 行为 |
| --- | --- |
| `provider_bearer` | 使用聚合 API 主 key 作为 Bearer。 |
| `balance_bearer` | 优先使用 `balanceQueryAccessToken`，没有则回退到主 key。 |
| `none` | 不注入 Authorization。 |

JSON 路径使用点号，例如：

- `data.remaining`
- `data.items.0.balance`

## 常见配置示例

### OpenAI-compatible 供应商

```json
{
  "providerType": "codex",
  "supplierName": "OpenAI Compatible",
  "sort": 10,
  "url": "https://api.example.com",
  "authType": "apikey",
  "key": "sk-xxx",
  "status": "active"
}
```

客户端请求：

```text
POST /v1/chat/completions
```

最终上游：

```text
POST https://api.example.com/v1/chat/completions
Authorization: Bearer sk-xxx
```

### New API 供应商

```json
{
  "providerType": "codex",
  "supplierName": "New API",
  "url": "https://newapi.example.com",
  "authType": "apikey",
  "key": "sk-xxx",
  "balanceQueryEnabled": true,
  "balanceQueryTemplate": "new_api",
  "balanceQueryAccessToken": "admin-token-or-user-token"
}
```

余额查询会访问：

```text
GET https://newapi.example.com/api/user/self
```

### Anthropic-compatible 供应商

```json
{
  "providerType": "claude",
  "supplierName": "Claude Compatible",
  "url": "https://api.anthropic.com/v1",
  "authType": "apikey",
  "key": "sk-ant-xxx",
  "authCustomEnabled": true,
  "authParams": {
    "location": "header",
    "name": "x-api-key",
    "headerValueFormat": "raw"
  }
}
```

客户端请求：

```text
POST /v1/messages
```

最终上游：

```text
POST https://api.anthropic.com/v1/messages
x-api-key: sk-ant-xxx
```

### 带路径前缀的 Claude 供应商

```json
{
  "providerType": "claude",
  "supplierName": "Claude Proxy",
  "url": "https://open.bigmodel.cn/api/anthropic",
  "authType": "apikey",
  "key": "xxx",
  "actionCustomEnabled": true,
  "action": "/v1/messages"
}
```

最终上游：

```text
https://open.bigmodel.cn/api/anthropic/v1/messages
```

### Gemini 供应商

```json
{
  "providerType": "gemini",
  "supplierName": "Gemini",
  "url": "https://generativelanguage.googleapis.com",
  "authType": "apikey",
  "key": "AIza..."
}
```

Gemini 没有通用模型列表发现能力，需要在模型目录里手动添加/关联 source model。

## 模型目录与供应商模型

聚合 API 的模型管理分两层：

1. 供应商模型模板：`aggregate_api_supplier_models`
2. 实际模型来源映射：`model_source_models` / 模型来源映射

模型发现规则：

- `modelOverride` 已配置：直接返回该模型。
- `codex`：请求上游 `/models` 发现模型。
- `claude`：使用 Claude 供应商探测逻辑发现模型。
- `gemini`：不提供通用发现，需手动添加 source model。

请求时的模型过滤规则：

- 如果平台模型没有任何启用的来源映射，会返回 `model_unavailable: <model>`。
- 如果某个平台模型映射到了聚合 API，候选源只保留对应聚合 API。
- 映射里的上游模型会覆盖请求体里的 `model`。

## 管理接口

桌面端通过 Tauri command 调 service RPC：

| 功能 | Tauri command | RPC method |
| --- | --- | --- |
| 列表 | `service_aggregate_api_list` | `aggregateApi/list` |
| 创建 | `service_aggregate_api_create` | `aggregateApi/create` |
| 更新 | `service_aggregate_api_update` | `aggregateApi/update` |
| 读取 secret | `service_aggregate_api_read_secret` | `aggregateApi/readSecret` |
| 删除 | `service_aggregate_api_delete` | `aggregateApi/delete` |
| 测试连接 | `service_aggregate_api_test_connection` | `aggregateApi/testConnection` |
| 刷新余额 | `service_aggregate_api_refresh_balance` | `aggregateApi/refreshBalance` |
| 供应商模型列表/保存/删除/导入 | `service_aggregate_api_supplier_model_*` | `aggregateApi/supplierModel*` |

前端 API 封装在：

```text
apps/src/lib/api/account-client.ts
```

桌面端调用必须走 `invoke` / `invokeFirst` 和 `withAddr()`，不要直接 `fetch()` service。

## 排障清单

### 请求没有走到聚合 API

检查：

1. 平台 Key 的 `rotationStrategy` 是否是 `aggregate_api_rotation` 或 `hybrid_rotation`。
2. 请求路径是否被识别成预期协议。
3. 聚合 API 的 `providerType` 是否和协议匹配。
4. 聚合 API 的 `status` 是否为 `active`。
5. 平台 Key 是否绑定了错误的 `aggregateApiId`。

### 只有某个模型不可用

检查：

1. 平台模型是否存在于模型目录。
2. 平台 Key 是否允许使用该模型。
3. 是否存在启用的模型来源映射。
4. 映射来源是否是 `aggregate_api`。
5. 映射的 source id 是否对应当前聚合 API。
6. 上游模型名是否填写为供应商真实模型名。

典型错误：

```text
model_unavailable: gpt-5.5
```

### 上游鉴权失败

检查：

1. `authType` 是否正确。
2. 默认 Bearer 是否符合供应商要求。
3. 如果供应商要 `x-api-key` 或 `api-key`，是否启用了 `authCustomEnabled`。
4. `headerValueFormat` 是否应该是 `raw`。
5. userpass 模式是否同时配置了 username 和 password。
6. 自定义 header 名是否合法。

### 上游路径不对

检查：

1. `url` 是否已经包含 `/v1`。
2. 是否误把完整 URL 写进 `action`。
3. 是否启用了 action 导致原始路径被忽略。
4. 供应商是否需要 base URL 路径前缀，例如 `/api/anthropic`。

### 余额刷新失败

检查：

1. `balanceQueryEnabled` 是否为 true。
2. 模板是否选对：`generic`、`new_api`、`custom`。
3. `balanceQueryBaseUrl` 是否需要单独配置。
4. New API 是否需要单独 `balanceQueryAccessToken`。
5. custom 模板的 `remainingPath` 是否能取到数字。
6. custom 模板的 `multiplier` 是否按供应商单位换算。

### 请求日志显示 502，但上游实际是 403/429

聚合 API 对非 2xx 上游响应会归一为网关失败，最终可能以 502 返回给客户端；上游原始状态和错误体摘要会记录在请求日志错误字段中。排查时以请求日志 tooltip/详情中的 upstream 摘要为准。

## 推荐配置规范

1. 每个供应商都填写清晰的 `supplierName`，例如 `New API - 主线路`。
2. 用 `sort` 表达固定优先级，主线路填小值，备用线路填大值。
3. 默认不要启用 action；只有供应商路径和客户端路径不一致时再启用。
4. 默认不要配置 `modelOverride`；需要固定模型时优先用模型映射。
5. New API 优先用 `balanceQueryTemplate = new_api`。
6. Claude-compatible 供应商优先显式配置 `x-api-key` raw header。
7. Gemini 供应商要手动维护 source model，不要依赖远端刷新。
8. 多供应商生产环境建议使用 `CODEXMANAGER_ROUTE_STRATEGY=balanced`，但首选供应商仍可通过平台 Key 的 `aggregateApiId` 固定在第一位。
