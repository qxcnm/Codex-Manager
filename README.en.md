<p align="center">
  <img src="assets/logo/logo.png" alt="CodexManager Logo" width="220" />
</p>

<h1 align="center">CodexManager</h1>

<p align="center">A local desktop + service toolkit for Codex-compatible account and gateway management.</p>

<p align="center">
  <a href="README.md">中文</a>
</p>

A local desktop + service toolkit for managing a Codex-compatible ChatGPT account pool, usage, and platform keys, with a built-in local gateway.

## Recent Changes
- Current latest version: `v0.1.6` (2026-03-07)
- This release mainly fixes the `release-all.yml` fallback path when `run_verify=false`, continues to simplify Windows release artifacts, and improves SOCKS5 upstream proxy support plus settings hints.
- Full version history is now maintained in [CHANGELOG.md](CHANGELOG.md).

## Maintenance Docs
- Version history: [CHANGELOG.md](CHANGELOG.md)
- Contribution guide: [CONTRIBUTING.md](CONTRIBUTING.md)
- Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md)

## Features
- Account pool management: group, tag, sort, note
- Bulk import / export: supports multi-file import, desktop-only recursive folder import for JSON files, and one-file-per-account export
- Usage dashboard: supports 5-hour + 7-day dual windows, and accounts that only return a 7-day single window (for example free weekly quota)
- OAuth login: browser flow + manual callback parsing
- Platform keys: create, disable, delete, bind model
- Local service: auto-start with configurable port
- Local gateway: OpenAI-compatible entry for CLI/tools

## Screenshots
![Dashboard](assets/images/dashboard.png)
![Accounts](assets/images/accounts.png)
![Platform Key](assets/images/platform-key.png)
![Logs](assets/images/log.png)
![Settings](assets/images/themes.png)

## Tech Stack
- Frontend: Vite + vanilla JavaScript
- Desktop: Tauri (Rust)
- Service: Rust (local HTTP/RPC + Gateway)

## Project Structure
```text
.
├─ apps/                # Frontend + Tauri desktop app
│  ├─ src/
│  ├─ src-tauri/
│  └─ dist/
├─ crates/              # Rust core/service
│  ├─ core
│  ├─ service
│  ├─ start              # Service edition starter (launches service + web)
│  └─ web                # Service edition Web UI (optional embedded assets + /api/rpc proxy)
├─ scripts/             # build/release scripts
├─ portable/            # portable output
└─ README.en.md
```

## Quick Start
1. Launch desktop app and click "Start Service".
2. Add accounts in Account Management and finish OAuth.
3. If callback fails, paste callback URL into manual parser.
4. Refresh usage and verify account status.

## Import / Export Accounts
- `Bulk Import`: choose multiple `.json/.txt` files and import them in one run.
- `Import by Folder` (desktop only): choose a directory and recursively import all `.json` files under it; empty files are skipped automatically.
- `Export Users`: choose a folder and export accounts as one JSON file per account for backup or migration.

## Service Edition (Headless service + Web UI, no desktop runtime)
1. Download `CodexManager-service-<platform>-<arch>.zip` from the Release page and unzip.
2. Recommended: start `codexmanager-start` (one process that launches both service + web, and you can Ctrl+C to stop).
3. You can also start `codexmanager-web` directly (it will auto-spawn `codexmanager-service` from the same directory and open the browser).
4. Or start `codexmanager-service` first (shows console logs), then start `codexmanager-web`.
5. Default addresses: service `localhost:48760`, Web UI `http://localhost:48761/`.
6. Quit: open `http://localhost:48761/__quit` (stops web; if web auto-spawned the service, it will try to stop the service as well).

## Docker Deployment
### Option 1: docker compose (Recommended)
```bash
docker compose -f docker/docker-compose.yml up --build
```
Open in browser: `http://localhost:48761/`

