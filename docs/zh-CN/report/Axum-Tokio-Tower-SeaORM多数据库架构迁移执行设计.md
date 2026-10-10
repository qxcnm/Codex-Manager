# Axum + Tokio + Tower + SeaORM 多数据库架构迁移执行设计

本文档是给下一位执行者的实施设计。目标是把 CodexManager 的 HTTP 入口逐步统一到 Axum + Tokio + Tower，并为 Service 模式增加 SeaORM 驱动的 MySQL / PostgreSQL 存储能力，同时保留桌面模式的 SQLite 和离线可用性。

本文档只描述实施方案、边界和验收条件，不代表迁移已经完成。执行者必须以当前工作区代码、测试结果和实际数据库读写结果为准，不得把本文档中的“计划”当成“已实现”。

## 1. 最终判断

不做一次性重写，采用“双运行配置 + 渐进替换”的路线：

```text
共享领域逻辑、权限模型、网关协议适配、请求日志语义
                  │
       ┌──────────┴──────────┐
       │                     │
桌面模式 / 本地模式       Service / 服务器模式
Axum + Tokio + Tower      Axum + Tokio + Tower
SQLite 现有存储           SeaORM + MySQL/PostgreSQL
无外部基础设施             Redis 不属于本次范围，可后续按需增加
```

必须保持的原则：

1. Axum、Tokio、Tower 是 HTTP 运行时和服务边界的统一方向。
2. SeaORM 只解决服务端多数据库存储，不强迫桌面端改成远程数据库。
3. SQLite 仍是桌面模式的权威数据源，不能因服务器方案而被删除。
4. MySQL 和 PostgreSQL 是 Service 模式的可选后端，二者都必须经过真实数据库测试。
5. 迁移期间优先保持现有 API、RPC、鉴权、网关协议和计费语义不变。
6. 不把 Redis、消息队列或其他外部服务作为本次迁移的隐含前置条件。

## 2. 当前基线

当前仓库不是纯同步架构，也不是纯异步架构，而是混合实现：

- `crates/web` 已使用 Axum、Tokio 和 `tower-http`，负责 Web UI、静态资源和到 Service 的代理。
- `crates/service` 已依赖 Axum/Tokio，但核心后端入口仍由 `tiny_http` 提供。
- `crates/service/src/http/backend_runtime.rs` 使用操作系统线程、`crossbeam-channel` 有界队列和独立的流式请求队列。
- 网关和账号管理中仍有 `reqwest::blocking::Client`、`std::thread` 以及 `spawn_blocking`。
- `crates/core` 通过 `crates/rusqlite` 使用 SQLite；当前存储暴露同步接口。
- `crates/core/migrations/` 已存在大量 SQLite 迁移，且存储代码包含手写 SQL、事务、索引、PRAGMA、备份和查询计划验证。
- 数据库保存账号、Token、API Key、请求日志、token 统计、模型目录、设置和插件任务等持久状态。
- 桌面模式要求本地、离线、单文件或低依赖运行；Service 模式才适合依赖远程数据库。

因此，所谓“上 Axum/Tokio/Tower”主要是替换 Service 的 HTTP 运行边界；所谓“上 SeaORM”主要是新增可切换的服务器存储实现。两者不能混成一个大重写任务。

## 3. 本次范围与非范围

### 3.1 本次范围

- 统一 Service HTTP 入口到 Axum。
- 用 Tokio 管理 HTTP、长连接、后台任务和优雅关闭。
- 用 Tower 统一超时、限流、背压、追踪、panic 隔离和请求体限制。
- 抽象存储接口，使业务层不直接依赖某一种数据库实现。
- 新增 SeaORM 存储适配器，支持 MySQL 和 PostgreSQL。
- 保留现有 SQLite 适配器，并确保桌面端继续使用它。
- 增加数据库后端选择、连接池、迁移、导入导出和健康检查。
- 对 OpenAI 风格网关、RPC、SSE、WebSocket、tools 和鉴权做兼容验证。

### 3.2 明确不在本次范围

