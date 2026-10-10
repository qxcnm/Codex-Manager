# Release and Artifacts

## Scope

This document describes the unified release entry point, artifact list, manual trigger parameters, and common troubleshooting paths used in the current repository.

## Unified release entry point

Current release workflow:

- `.github/workflows/release-all.yml`
- The frontend `dist` is built once, then reused by packaging jobs for each platform

Trigger mode:

- Pushing a `v*` tag automatically runs `build-and-publish`
- `workflow_dispatch` supports manual `build-and-publish`, `build-artifacts`, and `publish-artifacts` modes
- Ordinary branch pushes and pull requests do not trigger it

Key inputs:

- `mode`: `build-and-publish | build-artifacts | publish-artifacts`
- `tag`: required
- `ref`: build baseline branch or commit, default `main`
- `prerelease`: `auto | true | false`

## Artifact list

### Desktop

- Windows: `CodexManager_<version>_x64-setup.exe`
- Windows: `CodexManager-portable.exe`
- macOS: `CodexManager_<version>_aarch64.dmg`
- macOS: `CodexManager_<version>_x64.dmg`
- Linux: `CodexManager_<version>_amd64.AppImage`
- Linux: `CodexManager_<version>_amd64.deb`
- Linux: `CodexManager-linux-portable.zip`

### Service

- Windows: `CodexManager-service-windows-x86_64.zip`
- macOS: `CodexManager-service-macos-arm64.zip`
- macOS: `CodexManager-service-macos-x64.zip`
- Linux: `CodexManager-service-linux-x86_64.zip`
- Linux (web test bundle): `CodexManager-web-linux-x86_64.zip`

### Default GitHub attachments

GitHub Releases also attach:

- `Source code (zip)`
- `Source code (tar.gz)`

## Pre-release rules

- `prerelease=auto` and `tag` contains `-`: publish as a pre-release
- `prerelease=auto` and `tag` does not contain `-`: publish as a stable release
- `prerelease=true|false`: override auto-detection
- Rerunning the same `tag` updates the Release metadata using the current input values

## Local trigger entry point

Windows helper script:

- `scripts/rebuild.ps1`

Common example:

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.6.0 `
  -GithubToken <token>
```

## Platform-specific notes

### Windows

- Produces both installer and portable builds
- The portable build is distributed as a standalone `exe`, not wrapped in an extra zip

### macOS

- Current artifacts are shipped as `dmg`
- Because the app is not notarized with an Apple Developer account, Gatekeeper may still block the first launch
- The `dmg` bundle includes:
  - `Open CodexManager.command`
  - `README-macOS-first-launch.txt`

### Linux

- Desktop builds currently ship as `AppImage` and `deb`
- Service builds are shipped as zip archives

## Recommended checks before release

1. Synchronize the project version in the root `Cargo.toml`, `apps/package.json`, Tauri `Cargo.toml` / `tauri.conf.json`, and both `Cargo.lock` files
2. Make sure `CHANGELOG.md` has been updated
3. Make sure the desktop frontend build passes: `pnpm -C apps run build:desktop`
4. Make sure core tests pass: `pnpm -C apps run test`, `cargo test --workspace`
5. If the gateway protocol changed, run `cargo test -p codexmanager-service gateway --offline -- --test-threads=1`; the old `scripts/tests/gateway_regression_suite.ps1` probe has been removed.

## Common failure cases

### Missing frontend artifacts

Check:

- whether `apps/out/` builds successfully
- whether the frontend build step succeeded in the workflow

### Incorrect Release metadata

Check:

- whether `tag` contains `-`
- whether `prerelease` explicitly overrides auto-detection

### macOS artifact downloads but will not open

This is expected for the current non-notarized build and does not mean the workflow failed.

Workaround:

1. Drag `CodexManager.app` into `Applications`
2. Double-click `Open CodexManager.command`
3. Or run:

```bash
xattr -dr com.apple.quarantine /Applications/CodexManager.app
```

## Related documents

- Project overview: [README.md](../README.md)
- Testing baseline: [TESTING.md](../TESTING.md)
- Architecture notes: [ARCHITECTURE.md](../ARCHITECTURE.md)
