# Build, Release, and Script Guide

## Local development

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

# Bundle frontend static assets into codexmanager-web (single-binary mode)
pnpm -C apps run build:desktop
cargo build -p codexmanager-web --release --features embedded-ui
```

## Tauri packaging

### Windows
```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

### Linux / macOS
```bash
./scripts/rebuild-linux.sh --bundles "appimage,deb" --clean-dist
./scripts/rebuild-macos.sh --bundles "dmg" --clean-dist
```

Linux packaging requires `squashfs-tools` and checks that each AppImage contains a valid root `.DirIcon`, repairing the bundle when needed. The interface language defaults to **Auto-detect**, following the system language in the desktop app or the browser language in Web mode. Chinese, English, Russian, and Korean are supported; other languages fall back to English. Choose Auto-detect or a fixed language in Settings or the header; the choice applies immediately and remains saved.

## GitHub Actions
The unified release workflow is `.github/workflows/release-all.yml`. Pushing a `v*` tag automatically runs `build-and-publish`; `workflow_dispatch` also supports manual runs in `build-and-publish`, `build-artifacts`, or `publish-artifacts` mode.

### `release-all.yml`
- Purpose: publish Desktop + Service artifacts for all supported platforms in one run
- Build targets: `Windows`, `macOS (dmg)`, `Linux`
- The frontend `dist` is built once, then reused by the packaging jobs for each platform
- Desktop and Service builds run concurrently for all five platform targets. Service jobs also stage the Web packages and Linux x86_64 Docker archive; publishing waits for both build groups.
- Desktop caches use the `apps/src-tauri` workspace and Service caches use the root workspace, with separate keys to prevent cleanup across different dependency graphs.
- Inputs:
  - `mode`: defaults to `build-and-publish`, options: `build-and-publish | build-artifacts | publish-artifacts`
  - `tag`: required
  - `ref`: defaults to `main`
  - `prerelease`: defaults to `auto`, options: `auto | true | false`
- Behavior: packages and publishes artifacts only; it no longer includes server-side test gates

### Cache reuse across versions

After pushing the verified commit to `main`, dispatch from `main` with the build ref pinned to that commit:

```powershell
$releaseCommit = git rev-parse HEAD
gh workflow run release-all.yml --ref main -f mode=build-and-publish -f tag=v0.6.0 -f ref=$releaseCommit -f prerelease=false
```

Do not also push the same release tag: publishing creates the tag and Release after successful builds. Caches saved on the default branch are available to later branch and tag releases; caches saved only under one version tag cannot be reused directly by other tags. Initial builds, toolchain changes, and cache eviction can still require cold builds.

## Release artifacts

### Desktop
- Windows: `CodexManager_<version>_x64-setup.exe`, `CodexManager-portable.exe`
- macOS: `CodexManager_<version>_aarch64.dmg`, `CodexManager_<version>_x64.dmg`
- Linux: `CodexManager_<version>_amd64.AppImage`, `CodexManager_<version>_amd64.deb`, `CodexManager-linux-portable.zip`

### Service
- Windows: `CodexManager-service-windows-x86_64.zip`
- macOS: `CodexManager-service-macos-arm64.zip`, `CodexManager-service-macos-x64.zip`
- Linux: `CodexManager-service-linux-x86_64.zip`
- Linux (web test bundle): `CodexManager-web-linux-x86_64.zip`

### Release type
- When `prerelease=auto`, tags containing `-` are published as pre-releases.
- When `prerelease=auto`, tags without `-` are published as stable releases.
- When `prerelease=true|false`, the tag-based auto-detection is overridden.
- Rerunning the same `tag` updates the Release metadata using the current input values.
- GitHub still attaches `Source code (zip/tar.gz)` automatically.

## `scripts/rebuild.ps1`
By default, this script packages Windows builds locally. In `-AllPlatforms` mode, it triggers the GitHub release workflow.

### Common examples
```powershell
# Local Windows build
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable

# Trigger the release workflow (and download artifacts)
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms `
  -GitRef main `
  -ReleaseTag v0.6.0 `
  -GithubToken <token>

# Force a pre-release
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 `
  -AllPlatforms -GitRef main -ReleaseTag v0.6.0-beta.1 -GithubToken <token> -Prerelease true
```

### Main parameters
- `-Bundle nsis|msi`: defaults to `nsis`
- `-NoBundle`: compile only, do not create an installer
- `-CleanDist`: clean `apps/out` before building
- `-Portable`: also output a portable build
- `-PortableDir <path>`: portable build output directory, default `portable/`
- `-AllPlatforms`: trigger the release workflow
- `-GithubToken <token>`: GitHub token; if omitted, the script tries `GITHUB_TOKEN` / `GH_TOKEN`
- `-WorkflowFile <name>`: defaults to `release-all.yml`
- `-GitRef <ref>`: workflow build ref; defaults to the current branch or current tag
- `-ReleaseTag <tag>`: release tag; recommended explicitly when using `-AllPlatforms`
- `-Prerelease <auto|true|false>`: defaults to `auto`
- `-DownloadArtifacts <bool>`: defaults to `true`
- `-ArtifactsDir <path>`: artifact download directory, default `artifacts/`
- `-PollIntervalSec <n>`: polling interval, default `10`
- `-TimeoutMin <n>`: timeout in minutes, default `60`
- `-DryRun`: print the execution plan only

## Version synchronization

The repository no longer contains `scripts/bump-version.ps1`. Before a release, follow the checklist in [Release and Artifacts](release-and-artifacts.md) and verify the versions in the root `Cargo.toml`, `apps/package.json`, `apps/src-tauri/Cargo.toml`, `apps/src-tauri/tauri.conf.json`, and both `Cargo.lock` files.

## Protocol regression probe
```powershell
cargo test -p codexmanager-service gateway --offline -- --test-threads=1
```

This runs the Service gateway regression tests. The old `scripts/tests/gateway_regression_suite.ps1`, `chat_tools_hit_probe.ps1`, and `codex_stream_probe.ps1` probes have been removed from the repository.

## Related documents
- Release and artifacts: [Release and Artifacts](release-and-artifacts.md)
- Script responsibility matrix: [Script and Release Responsibility Matrix](../report/script-and-release-responsibility-matrix.md)