### Option 2: Build/Run separately
```bash
# service
docker build -f docker/Dockerfile.service -t codexmanager-service .
docker run --rm -p 48760:48760 -v codexmanager-data:/data \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-service

# web (must reach the service)
docker build -f docker/Dockerfile.web -t codexmanager-web .
docker run --rm -p 48761:48761 \
  -e CODEXMANAGER_WEB_NO_SPAWN_SERVICE=1 \
  -e CODEXMANAGER_SERVICE_ADDR=host.docker.internal:48760 \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-web
```

## Development & Build
### Frontend
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

# Release/containers: embed frontend assets into codexmanager-web (single binary)
pnpm -C apps run build
cargo build -p codexmanager-web --release --features embedded-ui
```

### Tauri Packaging (Windows)
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Tauri Packaging (Linux/macOS)
```bash
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

## macOS First Launch
- Current macOS release builds are not notarized with an Apple Developer account yet, so Gatekeeper may mark the downloaded app as damaged or block it on first launch.
- The macOS `dmg` now includes `Open CodexManager.command` and `README-macOS-first-launch.txt`. Recommended flow: drag `CodexManager.app` into Applications first, then double-click the helper script once.
- Manual command:

```bash
xattr -dr com.apple.quarantine /Applications/CodexManager.app
```

- If macOS still blocks it, right-click `CodexManager.app` and choose `Open` once.

## GitHub Actions (Manual Only)
The current release entry is `release-all.yml`. It is `workflow_dispatch` only and never runs automatically.

- `release-all.yml`
  - Purpose: one-click release for Desktop + Service artifacts across platforms
  - Target platforms: `Windows`, `macOS (dmg)`, `Linux`
  - Trigger: manual only
  - Inputs:
    - `tag` (required)
    - `ref` (default: `main`)
    - `run_verify` (default: `true`)
    - `prerelease` (default: `auto`, supports `auto|true|false`)

## Release Asset List (`release-all.yml`)
### Desktop
- Windows: `CodexManager_<version>_x64-setup.exe`, `CodexManager-portable.exe`
- macOS: `CodexManager_<version>_aarch64.dmg`, `CodexManager_<version>_x64.dmg` (the dmg includes `Open CodexManager.command` and a first-launch note)
- Linux: `CodexManager_<version>_amd64.AppImage`, `CodexManager_<version>_amd64.deb`, `CodexManager-linux-portable.zip`

### Service
- Windows: `CodexManager-service-windows-x86_64.zip`
- macOS: `CodexManager-service-macos-arm64.zip`, `CodexManager-service-macos-x64.zip`
- Linux: `CodexManager-service-linux-x86_64.zip`

### Release Type
- With `prerelease=auto`, a tag containing `-` (for example `v0.1.6-beta.1`) is published as a **pre-release**
- With `prerelease=auto`, a tag without `-` (for example `v0.1.6`) is published as a stable release
- `prerelease=true|false` overrides the automatic tag-based rule
- Re-running the same tag re-syncs release metadata to the current inputs, so the `prerelease` flag does not drift
- GitHub will still auto-attach `Source code (zip/tar.gz)`

## Script Reference
### `scripts/rebuild.ps1` (Windows)
Primarily for local Windows packaging. `-AllPlatforms` mode dispatches GitHub workflow.

Examples:
```powershell
# Local Windows build
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable

# Dispatch a release workflow (and download artifacts)
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.0.9 `
  -GithubToken <token>

# Skip verify gate inside release workflow
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9 -GithubToken <token> -NoVerify

