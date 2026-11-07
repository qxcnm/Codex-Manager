# Codex compatibility baseline

Follow-up: [2026-11-06 review against Codex 0.167.0](codex-upstream-alignment-20261106.md) records subsequent findings, corrections and remaining differences. The dated 0.164 record below is retained as history.

Reviewed on 2026-10-26 against the latest stable release, [rust-v0.164.0](https://github.com/openai/codex/releases/tag/rust-v0.164.0), commit `6aaac694803004a63c6f393d153c96d22521a6b9`. The npm `@openai/codex` latest version was also `0.164.0`. This is a pinned integration review, not a claim of parity with every Codex feature or its moving `main` branch.

## Changes from the previous implementation

| Integration | Previous behavior | Updated behavior |
| --- | --- | --- |
| Responses HTTP and WebSocket | The Codex field filter dropped `stream_options` and `access_programs`. | Both fields survive native and compatibility rewriting, including HTTP-to-WebSocket conversion. The gateway does not invent access selections; upstream authorization still applies. |
| WebSocket turn metadata | Handshake headers overwrote metadata supplied by subsequent `response.create` frames. | Frame metadata wins; handshake headers fill missing entries for older clients. |
| OAuth refresh | Form body with a fixed `scope=openid profile email`. | JSON body with `client_id`, `grant_type`, and `refresh_token`, without reducing the original grant's scopes. |
| Skill manifests | Line parsing lost multiline descriptions and could read nested metadata as top-level fields. | Full YAML parsing, normalized multiline text, bounded prose repair matching Codex, required description, and a 64-character name limit. |
| User skill discovery | Only `$CODEX_HOME/skills` appeared in the inventory. | Also lists the service user's `~/.agents/skills` as read-only entries; installation and removal stay in the managed root. |
| History visibility repair | Only plain `.jsonl` files were scanned. | Recognizes `.jsonl.zst` histories, preserves their compressed format and original backups, and coordinates replacement with Codex's maintenance/writer locks. |
| Default client version | `0.155.0`. | `0.164.0`; existing runtime configuration and npm version synchronization continue to apply. |

## Contracts checked and retained

- Provider configuration still uses `wire_api = "responses"`. The provider ID and its display name remain separate. Existing bearer-token, model-catalog and WebSocket settings are supported by this release.
- Official account-pool model catalogs preserve raw model objects, including unknown fields. Managed catalogs continue using neutral instructions and disabling Responses Lite without official prompt metadata.
- `state_5.sqlite` remains Codex's state database filename.
- WebSocket fork revisions still match upstream: `tokio-tungstenite` `0e5b2d73aa18dd9f0a50ee9ff199d5aef7594186` and `tungstenite` `4fffad30fe373adbdcffab9545e9e9bf4f2fc19f`.
- OAuth login scopes, AgentIdentity registration/assertions, usage account/residency headers and Luna Reserve opt-in headers were compared with the pinned source.
- The legacy neutral top-level `instructions` compatibility field remains for older clients. Codex 0.164 constructs its own instructions within `input`.

## Deliberate differences and remaining scope

The gateway owns account selection, model/tier overrides and session-affinity recovery. These differ from a single-account Codex client by design; see [the boundary notes](gateway-vs-codex-official-params.md).

The new `x-codex-routing-hint` header is a model/tier pre-routing hint. Blind forwarding could disagree with gateway overrides, so it is not added in this change. Personal Access Token and AgentIdentity JWT login modes are outside the existing account import contract. The additional rate-limit `normal_model_slug` metadata is not currently surfaced.

Skill imports continue requiring an explicit safe name, although Codex can use the directory name as a fallback. Installation and removal remain confined to the existing managed skill root.

## Pinned upstream evidence

- [HTTP/WS request types and stream options](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/codex-api/src/common.rs)
- [Canonical per-turn metadata](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/core/src/responses_metadata.rs)
- [ChatGPT refresh encoding](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/login/src/auth/manager.rs) and [OAuth request fields](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/login/src/oauth/client.rs)
- [Skill YAML parser](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/skills/src/parser.rs)
- [Current and legacy user skill roots](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/ext/skills/src/host_roots.rs)
- [Model provider settings](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/model-provider-info/src/lib.rs) and [model catalog compatibility](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/protocol/src/openai_models.rs)
- [State database version](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/state/src/sqlite.rs) and [dependency pins](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/Cargo.toml)
- [Compressed rollout format](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/rollout/src/compression.rs), [maintenance lock](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/rollout/src/maintenance.rs) and [writer coordination](https://github.com/openai/codex/blob/6aaac694803004a63c6f393d153c96d22521a6b9/codex-rs/rollout/src/writer_lock.rs)
- [Official configuration reference](https://developers.openai.com/codex/config-reference)

For future updates, pin the next stable tag and commit, compare these integration points, preserve account and filesystem boundaries, and run protocol regressions before updating this baseline. Updating the advertised client version alone does not update protocol support.
