# Documentation Index

`docs/` is the official long-form documentation directory for CodexManager.

Its purpose is simple:
- Keep governance notes, release guides, and operating manuals inside the repository.
- Make it easy for new contributors to find the right document without relying on tribal knowledge.

## Project Snapshot

CodexManager is a local desktop + service-process account pool manager and gateway relay for Codex workflows.

- Unified account, usage, and platform-key management.
- Local OpenAI-compatible gateway for Codex CLI, Gemini CLI, Claude Code, and third-party tools.
- Supports account routing, model/profile overrides, and aggregate API upstream relays.

## Feature Overview

- Account pool management: groups, tags, ordering, notes, ban recognition, and filtering.
- Batch import/export: multi-file import, desktop recursive folder import, per-account export.
- Usage display: 5-hour + 7-day windows, single-window accounts, and official extra buckets such as Code Review / Spark.
- Platform keys: create, disable, delete, model binding, reasoning tier, and service tier.
- Aggregate API: create/edit/test third-party relay upstreams with supplier naming and priority ordering.
- Plugin center: built-in, private, and custom source modes with task/log views and Rhai integration.
- Local service + gateway: custom bind/listen settings, upstream proxy, total request timeout, stream idle timeout, SSE keepalive, and a unified compatible endpoint.
- Image generation: automatically injects the official Codex `image_generation` tool for `/v1/responses` by default, forwards explicit tools unchanged, and exposes compatible `/v1/images/generations` and `/v1/images/edits` endpoints with `gpt-image-2` as the default image tool model.

## Quick Start

1. Launch desktop app and click **Start Service**.
2. Open **Account Management** and complete account authorization.
3. If callback parsing fails, paste the callback URL for manual parsing.
4. Refresh usage and verify account status.

## Screenshots

![Dashboard](../../assets/images/dashboard.png)
![Account Management](../../assets/images/accounts.png)
![Platform Key](../../assets/images/platform-key.png)
![Aggregate API](../../assets/images/aggregate-api.png)
![Plugin Center](../../assets/images/plug.png)
![Log View](../../assets/images/log.png)
![Settings](../../assets/images/themes.png)

## Scope
- Root `README.md` and localized `docs/*/README.md`: project overview and quick start.
- Root `CHANGELOG.md`: version history and unreleased changes.
- `report/*`: operations, troubleshooting, compatibility notes, and FAQs.
- `release/*`: build, packaging, release, and artifact documentation.

## Start here
- For the latest release notes, see [CHANGELOG.md](CHANGELOG.md).
- If you are not sure which document to open first, use the table below.

## Sponsors

Thanks to the following sponsors for supporting CodexManager.

<table>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://gzxsy.vip/register?aff=eapz">
        <img src="../../assets/images/sponsors/xingsiyan.jpg" alt="Xing Si Yan Gateway" width="120" />
      </a>
    </td>
    <td valign="top">
      <strong>Xing Si Yan Gateway</strong> provides stable relay and supporting services for Claude Code, Codex, and similar model-call scenarios. It is suitable for developers and teams that require highly available APIs, convenient onboarding, and continuous delivery support. Visit the <a href="https://gzxsy.vip/register?aff=eapz">official site</a> for the latest plans.
    </td>
  </tr>
</table>