# Force a pre-release build
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.0.9-beta.1 -GithubToken <token> -Prerelease true
```

Parameters (with defaults):
- `-Bundle nsis|msi`: default `nsis`
- `-NoBundle`: compile only, no installer bundle
- `-CleanDist`: clean `apps/dist` before build
- `-Portable`: also stage portable output
- `-PortableDir <path>`: portable output dir, default `portable/`
- `-AllPlatforms`: dispatch the selected release workflow (`-WorkflowFile`)
- `-GithubToken <token>`: GitHub token; falls back to `GITHUB_TOKEN`/`GH_TOKEN`
- `-WorkflowFile <name>`: default `release-all.yml` (single one-click release entry)
- `-GitRef <ref>`: workflow ref; defaults to current branch or current tag
- `-ReleaseTag <tag>`: release tag; strongly recommended in `-AllPlatforms`
- `-NoVerify`: sets workflow input `run_verify=false`
- `-Prerelease <auto|true|false>`: default `auto`; forwarded to the workflow `prerelease` input
- `-DownloadArtifacts <bool>`: default `true`
- `-ArtifactsDir <path>`: artifact download dir, default `artifacts/`
- `-PollIntervalSec <n>`: polling interval, default `10`
- `-TimeoutMin <n>`: timeout minutes, default `60`
- `-DryRun`: print plan only

### `scripts/bump-version.ps1` (Unified Version Bump)
Use this to bump release version in one command instead of editing multiple files manually.

```powershell
pwsh -NoLogo -NoProfile -File scripts/bump-version.ps1 -Version 0.1.6
```

It updates:
- root `Cargo.toml` workspace version
- `apps/src-tauri/Cargo.toml`
- `apps/src-tauri/tauri.conf.json`

### Protocol Regression Probes
Unified entry:

```powershell
pwsh -NoLogo -NoProfile -File scripts/tests/gateway_regression_suite.ps1 `
  -Base http://localhost:48760 -ApiKey <key> -Model gpt-5.3-codex
```

It runs, in order:
- `chat_tools_hit_probe.ps1` non-stream
- `chat_tools_hit_probe.ps1 -Stream`
- `codex_stream_probe.ps1` (covers both chat and responses streaming)

Troubleshooting guide:
- [docs/report/20260307234235414_最小排障手册.md](docs/report/20260307234235414_最小排障手册.md)

## Environment Variables
### Load Rules and Precedence
- Desktop / `codexmanager-service` / `codexmanager-web` load env files from executable directory in this order:
  - `codexmanager.env` -> `CodexManager.env` -> `.env` (first hit wins)
- Existing process/system env vars are not overridden by env-file values.
- Most vars are optional. If the run directory is not writable (for example an install directory), set `CODEXMANAGER_DB_PATH` to a writable path.
- The tables below are split into common vs advanced knobs. For the full list, search `CODEXMANAGER_` in the source code as the source of truth.

