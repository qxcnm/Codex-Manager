<p align="center">
  <img src="assets/logo/logo.png" alt="CodexManager Logo" width="220" />
</p>

<h1 align="center">CodexManager</h1>

<p align="center">本地桌面端 + 服务进程的 Codex 账号池管理器</p>

<p align="center">
  <a href="README.en.md">English</a>
</p>

本地桌面端 + 服务进程的 Codex 账号池管理器，用于统一管理账号、用量与平台 Key，并提供本地网关能力。

## 最近变更
- 当前最新版本：`v0.1.6`（2026-03-07）
- 本次版本重点修复了 `release-all.yml` 在 `run_verify=false` 时的前端工件回退构建，继续收敛 Windows 发布产物，并完善了 SOCKS5 上游代理支持与提示文案。
- 完整版本历史请查看 [CHANGELOG.md](CHANGELOG.md)。

## 协作与维护
- 版本历史：[CHANGELOG.md](CHANGELOG.md)
- 协作约定：[CONTRIBUTING.md](CONTRIBUTING.md)
- 架构说明：[ARCHITECTURE.md](ARCHITECTURE.md)

## 功能概览
- 账号池管理：分组、标签、排序、备注
- 批量导入 / 导出：支持多文件导入、桌面端文件夹递归导入 JSON、按账号导出单文件
- 用量展示：兼容 5 小时 + 7 日双窗口，以及仅返回 7 日单窗口（如免费周额度）的账号
- 授权登录：浏览器授权 + 手动回调解析
- 平台 Key：生成、禁用、删除、模型绑定
- 本地服务：自动拉起、可自定义端口
- 本地网关：为 CLI/第三方工具提供统一 OpenAI 兼容入口

## 截图
![仪表盘](assets/images/dashboard.png)
![账号管理](assets/images/accounts.png)
![平台 Key](assets/images/platform-key.png)
![日志视图](assets/images/log.png)
![设置页](assets/images/themes.png)

## 技术栈
- 前端：Vite + 原生 JavaScript
- 桌面端：Tauri (Rust)
- 服务端：Rust（本地 HTTP/RPC + Gateway）

## 目录结构
```text
.
├─ apps/                # 前端与 Tauri 桌面端
│  ├─ src/
│  ├─ src-tauri/
│  └─ dist/
├─ crates/              # Rust core/service
│  ├─ core
│  ├─ service
│  ├─ start              # Service 版本一键启动器（拉起 service + web）
│  └─ web                # Service 版本 Web UI（可内嵌静态资源 + /api/rpc 代理）
├─ scripts/             # 构建与发布脚本
├─ portable/            # 便携版输出目录
└─ README.md
```

## 快速开始
1. 启动桌面端，点击“启动服务”。
2. 进入“账号管理”，添加账号并完成授权。
3. 如回调失败，粘贴回调链接手动完成解析。
4. 刷新用量并确认账号状态。

## 账号导入 / 导出
- `批量导入`：选择多个 `.json/.txt` 文件后统一导入。
- `按文件夹导入`（仅桌面端）：选择目录后递归扫描其中 `.json` 文件并批量导入，空文件会自动跳过。
- `导出用户`：选择目录后按“一个账号一个 JSON 文件”导出，便于备份与迁移。

## Service 版本（后台服务 + Web UI，无桌面环境）
1. 下载 Release 中的 `CodexManager-service-<platform>-<arch>.zip` 并解压。
2. 推荐：启动 `codexmanager-start`（一个进程拉起 service + web，且可在控制台 Ctrl+C 关闭）。
3. 也可以只启动 `codexmanager-web`（会自动拉起同目录的 `codexmanager-service`，并打开浏览器）。
4. 或者先启动 `codexmanager-service`（会显示控制台日志），再启动 `codexmanager-web`。
5. 默认地址：service `localhost:48760`，Web UI `http://localhost:48761/`。
6. 关闭：访问 `http://localhost:48761/__quit`（会关闭 web；若 web 自动拉起过 service，会尝试一并关闭 service）。

## Docker 部署
### 方式 1：docker compose（推荐）
```bash
docker compose -f docker/docker-compose.yml up --build
```
浏览器打开：`http://localhost:48761/`