Other supporters: [Wonderdch](https://github.com/Wonderdch), [suxinwl](https://github.com/suxinwl), [Hermit](https://github.com/HermitChen), [Suifeng023](https://github.com/Suifeng023), [HK-hub](https://github.com/HK-hub)

## Ecosystem Pairing

### OpenCowork

- Repository: [AIDotNet/OpenCowork](https://github.com/AIDotNet/OpenCowork)
- Recommended pairing: use OpenCowork for local file operations, multi-agent execution, workplace messaging, and desktop automation, while CodexManager handles Codex account management, usage tracking, platform keys, and the local gateway entry point.
- Best for: teams that want to separate the execution workspace and office integration from account-pool management and gateway access.
- A simple way to think about it: **OpenCowork executes in the real workspace, CodexManager manages accounts and gateway access.**

## Quick navigation
| What you need | Open this document |
| --- | --- |
| First launch, deployment, Docker, macOS allowlisting | [Runtime and Deployment Guide](report/runtime-and-deployment-guide.md) |
| Configure Codex CLI / ccswitch `auth.json` and `config.toml` | [Runtime and Deployment Guide](report/runtime-and-deployment-guide.md#connect-through-ccswitch) |
| Environment variables, database, ports, proxy, listen address | [Environment and Runtime Configuration](report/environment-and-runtime-config.md) |
| Account routing, import errors, challenge interception | [FAQ and Account Routing Rules](report/faq-and-account-routing-rules.md) |
| Why background jobs skip or disable accounts | [Background Task Account Skip Notes](report/background-task-account-skip-notes.md) |
| Minimum plugin marketplace integration | [Plugin Center Minimal Integration](report/plugin-center-minimal-integration.md) |
| Internal commands and integration surfaces | [System Internal Interface Inventory](report/system-internal-interface-inventory.md) |
| Local build, packaging, and release scripts | [Build, Release, and Script Guide](release/build-release-and-scripts.md) |

## Directory guide

### `release/`
Release notes, rollback notes, artifact descriptions, and packaging guides.

### `report/`
Operational guides, troubleshooting notes, compatibility reports, and FAQs.

## Recommended reading

### Operations
| Document | Summary |
| --- | --- |
| [Runtime and Deployment Guide](report/runtime-and-deployment-guide.md) | Desktop first launch, Service edition, Docker, and macOS first-run handling |
| [Environment and Runtime Configuration](report/environment-and-runtime-config.md) | Runtime configuration, defaults, and environment variables |
| [FAQ and Account Routing Rules](report/faq-and-account-routing-rules.md) | Common account-routing issues and troubleshooting tips |
| [Gateway vs Official Codex Params](report/gateway-vs-codex-official-params.md) | Current outbound parameter differences compared with official Codex |
| [Background Task Account Skip Notes](report/background-task-account-skip-notes.md) | Why background jobs skip, cool down, or disable accounts |
| [Minimal Troubleshooting Guide](report/minimal-troubleshooting-guide.md) | Fast checks for the most common startup and relay issues |
| [Plugin Center Minimal Integration](report/plugin-center-minimal-integration.md) | Minimum fields and interfaces required for plugin marketplace access |
| [Gateway vs Codex Headers and Params](report/gateway-vs-codex-headers-and-params.md) | Header and request parameter differences between the gateway and Codex |
| [Plugin Center Integration and Interfaces](report/plugin-center-integration-and-interfaces.md) | Marketplace modes, RPC/Tauri commands, manifest fields, and Rhai interfaces |
| [System Internal Interface Inventory](report/system-internal-interface-inventory.md) | Internal commands, RPC endpoints, and built-in plugin functions |

### Build and release
| Document | Summary |
| --- | --- |
| [Build, Release, and Script Guide](release/build-release-and-scripts.md) | Local builds, script parameters, and GitHub workflow entry points |
| [Release and Artifacts](release/release-and-artifacts.md) | Release artifacts, naming, and publication rules |
| [Script and Release Responsibility Matrix](report/script-and-release-responsibility-matrix.md) | Which script or workflow is responsible for which task |

## Contribution rules

### Commit documentation when it
- remains useful for future contributors,
- affects development, testing, release, or troubleshooting,
- or serves as a long-term source of truth.

### Do not commit documentation when it is
- a temporary draft,
- personal working notes,
- a disposable intermediate file,
- or a local-only experiment record.

## Ignored patterns
- `docs/**/*.tmp.md`
- `docs/**/*.local.md`

Do not use those suffixes for formal documentation.

## Naming

```text
Long-lived documents: topic.md
One-off reports: yyyyMMddHHmmssfff_topic.md
```

## Maintenance notes
- Add important governance material under `docs/` instead of expanding the README indefinitely.
- Keep version history in `CHANGELOG.md`.
- Keep architecture notes in `ARCHITECTURE.md`.
- Keep collaboration rules in `CONTRIBUTING.md`.
- Put unreleased change details in `CHANGELOG.md`; keep the README focused on navigation and summary.

## Contact
- WeChat: add `ProsperGao` to join the group, and please mention your purpose
- Telegram group: [CodexManager TG group](https://t.me/+OdpFa9GvjxhjMDhl)