### Common Variables (`CODEXMANAGER_*`)
| Variable | Default | Description |
|---|---|---|
| `CODEXMANAGER_SERVICE_ADDR` | `localhost:48760` | Service bind address and default RPC target used by desktop app. |
| `CODEXMANAGER_WEB_ADDR` | `localhost:48761` | Service edition Web UI bind address (used by `codexmanager-web` only). |
| `CODEXMANAGER_WEB_ROOT` | `web/` next to executable | Web static assets directory (used by `codexmanager-web` only; not needed when using embedded UI assets). |
| `CODEXMANAGER_WEB_NO_OPEN` | Unset | If set, `codexmanager-web` will not auto-open the browser. |
| `CODEXMANAGER_WEB_NO_SPAWN_SERVICE` | Unset | If set, `codexmanager-web` will not try to auto-spawn `codexmanager-service` from the same directory. |
| `CODEXMANAGER_DB_PATH` | `codexmanager.db` next to executable (Service/Web); desktop auto-sets | SQLite path. Desktop sets `app_data_dir/codexmanager.db`. |
| `CODEXMANAGER_RPC_TOKEN` | Auto-generated random 64-hex string | `/rpc` auth token. Auto-generated if missing, and persisted to `codexmanager.rpc-token` by default for cross-process reuse. |
| `CODEXMANAGER_RPC_TOKEN_FILE` | `codexmanager.rpc-token` next to DB | Custom `/rpc` token file path (relative paths are resolved from DB directory). |
| `CODEXMANAGER_NO_SERVICE` | Unset | If present (any value), desktop app does not auto-start embedded service. |
| `CODEXMANAGER_ISSUER` | `https://auth.openai.com` | OAuth issuer. |
| `CODEXMANAGER_CLIENT_ID` | `app_EMoamEEZ73f0CkXaXp7hrann` | OAuth client id. |
| `CODEXMANAGER_ORIGINATOR` | `codex_cli_rs` | OAuth authorize `originator` value. |
| `CODEXMANAGER_REDIRECT_URI` | `http://localhost:1455/auth/callback` (or dynamic login-server port) | OAuth redirect URI. |
| `CODEXMANAGER_LOGIN_ADDR` | `localhost:1455` | Local login callback listener address. |
| `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` | `false` | Allows non-loopback login callback address when set to `1/true/TRUE/yes/YES`. |
| `CODEXMANAGER_USAGE_BASE_URL` | `https://chatgpt.com` | Base URL for usage requests. |
| `CODEXMANAGER_DISABLE_POLLING` | Unset (polling enabled) | Legacy-compatible switch: if present (any value), disables usage polling thread. |
| `CODEXMANAGER_USAGE_POLLING_ENABLED` | `true` | Global usage-polling switch (`1/true/on/yes` to enable, `0/false/off/no` to disable). If both this and `CODEXMANAGER_DISABLE_POLLING` are present, this one wins. |
| `CODEXMANAGER_USAGE_POLL_INTERVAL_SECS` | `600` | Usage polling interval in seconds, minimum `30`. Invalid values fall back to default. |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED` | `true` | Global gateway-keepalive switch (`1/true/on/yes` to enable, `0/false/off/no` to disable). |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS` | `180` | Gateway keepalive interval in seconds, minimum `30`. |
| `CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED` | `true` | Global token-refresh polling switch (`1/true/on/yes` to enable, `0/false/off/no` to disable). |
| `CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS` | `60` | Token-refresh polling interval in seconds, minimum `10`. |
| `CODEXMANAGER_UPSTREAM_BASE_URL` | `https://chatgpt.com/backend-api/codex` | Primary upstream base URL. Bare ChatGPT host values are normalized to backend-api/codex. |
| `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL` | Auto-inferred | Explicit fallback upstream. If unset and primary is ChatGPT backend, fallback defaults to `https://api.openai.com/v1`. |
| `CODEXMANAGER_UPSTREAM_COOKIE` | Unset | Upstream Cookie, mainly for Cloudflare/WAF challenge scenarios. |
| `CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE` | `0` | Enable header compaction policy: suppress `x-codex-turn-state`/`Conversation_id`/fixed `Openai-Beta`/`Chatgpt-Account-Id` by default to reduce Cloudflare/WAF challenges. Also available in Settings UI. |
| `CODEXMANAGER_ROUTE_STRATEGY` | `ordered` | Gateway account routing strategy: default `ordered` (follow account order, fail over to next on failure); set `balanced`/`round_robin`/`rr` to enable key+model-based balanced round-robin starts. |
| `CODEXMANAGER_UPSTREAM_CONNECT_TIMEOUT_SECS` | `15` | Upstream connect timeout in seconds. |
| `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` | `120000` | Upstream total timeout per request in milliseconds. Set `0` to disable. |
| `CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS` | `300000` | Upstream stream timeout in milliseconds. Set `0` to disable. |
| `CODEXMANAGER_UPSTREAM_PROXY_URL` | Unset | Single proxy URL for OpenAI upstream traffic (for example `http://127.0.0.1:7890`). Empty means direct connection. You can also configure it in Settings -> Gateway Policy -> OpenAI Upstream Proxy. |
| `CODEXMANAGER_PROXY_LIST` | Unset | Upstream proxy pool (max 5 entries, separated by comma/semicolon/newlines). Each account is stably hash-mapped to one proxy to avoid proxy drift. |
| `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS` | `300` | Request-gate wait budget in milliseconds. |
| `CODEXMANAGER_ACCOUNT_MAX_INFLIGHT` | `0` | Per-account soft inflight cap. `0` means unlimited. |
| `CODEXMANAGER_TRACE_BODY_PREVIEW_MAX_BYTES` | `0` | Max bytes for trace body preview. `0` disables body preview. |
| `CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES` | `16777216` | Max accepted request body size for front proxy (16 MiB default). |
| `CODEXMANAGER_HTTP_WORKER_FACTOR` | `4` | Backend worker factor; workers = `max(cpu * factor, worker_min)` (service restart required after runtime change). |
| `CODEXMANAGER_HTTP_WORKER_MIN` | `8` | Minimum backend workers (service restart required after runtime change). |
| `CODEXMANAGER_HTTP_QUEUE_FACTOR` | `4` | Backend queue factor; queue = `max(worker * factor, queue_min)`. |
| `CODEXMANAGER_HTTP_QUEUE_MIN` | `32` | Minimum backend queue size. |