### 方式 2：分别构建/运行
```bash
# service
docker build -f docker/Dockerfile.service -t codexmanager-service .
docker run --rm -p 48760:48760 -v codexmanager-data:/data \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-service

# web（需要能访问到 service）
docker build -f docker/Dockerfile.web -t codexmanager-web .
docker run --rm -p 48761:48761 \
  -e CODEXMANAGER_WEB_NO_SPAWN_SERVICE=1 \
  -e CODEXMANAGER_SERVICE_ADDR=host.docker.internal:48760 \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-web
```

## 开发与构建
### 前端
```bash
pnpm -C apps install
pnpm -C apps run dev
pnpm -C apps run test
pnpm -C apps run test:ui
pnpm -C apps run build
```

### Rust
```bash
cargo test --workspace
cargo build -p codexmanager-service --release
cargo build -p codexmanager-web --release
cargo build -p codexmanager-start --release

# 发行物/容器：将前端静态资源打进 codexmanager-web（二进制单文件）
pnpm -C apps run build
cargo build -p codexmanager-web --release --features embedded-ui
```

### Tauri 打包（Windows）
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Tauri 打包（Linux/macOS）
```bash
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

## macOS 首次启动
- 当前 macOS Release 产物未使用 Apple Developer 账号完成公证，因此首次从浏览器下载后，Gatekeeper 可能提示“已损坏”或拒绝打开。
- Release 中的 macOS `dmg` 现已内置 `Open CodexManager.command` 与 `README-macOS-first-launch.txt`，推荐先把 `CodexManager.app` 拖到“应用程序”，再双击该脚本完成首次放行。
- 也可以直接执行：

```bash
xattr -dr com.apple.quarantine /Applications/CodexManager.app
```

- 如果仍被拦截，再对 `CodexManager.app` 执行一次“右键 -> 打开”。

## GitHub Actions（全部手动触发）
当前发布入口为 `release-all.yml`，触发方式为 `workflow_dispatch`，不会自动触发。

- `release-all.yml`
  - 用途：一键发布 Desktop + Service 全平台产物（单次触发）
  - 构建平台：`Windows`、`macOS（dmg）`、`Linux`
  - 触发：手动
  - 输入：
    - `tag`（必填）
    - `ref`（默认 `main`）
    - `run_verify`（默认 `true`，可关闭）
    - `prerelease`（默认 `auto`，可选 `auto|true|false`）

## Release 产物清单（`release-all.yml`）
### Desktop
- Windows：`CodexManager_<version>_x64-setup.exe`、`CodexManager-portable.exe`
- macOS：`CodexManager_<version>_aarch64.dmg`、`CodexManager_<version>_x64.dmg`（dmg 内含 `Open CodexManager.command` 与首次启动说明）
- Linux：`CodexManager_<version>_amd64.AppImage`、`CodexManager_<version>_amd64.deb`、`CodexManager-linux-portable.zip`

### Service
- Windows：`CodexManager-service-windows-x86_64.zip`
- macOS：`CodexManager-service-macos-arm64.zip`、`CodexManager-service-macos-x64.zip`
- Linux：`CodexManager-service-linux-x86_64.zip`

### 发布类型
- `prerelease=auto` 时，`tag` 包含 `-`（例如 `v0.1.6-beta.1`）会发布为 **pre-release**
- `prerelease=auto` 时，不包含 `-`（例如 `v0.1.6`）会发布为正式版
- `prerelease=true|false` 时，会覆盖基于 tag 的自动判断
- 重跑同一 `tag` 时，会按当前输入同步 Release 元数据，避免 `prerelease` 状态漂移
- GitHub 仍会自动附带 `Source code (zip/tar.gz)`

## 脚本说明
### `scripts/rebuild.ps1`（Windows）
默认用于本地 Windows 打包；`-AllPlatforms` 模式会调用 GitHub workflow。

常用示例：
```powershell
# 本地 Windows 构建
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable

# 触发 release workflow（并下载工件）
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.0.9 `
  -GithubToken <token>

# 跳过 workflow 内质量门
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9 -GithubToken <token> -NoVerify

# 强制发布为 pre-release
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9-beta.1 -GithubToken <token> -Prerelease true
```

