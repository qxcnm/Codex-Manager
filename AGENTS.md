# Repository Engineering Standards

This file applies to the whole CodexManager repository. For work under `apps/`,
also read `apps/AGENTS.md`; that file contains the more specific frontend and
Tauri rules.

## 1. Project Shape
- `apps/`: Next.js frontend plus the Tauri desktop shell.
- `apps/src/`: App Router UI, components, hooks, API clients, runtime helpers,
  i18n, and Zustand state.
- `apps/src-tauri/`: Tauri v2 application shell, desktop lifecycle, tray/window
  behavior, native commands, and desktop RPC client code.
- `crates/core/`: SQLite migrations, storage primitives, auth helpers, and core
  usage/account data structures.
- `crates/service/`: local HTTP/RPC service, gateway routing, protocol adapters,
  account/API key/usage domains, plugins, app settings, and runtime sync.
- `crates/web/`: service-mode Web UI shell, embedded static UI serving, and
  `/api/runtime` / `/api/rpc` proxy behavior.
- `crates/start/`: service-mode launcher that starts service + web together.
- `scripts/`, `docker/`, `.github/`: build, release, probe, container, and CI
  automation.

## 2. Ownership Boundaries
- Keep UI behavior in `apps/src/`, desktop shell behavior in `apps/src-tauri/`,
  and service/gateway behavior in `crates/service/`.
- Put schema and persistence foundation changes in `crates/core/`, especially
  SQLite migrations and reusable storage helpers.
- Avoid expanding central entrypoints with unrelated orchestration. Large files
  should be treated as legacy surfaces; new substantial logic should move into
  focused modules, hooks, or domain helpers.
- Do not mix release/script changes with product behavior unless the task
  explicitly requires it.

## 3. API, RPC, and Command Sync
- Frontend code must call backend capabilities through typed wrappers in
  `apps/src/lib/api/`.
- Desktop IPC should use the centralized `invoke` / `invokeFirst` helpers from
  `@/lib/api/transport`; do not use raw `fetch()` for desktop commands.
- Service commands that require a service address should pass parameters through
  `withAddr()`. App-shell commands such as `app_*`, `open_*`, and window/update
  helpers may omit it when no service address is needed.
- Web/service-mode fallback is allowed only through the existing transport
  stack: `transport.ts`, `transport-web-commands.ts`, `rpc-http.ts`, and
  `fetchWithRetry`.
- When adding or renaming a backend command, keep the chain synchronized:
  Rust implementation, Tauri command registry when applicable, service RPC
  dispatch/Web command mapping when applicable, and the frontend API wrapper.
- Preserve the existing underscore command names and camelCase RPC method
  mapping conventions.

## 4. Settings and Persistence
- New persisted settings need explicit defaults, storage behavior, runtime sync
  behavior, and UI/API exposure.
- Check whether a setting affects desktop mode, service mode, web mode, or all
  three before choosing where to implement it.
- New `CODEXMANAGER_*` environment variables require documentation updates and
  should not bypass existing app settings unless startup-time behavior requires
  an environment-level setting.
- SQLite schema changes belong in `crates/core/migrations/` and should include
  storage-level tests when behavior is non-trivial.

## 5. Frontend and Desktop Rules
- Follow `apps/AGENTS.md` for Next.js, Tailwind, shadcn/Base UI, React Query,
  Zustand, glass theme, static export, and Tauri-specific rules.
- The frontend is statically exported for the desktop shell. Keep routing and
  asset paths compatible with `output: "export"` and `trailingSlash: true`.
- The Web UI must continue to work through `codexmanager-web`; a plain static
  page or ordinary Next dev server is not the complete service-mode runtime.

## 6. Rust Service Rules
- Keep gateway/protocol changes localized under `crates/service/src/gateway/`
  and `crates/service/src/http/` unless shared service state is genuinely needed.
- Protocol adapter changes must consider `/v1/responses`, `/v1/chat/completions`,
  streaming SSE, non-streaming JSON, tools, and `tool_calls`.
- Prefer typed request/response structs and existing storage helpers over ad hoc
  JSON or string manipulation.
- Web access, roles, billing/account mode, and API key ownership are security
  boundaries; do not weaken checks for UI convenience.

## 7. Validation
- Frontend-only changes: run at least `pnpm -C apps run build` and, when runtime
  behavior is touched, `pnpm -C apps run test:runtime`.
- Desktop/static-export changes: run `pnpm -C apps run build:desktop`.
- Rust/service changes: run `cargo test --workspace`, or the narrowest relevant
  package test only when the change is clearly isolated.
- Web shell/transport changes: add `cargo test -p codexmanager-web` and the
  relevant runtime probe scripts when available.
- Gateway/protocol changes require targeted regression coverage for streaming,
  non-streaming, tools, and both supported OpenAI-style endpoints.
- If a validation step cannot run in the current environment, record the exact
  command and the reason it was not executed.

## 8. Documentation
- Keep `README.md`, localized docs under `docs/`, and app-level docs aligned
  with user-visible behavior, deployment modes, environment variables, and
  release/build commands.
- Root governance docs describe repository-level boundaries. App-specific
  frontend rules belong in `apps/AGENTS.md`.

## 9. Live Runtime Protection
- Treat the running OpenRuntime service and Web UI as user-critical. Development,
  tests, frontend export, Rust compilation, and packaging must not stop, replace,
  or lock the live processes on ports `48764` and `48763`.
- Build candidate service/Web executables into a separate hot-update target
  directory while the current version remains online. Never compile directly
  over a running Windows executable.
- Do not restart for intermediate changes. Only perform one short cutover after
  all requested changes, migrations, tests, frontend assets, and candidate
  binaries are complete.
- Before cutover, preserve the currently running binaries and database, prepare
  an automatic rollback path, and verify that ports `48760` and other legacy CPA
  services are outside the operation scope.
- During cutover, start and health-check the new service and Web UI immediately.
  If either health check fails, restore the previous binaries and restart the
  previous version automatically. The goal is to keep the user's active Codex
  sessions and this task reachable throughout development.
