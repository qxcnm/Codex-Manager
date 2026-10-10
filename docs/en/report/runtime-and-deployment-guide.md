# Runtime and Deployment Guide

## Scope
- First-time desktop setup
- Standalone Service edition
- Docker deployment
- macOS first-run handling

## Quick start
1. Launch the desktop app and click `Start Service`.
2. Open `Account Management`, add an account, and complete authorization.
3. If callback parsing fails, paste the callback URL and complete it manually.
4. Refresh usage and confirm the account status.

## Connect through ccswitch
If you want to use CodexManager through ccswitch or directly from Codex CLI, keep the platform key, `auth.json`, and `config.toml` aligned:

1. Open `Platform Keys` and create a general-purpose key. If ccswitch / Codex CLI should keep reusing a fixed `OPENAI_API_KEY`, fill `Custom API key`; otherwise leave it empty to generate one automatically.
2. In ccswitch, create a provider and paste the generated or custom key into the provider API key field.
3. Write the same key into Codex CLI's `auth.json`. Do not put account `access_token`, `refresh_token`, or OpenAI login tokens here.
4. Copy the sample `config.toml` below into the ccswitch / Codex config, then restart Codex CLI.

Common paths:

- macOS / Linux: `~/.codex/auth.json`, `~/.codex/config.toml`
- Windows: `%USERPROFILE%\.codex\auth.json`, `%USERPROFILE%\.codex\config.toml`

Example `auth.json`:

```json
{
  "OPENAI_API_KEY": "replace_with_codexmanager_platform_key",
  "auth_mode": "apikey"
}
```

If you used a custom platform key, `OPENAI_API_KEY` must be that same custom value.

Example `config.toml`:

```toml
model = "gpt-6-sol"
model_provider = "cm"
review_model = "gpt-6-sol"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
cli_auth_credentials_store = "file"
service_tier = "fast"

[model_providers.cm]
name = "OpenAI"
base_url = "http://localhost:48760/v1"
wire_api = "responses"
supports_websockets = true
```

- If you changed the service port in Settings, update `base_url` accordingly.
- Restart Codex CLI after changing `auth.json` or `config.toml`.

## Import and export
- `Batch import`: select multiple `.json/.txt` files and import them together.
- `Import by folder`: desktop only. After selecting a directory, the app recursively scans `.json` files and imports them in batches. Empty files are skipped automatically.
- `Export users`: after selecting a directory, click `One JSON file per account` to export for backup or migration.

## Service edition
1. Download `CodexManager-service-<platform>.zip` from the Release page and extract it.
2. We recommend starting `codexmanager-start`. It launches service + web together and can be stopped with `Ctrl+C`.
3. You can also start only `codexmanager-web`; it automatically launches `codexmanager-service` from the same directory and opens the browser.
4. Or start `codexmanager-service` first, then `codexmanager-web`.
5. Default addresses: service `localhost:48760`, Web UI `http://localhost:48761/`.
6. To stop everything, visit `http://localhost:48761/__quit`. If the web process launched the service automatically, it will try to stop both.
7. If you reverse-proxy or split-deploy frontend assets yourself, you must forward both `/api/runtime` and `/api/rpc`. Serving static assets alone is not enough.

### Run the source build with embedded UI

For source-based local runs, use:

```bash
./scripts/run-service-app.sh
```

The script builds the frontend with `pnpm -C apps run build:desktop`, then compiles `codexmanager-service`, `codexmanager-web`, and `codexmanager-start`. `codexmanager-web` embeds `apps/out` by default, so you no longer need to pass `CODEXMANAGER_WEB_ROOT` for normal local runs.

Useful flags:

- `--debug`: use a debug Rust build for faster local iteration.
- `--clean-dist`: remove `apps/out` before building.
- `--no-open`: do not open the browser automatically.

You can still set `CODEXMANAGER_WEB_ROOT=/path/to/out` when you intentionally want to override the embedded UI with an external static directory.

## Docker deployment

Docker images default to `TZ=Asia/Shanghai`, and compose examples use `TZ=${TZ:-Asia/Shanghai}`: if the deployment environment already sets `TZ`, compose passes it through; otherwise it falls back to `Asia/Shanghai`. If you deploy in another region, set `TZ` or change it under `environment` to your own IANA time zone, for example `Europe/London` or `America/Los_Angeles`.

