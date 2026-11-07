# Automatic model discovery per source

Administrators can enable background model synchronization separately for an account or aggregate API (#515). It is off by default; visiting pages, importing accounts, or adding connections does not start model discovery.

## Use

1. Open Accounts or Aggregate API and select the source's background-sync control.
2. Choose an interval and start background synchronization. The default is 60 minutes; supported intervals are 5–10080 minutes.
3. Starting schedules an immediate run. Later runs are scheduled by the service. The dialog shows running state, recent success/error, discovery and addition counts, and the next scheduled time.
4. Stop synchronization to cancel future work for that source. Existing models and routes are retained.

Closing a page does not stop service-side work. Stopping the desktop service or standalone service stops execution; persisted settings and recent results are restored when the service starts again. Failures use delayed retries rather than a tight retry loop.

## Behavior

- Discovery reuses the source's existing authentication, proxy and catalog endpoint. It does not run inference or connectivity probes.
- An account supplies discovery credentials; its models use the existing shared account-pool route rather than binding inference to that account. Aggregate API routes point to the selected connection.
- Only missing models and routes are added. Existing names, prices, ordering, capabilities, enabled states and other routes are preserved, including edits made while discovery is running.
- Missing upstream entries are not deleted locally. New models use the existing manual-association defaults (text capabilities and missing prices); verify their actual capabilities and prices separately. Catalog membership does not guarantee successful inference.
- Manual model fetching and selective association remain available.

Deleted or disabled sources are not queried. Stopping coordinates with catalog writes: after stop returns, a cancelled older run cannot begin adding models. Previously completed writes are not rolled back.

## Permissions and deployment

Listing and configuring jobs both require administrator access on desktop and Web. Configuration stores source IDs and job state, not copied account tokens or API secrets. Only sanitized stage errors are persisted.

Settings use the existing SQLite/SeaORM `app_settings` storage with no new environment variables. RPC methods are `modelDiscovery/list` and `modelDiscovery/update`, with `sourceKind` values `openai_account` and `aggregate_api`. Settings writes are not automatically replayed.

One service process runs at most two sources concurrently and prevents overlap for a source. There is no cross-process lease; deployments sharing a database should run this scheduler in a single service instance. In-process deduplication is not a distributed execution guarantee.

## Validation

Validation uses temporary databases and local mock sources; existing accounts and aggregate APIs are not enabled for background discovery. The combined source tree also contains the previously pending Codex compatibility, skill, and profile changes.

- `pnpm -C apps run build:desktop`: passed.
- `pnpm -C apps run test:runtime`: 330 passed, 0 failed.
- `pnpm -C apps exec playwright test tests/source-model-sync.spec.ts tests/aggregate-api-filter-labels.spec.ts`: 8 passed, covering start/stop, interval validation, persistence/failure display, polling, member restrictions, and localized controls.
- `cargo check --manifest-path apps/src-tauri/Cargo.toml --locked --offline --target-dir D:/Data/Code/Codex-Manager/apps/src-tauri/target`: passed.
- Isolated service smoke using the final compiled binary: all five scenarios passed (default off and immediate discovery, preservation of manual settings, stopping an in-flight fetch, restart recovery, and source deletion). Only five local mock catalog requests were made; the test service was stopped.

- `cargo test --workspace --locked --offline --no-fail-fast --target-dir D:/Data/Code/Codex-Manager/target -- --skip runtime::service_runtime::tests::`: 2930 passed, 0 failed, 19 ignored; the three runtime tests were run separately.
- The service test binary produced by that workspace build (`codexmanager_service-c75573c0b1931ddb.exe runtime::service_runtime::tests:: --test-threads=1`): 3 passed, 0 failed. Total executed Rust tests: 2933 passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The existing three runtime tests compete for a process-wide capacity semaphore when run in parallel. This was reproduced with only those tests and without starting the discovery scheduler; serial execution passed. The 19 tests marked ignored require explicit isolated MySQL/PostgreSQL services or live third-party repositories/catalogs and were not exercised in this offline run. Validation completed on 2026-11-07.