- 不迁移 Tauri 窗口、托盘、更新器和桌面 UI 到服务器架构。
- 不删除或重写现有 SQLite 迁移历史。
- 不把 Redis 加入必需依赖。
- 不改变公开 API 路径、RPC 方法名、错误码和鉴权规则，除非兼容测试证明现状有缺陷。
- 不把所有手写 SQL 强行改成 SeaORM 查询。
- 不进行无验证的双写、实时同步或在线数据库切换。
- 不以“编译通过”代替真实 MySQL/PostgreSQL 读写和网关验收。

## 4. 目标模块边界

建议的目录方向如下，执行时可以按现有模块命名做小幅调整，但不能把所有逻辑重新塞进 `crates/service/src/lib.rs`：

```text
crates/
├─ core/
│  ├─ src/domain/                 # 与数据库无关的领域类型和规则
│  ├─ src/storage/
│  │  ├─ sqlite/                  # 保留现有 SQLite 实现
│  │  ├─ traits.rs                # 业务需要的存储接口
│  │  └─ records.rs               # 跨后端稳定的数据记录类型
│  └─ migrations/                 # 现有 SQLite 迁移，继续维护
├─ service/
│  ├─ src/http/
│  │  ├─ router.rs                # Axum 路由装配
│  │  ├─ state.rs                 # AppState、配置和依赖注入
│  │  ├─ middleware/              # Tower 层和鉴权提取器
│  │  └─ adapters/                # 旧 tiny_http 处理器的过渡适配
│  ├─ src/gateway/                # 网关和协议适配，保持领域边界
│  └─ src/runtime/                # Tokio 任务、关闭、信号和后台调度
├─ storage-seaorm/                # 可选独立 crate，或 service 内独立模块
│  ├─ src/entities/               # SeaORM 实体
│  ├─ src/repositories/           # Repository 实现
│  └─ src/migration/              # MySQL/PostgreSQL 迁移
└─ web/
   └─ src/                        # 保持 Web 壳和代理职责
```

推荐先建立 `StorageBackend` 或等价的 trait，再迁移具体业务。业务模块只依赖 trait 和领域记录类型，不直接接触 `DatabaseConnection`、`rusqlite::Connection` 或 SQLx pool。

## 5. Axum 迁移设计

### 5.1 路由统一

Service 的 Axum Router 至少需要覆盖现有入口：

```text
GET/POST /health、/metrics
POST     /rpc
GET/POST /v1/*
GET      /auth/callback
GET      /account-test-events 或现有事件路径
GET      /__shutdown（仅本地受控关闭，不暴露到公网）
```

真实路径必须从当前路由和测试中读取，不能照抄本文档示例。要保留：

- `/v1/responses`
- `/v1/chat/completions`
- 流式 SSE 和非流式 JSON
- WebSocket 升级与双向转发
- `tool_calls`、tools、图片和特殊请求头
- RPC JSON-RPC 错误结构
- 原有 loopback 绑定和 Web 代理行为

### 5.2 迁移方式

采用三步适配，不直接改写全部 handler：

1. **入口适配**：用 Axum 接收 `Request`，提取 method、URI、headers、body 和 actor 信息。
2. **兼容桥**：把数据转换成现有业务层可接受的上下文；旧的 `tiny_http::Request` 处理器先通过独立适配器运行。
3. **领域迁移**：按路由逐个把 handler 改成 Axum 的 extractor 和 response，最后删除对应 tiny_http 依赖。

兼容桥期间不得把一个请求同时交给新旧处理器，避免重复写日志、重复计费或重复转发。

### 5.3 状态设计

使用 `Arc<AppState>` 注入：

- 运行配置快照和动态配置句柄；
- `Storage` trait 对象或枚举后端；
- 异步上游 HTTP client；
- 网关并发控制和关闭信号；
- 请求日志、usage 和后台任务发送端；
- Web/RPC 鉴权所需的只读配置。

禁止在 handler 中重新创建数据库连接池、HTTP client、Tokio runtime 或线程池。