### Advanced Variables (Optional)
| Variable | Default | Description |
|---|---|---|
| `CODEXMANAGER_ACCOUNT_IMPORT_BATCH_SIZE` | `200` | Import batch size for auth.json bulk imports. |
| `CODEXMANAGER_TRACE_QUEUE_CAPACITY` | `2048` | Gateway trace async queue capacity (too small may drop traces; too large may increase memory). |
| `CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR` | `1` | Backend stream worker factor (SSE/long-lived responses; service restart required after runtime change). |
| `CODEXMANAGER_HTTP_STREAM_WORKER_MIN` | `2` | Minimum backend stream workers (service restart required after runtime change). |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_FACTOR` | `2` | Backend stream queue factor. |
| `CODEXMANAGER_HTTP_STREAM_QUEUE_MIN` | `16` | Minimum backend stream queue size. |
| `CODEXMANAGER_POLL_JITTER_SECS` | Unset | Common polling jitter in seconds; can be overridden by module-specific jitter envs. |
| `CODEXMANAGER_POLL_FAILURE_BACKOFF_MAX_SECS` | Unset | Common failure backoff cap in seconds; can be overridden by module-specific backoff envs. |
| `CODEXMANAGER_USAGE_POLL_JITTER_SECS` | `5` | Usage polling jitter in seconds. |
| `CODEXMANAGER_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS` | `1800` | Usage polling failure backoff cap in seconds. |
| `CODEXMANAGER_USAGE_REFRESH_WORKERS` | `4` | Usage refresh worker count (configurable in Settings; service restart required after runtime change). |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_JITTER_SECS` | `5` | Keepalive jitter in seconds. |
| `CODEXMANAGER_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS` | `900` | Keepalive failure backoff cap in seconds. |
| `CODEXMANAGER_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS` | `60` | Dedupe window (seconds) for inserting usage refresh failure events, to avoid spamming the event table on transient failures. |
| `CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT` | `200` | Usage snapshots retained per account (0 disables pruning). |
| `CODEXMANAGER_CANDIDATE_CACHE_TTL_MS` | `500` | Gateway candidate snapshot cache TTL in ms (reduces DB pressure on high-QPS). Set `0` to disable. |
| `CODEXMANAGER_PROMPT_CACHE_TTL_SECS` | `3600` | Prompt cache TTL in seconds. |
| `CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS` | `60` | Prompt cache cleanup interval in seconds. |
| `CODEXMANAGER_PROMPT_CACHE_CAPACITY` | `4096` | Prompt cache capacity (0 disables capacity limit). |
| `CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES` | `131072` | Cap accumulated `output_text` bytes extracted from upstream responses (0 disables limit). |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED` | `true` | Enable candidate health-based P2C (Power of Two Choices) routing. |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW` | `3` | P2C window size in `ordered` mode. |
| `CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW` | `6` | P2C window size in `balanced` mode. |
| `CODEXMANAGER_ROUTE_STATE_TTL_SECS` | `21600` | Route-state TTL in seconds to cap key/model state growth. |
| `CODEXMANAGER_ROUTE_STATE_CAPACITY` | `4096` | Route-state capacity cap. |
| `CODEXMANAGER_UPDATE_REPO` | `qxcnm/Codex-Manager` | GitHub repo (`owner/name`) used by the in-app updater. |
| `CODEXMANAGER_GITHUB_TOKEN` | Unset | GitHub token for in-app one-click update (falls back to `GITHUB_TOKEN`/`GH_TOKEN`). Leaving it unset may hit API rate limits and degrade asset metadata lookup. |