参数（含默认值）：
- `-Bundle nsis|msi`：默认 `nsis`
- `-NoBundle`：仅编译，不出安装包
- `-CleanDist`：构建前清理 `apps/dist`
- `-Portable`：额外输出便携版
- `-PortableDir <path>`：便携版输出目录，默认 `portable/`
- `-AllPlatforms`：触发指定 release workflow（由 `-WorkflowFile` 指定）
- `-GithubToken <token>`：GitHub token；不传时尝试 `GITHUB_TOKEN`/`GH_TOKEN`
- `-WorkflowFile <name>`：默认 `release-all.yml`（单一一键发布入口）
- `-GitRef <ref>`：workflow 构建 ref；默认当前分支或当前 tag
- `-ReleaseTag <tag>`：发布 tag；`-AllPlatforms` 时建议显式传入
- `-NoVerify`：将 workflow 输入 `run_verify` 设为 `false`
- `-Prerelease <auto|true|false>`：默认 `auto`；传给 workflow 的 `prerelease` 输入
- `-DownloadArtifacts <bool>`：默认 `true`
- `-ArtifactsDir <path>`：工件下载目录，默认 `artifacts/`
- `-PollIntervalSec <n>`：轮询间隔，默认 `10`
- `-TimeoutMin <n>`：超时分钟数，默认 `60`
- `-DryRun`：仅打印执行计划

### `scripts/bump-version.ps1`（统一版本号）
用于一次性更新发版版本号，避免手改多个文件。

```powershell
pwsh -NoLogo -NoProfile -File scripts/bump-version.ps1 -Version 0.1.6
```

会同步更新：
- 根 `Cargo.toml` 的 workspace 版本
- `apps/src-tauri/Cargo.toml`
- `apps/src-tauri/tauri.conf.json`

### 协议回归探针
统一入口：

```powershell
pwsh -NoLogo -NoProfile -File scripts/tests/gateway_regression_suite.ps1 `
  -Base http://localhost:48760 -ApiKey <key> -Model gpt-5.3-codex