### 5.4 响应和流式语义

- 普通 JSON 使用 `axum::Json` 或明确的 `Response`。
- SSE 使用 `Body::from_stream` 或等价流式 body，保持 `text/event-stream`、缓存控制和连接关闭语义。
- WebSocket 使用 Axum extractor，保留上游 header、压缩配置、关闭码和超时行为。
- 长流请求不能套用很短的整体 `TimeoutLayer`；需要区分连接建立超时、首字节超时、流式空闲超时和总请求超时。
- 请求日志和 usage 记录必须在流成功、上游错误、客户端断开、取消和超时路径都能落库。

## 6. Tokio 运行时设计

### 6.1 运行时原则

- 每个进程只维护一个主 Tokio runtime。
- `start_server`、Web 启动器和测试入口不得互相嵌套 runtime。
- 后台任务使用明确的 `JoinHandle`、取消信号和关闭等待。
- 不在 async handler 中执行长时间同步 I/O、文件扫描、SQLite 查询或阻塞加密操作。

### 6.2 阻塞代码隔离

迁移初期仍有同步代码时：

- 使用 `spawn_blocking` 包住同步数据库和文件操作。
- 用 `Semaphore` 限制阻塞任务数量，不能无限制提交。
- 记录阻塞任务等待时间、执行时间和失败原因。
- 不要把 SSE/WebSocket 主循环放入 `spawn_blocking`，除非整个实现仍是同步协议。
- 现有 `crossbeam` 队列可以在过渡期保留，但新异步路径优先使用 Tokio channel。

### 6.3 上游 HTTP

网关主链路应逐步从 `reqwest::blocking::Client` 改为复用的异步 `reqwest::Client`：

- 每个代理配置使用可复用 client，而不是每次请求构建。
- 连接池、DNS、代理、TLS、重试和请求超时配置要与现有行为逐项对比。
- 重试必须区分可重试的 HTTP 状态、网络失败、流中断和 `pending_unknown` 等现有语义。
- 不得因为“全异步”而改变账号选择、会话锚点、prompt cache key 或故障转移顺序。

## 7. Tower 层设计

建议按路由组装层，而不是给所有请求套同一套限制：

```text
Trace / request-id
    ↓
CatchPanic
    ↓
BodyLimit
    ↓
路由级鉴权和来源检查
    ↓
ConcurrencyLimit / LoadShed
    ↓
Timeout（普通请求）或流式专用超时
    ↓
业务 handler
```

执行时应确认具体 layer 的顺序和错误响应类型。至少需要覆盖：

- body size 超限返回稳定的 413；
- 队列或并发耗尽返回稳定的 503；
- handler panic 转为内部错误并记录 request id；
- 未授权 RPC 和网关请求不能被并发层绕过；
- `/health` 和 `/metrics` 在业务拥塞时仍能快速响应；
- 流式响应不会因普通请求超时层被提前截断；
- trace 不记录 Token、Authorization、Cookie、请求体和上游密钥。

当前项目有普通请求队列和流式请求队列。迁移时需要保留这两个队列的独立容量、指标、超时和降级规则，不能简单地用一个全局 `ConcurrencyLimitLayer` 替代后宣称行为等价。

## 8. SeaORM 多数据库设计

### 8.1 数据库后端选择

建议配置抽象如下，具体环境变量名称在实现前固定并写入运行配置文档：

```text
CODEXMANAGER_STORAGE_BACKEND=sqlite|mysql|postgres
CODEXMANAGER_DATABASE_URL=<backend-specific URL>
CODEXMANAGER_DB_MAX_CONNECTIONS=<positive integer>
CODEXMANAGER_DB_ACQUIRE_TIMEOUT_MS=<positive integer>
```

规则：

- 未指定后端时，桌面模式默认 SQLite。
- Service 模式可明确选择 SQLite、MySQL 或 PostgreSQL。
- `DATABASE_URL`、密码和连接参数不能写入普通日志。
- 后端不匹配、URL 缺失、迁移失败或连接失败必须在启动阶段明确报错。
- 健康检查要区分“进程存活”和“数据库可用”。

