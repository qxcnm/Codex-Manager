# ARCHITECTURE

本文档说明 CodexManager 当前仓库结构、运行关系和发布链路，目标是帮助协作者快速判断改动应该落在哪一层。

## 1. 总体形态

CodexManager 由两类运行模式组成：

1. 桌面模式：Tauri 桌面端 + 本地 service 进程
2. Service 模式：独立 service + web UI，可用于服务器、Docker 或无桌面环境

统一目标：

- 管理账号、用量、平台 Key
- 提供本地网关能力
- 对外兼容 OpenAI 风格入口，并适配多种上游协议

## 2. 目录结构与职责

```text
.
├─ apps/                  # 前端与 Tauri 桌面端
│  ├─ src/                # Next.js App Router + TypeScript 前端
│  ├─ src-tauri/          # Tauri 桌面壳与原生命令桥接
│  ├─ tests/              # 前端 UI/结构测试
│  └─ out/                # Next.js 静态导出产物
├─ crates/
│  ├─ core/               # 数据库迁移、存储基础、认证/用量底层能力
│  ├─ service/            # 本地 HTTP/RPC 服务、网关、协议适配、设置持久化
│  ├─ storage-seaorm/     # Service 可选 SeaORM 适配层（SQLite/MySQL/PostgreSQL）
│  ├─ web/                # Web UI 服务壳，可嵌入前端静态资源
│  └─ start/              # Service 一键启动器（拉起 service + web）
├─ scripts/               # 本地构建、统一版本、测试探针、发布辅助脚本
├─ docker/                # Dockerfile 与 compose 配置
├─ assets/                # README 图片、Logo 等静态资源
└─ .github/workflows/     # CI / release workflow
```

## 3. 核心复杂域入口索引

### 3.1 前端总控入口

- `apps/src/app/layout.tsx`：App Router 根布局与全局 Provider 装配
- `apps/src/components/providers.tsx`：React Query、next-themes、i18n、Tooltip 与 Toaster 装配
- `apps/src/components/layout/app-bootstrap.tsx`：桌面/Web 运行时初始化与 service 连接编排
- `apps/src/components/layout/page-keep-alive-viewport.tsx`：顶层页面懒加载与 keep-alive 缓存
- `apps/src/lib/api/transport.ts`：Tauri invoke 与 Web RPC fallback 的统一传输层
- `apps/src/lib/app-shell/top-level-routes.ts`：顶层路由、角色可见性与导航分组配置

### 3.2 桌面端壳层入口

- `apps/src-tauri/src/lib.rs`：Tauri 应用装配入口
- `apps/src-tauri/src/commands/registry.rs`：Tauri command 注册入口
- `apps/src-tauri/src/service_runtime.rs`：桌面内嵌 service 生命周期
- `apps/src-tauri/src/rpc_client/`：桌面端 RPC 调用基础设施

### 3.3 service 网关与协议入口

- `crates/service/src/lib.rs`：service 总入口与运行时装配
- `crates/service/src/http/`：HTTP 路由入口
- `crates/service/src/rpc_dispatch/`：RPC 分发入口
- `crates/service/src/gateway/mod.rs`：网关聚合入口
- `crates/service/src/gateway/observability/http_bridge/mod.rs`：请求追踪、协议桥接、日志写入
- `crates/service/src/gateway/protocol_adapter/request_router.rs`：OpenAI/Codex 输入映射
- `crates/service/src/gateway/observability/http_bridge/response_helpers.rs`：非流式结果总转换入口
- `crates/service/src/gateway/observability/http_bridge/stream_readers/`：流式 SSE 转换入口
- `crates/service/src/gateway/observability/http_bridge/stream_readers/chat_completions.rs`：OpenAI Chat 结果适配
- `crates/service/src/gateway/observability/http_bridge/stream_readers/common.rs`：工具名缩短与还原

### 3.4 设置与运行配置入口

- `crates/service/src/app_settings/`：设置持久化、环境变量覆盖、运行时同步
- `crates/service/src/auth/web_access.rs`：Web 访问密码与会话令牌

## 4. 运行关系

### 4.1 桌面模式

