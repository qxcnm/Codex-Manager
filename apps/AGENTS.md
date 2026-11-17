# Frontend and Desktop Engineering Standards

This file applies to `apps/`: the Next.js frontend, the Tauri desktop shell, and
their tests/configuration.

## 1. Tech Stack
- **Framework**: Next.js 16 App Router with static export.
- **Runtime UI**: React 19.
- **Language**: TypeScript strict mode.
- **Styling**: Tailwind CSS v4 via `src/app/globals.css`.
- **UI Components**: shadcn/ui style components built on `@base-ui/react`.
- **Icons**: `lucide-react`.
- **State Management**: Zustand in `src/lib/store/`.
- **Data Fetching**: TanStack Query v5.
- **Desktop Runtime**: Tauri v2.

## 2. Static Export and Routing
- `next.config.ts` uses `output: "export"` and `trailingSlash: true`; keep new
  routes, links, redirects, and asset paths compatible with static output.
- Use `buildStaticRouteUrl()` and the top-level route helpers when adding shell
  navigation entries.
- The shell uses keep-alive page panels and lazy-loaded top-level routes. Add
  new primary pages to the route config, sidebar icon map, and keep-alive map.
- Validate significant frontend changes with `pnpm run build:desktop` from this
  directory, or `pnpm -C apps run build:desktop` from the repository root.

## 3. Design Language: Glassmorphism and Themes
- Ambient backgrounds and theme tokens live in `src/app/globals.css`.
- Use `.glass-sidebar` for the navigation bar, `.glass-header` for the top bar,
  and `.glass-card` for main content surfaces.
- Always respect `body.low-transparency`: when active, blur and mesh gradients
  must be disabled in favor of solid colors using `var(--card-solid)`.
- Theme handling uses `next-themes` with 12 supported themes:
  `tech`, `dark`, `dark-one`, `business`, `mint`, `sunset`, `grape`, `ocean`,
  `forest`, `rose`, `slate`, and `aurora`.
- Keep theme labels, defaults, app settings, and CSS variables synchronized when
  adding or renaming a theme.

## 4. Component Guidelines
- Mark interactive components with `"use client"`. Prefer Server Components only
  where the surrounding route/layout can remain static.
- Move new non-trivial business logic into hooks under `src/hooks/` or focused
  helpers under `src/lib/`. Existing large pages are legacy surfaces; do not add
  more orchestration there unless the change is very small.
- Avoid nested `<button>` elements. For Base UI triggers, use
  `render={<span />}` / `nativeButton={false}` or an equivalent non-button
  render target when the trigger wraps button-like children.
- Reuse `components/ui/` primitives and `lucide-react` icons instead of adding
  one-off SVG controls.
- Keep lists, tables, dialogs, and settings forms dense and operational; this app
  is a management tool, not a landing page.

## 5. API, IPC, and Web Fallback
- Import `invoke`, `invokeFirst`, and `withAddr` from `@/lib/api/transport`.
- Service commands that talk to the running backend should use `withAddr()` so
  the current service address is injected.
- App-shell commands such as window, updater, file-manager, and external-open
  helpers may call `invoke` without `withAddr()` when they do not target the
  service.
- Do not use raw `fetch()` for desktop IPC. Web/service-mode HTTP access should
  go through `transport.ts`, `transport-web-commands.ts`, `rpc-http.ts`, and
  `fetchWithRetry`.
- When adding a new backend command, update the typed wrapper in `src/lib/api/`
  and, if it must work in service-mode Web UI, update the web command map with
  the correct underscore-to-camelCase RPC mapping.
- Standardize displayed errors through `getAppErrorMessage()` and the existing
  transport error unwrapping helpers.

## 6. Directory Structure
- `src/app/`: App Router pages and layout.
- `src/components/ui/`: atomic shadcn/Base UI primitives.
- `src/components/layout/`: shell, sidebar, header, bootstrapping, and page cache.
- `src/components/modals/`: feature-specific dialogs.
- `src/hooks/`: business/data hooks.
- `src/lib/api/`: typed backend client wrappers and transport.
- `src/lib/store/`: Zustand global state.
- `src/lib/i18n/`: locale provider and message dictionaries.
- `src/types/`: shared TypeScript interfaces.
- `src-tauri/`: Tauri app shell, command registry, lifecycle, tray/window logic,
  desktop RPC client, and packaging config.
- `tests/`: runtime and Playwright regression coverage.

## 7. Development Workflow
- Frontend changes: run `pnpm run build` or `pnpm run build:desktop`.
- Runtime/transport changes: also run `pnpm run test:runtime`.
- Navigation/cache changes: run `pnpm run test:navigation` when practical.
- UI-flow changes with meaningful behavior risk should include or update
  Playwright coverage under `tests/`.
- Tauri command/lifecycle changes usually need Rust validation from the repo
  root with `cargo test --workspace`.

<!-- BEGIN:nextjs-agent-rules -->

## This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` (resolved from this file's directory; in monorepos the `next` package may not be visible from the repo root) before writing any code. Heed deprecation notices.

This block is written and re-added by `next dev` — verify at `node_modules/next/dist/server/lib/generate-agent-files.js`. Removing it from a diff only re-creates the uncommitted change; committing it with your work keeps the tree clean.

<!-- END:nextjs-agent-rules -->