### 8.2 SeaORM feature 建议

SeaORM 依赖应按 feature 拆分，避免桌面二进制无条件带入 MySQL/PostgreSQL 驱动：

```toml
[features]
default = ["sqlite"]
sqlite = ["sea-orm/sqlx-sqlite"]
mysql = ["sea-orm/sqlx-mysql"]
postgres = ["sea-orm/sqlx-postgres"]
```

实际 feature 名称和版本以锁定的 SeaORM 版本文档及编译结果为准。运行时 tokio、TLS 和宏 feature 要集中在一个依赖声明中管理，避免各 crate 版本漂移。

### 8.3 实体和 Repository

SeaORM 只负责持久化映射。建议分层：

```text
HTTP/RPC handler
      ↓
Application service / domain service
      ↓
Storage trait
      ├─ SqliteStorage（现有实现）
      └─ SeaOrmStorage
          ├─ MySQL connection
          └─ PostgreSQL connection
```

不要把 SeaORM Entity 直接暴露给 HTTP 响应。跨数据库稳定的记录类型应由 core 定义，再由 Repository 映射。

对于查询复杂、依赖窗口函数、聚合、账本和性能调优的路径，可以保留经过参数化的原生 SQL；SeaORM 不要求每条 SQL 都改成实体链式查询。

### 8.4 类型兼容规则

必须提前建立类型映射表并写测试：

| 领域含义 | SQLite | MySQL | PostgreSQL | 约束 |
|---|---|---|---|---|
| 主键/外部 ID | TEXT | VARCHAR/TEXT | TEXT/UUID | API 返回值保持字符串兼容 |
| 时间 | INTEGER 秒或毫秒 | DATETIME/TIMESTAMP | TIMESTAMPTZ | 统一 UTC，禁止本地时区混入 |
| 布尔 | INTEGER 0/1 | TINYINT/BOOLEAN | BOOLEAN | Repository 统一转换 |
| JSON 扩展 | TEXT | JSON/TEXT | JSONB/TEXT | 读取输出结构不能变化 |
| 金额/Token 计数 | INTEGER | BIGINT/DECIMAL | BIGINT/NUMERIC | 计费计算禁止浮点 |
| 可选值 | NULL | NULL | NULL | 空字符串与 NULL 语义不能混淆 |

账号 Token、API Key、代理密码等敏感字段仍由应用层加密；更换 ORM 不能降低现有加密和脱敏等级。

### 8.5 迁移策略

不要把现有 SQLite 迁移文件直接“翻译”为 SeaORM migration 后覆盖原目录。采用三套明确边界：

1. `core/migrations`：继续服务 SQLite，保持现有版本和升级路径。
2. `storage-seaorm/migration`：为 MySQL/PostgreSQL 建立新的、后端兼容的迁移序列。
3. 导入工具：把 SQLite 数据导出成版本化中间格式，再导入服务器数据库。

导入工具必须处理：

- 外部 ID 保留；
- 时间和时区转换；
- 加密字段原样保留；
- 唯一键和外键顺序；
- 空值和 JSON；
- 大表分批导入；
- 失败可重试且不重复计费；
- 导入前后数量、哈希或关键汇总可比对。

默认不做生产双写。若未来必须双写，需先定义主库、失败补偿、幂等键、顺序保证和回滚方式，并单独立项。

## 9. 业务迁移顺序

建议按以下顺序实施，每阶段都保持可编译、可测试、可回退：

### 阶段 0：基线冻结

- 记录当前分支、工作区修改和现有测试结果。
- 固定公开路由、RPC 方法、错误结构、数据库 schema 版本和配置默认值。
- 补充 `/v1/responses`、`/v1/chat/completions`、SSE、WebSocket、tools 的基线测试。

### 阶段 1：抽象存储接口

- 定义最小的领域存储 trait。
- 先让现有 SQLite 实现通过 trait 工作。
- 不改变 SQL、迁移、事务和返回结构。
- 按账号、API Key、请求日志、usage、模型目录、设置、插件任务拆分接口。