```

它会串行执行：
- `chat_tools_hit_probe.ps1` 非流式
- `chat_tools_hit_probe.ps1 -Stream`
- `codex_stream_probe.ps1`（同时覆盖 chat / responses 流式）

排障手册：
- [docs/report/20260307234235414_最小排障手册.md](docs/report/20260307234235414_最小排障手册.md)

## 环境变量说明
### 加载与优先级
- 桌面端 / `codexmanager-service` / `codexmanager-web` 均会在可执行文件同目录按顺序查找环境文件：`codexmanager.env` -> `CodexManager.env` -> `.env`（命中第一个即停止）。
- 环境文件中只会注入“当前进程尚未定义”的变量，已有系统/用户变量不会被覆盖。
- 绝大多数变量均为可选；若运行目录不可写（如安装目录），可用 `CODEXMANAGER_DB_PATH` 指向可写路径。
- 下表按“常用/高级”拆分；若需要完整列表，可在源码中搜索 `CODEXMANAGER_` 作为最终准入标准。

### 常用变量（`CODEXMANAGER_*`）
| 变量 | 默认值 | 说明 |
|---|---|---|
| `CODEXMANAGER_SERVICE_ADDR` | `localhost:48760` | service 监听地址；桌面端也会用它作为默认 RPC 目标地址。 |
| `CODEXMANAGER_WEB_ADDR` | `localhost:48761` | Service 版本 Web UI 监听地址（仅 `codexmanager-web` 使用）。 |
| `CODEXMANAGER_WEB_ROOT` | 同目录 `web/` | Web 静态资源目录（仅 `codexmanager-web` 使用；若使用内嵌前端资源则无需该目录）。 |
| `CODEXMANAGER_WEB_NO_OPEN` | 未设置 | 若设置则 `codexmanager-web` 不会自动打开浏览器。 |
| `CODEXMANAGER_WEB_NO_SPAWN_SERVICE` | 未设置 | 若设置则 `codexmanager-web` 不会尝试自动拉起同目录的 `codexmanager-service`。 |
| `CODEXMANAGER_DB_PATH` | 同目录 `codexmanager.db`（Service/Web）；桌面端自动设置 | SQLite 数据库路径。桌面端会自动设为 `app_data_dir/codexmanager.db`。 |
| `CODEXMANAGER_RPC_TOKEN` | 自动生成 64 位十六进制随机串 | `/rpc` 鉴权 token。未设置时自动生成，并默认落盘到 `codexmanager.rpc-token` 便于跨进程复用。 |
| `CODEXMANAGER_RPC_TOKEN_FILE` | 同目录 `codexmanager.rpc-token` | 指定 `/rpc` token 文件路径（相对路径以 DB 所在目录为基准）。 |
| `CODEXMANAGER_NO_SERVICE` | 未设置 | 只要变量存在（值可为空）就不自动拉起内嵌 service。 |
| `CODEXMANAGER_ISSUER` | `https://auth.openai.com` | OAuth issuer。 |
| `CODEXMANAGER_CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` | OAuth client id。 |
| `CODEXMANAGER_ORIGINATOR` | `codex_cli_rs` | OAuth authorize 请求中的 `originator`。 |
| `CODEXMANAGER_REDIRECT_URI` | `http://localhost:1455/auth/callback`（或登录服务动态端口） | OAuth 回调地址。 |
| `CODEXMANAGER_LOGIN_ADDR` | `localhost:1455` | 本地登录回调监听地址。 |
| `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` | `false` | 是否允许非 loopback 回调地址。仅 `1/true/TRUE/yes/YES` 视为开启。 |
| `CODEXMANAGER_USAGE_BASE_URL` | `https://chatgpt.com` | 用量接口 base URL。 |
| `CODEXMANAGER_DISABLE_POLLING` | 未设置（即开启轮询） | 兼容旧开关：只要变量存在（值可为空）就禁用后台用量轮询线程。 |
| `CODEXMANAGER_USAGE_POLLING_ENABLED` | `true` | 用量轮询总开关（`1/true/on/yes` 开启，`0/false/off/no` 关闭）。与 `CODEXMANAGER_DISABLE_POLLING` 同时存在时，以该值为准。 |
| `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` | `600` | 用量轮询间隔（秒），最小 `30`。非法值回退默认。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED` | `true` | 网关保活轮询总开关（`1/true/on/yes` 开启，`0/false/off/no` 关闭）。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS` | `180` | Gateway keepalive 间隔（秒），最小 `30`。 |
| `CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED` | `true` | 令牌刷新轮询总开关（`1/true/on/yes` 开启，`0/false/off/no` 关闭）。 |
| `CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS` | `60` | 令牌刷新轮询间隔（秒），最小 `10`。 |
| `CODEXMANAGER_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` | 主上游地址。若填 `https://chatgpt.com`/`https://chat.openai.com` 会自动归一化到 backend-api/codex。 |
| `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL` | 自动推断 | 明确指定 fallback 上游。若未设置且主上游是 ChatGPT backend，则默认 fallback 到 `https://api.openai.com/v1`。 |
| `CODEXMANAGER_UPSTREAM_COOKIE` | 未设置 | 上游 Cookie（主要用于 Cloudflare/WAF challenge 场景）。 |
| `CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE` | `0` | 启用请求头收敛策略：默认不发 `x-codex-turn-state`/`Conversation_id`/固定 `Openai-Beta`/`Chatgpt-Account-Id`，降低 Cloudflare/WAF 拦截概率。可在设置页切换。 |
| `CODEXMANAGER_ROUTE_STRATEGY` | `ordered` | 网关账号选路策略：默认 `ordered`（按账号顺序优先，失败再下一个）；可设 `balanced`/`round_robin`/`rr` 启用按 `Key+模型` 的均衡轮询起点。 |
| `CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS` | `15` | 上游连接阶段超时（秒）。 |
| `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` | `120000` | 上游单次请求总超时（毫秒）。设为 `0` 表示关闭总超时。 |
| `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS` | `300000` | 上游流式请求超时（毫秒）。设为 `0` 表示关闭流式超时。 |
| `CODEXMANAGER_UPSTREAM_PROXY_URL` | 未设置 | OpenAI 上游单代理地址（例如 `http://127.0.0.1:7890`）。留空表示直连；设置页“网关策略 -> OpenAI 上游代理”可直接配置。 |
| `CODEXMANAGER_PROXY_LIST` | 未设置 | 上游代理池（最多 5 条，逗号/分号/换行分隔）。按 `account_id` 稳定哈希绑定到某个代理，避免同账号跨代理漂移。 |
| `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS` | `300` | 请求闸门等待预算（毫秒）。 |
| `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT` | `0` | 单账号并发软上限。`0` 表示不限制。 |
| `CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES` | `0` | Trace body 预览最大字节数。`0` 表示关闭 body 预览。 |
| `CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES` | `16777216` | 前置代理允许的请求体最大字节数（默认 16 MiB）。 |
| `CODEXMANAGER_HTTP_WORKER_FACTOR` | `4` | backend worker 数量系数，worker = `max(cpu * factor, worker_min)`（运行中修改需重启 service 生效）。 |
| `CODEXMANAGER_HTTP_WORKER_MIN` | `8` | backend worker 最小值（运行中修改需重启 service 生效）。 |
| `CODEXMANAGER_HTTP_QUEUE_FACTOR` | `4` | backend 请求队列系数，queue = `max(worker * factor, queue_min)`。 |
| `CODEXMANAGER_HTTP_QUEUE_MIN` | `32` | backend 请求队列最小值。 |

