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
To use CodexManager through ccswitch or Codex CLI, keep Codex's existing ChatGPT or API key authentication and put the platform key only in `config.toml`. There is no need to switch sign-in methods:

1. Confirm the target Codex profile. With a custom `CODEX_HOME`, Codex CLI and CodexManager must use the same directory.
2. Open `Platform Keys` and create a general-purpose key. For a fixed key, fill `Custom API key`; otherwise leave it empty to generate one.
3. Apply `Use CodexManager` on the Codex Connection page, or use ccswitch to update only `config.toml` as shown below. Put the platform key in `experimental_bearer_token` and preserve the existing authentication settings.
4. Restart Codex CLI after applying the configuration.

Common paths:

- macOS / Linux: `~/.codex/auth.json`, `~/.codex/config.toml`
- Windows: `%USERPROFILE%\.codex\auth.json`, `%USERPROFILE%\.codex\config.toml`

- Account connections write `auth.json` to switch to the selected account. Direct aggregate API connections update only `config.toml`, write the selected aggregate API key to the provider field `experimental_bearer_token`, and preserve any existing `auth.json`.
- Gateway setup preserves `auth.json` byte for byte and puts the platform key only in `config.toml`. Starting CodexManager does not write `auth.json` either.
- Both ChatGPT and API key authentication (`auth_mode = "chatgpt"` or `"apikey"`) are supported. Gateway setup does not require `codex login`, check ChatGPT login tokens, or block configuration writes when `auth.json` is missing. It does not create an auth file.
- The gateway provider always uses `name = "cm"` and `requires_openai_auth = true`; there is no switch to remove this field.

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
name = "cm"
base_url = "http://localhost:48760/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "replace_with_codexmanager_platform_key"
supports_websockets = true
```

- If you changed the service port in Settings, update `base_url` accordingly.
- In Docker / Web mode, use the browser-accessible Web address plus `/v1`, such as `http://10.10.121.253:48761/v1`, rather than the container's internal service address.
- `supports_websockets = true` is available for platform keys using the OpenAI-compatible protocol at `/v1/responses`. Account-pool routes to the official Codex upstream use native WebSocket. Aggregate APIs, hybrid routes, and non-official upstreams use CM's HTTP/SSE compatibility bridge: the client still uses WebSocket, while the upstream follows the existing HTTP routing and protocol adapters. The bridge does not add model or protocol capabilities that the selected upstream lacks.
- Every bridged generation request passes through the ordinary gateway's authentication, model and route checks, concurrency limits, and usage accounting. In request logs, `ws` identifies the client's WebSocket transport; `method=POST` identifies the actual internal HTTP request method for a bridged turn. It does not mean the client fell back to HTTP.
- The Responses endpoint can adapt an aggregate Chat Completions action while preserving text, function calls, tool results, and usage. A Messages endpoint routed directly to a Chat Completions action is outside this release's supported combinations and returns an explicit error; configure a Messages-compatible action or use the Responses endpoint. Successful WS bridge delivery requires both consumption of the internal HTTP terminal and a successful WebSocket terminal send; genuine disconnects remain failures.
- The HTTP/SSE bridge retains completed context within the same WebSocket connection, subject to cache limits, and expands `previous_response_id` continuations into complete HTTP input. An ID unavailable on a new connection or after cache overflow or eviction returns a context recovery error. The client must remove that ID and send the complete `input` instead of repeatedly retrying the old ID.
- In bridge mode, `generate:false` warmup returns empty output and zero usage locally, without submitting generation to the upstream. Actual generation still performs the full gateway checks.
- Native WebSocket forwards generation interrupts to an upstream that supports them. An upstream `response.incomplete` with reason `interrupted` records normal interruption usage and retains completed context. The HTTP/SSE bridge instead cancels the active HTTP request, returns an interrupt acknowledgement and incomplete terminal, and retains only completed items already sent to the client for a subsequent turn on the same connection. Closing the client connection also cancels the active request.
- The bridge does not automatically replay a submitted generation after interruption or disconnection. Cancellation does not establish that the upstream did no work or consumed no quota; the normal gateway still records usage already received, and genuine disconnects and upstream failures retain their error semantics.
- If the WS handshake returns `426`, check the version and platform-key protocol of the service reached by the actual `base_url`. The older error `responses websocket is only available for official Codex upstream` means that service lacks the compatibility bridge and needs updating. Increasing reconnect attempts does not change service capabilities.

