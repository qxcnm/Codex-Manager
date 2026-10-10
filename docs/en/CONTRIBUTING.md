# CONTRIBUTING

This document is used to constrain the daily collaboration method of CodexManager, with the goal of allowing new collaborators to complete development, verification, submission and release with as little verbal handover as possible.

## 1. Project positioning

CodexManager Not a single front-end project, nor a single Rust service project.
The current repository also contains:

- Desktop: `apps/` + `apps/src-tauri/`
- Local service: `crates/service`
- Web Shell: `crates/web`
- Service Launcher: `crates/start`
- Data and storage base: `crates/core`
- Build/release script: `scripts/`
- GitHub Actions publishing link: `.github/workflows/`

Therefore, before submitting, you must first determine which boundary your changes belong to, and avoid stacking multiple responsibilities directly into the same file.

Governance document entry:

- `README.md`: Project introduction and quick start
- `ARCHITECTURE.md`: Structural boundaries and operational relationships
- `TESTING.md`: Warehouse-level verification baseline
- `SECURITY.md`: Security issues and rules for handling sensitive information
- `docs/zh-CN/README.md`: Governance document directory and submission rules

## 2. Development environment

### 2.1 Essential Tools

- Node.js 20
- pnpm 9
- Rust stable
- Windows Local packaging requires PowerShell 7+
- Tauri Packaging needs to correspond to platform dependencies

### 2.2 Install dependencies

```bash
pnpm -C apps install
cargo test --workspace
```

### 2.3 Commonly used local commands

front end:

```bash
pnpm -C apps run dev
pnpm -C apps run test
pnpm -C apps run test:ui
pnpm -C apps run build
pnpm -C apps run check
```

Rust:

```bash
cargo test --workspace
cargo build -p codexmanager-service --release
cargo build -p codexmanager-web --release
cargo build -p codexmanager-start --release
```

Desktop packaging:

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

## 3. Commit boundaries

### 3.1 Modify files according to responsibilities

Prioritize the following boundaries:

- Front-end page, interaction, status: `apps/src/`
- Desktop shell, tray, window, Tauri command: `apps/src-tauri/src/`
- Server HTTP / RPC / Gateway / Protocol Adaptation: `crates/service/src/`
- Database migration, storage infrastructure: `crates/core/`
- Release and build scripts: `scripts/`, `.github/workflows/`

### 3.2 Current high-risk files

The following files are obviously too large, so you must refrain from adding general control logic when modifying them:

- `apps/src/components/layout/app-bootstrap.tsx`
- `apps/src-tauri/src/lib.rs`
- `crates/service/src/lib.rs`
- `crates/service/src/gateway/observability/http_bridge/`
- `.github/workflows/release-all.yml`

### 3.3 Large file warning threshold

When the following threshold is reached, the logic should not be continued to be piled in by default, but the split should be evaluated first:

- JavaScript/TypeScript: Start warning after lines exceeding `500`. Lines exceeding `800` must explain why they are not split.
- Rust: Alerts start for lines exceeding `400`. Lines exceeding `700` must explain why they are not to be demolished.
- Workflow/YAML: Alerts will start for lines exceeding `250`. Lines exceeding `400` must explain why they are not to be split.
- Markdown documentation: Alerts will start when lines exceed `300`, and priority will be given to the `docs/` subdocument.

illustrate:

- "Start warning" means that you should proactively determine whether to continue to split responsibilities before submitting.
- "Must explain why not split" means that the reason should be clearly given in the submission instructions or PR description.
- These thresholds are long-term maintenance constraints, not one-time cleanup indicators

### 3.4 Prohibited Items

- Do not continue to pile up settings, event bindings or protocol branches at the main entrance.
- Do not treat README as a changelog for long-term maintenance.
- Do not change scripts, workflows, or version numbers without verification.
- Don't roll back user changes you didn't create.
- Do not copy and expand the inline script in the release workflow again, reuse `scripts/release/` first.

## 4. Check before submission

### 4.1 Minimum Checklist

Perform at least the following content according to the scope of changes:

Front-end changes:

```bash
pnpm -C apps run test
pnpm -C apps run build
pnpm -C apps run test:ui
```

Rust/server-side changes:

```bash
cargo test --workspace
```

Desktop/packaging link changes:

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -DryRun
```

### 4.2 Changes related to protocol adaptation

If the following paths are changed, minimum regression verification must be performed:

- `crates/service/src/gateway/`
- `crates/service/src/http/`
- `crates/service/src/lib.rs`

Minimum coverage:

- `/v1/chat/completions`
- `/v1/responses`
- Streaming return
- non-streaming return
- `tool_calls` / tools related paths

### 4.3 Changes related to setting items

If you add settings page fields, environment variables or persistence configuration, you must also confirm:

- Is the default value clear?
- Do you need to write `app_settings`
- Does it affect the behavior of desktop / service / web terminals?
- Does the README or dedicated documentation need to be updated?

## 5. Submit information and PR agreement

### 5.1 Submit information

The current repository mainly uses Chinese submission instructions, requiring:

- One submission only solves one type of problem
- The title directly describes the results and does not write empty words.
- Don’t cram multiple unrelated changes into the same commit

### 5.2 PR describes minimum requirements

PR should at least clearly state:

- Which files were changed?
- what problem to solve
- Which platforms or interfaces are affected?
- What verifications were run?
- Are there any risks that are not covered?

## 6. Pre-release inspection

Before each release, you must confirm:

1. `CHANGELOG.md` Updated.
2. `README.md` is consistent with the current version entry of `docs/en/README.md`.
3. Consistent with versions `Cargo.toml`, `apps/src-tauri/Cargo.toml`, and `apps/src-tauri/tauri.conf.json`.
4. The release workflow input description, script parameter description, and actual workflow are consistent.
5. High-risk compatibility paths complete at least one round of local verification.
6. If you change product naming or release type logic, you must verify `prerelease` and tag behavior.

## 7. Document maintenance rules

The long-term maintenance agreement is as follows:

- `README.md` / `docs/en/README.md` are responsible for project introduction, quick start, and entry instructions.
- `CHANGELOG.md` Responsible for version history.
- `ARCHITECTURE.md` Responsible for structural boundaries and operational relationships.
- `CONTRIBUTING.md` Responsible for collaboration rules and pre-submission checks.

Stop piling version history, architecture description, and release details back into the README.

## 8. How to deal with major changes

If any of the following conditions are met, it is recommended to split the task before submitting it:

- It also involves three boundaries: front-end, desktop and server.
- At the same time, change protocol adaptation, set persistence, and publish links.
- It is necessary to rename the product, modify the workflow, and adjust the version strategy
- Need to split high-risk large files

Suggested order:

1. Supplement test or verification script first
2. Do refactoring or structural adjustment
3. Final documentation and version notes
