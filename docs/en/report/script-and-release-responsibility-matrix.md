# Comparison of scripts and publishing responsibilities

## Comparison table between `.github/actions/` and `scripts/release/`

| Scenario | GitHub Action | Corresponding script/responsibility |
|---|---|---|
| Tauri Build retry | [`build-tauri-with-retry`](../../../.github/actions/build-tauri-with-retry/action.yml) | The retry loop is implemented in the composite action. Local equivalents are [`scripts/rebuild.ps1`](../../../scripts/rebuild.ps1), [`scripts/rebuild-linux.sh`](../../../scripts/rebuild-linux.sh), and [`scripts/rebuild-macos.sh`](../../../scripts/rebuild-macos.sh). |
| Service Packaging | [`stage-service-package`](../../../.github/actions/stage-service-package/action.yml) | The workflow stages the package in the composite action. Local binary builds use [`scripts/release/build-service-binaries.ps1`](../../../scripts/release/build-service-binaries.ps1) / [`build-service-binaries.sh`](../../../scripts/release/build-service-binaries.sh), with platform staging scripts in the same directory. |
| GitHub Release | [`publish-github-release`](../../../.github/actions/publish-github-release/action.yml) | The `gh release` upload flow is implemented in the composite action; there is no standalone local release-publishing script. |
| Release environment preparation | [`setup-release-env`](../../../.github/actions/setup-release-env/action.yml) | Workflow internal environment assembly; there is no completely equivalent independent top-level script. |
| Front-end dist preparation | [`prepare-frontend-dist`](../../../.github/actions/prepare-frontend-dist/action.yml) | The workflow restores the shared `apps/out` artifact. The matching local packaging helper is [`scripts/release/pack-frontend-dist.sh`](../../../scripts/release/pack-frontend-dist.sh); a local build starts with `pnpm -C apps run build`. |

## Boundary Agreement

### `.github/actions/`

Responsible for:

- Reusable steps within workflow
- Unify input and output across jobs
- CI/Release environment encapsulation

### `scripts/release/`

Responsible for:

- Actual building, packaging, and image-publishing actions
- Script implementation called locally or by workflow
- Try to keep it independently executable

### `scripts/*.ps1|*.sh`

Responsible for:

- Top-level entrance for developers
- Unified parameter organization
- Close the complex release sub-steps

## Current historical script inventory

### Still valuable

- [`scripts/rebuild.ps1`](../../../scripts/rebuild.ps1)
- [`scripts/rebuild-linux.sh`](../../../scripts/rebuild-linux.sh)
- [`scripts/rebuild-macos.sh`](../../../scripts/rebuild-macos.sh)
- [`scripts/release/build-service-binaries.ps1`](../../../scripts/release/build-service-binaries.ps1)
- [`scripts/release/build-service-binaries.sh`](../../../scripts/release/build-service-binaries.sh)
- [`scripts/release/pack-frontend-dist.sh`](../../../scripts/release/pack-frontend-dist.sh)
- [`scripts/release/publish-ghcr-images.sh`](../../../scripts/release/publish-ghcr-images.sh)
- Platform staging helpers under [`scripts/release/`](../../../scripts/release/)
- [`scripts/ci/check-websocket-pins.sh`](../../../scripts/ci/check-websocket-pins.sh)

Reason:

- Still used by local development, release workflows, or CI
- Not orphaned legacy files

### Actions without standalone scripts

The following workflow actions intentionally contain their implementation in
`.github/actions/` and do not have a same-named file under `scripts/release/`:

- `build-tauri-with-retry`
- `stage-service-package`
- `publish-github-release`
- `setup-release-env`
- `prepare-frontend-dist`

They should be changed in their action files, while local entry points remain in
`scripts/` where one exists.

The more appropriate strategy at this stage is not deletion, but:

1. Use `README.md` to indicate entrance stratification
2. Describe CI-specific scripts and local entries separately
3. If a script is not referenced in multiple versions in a row, consider archiving or merging it.

## Maintenance recommendations

- When adding workflow capabilities, give priority to determining whether existing actions should be reused.
- When adding a new release script, first decide whether it is a "top-level entry" or a "CI sub-step"
- If the script is not suitable for local execution, it must be clearly marked as "CI only" in the README