### 高级变量（可选）
| 变量 | 默认值 | 说明 |
|---|---|---|
| `CODEXMANAGER_ACCOUNT_IMPORT_BATCH_SIZE` | `200` | 账号导入分批大小（用于一次导入大量 auth.json）。 |
| `CODEXMANAGER_TRACE_QUEUE_CAPACITY` | `2048` | gateway trace 异步写队列容量（过小可能丢 trace；过大可能占内存）。 |
| `CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR` | `1` | backend stream worker 数量系数（SSE 等长连接请求，运行中修改需重启 service 生效）。 |
| `CODEXMANAGER_HTTP_STREAM_WORKER_MIN` | `2` | backend stream worker 最小值（运行中修改需重启 service 生效）。 |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_FACTOR` | `2` | backend stream 队列系数。 |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_MIN` | `16` | backend stream 队列最小值。 |
| `CODEXMANAGER_POLL_JITTER_SECS` | 未设置 | 通用轮询 jitter（秒），可被各模块各自的 jitter 覆盖。 |
| `CODEXMANAGER_POLL_FAILURE_BACKOFF_MAX_SECS` | 未设置 | 通用失败退避上限（秒），可被各模块各自的 backoff 覆盖。 |
| `CODEXMANAGER_USAGE_POLL_JITTER_SECS` | `5` | 用量轮询 jitter（秒）。 |
| `CODEXMANAGER_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS` | `1800` | 用量轮询失败退避上限（秒）。 |
| `CODEXMANAGER_USAGE_REFRESH_WORKERS` | `4` | 用量刷新 worker 数（可在设置页配置；运行中修改需重启 service 生效）。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_JITTER_SECS` | `5` | keepalive jitter（秒）。 |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS` | `900` | keepalive 失败退避上限（秒）。 |
| `CODEXMANAGER_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS` | `60` | 用量刷新失败事件去重窗口（秒），避免瞬时抖动刷爆事件表。 |
| `CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT` | `200` | 每账号保留用量快照条数（0 表示不裁剪）。 |
| `CODEXMANAGER_CANDIDATE_CACHE_TTL_MS` | `500` | 网关候选快照缓存 TTL（毫秒），减少高频请求时的 DB 压力；设为 `0` 关闭缓存。 |
| `CODEXMANAGER_PROMPT_CACHE_TTL_SECS` | `3600` | prompt cache TTL（秒）。 |
| `CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS` | `60` | prompt cache 清理间隔（秒）。 |
| `CODEXMANAGER_PROMPT_CACHE_CAPACITY` | `4096` | prompt cache 容量上限（0 表示不限制）。 |
| `CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES` | `131072` | 上游响应 `output_text` 累积上限（字节），避免内存增长（0 关闭限制）。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED` | `true` | 是否启用候选健康度 P2C（Power of Two Choices）选路。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW` | `3` | `ordered` 模式下 P2C 参与窗口大小。 |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW` | `6` | `balanced` 模式下 P2C 参与窗口大小。 |
| `CODEXMANAGER_ROUTE_STATE_TTL_SECS` | `21600` | 路由状态 TTL（秒），避免 key/model 高基数导致状态无限增长。 |
| `CODEXMANAGER_ROUTE_STATE_CAPACITY` | `4096` | 路由状态容量上限。 |
| `CODEXMANAGER_UPDATE_REPO` | `qxcnm/Codex-Manager` | 应用内更新检查的 GitHub 仓库（`owner/name`）。 |
| `CODEXMANAGER_GITHUB_TOKEN` | 未设置 | 应用内“一键更新”用 GitHub token（也会回退到 `GITHUB_TOKEN`/`GH_TOKEN`）；不设置可能受 API 限流影响导致下载元数据降级。 |

### 发布脚本相关变量
| 变量 | 默认值 | 是否必填 | 说明 |
|---|---|---|---|
| `GITHUB_TOKEN` | 无 | 条件必填 | 仅在 `rebuild.ps1 -AllPlatforms` 且未传 `-GithubToken` 时必填。 |
| `GH_TOKEN` | 无 | 条件必填 | 与 `GITHUB_TOKEN` 等价的后备变量。 |

## 环境文件示例（放在可执行文件同目录）
```dotenv
# codexmanager.env / CodexManager.env / .env
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_WEB_ADDR=localhost:48761
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
# 可选：后台任务总开关
# CODEXMANAGER_USAGE_POLLING_ENABLED=1
# CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED=1
# CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED=1
# 可选：固定 RPC token 方便外部工具长期复用
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