桌面模式由以下部分组成：

- `apps/src/`：前端 UI
- `apps/src-tauri/`：桌面壳
- `crates/service/`：本地 service

运行方式：

1. 用户启动桌面应用。
2. Tauri 壳负责窗口、托盘、更新、单实例、设置桥接等桌面行为。
3. 桌面端通过 RPC 或本地地址与 `codexmanager-service` 通信。
4. 前端 UI 展示账号、用量、请求日志、设置等页面。

### 4.2 Service 模式

Service 模式由以下二进制组成：

- `codexmanager-service`
- `codexmanager-web`
- `codexmanager-start`

职责：

- `codexmanager-service`：核心服务进程，提供账号管理、网关转发、请求日志、设置持久化、RPC/HTTP 接口。
- `codexmanager-web`：Web UI 服务壳，可直接提供前端页面，并代理到本地 service。
- `codexmanager-start`：面向发布包的一键启动器，负责同时拉起 service 和 web。

## 5. 模块职责

### 5.1 `apps/src/`

主要负责：

- 页面渲染
- 用户交互
- 状态管理
- 调用本地 API / Tauri command
- 设置页与账号页的前端逻辑

### 5.2 `apps/src-tauri/`

主要负责：

- Tauri 应用启动
- 单实例控制
- 系统托盘与窗口事件
- 桌面更新与安装器行为
- 将前端操作桥接到 service / 本地运行时

### 5.3 `crates/core/`

主要负责：

- SQLite 迁移
- 存储底层能力
- 认证 / usage 等核心基础逻辑
- 可被 service 复用的数据访问能力

### 5.4 `crates/service/`

主要负责：

- HTTP / RPC 入口
- 账号、用量、API Key 管理
- 本地网关能力
- 协议适配与上游转发
- 请求日志与设置持久化
- 运行时配置同步

重点子目录：

- `src/gateway/`：网关、协议适配、流式与非流式转换
- `src/http/`：HTTP 路由入口
- `src/rpc_dispatch/`：RPC 分发
- `src/account/`、`src/apikey/`、`src/requestlog/`、`src/usage/`：领域逻辑

### 5.5 `crates/web/`

主要负责：

- 提供 Web UI 静态资源
- 挂载或代理到 service
- 可选把 `apps/out` 内嵌到二进制，形成单文件发布物

### 5.6 `crates/start/`

主要负责：

- 在 Service 发布包里提供一个更直接的启动入口
- 协调 service 与 web 的生命周期

## 6. 数据与配置

### 6.1 数据库

桌面模式继续使用 SQLite；Service 模式可通过独立的 `codexmanager-storage-seaorm`
适配层选择 SQLite、MySQL 或 PostgreSQL。SeaORM 实体和连接不会直接暴露给 HTTP。
显式数据库 URL 选择 SeaORM（含 SQLite）；默认未设置 URL 的 SQLite 模式保留桌面存储流程。
数据库迁移位于：

- `crates/core/migrations/`：现有桌面 SQLite 迁移。
- `crates/storage-seaorm/src/migration.rs`：Service SeaORM 跨数据库迁移。

SeaORM 适配层按 Cargo feature 引入驱动：`sqlite`（默认）、`mysql`、`postgres`。
未选择服务器后端时不会强制安装 MySQL、PostgreSQL 或 Redis。
Service 部署可使用 `CODEXMANAGER_STORAGE_BACKEND`、
`CODEXMANAGER_DATABASE_URL`、`CODEXMANAGER_DB_MAX_CONNECTIONS` 和
`CODEXMANAGER_DB_ACQUIRE_TIMEOUT_MS` 选择后端与连接池参数；密码只存在于环境配置，
不会进入请求日志。

HTTP 前置代理的短请求超时可通过 `CODEXMANAGER_HTTP_TIMEOUT_MS` 配置（毫秒，默认
120000；设置为 `0` 禁用该层；SSE/WebSocket 请求始终使用各自的流式超时策略）。
异步上游并发任务数可通过 `CODEXMANAGER_GATEWAY_ASYNC_STREAM_WORKERS` 配置
（默认 32，最大 256）；许可覆盖响应体生命周期、背压和取消，不会为每条流创建线程。

