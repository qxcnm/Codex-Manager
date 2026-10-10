# Changelog

This file records externally visible changes to CodexManager and serves as the single source of truth for version history.
It follows Keep a Changelog with a lightweight adaptation for this repository.

## [Unreleased]

## [0.6.3] - 2026-09-30

### Added

- Added Auto-detect to the interface language selector in Settings and the header. It follows supported system/browser languages and falls back to English; fixed language choices remain saved.
- Added built-in `gpt-6.1-sol` in model catalog revision 10 with its official description, reasoning levels, and short/long-context input, cached-input, cache-write, and output prices. Custom models, user edits, and deletion tombstones are preserved.

### Changed

- Refined desktop navigation, settings, dashboard, platform-mode, and model-management layouts for denser scanning and clearer state presentation across supported locales.

### Fixed

- Linux packaging now checks and repairs missing `.DirIcon` metadata in final AppImages, resolving the AppImageHub catalog packaging failure.
- Improved streaming terminal-delivery tracking and bridge diagnostics so responses that already delivered a terminal event are distinguished from upstream-incomplete or client-delivery failures.

## [0.6.2] - 2026-09-25

### Added

- Added model catalog revision 9 with `gpt-6-sol`, `gpt-6-luna`, `gpt-image-2.5-sunburst`, and `gpt-image-2.5-flare`. A fresh catalog now contains 11 built-ins: 10 visible models and the hidden `codex-auto-review` entry.

### Changed

- Removed `gpt-5.4` and `gpt-5.4-mini`, which retired from Codex with ChatGPT sign-in on August 31, 2026, plus the `gpt-5.2` built-in deprecated for that sign-in path. General API-key access to `gpt-5.2` remains available through a custom model. Revision 9 deletes untouched built-ins and their default account-pool routes, while user-edited models, custom prices or tiers, non-default routes, and permission/API-key associations are preserved as custom models.
- Rebuilt the Free account model ceiling choices from the current text-model catalog. Persisted values no longer eligible for that catalog, image-only values, and legacy fallbacks are normalized to `auto`; account warmup and account text tests now fall back to `gpt-6-luna`, while the observation bridge defaults to `gpt-6-sol` when no model is supplied.
- Price sync still preserves custom prices when no models are selected; explicitly selected models may have their custom prices replaced by external prices, while unselected custom models remain untouched.
- Bumped the release to `0.6.2` and synchronized the workspace, frontend, Tauri desktop metadata, and lockfiles.

### Fixed

- Fixed pnpm command resolution and the Next proxy startup race in desktop development; hidden pages now stop page-level polling and background work, while account-test and SSE listeners are cleaned up at terminal states.
- Fixed resource lifetime, request cancellation, queue metrics, and concurrency edges across service, gateway, account, and usage paths, reducing legacy synchronous bridge work on production paths.
- Hardened automatic migration of legacy SQLite model catalogs with a migration lock, online backup, preflight checks, case-conflicting slug handling, and a post-migration smoke check; failed migrations restore the backup before returning an error.
- Added optional Docker OTA profiles to the GHCR Release Compose file; the default checker waits for user confirmation, while `ota-auto` updates labeled service/web images only after explicit opt-in, and pinned version tags remain fixed.

## [0.6.1] - 2026-09-24

### Added

