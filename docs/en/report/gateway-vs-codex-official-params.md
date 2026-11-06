# Historical Differences from Official Codex

This is a historical workspace snapshot. In particular, the statements below about fixed originator values, session-header spelling and complete alignment are not current guarantees. Refer to the [2026-11-06 upstream audit](codex-upstream-alignment-20261106.md), including its separate publication scope and validation evidence.

This document keeps only the remaining request-header differences that are still worth tracking.

## `/v1/responses` request headers

| Field | Official Codex | Current gateway | Difference |
| --- | --- | --- | --- |
| `Authorization` | `Bearer <official account token>` | `Bearer <current account token>` | The gateway replaces the account token |
| `User-Agent` | `codex_cli_rs/<compile-time version> (<os/version; <arch>) <terminal>` | `codex_cli_rs/<database-configured version> (<os/version; <arch>) <terminal>` | Official Codex uses `env!("CARGO_PKG_VERSION")`; the gateway currently reads this from a configurable database field |
| `x-client-request-id` | Always equals `conversation_id` | Prefer the thread anchor | When switching accounts or threads, it becomes the new thread anchor |
| `session_id` | Always equals `conversation_id` | Prefer the thread anchor | Not sent for normal `/responses` traffic when there is no thread anchor |
| `x-codex-turn-state` | Replayed within the same turn | Replayed while the same thread remains stable | Dropped proactively when switching accounts or replacing the thread |

## Current conclusion

1. These five request-header / transport-layer behaviors are still the main gaps worth tracking.
2. The `gatewayOriginator` setting is still stored locally, but it no longer affects the actual outbound `originator`. Outbound traffic now always uses the official default value: `codex_cli_rs`.
3. For `User-Agent`, official Codex uses the compile-time package version, while the current gateway allows the version segment to be configured from the database.

## Source references

- Official `codex`
  - `D:\MyComputer\own\GPTTeam相关\CodexManager\codex\codex-rs\core\src\client.rs`
  - `D:\MyComputer\own\GPTTeam相关\CodexManager\codex\codex-rs\codex-api\src\endpoint\responses.rs`
  - `D:\MyComputer\own\GPTTeam相关\CodexManager\codex\codex-rs\codex-api\src\requests\headers.rs`
  - `D:\MyComputer\own\GPTTeam相关\CodexManager\codex\codex-rs\core\src\default_client.rs`
- Current gateway
  - [transport.rs](../../../crates/service/src/gateway/upstream/attempt_flow/transport.rs)
  - [codex_headers.rs](../../../crates/service/src/gateway/upstream/headers/codex_headers.rs)
  - [runtime_config.rs](../../../crates/service/src/gateway/core/runtime_config.rs)