数据库里不只存账号，也已经承担：

- API Key
- 请求日志
- token 统计
- app settings

SeaORM 迁移和 Repository 已覆盖账号/Token、API Key 及其 secret/quota/rollup、
模型目录与价格/路由、权限 groups、模型组授权、钱包/账本、登录会话、请求日志与
token 统计、用量汇总、代理配置/历史、插件及聚合 API。Service 远端模式通过领域
facade 调用这些 Repository；现有 SQLite 迁移和 Storage 保留用于桌面模式。
远端模式的旧 Storage 参数只承载无 schema 的兼容句柄，不会打开本地业务 SQLite。
后端配置错误在启动时失败，运行中切换数据库需要重启。

`storage-transfer` 可将旧 SQLite 只读导出为带逐表校验和的 JSONL 快照，再事务导入
独立空目标库。导入拒绝有数据的未映射表/列，保留历史 ID，并修复 PostgreSQL 序列。
切换流程与编译 feature 见[环境变量与运行配置](report/环境变量与运行配置说明.md#service-数据库切换与离线导入)。

### 6.2 运行配置

配置主要来源包括：

- 环境变量 `CODEXMANAGER_*`
- 应用运行目录下的 `.env` / `codexmanager.env`
- `app_settings` 持久化表
- 桌面端设置页

当前约定：

- 启动前必须生效的配置保留在环境变量层。
- 运行时可调配置优先通过设置页 + `app_settings` 管理。
- 设置变更不应无边界地散落在桌面端、前端和 service 各处。

## 7. 请求链路概览

Service 的公开 HTTP 监听器、RPC、网关、SSE、WebSocket 和回调由 Axum/Tokio
提供，生产依赖已移除 tiny_http。Axum Router 统一负责请求体上限、并发背压、
panic 隔离、trace 和 `x-request-id`，并通过优雅关闭等待活动请求。
Service 的 HTTP、OAuth、认证、usage、后台任务、SeaORM 和插件网络共用
`runtime/service_runtime.rs::process_runtime`；Web 主入口和桌面 Tauri IPC 也使用该执行器。
listener 退出仍关闭端口并排空响应；后台认证和重启后的 listener 可以继续使用共享客户端的
keepalive 连接。runtime 线程参数首次初始化时生效。
auth/usage/aggregate 的同步兼容入口在桥接容量耗尽时返回原有错误类型，拒绝发生在
网络 future 被轮询前；不会把过载变成 panic 或发出未获准的请求。
网关入口、上游响应头等待、请求锁等待、重试退避、鉴权恢复及响应转换使用 `async/await`；
SSE/WS、取消和背压通过 Tokio 通道管理。每个请求在联系 provider 前预留响应容量，
请求锁保持到最终日志和计费完成；停服先等待路由任务，再排空响应与计费任务。
模型发现、账户测试和预热、代理测试、用量刷新、OAuth/Token、聚合 provider 管理、
插件目录和 Skills 仓库下载均提供原生异步入口。已发出的 Token 轮换请求由限并发完成任务
负责条件写回，调用方断连不会丢弃新凭据；停服等待这些任务、trace 队列排空和文件缓冲 flush。
该 flush 不包含文件系统 fsync，不提供断电后的持久性保证。
Service 和 Web 的 WebSocket relay 收到 Close 后，限时 flush 自动排队的应答再释放连接；
正常关闭测试覆盖发起请求前、响应后及 Web 双向转发的关闭码和原因。
独立 Service 的 `GET /__shutdown` 要求 RPC token、管理员 actor 和 RPC 相同的来源校验，
不占用普通请求槽；`request_shutdown` 的跨进程通知携带该 token。Web 的 `/__quit` 仍需要
有效 Web 登录会话。收到 HTTP 成功或进程最终消失本身不能证明优雅关闭，应核对退出码和排空结果。
同步数据库 facade 在生产多线程 runtime 的底层短操作边界让出 Tokio 执行线程，SeaORM 池和 SQLite SQLx 池
由进程复用。ZIP、文件操作、Rhai 同步脚本 ABI 和旧同步兼容接口仍有受控阻塞边界。
普通同步 RPC、Rhai 和插件调度使用固定 worker，避免等待异步结果时阻塞其内部文件/数据库
任务所需的 Tokio blocking pool。OAuth listener、设备码登录、取消后的登录清理及插件
scheduler 登记并在关闭时等待，支持同进程重新启动。废弃的 blocking HTTP client 缓存已删除；
历史协议测试仍保留测试专用同步适配。边界说明见
[运行时收敛](report/Axum-SeaORM运行时收敛验收.md)。最新验收范围见
[迁移实证记录](report/Axum-Tokio-Tower-SeaORM迁移进度与会话交接.md)。

典型请求链路如下：

1. 客户端或 UI 发起请求。
2. 请求进入 `crates/service` 的 HTTP / RPC 层。
3. 网关模块决定转发策略、账号、头部策略、上游代理等。
4. 协议适配层负责处理：
   - `/v1/chat/completions`
   - `/v1/responses`
   - 流式 SSE
   - 非流式 JSON
   - `tool_calls` / tools 映射与聚合
5. 结果回写请求日志和统计信息，再返回给调用方。

## 8. 构建与发布链路

### 8.1 本地开发构建

前端：

- `pnpm -C apps run dev`
- `pnpm -C apps run build`
- `pnpm -C apps run build:desktop`
- `pnpm -C apps run test:runtime`

Rust：

- `cargo test --workspace`
- `cargo build -p codexmanager-service --release`
- `cargo build -p codexmanager-web --release`
- `cargo build -p codexmanager-start --release`

桌面端：

- `scripts/rebuild.ps1`
- `scripts/rebuild-linux.sh`
- `scripts/rebuild-macos.sh`

### 8.2 版本管理

版本目前由根工作区统一维护：

- 根 `Cargo.toml` 的 `[workspace.package].version`

桌面端额外同步：

- `apps/src-tauri/Cargo.toml`
- `apps/src-tauri/tauri.conf.json`

统一修改入口：

- 当前没有单独的 `scripts/bump-version.ps1`；发版前按发布清单核对根 `Cargo.toml`、`apps/package.json`、Tauri 配置和两个 `Cargo.lock` 的版本。

### 8.3 GitHub Release

主要发布入口：

- `.github/workflows/release-all.yml`

职责：

- 构建 Windows / macOS / Linux 桌面产物
- 构建 Service 版本产物
- 上传 GitHub Release 附件
- 根据 tag / `prerelease` 输入决定发布类型

## 9. 当前结构风险

当前仓库需要重点关注以下问题：

1. `apps/src/app/settings/page.tsx`、`apps/src/app/logs/page.tsx`、`apps/src/app/aggregate-api/page.tsx` 等页面仍偏厚，新增逻辑应优先下沉到 hooks、feature components 或 `src/lib/` helper。
2. `apps/src-tauri/src/lib.rs` 仍是桌面壳层装配入口，新增桌面能力应优先落在 `app_shell/`、`commands/`、`rpc_client/` 等子模块。
3. `crates/service/src/lib.rs` 是 service 总出口，新增业务实现应优先落在领域模块，避免继续扩大导出和副作用边界。
4. `crates/service/src/gateway/` 协议兼容分支较多，回归风险高。
5. `.github/workflows/release-all.yml` 仍然较长，多平台逻辑需要持续约束。

## 10. 建议的改动落点

为了减少结构污染，新增需求尽量按以下原则落点：

- 新页面或前端交互：优先落在 `apps/src/app/`、`apps/src/components/`、`apps/src/hooks/`、`apps/src/lib/`
- 新桌面能力：优先落在 `apps/src-tauri/src/` 的独立模块，而不是全部继续塞进 `lib.rs`
- 新设置项：先判断属于环境变量、持久化配置还是运行时状态
- 新协议兼容：优先落在 gateway / protocol adapter 子模块，不要把条件分支继续无序堆叠
- 新发布逻辑：优先抽成脚本或复用步骤，不要三平台重复改三份