### Account still displayed, or request logs show 499

Connecting through CodexManager preserves Codex's sign-in state, so Codex may still display the existing ChatGPT account. Model requests use the selected provider's `base_url` and platform key. The account label alone does not establish that requests bypass the gateway, and there is no need to delete `auth.json` or sign out for this reason.

Confirm that Codex and CodexManager use the same `CODEX_HOME`, restart Codex after applying the configuration, and correlate requests by time in the gateway logs. A request recorded by CodexManager has reached the gateway; its upstream address identifies the route selected by the gateway.

Status `499` with `response write failed: broken pipe` or `downstream HTTP body closed` means the downstream client closed the response connection. Possible causes include stopping a turn, changing sessions, or a network disconnect; this is different from a `401/403` authentication failure. If Codex also reports an error, retain that turn's client error, timestamp, and matching gateway record to distinguish cancellation, network issues, and upstream failures. A screenshot showing only `499` cannot establish why the client disconnected.

If a successful Responses HTTP `response.completed` contains complete assistant text but the upstream omitted `response.output_item.done`, Codex can finish without displaying that text. CM inserts the missing message-done event before completion for explicitly completed assistant text, without duplicating items whose done event was already sent. Tools, images, failed responses, and incomplete responses are not repaired. Added events have a cumulative 1 MiB limit per request, and item tracking is bounded; exceeding either limit preserves the original stream without that repair. This compatibility fix does not change the meaning of `499` or resolve every downstream disconnect.

Responses HTTP streams end after the complete response terminal event has been handed to the HTTP body consumer. CM no longer waits for the upstream connection to close or forwards heartbeats and trailing frames after that event. A missing SSE terminal delimiter is completed so clients can parse the final event before EOF. This prevents normal Codex stream completion from being recorded as `499`. Parsing a terminal event or queueing only part of it is insufficient: a disconnect before its full delivery remains a failure, and `response.failed` / `response.incomplete` are never repaired into fully successful responses. Output-limit responses retain usable partial text and usage and map to `length` / `max_tokens` for Chat / Messages clients; truncated tool arguments and other incomplete states that cannot be represented retain their errors.

Status `499` does not establish that the upstream did no work or consumed no quota. When investigating retries after an interrupted generation, count all requests from the start of the operation through its eventual success. The gateway list stores one final record per client request; check that request's upstream attempts separately in its details.

The account's five-hour quota comes from the official usage endpoint, not a conversion of local request-log tokens. When upstream usage is unavailable, the gateway estimates input tokens as roughly one quarter of the request JSON's character count. This includes request structure overhead and cannot establish actual model usage or official quota consumption. Nonzero cached tokens come from upstream usage and are already included in input tokens; do not add them again. When investigating a quota drop across failed attempts and eventual success, correlate usage refresh times for the same account, client retry records, and each request's trace ID, upstream attempt count, and usage source. The final success record or number of `499` records alone cannot determine additional consumption.

To avoid duplicate submissions, if a connection closes or response headers time out while establishing the upstream HTTP response and delivery is uncertain, CM stops automatic retries, account failover, and fallback routes within that request, and reports that the upstream may already have processed it. Failures known to occur before connection establishment remain recoverable, as do existing recovery flows for explicit rejection responses. A subsequent client retry is a new request and may resend the full context; this protection does not guarantee that failed requests consume no official quota.

### Model and route ordering

Administrators can drag the handle in the **Order** column, click the up/down buttons, or focus a handle and use the arrow keys. Model ordering saves automatically. Searches and filters reorder only the visible slots, retaining hidden and nonmatching models in place. Stale submissions from concurrent windows or catalog additions/deletions are rejected and the catalog is reloaded.

Display ordering is stored separately from model ceilings, pricing, enabled state, and routes. The editor's **Model rank (advanced)** field retains its existing meaning for model ceilings and is separate from display order. Service model lists and the CodexManager local catalog use the saved display order; official catalogs retain their own order. Use **Apply models** again to update a selected local catalog already applied to Codex.

The model editor's **Routes** tab also supports handles and arrows. An explicit move renumbers the entire route list with higher priority for earlier routes, committed by **Save model**. Without a move, existing priorities and equal-priority weights remain intact. Closing without saving does not submit route changes.

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
