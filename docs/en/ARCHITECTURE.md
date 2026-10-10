#ARCHITECTURE

This document describes CodexManager the current repository structure, running relationships, and release links. The goal is to help collaborators quickly determine which layer the changes should fall on.

## 1. Overall shape

CodexManager consists of two types of operating modes:

1. Desktop mode: Tauri Desktop + local service process
2. Service Mode: Standalone service + web UI, can be used with server, Docker or no desktop environment

Unified goal:

- Manage accounts, usage, and platform keys
- Provide local gateway capabilities
- Externally compatible with OpenAI style entry, and adapted to multiple upstream protocols

## 2. Directory structure and responsibilities

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
│  ├─ web/                # Web UI 服务壳，可嵌入前端静态资源
│  └─ start/              # Service 一键启动器（拉起 service + web）
├─ scripts/               # 本地构建、统一版本、测试探针、发布辅助脚本
├─ docker/                # Dockerfile 与 compose 配置
├─ assets/                # README 图片、Logo 等静态资源
└─ .github/workflows/     # CI / release workflow
```

## 3. Core complex domain entry index

### 3.1 Front-end master control entrance

- `apps/src/app/layout.tsx`: App Router root layout and global providers
- `apps/src/components/providers.tsx`: React Query, theme, i18n, and toast providers
- `apps/src/components/layout/app-bootstrap.tsx`: Desktop/Web runtime initialization and service connection setup
- `apps/src/lib/api/transport.ts`: Unified Tauri invoke and Web RPC transport facade

### 3.2 Desktop shell entrance

- `apps/src-tauri/src/lib.rs`: Tauri Application assembly entry
- `apps/src-tauri/src/commands/settings/mod.rs`: Desktop settings command module
- `apps/src-tauri/src/service_runtime.rs`: Desktop embedded service life cycle
- `apps/src-tauri/src/rpc_client/mod.rs`: Desktop RPC call infrastructure

### 3.3 service gateway and protocol entry

- `crates/service/src/lib.rs`: Service main entrance and runtime assembly
- `crates/service/src/http/`: HTTP routing entry
- `crates/service/src/rpc_dispatch/`: RPC Distribution entrance
- `crates/service/src/gateway/mod.rs`: Gateway aggregation entry
- `crates/service/src/gateway/observability/http_bridge/mod.rs`: Request tracking, protocol bridging, and log writing
- `crates/service/src/gateway/protocol_adapter/request_router.rs`: OpenAI/Codex request routing and upstream protocol selection
- `crates/service/src/gateway/protocol_adapter/types.rs`: Shared protocol adapter request/response types
- `crates/service/src/gateway/observability/http_bridge/stream_readers/`: Streaming response readers for supported upstream protocols
- `crates/service/src/gateway/request/request_rewrite.rs`: Request overrides and compatibility rewrites

### 3.4 Setup and run configuration entry

- `crates/service/src/app_settings/`: Set up persistence, environment variable coverage, runtime synchronization
- `crates/service/src/auth/web_access.rs`: Web Access password and session token

## 4. Running relationship

### 4.1 Desktop mode

Desktop mode consists of the following parts:

- `apps/src/`: Front-end UI
- `apps/src-tauri/`: Desktop shell
- `crates/service/`: local service

How to run:

1. The user launches the desktop application.
2. Tauri The shell is responsible for desktop behaviors such as windows, trays, updates, single instances, and setting bridges.
3. The desktop communicates with `codexmanager-service` via RPC or a local address.
4. The front-end UI displays pages such as account, usage, request log, and settings.

### 4.2 Service Mode

The Service pattern consists of the following binaries:

- `codexmanager-service`
- `codexmanager-web`
- `codexmanager-start`

Responsibilities:

- `codexmanager-service`: Core service process, providing account management, gateway forwarding, request logs, setting persistence, and RPC/HTTP interfaces.
- `codexmanager-web`: Web UI service shell, which can directly provide front-end pages and proxy to local services.
- `codexmanager-start`: A one-click launcher for publishing packages, responsible for launching service and web at the same time.

## 5. Module responsibilities

### 5.1 `apps/src/`

Mainly responsible for:

- Page rendering
- user interaction
- Status management
- Call local API / Tauri command
- Front-end logic of settings page and account page

### 5.2 `apps/src-tauri/`

Mainly responsible for:

- Tauri Application startup
- Single instance control
- System tray and window events
- Desktop updates and installer behavior
- Bridge front-end operations to service/local runtime

### 5.3 `crates/core/`

Mainly responsible for:

- SQLite Migration
- Storage underlying capabilities
- Core basic logic such as authentication/usage
- Data access capabilities that can be reused by services

### 5.4 `crates/service/`

Mainly responsible for:

- HTTP / RPC Portal
- Account, usage, API Key management
- Local gateway capabilities
- Protocol adaptation and upstream forwarding
- Request logging and setting persistence
- Runtime configuration synchronization

Key subdirectories:

- `src/gateway/`: Gateway, protocol adaptation, streaming and non-streaming conversion
- `src/http/`: HTTP routing entry
- `src/rpc_dispatch/`: RPC Distribution
- `src/account/`, `src/apikey/`, `src/requestlog/`, `src/usage/`: Domain logic

### 5.5 `crates/web/`

Mainly responsible for:

- Provide Web UI static resources
- Mount or proxy to service
- Optionally embed `apps/out` into the binary to form a single-file distribution

### 5.6 `crates/start/`

Mainly responsible for:

- Provide a more direct startup entry in the Service release package
- Coordinate the life cycle of service and web

## 6. Data and configuration

### 6.1 Database

Desktop mode uses SQLite. Service mode can use the SeaORM storage adapter for SQLite,
MySQL, or PostgreSQL when an explicit `CODEXMANAGER_DATABASE_URL` is configured.
Database migrations are located at:

- `crates/core/migrations/`: desktop SQLite migrations
- `crates/storage-seaorm/src/migration.rs`: service-mode cross-database migrations

The database not only stores accounts, but also assumes:

- API Key
- Request log
- token statistics
- app settings

### 6.2 Run configuration

The main sources of configuration include:

- Environment variables `CODEXMANAGER_*`
- `.env` / `codexmanager.env` in the application running directory
- `app_settings` Persistence table
- Desktop settings page

Current agreement:

- Configurations that must take effect before startup are retained at the environment variable layer.
- Runtime tunable configurations are first managed through the settings page + `app_settings`.
- Setting changes should not be scattered across desktops, frontends, and services without boundaries.

## 7. Request link overview

Typical request links are as follows:

1. The client or UI initiates the request.
2. Requests enter the HTTP/RPC layer of `crates/service`.
3. The gateway module determines the forwarding strategy, account number, header strategy, upstream proxy, etc.
4. The protocol adaptation layer is responsible for processing:
   - `/v1/chat/completions`
   - `/v1/responses`
   - Streaming SSE
   - Non-streaming JSON
   - `tool_calls` / tools mapping and aggregation
5. The results are written back to the request log and statistics, and then returned to the caller.

## 8. Build and publish links

### 8.1 Local development and build

front end:

- `pnpm -C apps run dev`
- `pnpm -C apps run build`
- `pnpm -C apps run check`

Rust:

- `cargo test --workspace`
- `cargo build -p codexmanager-service --release`
- `cargo build -p codexmanager-web --release`
- `cargo build -p codexmanager-start --release`

Desktop:

- `scripts/rebuild.ps1`
- `scripts/rebuild-linux.sh`
- `scripts/rebuild-macos.sh`

### 8.2 Version Management

The version is currently maintained uniformly by the root workspace:

- Root `Cargo.toml` of `[workspace.package].version`

Additional synchronization on desktop:

- `apps/src-tauri/Cargo.toml`
- `apps/src-tauri/tauri.conf.json`

Version changes are made directly in the workspace and Tauri manifests
(`Cargo.toml`, `apps/src-tauri/Cargo.toml`, and `apps/src-tauri/tauri.conf.json`);
no `scripts/bump-version.ps1` helper is currently checked in.

### 8.3 GitHub Release

Main publishing entrance:

- `.github/workflows/release-all.yml`

Responsibilities:

- Build Windows / macOS / Linux Desktop Product
- Build version Service artifact
- Upload GitHub Release attachment
- Determine release type based on tag / `prerelease` input

## 9. Current structural risks

The current repository needs to focus on the following issues:

1. `apps/src-tauri/src/lib.rs` It is still thick, and the desktop shell assembly and command implementation still need to be disassembled.
2. `crates/service/src/lib.rs` Configuration, runtime synchronization, and side effect boundaries are not clear enough.
3. `crates/service/src/gateway/protocol_adapter/` has many compatibility branches and a high regression risk.
4. `.github/workflows/release-all.yml` Still long, multi-platform logic requires persistence constraints.

## 10. Suggested changes

In order to reduce structural pollution, new demands should be targeted according to the following principles:

- New pages or front-end interactions: Priority falls in `apps/src/app/`, `apps/src/components/`, `apps/src/lib/`
- New Desktop Capabilities: Prioritize standalone modules that fall into `apps/src-tauri/src/`, rather than continuing to cram them all into `lib.rs`
- New setting item: first determine whether it belongs to environment variables, persistent configuration or runtime state
- Compatible with new protocols: priority should be placed in the gateway / protocol adapter submodule, and do not continue to stack conditional branches out of order.
- New release logic: Give priority to drawing scripts or reusing steps, and do not repeat modifications three times on three platforms.