说明：
- 环境文件在**桌面端 / service / web 进程启动时**读取一次；修改文件后需要重启对应进程才会生效。
- 桌面端会把 service 端口保存到本地存储；环境变量更多用于首次默认值（若需强制按环境变量重置，请在 UI 手动修改端口，或清理本地存储后重启）。
- 环境文件只会注入“当前进程尚未定义”的变量；若你已在系统环境变量中设置了同名 `CODEXMANAGER_*`，则系统环境变量优先生效。

## 常见问题
- 授权回调失败：优先检查 `CODEXMANAGER_LOGIN_ADDR` 是否被占用，或在 UI 使用手动回调解析。
- 模型列表/请求被挑战拦截：可尝试设置 `CODEXMANAGER_UPSTREAM_COOKIE`，或显式配置 `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL`。
- 仍被 Cloudflare/WAF 拦截：可在设置页开启“请求头收敛策略”，或设置 `CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE=1`。
- “部分数据刷新失败，已展示可用数据”频繁出现：自动刷新场景已改为仅记录日志；手动刷新会提示失败项与示例错误。可优先检查设置页“后台任务”间隔/开关是否过激进，以及 service 日志中的失败任务名。
- 独立运行 service/Web：若所在目录不可写（如安装目录），请设置 `CODEXMANAGER_DB_PATH` 到可写路径。
- macOS 代理环境下请求 `502/503`：优先确认系统代理未接管本地回环请求（`localhost/127.0.0.1` 走 `DIRECT`），并确保地址使用小写 `localhost:<port>`（例如 `localhost:48760`）。

## 账号命中规则
- `ordered`（顺序优先）模式下，网关按账号 `sort` 升序构建候选并依次尝试（例如 `0 -> 1 -> 2 -> 3`）。
- 这表示“按顺序尝试”，不是“永远命中 0 号”：前序账号若不可用/失败，会自动切到下一个。
- 以下情况会导致前序账号不被命中：
  - 账号状态不是 `active`
  - 账号缺少 token
  - 用量判定不可用（如主窗口已用尽、用量字段缺失等）
  - 账号处于 cooldown 或并发软上限触发跳过
- `balanced`（均衡轮询）模式会按 `Key + 模型` 维度轮换起点，不保证从最小 `sort` 开始。
- 排查时可查看数据库同目录 `gateway-trace.log`：
  - `CANDIDATE_POOL`：本次请求候选顺序
  - `CANDIDATE_START` / `CANDIDATE_SKIP`：实际尝试与跳过原因
  - `REQUEST_FINAL`：最终命中账号

## 🤝 鸣谢项目 (Special Thanks)
本项目在网关协议适配与稳定性治理上参考了以下开源项目的思路：

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)

对应实现可见：
- `crates/service/src/gateway/protocol_adapter/request_mapping.rs`
- `crates/service/src/gateway/upstream/transport.rs`

## 联系方式

<p align="center">
  <img src="assets/images/group.jpg" alt="交流群二维码" width="280" />
  <img src="assets/images/qq_group.jpg" alt="QQ 交流群二维码" width="280" />
</p>

- Telegram 交流群：<https://t.me/+8o2Eu7GPMIFjNDM1>
- QQ 交流群：扫码加入
- 微信公众号：七线牛马