### Release-Script Related Variables
| Variable | Default | Required | Description |
|---|---|---|---|
| `GITHUB_TOKEN` | None | Conditionally required | Required for `rebuild.ps1 -AllPlatforms` when `-GithubToken` is not passed. |
| `GH_TOKEN` | None | Conditionally required | Fallback token variable equivalent to `GITHUB_TOKEN`. |

## Env File Example (next to executable)
```dotenv
# codexmanager.env / CodexManager.env / .env
CODEXMANAGER_SERVICE_ADDR=localhost:48760
CODEXMANAGER_WEB_ADDR=localhost:48761
CODEXMANAGER_UPSTREAM_BASE_URL=https://chatgpt.com/backend-api/codex
CODEXMANAGER_USAGE_POLL_INTERVAL_SECS=600
CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS=180
# Optional: background task global switches
# CODEXMANAGER_USAGE_POLLING_ENABLED=1
# CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED=1
# CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED=1
# Optional: fixed RPC token for external clients
# CODEXMANAGER_RPC_TOKEN=replace_with_your_static_token
```

Notes:
- Env files are loaded **once when the desktop/service/web process starts**. After editing the file, restart the corresponding process for changes to take effect.
- The desktop app persists the service port in local storage; env vars mainly affect the initial default value (to force-reset, change it in UI or clear local storage and relaunch).
- Env-file values only apply to variables that are not already defined in the current process. If you set the same `CODEXMANAGER_*` in system/user env vars, those take precedence.

## Troubleshooting
- OAuth callback failures: check `CODEXMANAGER_LOGIN_ADDR` conflicts, or use manual callback parsing in UI.
- Model list/request blocked by challenge: try `CODEXMANAGER_UPSTREAM_COOKIE` or explicit `CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL`.
- Still blocked by Cloudflare/WAF: enable "Header compaction policy" in Settings, or set `CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE=1`.
- Frequent "Partial data refresh failed, showing available data": auto-refresh now logs these as warnings instead of popping repeated toasts; manual refresh still shows failed task names and one sample error. Check Background Tasks intervals/toggles and service logs first.
- Standalone service/Web: if the run directory is not writable, set `CODEXMANAGER_DB_PATH` to a writable path.
- macOS with a system proxy: ensure loopback requests (`localhost/127.0.0.1`) are `DIRECT`, and use lowercase `localhost:<port>` (for example `localhost:48760`).

## Account Hit Rules 
- In `ordered` mode, gateway candidates are built and attempted by account `sort` ascending (for example `0 -> 1 -> 2 -> 3`).
- This means "try in order", not "always hit account 0". If an earlier account is unavailable/fails, gateway automatically falls through to the next one.
- Common reasons an earlier account is not hit:
  - account status is not `active`
  - token record is missing
  - usage availability check marks it unavailable (for example primary window exhausted or required usage fields missing)
  - account is skipped by cooldown or soft inflight cap
- In `balanced` mode, the start candidate rotates by `Key + model`, so attempts do not necessarily start from the smallest `sort`.
- For diagnosis, check `gateway-trace.log` in the same directory as the database:
  - `CANDIDATE_POOL`: candidate order for this request
  - `CANDIDATE_START` / `CANDIDATE_SKIP`: actual attempt and skip reason
  - `REQUEST_FINAL`: final account selected

## 🤝 Special Thanks
This project references the following open-source project for gateway protocol adaptation and stability hardening ideas:

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)

Related implementation points:
- `crates/service/src/gateway/protocol_adapter/request_mapping.rs`
- `crates/service/src/gateway/upstream/transport.rs`

## Contact
<p align="center">
  <img src="assets/images/group.jpg" alt="Group QR Code" width="280" />
</p>

- Telegram group: <https://t.me/+8o2Eu7GPMIFjNDM1>
- WeChat Official Account: 七线牛马