### 阶段 2：建立 Axum Service Router

- 新增 Axum Router、AppState 和 graceful shutdown。
- 用适配器承接旧 handler。
- 先迁移 `/health`、`/metrics`、`/rpc`，再迁移网关。
- 保留 tiny_http 后端作为临时 fallback，但不再新增 tiny_http 业务逻辑。

### 阶段 3：接入 Tower

- 加入 trace、request id、panic 隔离、body limit、路由级超时和并发控制。
- 重现当前普通/流式队列行为。
- 记录 413、429/503、超时、取消和客户端断开指标。

### 阶段 4：异步化高价值路径

- 先改网关上游 HTTP 和 WebSocket。
- 再改 usage 刷新、账号健康检查和后台调度。
- SQLite 和文件操作先通过受控 `spawn_blocking` 隔离。
- 对每次转换比较吞吐、P95/P99、内存、连接数和失败率。

### 阶段 5：SeaORM MySQL/PostgreSQL

- 创建独立后端 crate 或严格隔离的模块。
- 先实现 settings、accounts、API keys 等简单 CRUD。
- 再实现 request logs、usage、model catalog、billing 等复杂领域。
- 每实现一个领域，必须补 MySQL 和 PostgreSQL 真数据库测试。

### 阶段 6：导入导出和切换

- 实现 SQLite → 中间格式 → MySQL/PostgreSQL 导入。
- 提供 dry-run、统计、失败记录和重复执行保护。
- 先在测试数据库验证，再考虑 Service 用户使用。
- 切换后保留 SQLite 原文件，直到读写和回滚验证结束。

### 阶段 7：清理过渡代码

- 删除已迁移路由的 tiny_http 适配。
- 删除无调用方的 crossbeam HTTP 队列代码。
- 删除重复的同步 client 和嵌套 runtime。
- 更新架构、运行配置、部署和故障排查文档。

## 10. 验收矩阵

### 10.1 HTTP 和协议

- `/v1/responses` 非流式 JSON 与基线一致。
- `/v1/responses` 流式 SSE 事件顺序、headers、结束事件和错误一致。
- `/v1/chat/completions` 非流式和流式一致。
- tools、`tool_calls`、图片请求和特殊 Codex headers 保持兼容。
- WebSocket 建连、转发、关闭码、超时和上游错误可验证。
- RPC JSON-RPC 成功、业务错误、鉴权失败、无效 JSON 和 panic 均有测试。

### 10.2 并发和可靠性

- 普通请求和流式请求有独立并发/背压验证。
- 队列满时返回稳定错误，不无限增长内存。
- 上游慢、断开、超时、重试和客户端取消都有结果。
- 优雅关闭等待活动请求和后台任务，不死锁、不丢关键日志。
- `/health`、`/metrics` 在业务拥塞时仍可访问。

### 10.3 数据库

- SQLite 现有迁移和桌面启动测试全部通过。
- MySQL 新库可从零迁移并完成关键 CRUD、事务和约束测试。
- PostgreSQL 新库可从零迁移并完成同一套测试。
- 关键数据类型、NULL、时间、JSON、金额和 Token 计数跨后端一致。
- SQLite 导入 MySQL/PostgreSQL 后，账号数、Key 数、日志数、usage 汇总和设置值可比对。
- 数据库连接失败、迁移失败和权限不足能在启动/健康检查中明确区分。

### 10.4 桌面和 Web

- 桌面模式不安装 MySQL/PostgreSQL 也能正常启动。
- 桌面模式默认仍使用 SQLite，Tauri RPC 与本地 service 正常。
- Web Service 模式可连接选定的数据库后端。
- Web UI 的登录、session、RPC 代理和网关代理行为不变。

## 11. 推荐测试命令