### GitHub Packages / GHCR
- After a Release is published, both `codexmanager-service` and `codexmanager-web` images are pushed to GitHub Packages (GHCR).
- Stable releases update the versioned, `stable`, and `latest` tags. Prereleases only publish their versioned tag and never replace the floating tags.
- Use `stable` for automatic updates, or pin a release such as `docker pull ghcr.io/qxcnm/codexmanager-service:v0.6.0`.
- [`docker/docker-compose.release.yml`](../../../docker/docker-compose.release.yml) defaults to `stable`. Set `CODEXMANAGER_RELEASE_TAG` only when pinning a version.
- Example: `CODEXMANAGER_RELEASE_TAG=v0.6.0 docker compose -f docker/docker-compose.release.yml up -d`
- `registry.cn-hangzhou.aliyuncs.com/kilimiao/codex-manager` is not part of the current official release workflow and may be outdated. The GHCR `service` and `web` images above are the maintained prebuilt images.

### Optional Docker OTA updates

- The official Release Compose file includes an opt-in Watchtower OTA checker. Keep `CODEXMANAGER_RELEASE_TAG` on the floating `stable` tag (or another moving tag), then run:

  ```bash
  docker compose -f docker/docker-compose.release.yml --profile ota up -d
  ```

- Watchtower checks the labeled CodexManager service and web images every 300 seconds, but the `ota` profile only reports an available update. Set `CODEXMANAGER_OTA_INTERVAL=60` to change the interval in seconds, and inspect it with `docker compose -f docker/docker-compose.release.yml logs -f codexmanager-ota`.
- After the user confirms the update, run this one-shot command to pull the new images and perform a rolling restart:

  ```bash
  docker compose -f docker/docker-compose.release.yml --profile ota run --rm --no-deps codexmanager-ota \
    --label-enable --run-once --cleanup --rolling-restart --stop-timeout 20s
  ```

- If the user explicitly consents to automatic checking and upgrading, enable the `ota-auto` profile:

  ```bash
  docker compose -f docker/docker-compose.release.yml --profile ota-auto up -d
  ```

- A pinned tag such as `CODEXMANAGER_RELEASE_TAG=v0.6.2` will not move between releases; switch back to `stable` to follow official releases. The OTA sidecar needs access to the Docker socket, so enable it only on hosts you trust.
- Service restarts reuse the same `/data` volume and run the automatic migration path. Keep that volume in place so migration locking, online backup, smoke checks, and failure recovery can work during an upgrade.

### Method 1: `docker compose`
```bash
docker compose -f docker/docker-compose.yml up --build
```

Then open: `http://localhost:48761/`

### Method 2: build and run separately
```bash
# service
docker build -f docker/Dockerfile.service -t codexmanager-service .
docker network create codexmanager-net

docker run --rm --name codexmanager-service \
  --network codexmanager-net \
  --network-alias codexmanager-service \
  -p 48760:48760 \
  -v codexmanager-data:/data \
  -e TZ=Asia/Shanghai \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-service

# web (containers should talk over the Docker network, not the host-mapped port)
docker build -f docker/Dockerfile.web -t codexmanager-web .
docker run --rm --name codexmanager-web \
  --network codexmanager-net \
  -p 48761:48761 \
  -v codexmanager-data:/data \
  -e TZ=Asia/Shanghai \
  -e CODEXMANAGER_WEB_NO_SPAWN_SERVICE=1 \
  -e CODEXMANAGER_SERVICE_ADDR=codexmanager-service:48760 \
  -e CODEXMANAGER_RPC_TOKEN=replace_with_your_token \
  codexmanager-web
```

- If you want the Web password, settings, cached model list, and other runtime state to stay consistent with the service, `codexmanager-web` and `codexmanager-service` must share the same `/data` volume.
- If you must reach the host-mapped port from the container, add `--add-host=host.docker.internal:host-gateway` on Linux; otherwise `host.docker.internal` often does not resolve.

## macOS first launch
- The current macOS release artifacts are not notarized with an Apple Developer account, so Gatekeeper may show `Corrupted` or refuse to open them the first time.
- The macOS `dmg` package includes `Open CodexManager.command` and `README-macOS-first-launch.txt`.
- We recommend dragging `CodexManager.app` into `Applications` first, then double-clicking the helper script.
- You can also run:

```bash
xattr -dr com.apple.quarantine /Applications/CodexManager.app
```

- If it is still blocked, try `Right click -> Open` on `CodexManager.app` again.

## Related documents
- Environment and runtime configuration: [Environment and Runtime Configuration](environment-and-runtime-config.md)
- Minimal troubleshooting guide: [Minimal Troubleshooting Guide](minimal-troubleshooting-guide.md)
- Nginx reverse proxy cache issue: [Nginx Reverse Proxy Cache Hit Issue](nginx-reverse-proxy-cache-hit-issue.md)