- Added quota reset warmup for OpenAI accounts, enabled by default. Exhausted 5-hour windows trigger a short message at their recorded reset time, followed by a usage refresh. Per-account and bulk switches are persisted, attempts are deduplicated across restarts, and disabled accounts or exhausted weekly quotas are skipped.
- Added direct aggregate API access for Codex. Active Codex or Compatible Responses API-key entries can be selected with their configured authentication headers and User-Agent, without routing through the local gateway (#475).

### Changed

- Completed the production hardening for the asynchronous service and SeaORM storage migration, including transaction, shutdown, and acceptance gates across remote accounts, aggregate APIs, API keys, usage, logs, and gateway paths.
- Unified model status-card styling. Applying models uses the full catalog when nothing is selected, limits the operation to selected models otherwise, and synchronizes pricing and catalog metadata.
- Model deletion now removes records. Deleted built-in models receive a tombstone so startup seeding does not restore them.
- Bumped the workspace, frontend, Tauri desktop metadata, and lockfiles to `0.6.1`.

### Fixed

- Fixed successful aggregate API responses being reported as 502 when a Windows client disconnected. Downstream `os error 10053/10054/10058` conditions are now treated as client disconnects (#479).
- Fixed account, aggregate API, quota-reset, and candidate-routing edge cases after the asynchronous service migration, with stronger transaction rollback and production configuration checks.

## [0.6.0] - 2026-09-05

### Fixed

- Fixed a regression where a newly added subscription User-Agent triggered web protection, interrupted quota refresh, and surfaced a subsequent token authorization error. Subscription requests no longer set User-Agent.
- Restored the standard Codex User-Agent for quota queries and token refresh, independent of custom gateway overrides, including default and account-specific proxy routes.

### Changed

- Bumped the workspace, frontend, Tauri desktop metadata, and lockfiles to `0.6.0`, retaining existing account and settings storage.
- `release-all` now builds service and desktop artifacts concurrently across five platform targets while preserving Desktop, Service, Web, and Docker assets. Caches are separated by Cargo workspace; dispatching a pinned release commit from the default branch enables cache reuse across versions.

## [0.5.7] - 2026-09-05

### Added

- Added a **Keep using account after quota exhaustion** switch to the account editor. It persists as `force_enabled`, is off by default, and keeps the account in the gateway candidate pool for manual quota handling.
- Added `gpt-6-astra` to the default model catalog. Revision 8 seeds fresh and upgraded databases with its official API limits and cache-write-aware pricing, Codex runtime metadata, and the default account-pool route, while preserving same-slug custom models and user-edited builtins.
- Added a remembered table/card view switch to the accounts page. Administrators can fetch upstream models for one explicitly selected ChatGPT account using that account's credentials, account identifier, and proxy, then selectively associate them with model catalog V2 account-pool routes without account-pool rotation.
- Added Standard and Ultrafast API-key service tiers across Responses HTTP, WebSocket, and request logs. Standard uses the upstream default speed and is logged as `standard`, Fast maps to `priority`, and Ultrafast passes through unchanged; Flex remains accepted for compatibility, while actual availability still depends on the model and upstream.
- Added configurable outbound gateway `User-Agent` handling: `gateway.user_agent` applies globally, per-connection `aggregate_apis.user_agent` takes priority for aggregate route forwarding, model discovery, probes, and balance requests, and unset values fall back to the dynamically generated Codex-compatible `User-Agent` (#459).

### Changed

- Bumped the release version to `0.5.7` and synchronized the workspace, frontend package, Tauri desktop metadata, and lockfiles.

### Fixed

- Fixed Responses WebSocket tool-output continuation: duplicate outputs for the same tool call are removed before forwarding, connection preamble events no longer consume terminal-recovery opportunities, and missing tool-call context can be restored within bounded limits across task boundaries (#458).
- Fixed Luna Reserve usage disappearing from the account page after later refreshes or transient empty-list responses, and normalized both snake_case and camelCase additional-quota payloads.
- Fixed the Luna Reserve routing boundary: only explicit `gpt-reserve` requests place accounts with usable Reserve quota in the Reserve candidate pool. Ordinary `gpt-5.6-luna` requests continue to use the regular account pool and cannot bypass exhausted standard 5-hour/7-day windows merely because Reserve remains available; hard authorization or deactivation states retain their existing handling.

## [0.5.6] - 2026-09-02

### Added

- Added an administrator-only **Test account** workflow that sends real text or image-model requests upstream and streams progress, results, and cancellation through Tauri events or Web SSE isolated by an unguessable `testId`. Account status is written back with a `status + updated_at` CAS so stale tests cannot overwrite newer state, and `banned`, `disabled`, or `inactive` accounts are never restored automatically (#452).
- Added **Hybrid rotation (aggregate API first)**. Aggregate APIs are tried first and the request falls back to the account pool only after every candidate fails without consuming the request; aggregate-only models cannot bypass their bound route (#454).
- Added upstream model discovery and selective association from aggregate APIs into model catalog V2 routes without restoring the legacy source tables or automatic association pipeline.

### Changed

- Bumped the release version to `0.5.6` and synchronized the workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Improved prompt-cache/cache-affinity routing by partitioning `prompt_cache_key` or root session IDs by platform key, protocol, and model, atomically converging concurrent cold starts, reusing one account across parent/child and streaming requests, and rebinding only after successful failover. The administrator dashboard now also shows cache hit rate.

### Fixed

- Fixed false failures when testing aggregate APIs whose suppliers enforce strict client validation. Probes now emulate the official Codex client identity by default or use a configured custom `User-Agent`, without changing normal route forwarding (#451).
- Fixed Codex Device Code login lifecycle, cancellation, and browser-callback races, and synchronized managed gateway-profile authentication and WebSocket capability.
- Fixed detection and routing of native Codex image-generation requests. Extension requests can reach the Codex image endpoint with the required identity headers, while ordinary OpenAI-compatible clients retain the compatibility-adapter path.
- The desktop updater now removes completed or stale release directories while preserving a pending release and updater logs.

## [0.5.5] - 2026-08-23

### Fixed

- Fixed a brief period where the service was unavailable when restoring the desktop window after startup-with-main-window was disabled.
- Streamlined startup and restore by coalescing window navigation, waiting for the home page before showing the window, reusing an initialized service first, and removing duplicate warmup requests.

## [0.5.4] - 2026-08-23

### Changed

- Bumped the release version to `0.5.4` and synchronized the workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Aligned Responses WebSocket behavior with the official semantics: bounded heartbeats and recovery, connection-limit continuation, large image frames, compression-negotiation fallback, safe first-frame/preamble replay, and account reselection while preserving HTTP fallback after the recovery budget is exhausted.
- Improved CI reliability for Cargo Git dependencies with CLI fetches, network retries, locked dependency prefetching, and caching without retrying real build or test failures.
- Updated AIXiamo sponsor entry points, tutorial links, and related copy.

### Fixed

- Fixed unreliable desktop main-window initialization, refresh, and automatic service startup when the launch-with-main-window option is disabled.
- Fixed direct OpenAI account switching when historical conversations still reference the `cm` provider. Direct mode now explicitly uses the built-in `openai` provider and avoids scanning and rewriting the entire Codex history directory during the switch.
- Added Codex 0.149 compatibility for the managed gateway by ensuring the `cm` provider sends `requires_openai_auth`, avoiding missing-API-key requests.
- Fixed Responses WebSocket recovery for early disconnects, connection limits, failed first frames, preamble continuation, `store=false` context, and changing account eligibility; requests are never silently duplicated after substantive output has been sent.
- Fixed account failover for some non-success upstream responses and corrected default model-group routing semantics.

## [0.5.3] - 2026-08-08

### Fixed

- Fixed same-team account logins overwriting an existing account by isolating persisted account identity by login subject.
- Limited retryable streaming upstream failures to one retry per account before failover, reducing rate-limit amplification.

## [0.5.2] - 2026-07-31

### Changed

- Bumped the release version to `0.5.2` and synchronized the workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Added cache-write token pricing to the model catalog and request billing, including cache-write usage extraction from Anthropic, Gemini, and OpenAI-style responses.
- Increased wide-screen sidebar navigation spacing while retaining the compact layout for short viewports to prevent overflow.

### Fixed

- Excluded non-`2xx` failed requests from token and estimated-cost rollups. Client-cancelled `499` requests can still debit the wallet when upstream work may already have incurred a cost, but remain excluded from successful usage totals.

## [0.5.1] - 2026-07-26

### Added

- Restored the **Free account model ceiling** setting with `auto` (no limit) and concrete model-catalog choices.

### Changed

- Bumped the release version to `0.5.1` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Cleaned up low-risk Rust warnings and suppressed only the `linker_messages` noise produced by normal `link.exe` library output on Windows MSVC targets; other compiler warnings remain visible.

### Fixed

- The Free account model ceiling now filters only Free-account candidates above the selected ceiling without rewriting the requested model. Plus and Pro accounts are unaffected, quota protection still runs after filtering, and account-first mixed mode retains its aggregate API fallback after account candidates are exhausted.

## [0.5.0] - 2026-07-23

### Added

- Added `chatgpt.com` Device Code account authorization with code display/copy, verification-page launch, automatic polling, cancellation, and expiry handling; browser authorization and manual callback parsing remain available (#370).
- `/skills/` now separates **Skills Installation** from **Codex Plugin Installation**. Individual Skills can be installed from built-in or custom GitHub repositories, skills.sh search results, ZIPs, and existing directories, with background repository refresh and safe uninstall; the native Codex Marketplace remains available for complete standard-plugin installation, and `.system` skills stay read-only (#126).
- Added `gpt-image-2` (snapshot `gpt-image-2-2026-04-21`) as the ninth builtin and eighth normally visible catalog model. The entry is limited to `/v1/images/generations` and `/v1/images/edits`, is not exposed as a text, Chat Completions, or direct Responses main model, and stores the official $8 / $2 / $30 per-1M image input / cached input / output token rates as catalog metadata.

### Changed

- Bumped the release version to `0.5.0` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Cleaned up frontend lint errors and warnings so the pre-release quality checks run cleanly.

### Fixed

- Fixed hybrid rotation sending aggregate-only model routes to the local account pool first. Aggregate-only models now go directly to their bound aggregate API, dual-routed models keep account-first fallback behavior, and account-only models no longer fall back to an unbound aggregate API (#381).
- Fixed the Windows tray preview not reliably applying its latest dimensions. Newly created and reused windows now reapply the fixed `360 × 430` logical size and constraints to prevent clipped content or stale window sizing (#380).
- Fixed Responses WebSocket failover when an upstream sends `response.created` before reporting quota exhaustion in a nested `response.failed` event. Content-free lifecycle events are now buffered, nested errors are recognized, exhausted accounts are marked limited immediately, and the current request is replayed silently on the next available account.
- Added a `compatible` aggregate API provider type so one upstream URL and key can serve both Codex/OpenAI and native Claude requests using the incoming request path, without duplicate provider records (#359).
- Fixed 401 responses for accounts imported with only a ChatGPT `/api/auth/session` `accessToken`. HTTP, Responses WebSocket, and account warmup paths now register and persist missing AgentIdentity material on demand, validate the account binding, use AgentAssertion, and accept the official nested snake_case/camelCase AgentIdentity shapes (#376).
- Fixed `/api/auth/session` accounts appearing as generic imported-account placeholders. Display names now resolve from the access-token OpenAI profile or session user data, while preserving manually edited names and upgrading legacy placeholder labels.
- Corrected GPT-5.6 Sol, Terra, and Luna billing to match official OpenAI API pricing: cached input now receives the 90% discount, and requests switch at the 272K input threshold to 2x input and 1.5x output long-context rates. Legacy automatic estimates migrate to official pricing while custom prices stay unchanged.

## [0.4.4] - 2026-07-20

### Changed

- Bumped the release version to `0.4.4` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Added `v*` tag-push publishing support to the `release-all` workflow so a release can be started directly by pushing the version tag.

### Fixed

- Fixed account-pool “refresh usage” reporting success when the selected account had no token or no refreshable target. The service now returns a structured run result, and the frontend distinguishes skipped, partial, and successful refreshes while refreshing account and usage caches.
- Fixed background task changes taking effect only after the previous sleep cycle finished. Usage polling, gateway keepalive, and token refresh polling now re-read dynamic configuration while waiting when an interval is shortened or a task is disabled.

## [0.4.3] - 2026-07-19

### Added

- Added account-level proxy management (#314): create reusable proxy profiles, assign one to an individual account, and use it for account requests, token refresh, and usage refresh flows.
- Added proxy connectivity, latency, download, and upload tests with unified runtime status, egress IP, country/region, and speed results in both desktop and Web service modes.
- Added multi-select bulk route assignment to the model catalog. Account-pool or Aggregate API routes can be added/updated across models or replace all existing routes, with each upstream model name derived from its model slug.

### Changed

- Shortened the AIXiamo sponsor copy to highlight ChatGPT, Claude, and Codex top-up assistance for users in China without an international bank card, together with after-sales support.
- Tightened Codex gateway detection and now prefer the current provider gateway URL, avoiding false Codex classification for ordinary OpenAI-compatible endpoints while preserving compact, Gemini, and multi-candidate fallback behavior (#346).
- Improved protocol compatibility for native Web Search, Image Generation, `/v1/models`, and Responses WebSocket traffic, including bounded decoding of zstd-encoded content (#363).
- Bumped the release version to `0.4.3` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

### Fixed

- The bulk route assignment entry is now always visible to administrators on the model management page; it stays disabled with clear guidance until models are selected, so the feature is no longer easy to miss.
- Fixed candidate failover after some Responses 400 errors by dropping inapplicable session/thread headers on retry and preserving encrypted content containers correctly.
- Fixed hosted image tool conflicts with Codex image-generation namespaces and narrowed legacy Responses path suppression to its intended scope.
- Fixed the Windows tray context menu closing immediately, click-time refresh regressions, and stale menu labels after usage updates (#362).
- Fixed blurry header badge text under Windows glass effects (#360) and white fills on the sidebar logo and selected navigation icons (#361).
- Completed Simplified Chinese localization for model-catalog metrics, filters, headers, source, price/instruction/route states, and built-in model descriptions instead of mixing English into the Chinese UI.
- Restored release validation after the merged features by fixing release compilation, frontend lint/effect issues, ambiguous end-to-end selectors, and Service parallel-test isolation.

## [0.4.2] - 2026-07-19

### Changed

- Combined today's total, cached-input, and reasoning-output usage in the administrator dashboard metric card.
- Bumped the release version to `0.4.2` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

### Fixed

- Fixed cleared request logs reappearing by hiding billed entries from operational log queries while preserving immutable usage and billing audit data.
- Corrected Aggregate API Responses probes to send `input_text` content, retain upstream error details, and expose failed-test details in the UI.
- Fixed `codexmanager-web` downgrading `/v1/responses` WebSocket handshakes to ordinary GET requests that returned 405. The Web gateway now tunnels WebSocket frames while preserving authentication and Codex headers, and managed/onboarding provider configs explicitly enable Responses WebSocket.
- Replaced the README Star History remote chart embeds with repository-hosted images so the curves remain visible.

## [0.4.1] - 2026-07-14

### Changed

- Cut over the model catalog to V2: fresh databases seed eight builtin models (seven in the normal list), while models, integer price tiers, routes, permission groups, and instructions policies are saved in one transaction.
- Gateway routing, local `/v1/models`, model groups, and model selectors now read V2 only; remote merge/prune, supplier `/models` discovery, request-time bootstrap, and compiled price fallbacks were removed.
- Added local JSON preview/commit plus price, route, and instructions editing. Builtins can only be disabled; custom models can be deleted.
- `models_cache.json` is now exported only by an explicit desktop or Web action and never carries a model personality prompt.
- Codex cache exports now preserve GPT-5.6 Max/Ultra plus `multi_agent_version`, `tool_mode`, Responses Lite, maximum context, comp hash, and related runtime metadata. Explicit Ultra requests are logged as client Ultra and upstream Max.
- Wallet charging now records integer price tiers, immutable charge snapshots, and idempotent ledger updates in one transaction.
- Added default GPT-5.6 Sol, Terra, and Luna input/output prices of 5/30, 2.5/15, and 1/6 USD per 1M tokens. Cached input is conservatively charged at the input rate, and migration only fills unedited builtins that still have missing prices.
- Bumped the release version to `0.4.1` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

### Fixed

- Fixed desktop startup refusing to continue with SQLite foreign-key error 787 when a legacy V2 builtin used an underscore ID.
- Fixed the model catalog editor opening existing models with an empty draft, and completed localized field labels, control titles, and accessible names in the Chinese UI.
- Fixed administrator deep links being reset to the home page during desktop session initialization.
- Isolated Codex request rewrites per upstream candidate so mutations cannot leak between candidate request bodies.

## [0.4.0] - 2026-06-24

### Changed
- Simplified the Codex first-time setup guide to focus on the minimum `auth.json` and `config.toml` configuration.
- Bumped the release version to `0.4.0` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.3.9] - 2026-06-11

### Added
- Added an Unknown status cleanup option to account cleanup by status.
- Disabled Aggregate APIs now retain discovered model pools and model mappings so they can be reused when re-enabled.

### Fixed
- Covered the newly imported account path in usage-refresh unavailable status handling.
- Stabilized same-session `prompt_cache_key` behavior so the same session does not receive a new cache key.
- Covered refresh-token invalidation cleanup after a session ends.
- Changed `CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL` default to `0`, so normal Responses requests no longer auto-inject the `image_generation` tool by default.

### Changed
- Improved upstream relay compatibility observability.
- Bumped the release version to `0.3.9` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.3.8] - 2026-05-30

### Fixed
- Fixed local prompt-cache account binding not being reused when a proxy preserved only `x-codex-turn-state` while the request still carried `prompt_cache_key`, and added `anchor_mode` diagnostics.
- Fixed `/v1/messages/count_tokens` requests potentially being forwarded as real Responses generations by using local token estimation.
- Hardened Anthropic-native and Claude Code compatibility against Cloudflare challenges by disabling `zstd` on compatibility streams, adding challenge cooldown and candidate switching, and keeping low-quota accounts as tail fallbacks.
- Normalized Claude Code 2.x mid-conversation `system` messages to the Responses `developer` role to reduce ChatGPT Codex backend Cloudflare challenges.

### Changed
- Bumped the release version to `0.3.8` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.3.7] - 2026-05-27

### Added
- Added Linux arm64 as a `release-all.yml` publishing target so it is covered by the unified multi-platform release pipeline.

### Fixed
- Fixed unstable local account routing for OpenAI-compatible `/v1/responses` requests with `prompt_cache_key`; matching client keys now reuse the same upstream account while preserving the original client key upstream.
- Fixed short `prompt_cache_key` values, such as `pc_1`, being skipped by local account binding and spread across balanced rotation.
- Fixed ChatGPT upstream transport errors not continuing failover across candidate accounts.
- Fixed missing beta marker headers during Responses WebSocket upstream handshakes.
- Fixed inconsistent `message_stop` terminal detection for `/v1/messages` compatibility streams.
- Fixed account upserts potentially clearing existing tokens.

### Changed
- Bumped the release version to `0.3.7` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.3.6] - 2026-05-25

### Added
- Added scheduled account warmup so configured accounts can be warmed periodically, reducing cold-start failures on first use.
- Added an experimental ChatGPT `/v1/responses` WebSocket upstream transport. When `CODEXMANAGER_USE_WEBSOCKET_UPSTREAM=1` is set, the gateway tries WebSocket first, falls back to HTTP streaming on failure, and keeps upstream proxy, connect-timeout, and protocol-frame validation behavior.

### Changed
- Request logs now show API key names instead of key IDs in the key column, making call-source debugging easier.
- Bumped the release version to `0.3.6` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.3.5] - 2026-05-24

### Added
- Added compact model forwarding rules in Settings, scoped to `/v1/responses/compact` requests.
- Added request-gate wait, first-response latency, and slow-trace stdout diagnostics for high-concurrency streaming investigations.

### Fixed
- Fixed Anthropic SSE completed snapshots being replayed after tool calls, which could duplicate assistant text in clients.
- Fixed Chat Completions compatibility when Responses only returns reasoning summaries; streaming and non-streaming responses now expose compatible reasoning fields.
- Suspended automatic token refresh for region-blocked accounts to avoid repeated background retries.
- Cleaned stale Aggregate API/model source routes and orphan automatic catalog models after model sync.
- Added an opt-in request-gate wait cap via `CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS=5000` to avoid long head-of-line blocking for matching key/path/model streaming requests.

### Changed
- Bumped the release version to `0.3.5` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.

### Added
- Aggregate API balance checks now refresh automatically on the same interval as account-pool usage polling, controlled by the Settings usage polling worker.
- Added Codex image-generation compatibility: `/v1/responses` now auto-injects the official `image_generation` tool by default to match Codex behavior, explicit tools are forwarded unchanged, and compatible `/v1/images/generations` plus `/v1/images/edits` endpoints are available with `gpt-image-2` as the default image tool model.
- Added an `auth.json` step to the Codex CLI first-time setup guide, clarifying how the platform key, `auth.json`, and `config.toml` fit together.

### Fixed
- Fixed missing Spark dedicated quota display by continuing to parse official `additional_rate_limits[].rate_limit` buckets.
- Adjusted the usage-details dialog so multiple additional quota windows can be shown in two columns with scrolling.

### Changed
- Bumped the release version to `0.3.2` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Bumped the release version to `0.2.6` and synchronized workspace, frontend package, Tauri desktop metadata, and lockfiles.
- Removed recent-commit blocks from README entry pages so they only keep stable feature and documentation entry points.
- Restored the upstream total-timeout setting in the Settings gateway transport card. `CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS` can now be viewed and changed there; default `0` means no service-side total-duration cutoff.
- Added a dedicated `/v1/images/` block to the Nginx example config for image uploads, large `b64_json` responses, and long-running image generation.
- Synchronized request-log cost estimates with official `gpt-5.5`, `gpt-5.5-pro`, and `gpt-image-*` pricing. Because current usage logs do not include modality buckets, image models are conservatively estimated with official Image token prices.

## [0.2.3] - 2026-04-15

### Fixed
- Restored the native Codex thread-anchor priority for resume flows: when a request already carries `conversation_id` or `x-codex-turn-state`, the gateway no longer lets request-body `prompt_cache_key` take over, reducing compatibility-mode regressions in native Codex context continuation.
- Added targeted regression coverage for native anchors, explicit `prompt_cache_key`, conflicting anchors, and Anthropic-native paths so compatibility fields cannot silently outrank native Codex semantics again.

### Changed
- Bumped the release version to `0.2.3` and synchronized the workspace, frontend package, Tauri desktop metadata, and lockfiles.

## [0.2.0] - 2026-04-12

### Added
- Top-level pages now use background keep-alive caching with a more visible full-area loading overlay, improving revisit latency across desktop, web, and Docker deployments.
- Added targeted regression coverage for `service_tier`, Cloudflare challenge handling, and compatibility forwarding across native Codex, Claude Code, and Gemini CLI request paths.

### Fixed
- Tightened the native Codex passthrough path so the default behavior stays close to the official request shape, keeping only account selection, auth replacement, routing, session affinity, and required internal-field cleanup; Claude and Gemini still use protocol adapters.
- Aligned upstream Codex behavior with the official client: requests sent to `chatgpt.com/backend-api/codex/responses` now map `service_tier=fast` to upstream `priority`, while `/responses/compact` no longer carries `service_tier`.
- Fixed the Claude compatibility path so `fast` service-tier mapping and error-stream model echoing are stable, reducing misleading diagnostics during `403` investigations.

### Changed
- The release version is now `0.2.0`, with workspace, frontend package, Tauri desktop, and external version notes updated together.

## [0.1.19] - 2026-04-08

### Added
- Aggregate API now supports multiple authentication types and custom `action` settings, and passthrough routing can hit the correct upstream based on auth mode and action.
- Internationalization now persists the selected locale and uses split message catalogs, with the remaining dashboard, modal, sidebar, and usage labels localized across languages.

### Fixed
- Fixed Aggregate API passthrough streams so Anthropic `message_stop` events are recognized correctly, reducing premature stream termination and state misreads.
- Fixed the gateway forwarding unsupported `service_tier` values upstream; standard Responses requests now keep only supported values to avoid upstream rejection.
- Changed the collaboration and security entry pages to show Chinese content by default and cleaned up the multilingual doc entry flow to make release docs easier to open from the repository root.

### Changed
- Formal documentation is now organized by language folders, and the root entry documents plus multilingual landing pages were refreshed together.
- The release version is upgraded to `0.1.19`, and the version notes of workspace, front-end package, Tauri desktop, lock file, README, and CHANGELOG are updated simultaneously.

## [0.1.18] - 2026-04-06

### Added
- Added direct sort controls for the account list and a quota-first sorting mode, making it easier to inspect accounts with tight remaining quota.
- Added the initial Gemini CLI compatibility layer, including key request paths such as streaming `tools`, MCP tool names, and SSE `tool_call` handling.

### Fixed
- Fixed the misaligned "quota details" popup on the account page by anchoring the right-hand panel to the center line of the quota overview card.
- Fixed Gemini issues around completed tool output being treated as plain text, streaming cached-token logging, request adaptation compatibility, and token refresh edge cases.

### Changed
- Aligned the Gemini → Codex / Responses request path toward CPA-compatible behavior, including developer messages, tool-name mapping, FIFO `call_id`, `reasoning`, `include`, and `parallel_tool_calls`.
- Removed unused Gemini-path code and refreshed the CPA acknowledgement plus version-related documentation notes.
- The release version is upgraded to `0.1.18`, and the version notes of workspace, front-end package, Tauri desktop, lock file, README, and CHANGELOG are updated simultaneously.

## [0.1.17] - 2026-04-05

### Added
- The request log has a new "final effective service level" caliber. The HTTP/WS log will now retain both the client's explicit `service_tier` and the final value after the request is rewritten, making it easier to check whether the platform Key's default `Fast` is actually sent to the upstream.
- A new global "model forwarding rule" has been added to the settings page, which supports the use of `pattern=target` format for model name rewriting, and takes effect during the runtime request rewriting phase.

### Changed
- The protocol type of common platform keys converges to "wildcard compatibility (Codex / Claude Code)". By default, Claude or Codex / OpenAI semantics are automatically selected according to the request path, reducing the cost of repeatedly maintaining multiple sets of Keys.
- The release version is upgraded to `0.1.17`, and the version notes of workspace, front-end package, Tauri desktop, lock file, README and CHANGELOG are updated simultaneously.

## [0.1.16] - 2026-04-05

### Added
- Added `/v1/responses` WebSocket request support, completed transmission type identification, request header normalization, proxy runtime and request log link.
- A new additional quota window display is added to the account page and usage pop-up window; after refreshing, the remaining quota and reset time of the standard quota and additional quotas such as Code Review/Spark will be displayed uniformly.
- The gateway trace adds a new `CLIENT_SERVICE_TIER` event to record whether the HTTP/WS original request explicitly carries `service_tier`, the original value and the log normalized value, making it easy to quickly distinguish the client's explicit `fast` from the platform Key's default service level.

### Fixed
- Fixed the issue of inconsistent calibers of `service_tier` in HTTP and WS request logs; now `fast` will only be recorded when the client requests that it explicitly carries `service_tier`, and the platform Key default value will no longer be mistakenly recorded as a request to explicitly enable it.
- Fixed the issue where the service level display on the log page is inconsistent with the gateway on-wire value; `priority` will be uniformly displayed as `fast`, and requests that do not explicitly carry a service level will continue to be displayed as `auto`.

### Changed
- The release version is upgraded to `0.1.16`, and the version notes of workspace, front-end package, Tauri desktop, lock file, README and CHANGELOG are updated simultaneously.

## [0.1.15] - 2026-04-03

### Changed
- The release version is upgraded to `0.1.15`, and the version notes of workspace, front-end package, Tauri desktop, running documentation and README are updated simultaneously.

## [0.1.14] - 2026-03-30

### Added
- A new "System Derivation" button and "Single Account Concurrency Limit" are added to the settings page, which can be backfilled according to the current machine resources with one click and take effect immediately.
- The entry layer adds short queue waiting and rapid overload degradation to prevent high concurrency from directly dragging down the service process.

### Changed
- README、workspace、前端包、Tauri 桌面端与版本一致性校验脚本统一提升到 `0.1.14`。

## [0.1.13] - 2026-03-25

### Added
- Added "Aggregation API" management page to support supplier name, sequence priority, classification according to `Codex / Claude`, connectivity testing and minimum forwarding upstream management.
- The platform key has a new `账号轮转 / 聚合 API 轮转` policy, and the aggregation API rotation will hit the corresponding suppliers first in order before continuing to the next channel.

### Fixed
- Fixed the automatic recovery behavior when starting the desktop service and switching between pages, to avoid being paged and restarted after shutting down, and to avoid accidentally clearing data on the dashboard when disconnected.

### Changed
- README, workspace, front-end package, Tauri desktop and version consistency verification scripts have been upgraded to `0.1.13`.

## [0.1.12] - 2026-03-20

### Fixed
- Fixed the issue where the platform key name editing link was not fully transparently transmitted on the desktop; now both Web and the desktop can correctly save and echo the name, and support Chinese names.
- Fixed the issue where the key ID in the platform key list was truncated by default; now it will be directly displayed in full for easy verification and troubleshooting.

### Changed
- README adds sponsorship support entrance and sponsorship area jump, making it easier to locate sponsorship instructions directly from the top of the document.
- The release version is upgraded to `0.1.12`, and workspace, front-end package, Tauri desktop, version consistency verification script and README are updated simultaneously.

## [0.1.11] - 2026-03-20

### Added
- Account management has added new ban identification, ban filtering and "one-click cleanup of banned accounts" portals; `account_deactivated` and `workspace_deactivated` will be automatically recognized as unavailable signals and can be filtered and cleared directly in the list.
- The 5-hour/7-day quota column of the account list will now display the reset time of the respective window; free accounts that only return the 7-day window will also display the reset time in the 7-day column.
- The platform key has new service level configurations: `跟随请求`, `Fast`, and `Flex`, among which `Fast` will be mapped to the upstream `priority`, and `Flex` will be directly transmitted to `flex`.

### Fixed
- Fixed the issue where `serviceTier` was not transparently transmitted when creating/editing the desktop platform key, causing the "service level" to not take effect or be echoed after being saved.
- Fixed the problem of Web occasionally downloading wrong files when refreshing other than the home page, and fixed the copy failure caused by `navigator.clipboard.writeText` being unavailable when copying API Key/login link in some operating environments.
- Fixed an issue where the "Check for Updates" button on the settings page continued to rotate incorrectly when automatically and silently checking for updates; now the loading status is only displayed when manually clicked.

### Changed
- The main link of the gateway continues to close to Codex-first: outbound semantics such as session binding, automatic number and thread switching, `originator` / `User-Agent` / request compression have been further aligned, and the upstream cookie link left behind by the old compatibility path has been removed.
- The setting page replenishment service listening address switch can be switched between `localhost` and `0.0.0.0`; README and documents have also been synchronized to the current mainline path.
- The release version is upgraded to `0.1.11`, and workspace, front-end package, Tauri desktop, version consistency verification script and README are updated simultaneously.

## [0.1.10] - 2026-03-18

### Fixed
- Fixed the problem that the Web / Docker version mistakenly entered the desktop-specific command branch, account enable/disable was missing the `sort` parameter, resulting in the inability to switch states, and the status column was not refreshed in time after the account details failed to be refreshed.
- Fixed an issue where disabled accounts still participate in manual batch refresh and background usage polling; batch refresh and background polling now skip manually disabled accounts and are executed as concurrent workers.
- Fixed the semantic confusion of account status: manual disablement is unified into `disabled`, quota exhaustion and `usage endpoint 401` are unified into `unavailable`, `refresh token 401` related links are also unified into `unavailable`, and the front-end status display is synchronously closed to "Disabled/Unavailable".
- Fixed the problem of Windows local Web `codexmanager-service` / `codexmanager-web` still remaining in the background after the launcher closes the console window; the launcher will now recycle the child process together through the Job Object.

### Changed
- The release version is upgraded to `0.1.10`, and workspace, Tauri desktop version, front-end package version, README latest version description and version consistency test are simultaneously updated.

## [0.1.9] - 2026-03-18

### Added
- The request log now supports backend paging, backend statistics, first try account and try link display, making it easier to distinguish the actual hit account from the final account after failover.
- A new free / 7-day single-window account usage model configuration is added to the settings page. Free accounts will initiate requests according to the setting model.

### Fixed
- Fixed stability issues such as desktop startup misjudgment, `/rpc` empty response, refresh failure caused by missing `spawn_blocking`, out-of-sync refresh of the usage pop-up window, lag on the first page cut, and Hydration inconsistency.
- Fixed issues such as incorrect refresh token extraction, free account request model not being rewritten correctly, unstable priority account behavior, and `503 no available account` missing contextual diagnosis.
- Fixed the issue of verify failure caused by the mismatch between the pnpm version and the current lock file in the release workflow.

### Changed
- The old front-end has been removed, and the desktop and Web management interfaces have been unified to the new `apps` front-end; account management, platform keys, request logs, settings pages, and navigation layouts have all undergone a desktop-first restructuring.
- Codex Request links continue to be aligned with actual on-wire behavior: login / callback / workspace validation, refresh semantics, `/v1/responses` and `/v1/responses/compact` rewrites, thread anchors, request compression, error summaries, and fallback diagnostics all continue to be aligned.
- Gateway failure diagnostics and disk logging continue to converge, with compact false success bodies, HTML/challenge pages, `401 refresh` subclasses, and exhausted candidate links all outputting clearer summaries.
- Unifiedly upgrade the release version to `0.1.9`, and simultaneously update the workspace, Tauri desktop, `tauri.conf.json` and front-end package versions.
- The fixed Tauri CLI versions in the GitHub Release workflow have been aligned to the actual version currently used on the Rust side, reducing the risk of CLI / crate drift in the packaging phase.
- The release documentation and README have been updated to `v0.1.9`, and the front-end static export directory description has been corrected to `apps/out`.

## [0.1.8] - 2026-03-11

### Fixed
- Removed the default `https://api.openai.com/v1` fallback path for ChatGPT-backed requests; upstream `challenge` and `403` outcomes are now returned from the primary login-account path instead of being rewritten into local fallback errors.
- ChatGPT login-account requests now recover from `401` by refreshing the local `access_token` with the stored `refresh_token` and retrying the current request once.

### Changed
- ChatGPT login-account turns now use `access_token` directly on the primary upstream path and no longer mix in `api_key_access_token` semantics.
- Synthetic gateway terminal failures now return structured OpenAI-style `error.message / error.type / error.code` payloads while keeping the existing trace and error-code headers.

## [0.1.7] - 2026-03-11

### Added
- Added gateway transmission parameters to the settings page: supports direct configuration of upstream streaming timeout and SSE keepalive interval, and takes effect hot when the service is running.
- Desktop startup snapshot completion: Dashboard statistics, account usage status, and request log first screen will restore the most recent snapshot first, reducing all 0/unknown status after source code running or service restart.

### Fixed
- Fixed an issue where `codexmanager-web`'s access password session could continue to be used across reboots; after closing and reopening the Web process, the old login cookie would become invalid and the password would need to be re-verified.
- Fix the startup and root routing compatibility issue when the source code runs `codexmanager-web`, and reduce the inconsistent behavior of Web static resources and root paths under Axum routing.
- Fixed the SSE idle disconnection and reconnection problem in long output scenarios to reduce the probability of long-term streaming responses being interrupted by misjudgment.
- Fixed desktop interaction issues such as saving the upstream agent on the settings page, closing the platform key creation pop-up window and submitting it repeatedly, and the account form not refreshing after successful login.
- Fixed some upstream compatibility issues caused by the default additional version parameters when pulling models, and changed the model request to not include a version number by default.
- Fixed the problem of inconsistent account merging logic between account import and login callback, and unified new or updated accounts according to the same identity rule.
- Fixed the problem of tool truncation in Claude / Anthropic `/v1/messages` when adapted to multiple MCP server scenarios; tools for subsequent servers will no longer be lost due to the first 16 tools being full.
- Fixed the problem of missing long tool name shortening and response restoration in Claude / Anthropic `/v1/messages` to avoid mapping instability when the MCP tool name is too long.

### Changed
- The gateway failure response adds structured `errorCode` / `errorDetail` fields, and simultaneously adds `X-CodexManager-Error-Code`, `X-CodexManager-Trace-Id` response headers to facilitate the client and log system to track failed links.
- Protocol adaptation continues to align the Codex / OpenAI compatible ecology: further unifying the forwarding semantics of `/v1/chat/completions`, `/v1/responses`, Claude `/v1/messages`, and stabilizing `tools` / `tool_calls`, thinking/reasoning, streaming bridging, and response restoration links.
- The settings page and runtime configuration continue to converge: high-frequency configurations such as background tasks, gateway transmission, upstream proxy, Web security, etc. are unified and persisted by `app_settings` and backfilled into the current process.
- The desktop and service startup links will continue to be managed, the startup boundaries and startup sequences between Web/service/desktop will be converged, and the behavior bifurcation between source code running and packaged running will be reduced.
- The project continues to promote long-term maintenance-oriented reconstruction management: the front-end main entrance, settings page, request log view, Tauri command registration, service life cycle, gateway protocol adapter, HTTP bridge, upstream attempt flow and other areas have further split module boundaries to reduce the coupling between large files and the root-level facade.
- The service/gateway directory structure continues to converge, and more wildcard imports, cross-layer direct connections, and extremely long facade lists have been replaced by explicit dependencies and layered modules, making subsequent maintenance and protocol regression positioning costs lower.
- The release link continues to converge to `release-all.yml` single entry, and the front-end construction products and protocol regression baselines are reused to reduce the risk of protocol regression during repeated builds and releases.

## [0.1.6] - 2026-03-07

### Fixed
- Fix the problem that `release-all.yml` still relies heavily on pre-built front-end artifacts when `run_verify` is manually closed; each platform task will automatically fall back to local `pnpm install + build` when `codexmanager-frontend-dist` is missing.

### Changed
- Windows Desktop release products continue to converge, only `CodexManager-portable.exe` portable version will be retained, and no additional `CodexManager-windows-portable.zip` will be generated.
- Improve SOCKS5 upstream proxy support and normalization, and supplement the proxy protocol prompt copy in the settings page.

## [0.1.5] - 2026-03-06

### Added
- Added "Import by folder": On the desktop, you can directly select a directory, recursively scan the `.json` files and import accounts in batches.
- Added OpenAI upstream proxy configuration and request header convergence policy switches, which can be saved directly on the settings page and take effect immediately.
- Supplement the chat tools hit probe script to facilitate local verification of whether the tool call is actually hit and transparently transmitted.

### Fixed
- Fix `tool_calls` / `tools` related regressions: Complete the tool call retention, tool name shortening and response restoration link in the chat aggregation path to avoid tool calls being lost or having names confused in OpenAI compatible returns, streaming increments and adaptation transformations.
- Improve the OpenClaw / Anthropic compatibility return adaptation to ensure that tool calls, SSE deltas, and non-streaming JSON responses are all correctly restored in a compatible format.
- Request log tracing is enhanced to supplement the original path, adaptation path and more context to facilitate locating `/v1/chat/completions -> /v1/responses` forwarding and protocol adaptation issues.

### Changed
- Gateway protocol adaptation further aligns Codex CLI: `/v1/chat/completions` and `/v1/responses` two links uniformly converge to Codex `responses` semantics, the upstream streaming/non-streaming behavior is closer to the official one, and is compatible with OpenAI compatible calls of clients such as Cherry Studio.
- The commonly used configurations at the top of the settings page have been changed to a unified three-column row layout, and the agent configuration remains consistent with it; it also supports hiding in the system tray after closing the window.
- The publishing process is integrated into a single one-click multi-platform workflow, and the desktop product form is converged; Windows directly provides portable exe, macOS uniformly uses DMG distribution.

## [0.1.4] - 2026-03-03

### Added
- Added "One-click removal of unavailable Free accounts": clean up "Unavailable + free plan" accounts in batches and return scan/skip/deletion statistics.
- Added "Export User": Supports selecting a local directory and exporting according to "one account, one JSON file".
- Import compatibility enhancement: support automatic identification of `tokens.*`, top-level `*_token`, camelCase fields (such as `accessToken` / `idToken` / `refreshToken`).

### Fixed
- Compatible with old services: The front-end will automatically normalize the top-level token format before importing it to avoid reporting `missing field: tokens` in the old version of the back-end.

### Changed
- The operation area of ​​the account management page is integrated into a single "Account Operation" drop-down menu, replacing the stack of multiple buttons on the right, making the interface more concise.

[Unreleased]: https://github.com/qxcnm/Codex-Manager/compare/v0.6.3...HEAD
[0.6.3]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.3
[0.6.2]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.2
[0.6.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.6.0
[0.5.4]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.4...v0.5.0
[0.4.4]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/qxcnm/Codex-Manager/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.9...v0.4.0
[0.3.9]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.8...v0.3.9
[0.3.8]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.7...v0.3.8
[0.3.7]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/qxcnm/Codex-Manager/compare/v0.3.4...v0.3.5
[0.2.6]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.3...v0.2.6
[0.2.3]: https://github.com/qxcnm/Codex-Manager/compare/v0.2.0...v0.2.3
[0.2.0]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.2.0
[0.1.19]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.19
[0.1.17]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.17
[0.1.16]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.16
[0.1.15]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.15
[0.1.14]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.14
[0.1.13]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.13
[0.1.12]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.12
[0.1.11]: https://github.com/qxcnm/Codex-Manager/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.10
[0.1.9]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.9
[0.1.8]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.8
[0.1.7]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.7
[0.1.6]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.6
[0.1.5]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.5
[0.1.4]: https://github.com/qxcnm/Codex-Manager/releases/tag/v0.1.4