以下命令是执行阶段的最低基线，具体可根据已完成阶段缩小范围，但必须记录未执行原因：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo test -p codexmanager-service
cargo test -p codexmanager-web
```

服务端数据库测试建议使用独立容器或 CI service：

```text
MySQL：从零数据库 + 迁移 + Repository 集成测试
PostgreSQL：从零数据库 + 迁移 + Repository 集成测试
SQLite：现有迁移、桌面启动和备份恢复测试
```

网关改动至少要有真实 HTTP 请求，而不是只调用函数：

- 真实监听端口；
- 真实请求 headers/body；
- 真实 SSE/WebSocket；
- 真实数据库写入和读回；
- 真实错误状态和关闭行为。

Mock upstream 只能证明协议转换逻辑，不能证明真实 provider、代理、TLS 或生产认证已经可用。

## 12. 回滚和失败处理

- 每个阶段使用独立提交，避免 Axum、存储和数据库迁移混成不可回退的大提交。
- Axum 入口迁移期间保留显式 fallback 开关，但默认路径必须经过新 Router 测试。
- 新数据库后端失败时，Service 应明确拒绝启动或回到用户选择的后端，不得静默写入错误数据库。
- 导入失败不得删除原 SQLite 文件，不得覆盖未确认的服务器数据。
- 迁移脚本必须支持重复执行检查；禁止通过删表重建解决版本冲突。
- 任何兼容性回归都优先停止阶段推进，先恢复上一个可验证版本。

## 13. 下一位执行者的任务拆分

建议拆成以下可独立审查的任务：

1. `storage-traits`：定义存储接口和跨后端记录类型，只接入现有 SQLite。
2. `axum-router`：建立 Service Router、AppState、关闭和入口适配。
3. `tower-middleware`：增加请求追踪、限制、超时、panic 和背压测试。
4. `async-gateway`：将上游 HTTP、SSE 和 WebSocket 主链路改为 Tokio 友好的异步实现。
5. `seaorm-schema`：建立 MySQL/PostgreSQL 实体、迁移和连接配置。
6. `seaorm-repositories`：按领域逐个实现 Repository，并补两种真实数据库测试。
7. `sqlite-import`：完成 SQLite 到中间格式再到服务器数据库的导入工具。
8. `compatibility-closeout`：跑协议、桌面、Web、数据库和性能验收，更新文档。

每个任务的 PR 或提交说明必须写明：改变的边界、保留的旧路径、执行的测试、没有覆盖的风险，以及是否影响桌面模式。

## 14. 不得采用的实现方式

- 一次性删除 SQLite 和 130 多个现有迁移。
- 只添加依赖，不改实际 HTTP 入口，却宣称已经迁移到 Axum。
- 在 async handler 中直接调用阻塞数据库、阻塞 HTTP 或无限等待的同步函数。
- 用一个全局并发限制替换普通/流式双队列而不做行为验证。
- 把 SeaORM Entity 直接作为 API 响应模型。
- 通过数据库双写掩盖未解决的一致性问题。
- 把 MySQL/PostgreSQL 连接密码、Token 或请求体写进日志和测试输出。
- 只做编译和单元测试，不做真实数据库和真实监听端口验证。
- 因为服务器端需要 MySQL/PG，就让桌面端强制依赖外部数据库。

## 15. 完成定义

只有同时满足以下条件，才能把本次架构迁移标为完成：

1. Service HTTP 入口已由 Axum 统一承载，旧 tiny_http 入口不再承载生产请求。
2. Tokio 任务、关闭、流式请求和阻塞隔离有明确实现和测试。
3. Tower 层已覆盖鉴权、超时、限流、背压、panic 和 body limit，且错误语义稳定。
4. SQLite 桌面模式完整保留并通过现有回归测试。
5. MySQL 和 PostgreSQL 均可从零迁移、启动、读写和完成关键事务。
6. 至少有一条经过验证的 SQLite 导入服务器数据库路径。
7. `/v1/responses`、`/v1/chat/completions`、SSE、WebSocket、tools、RPC、鉴权和日志回归通过。
8. 架构、运行配置、部署、备份恢复和排障文档已同步。
9. 所有未覆盖项目都明确标为 HANDOFF，而不是用“理论兼容”代替证据。
