# Gateway boundaries compared with official Codex

See the [2026-11-06 follow-up](codex-upstream-alignment-20261106.md) for the 0.167 source comparison and current corrections. The table below describes the earlier 0.164 review.

Baseline: Codex `rust-v0.164.0`, reviewed 2026-10-26. See the [compatibility report](gateway-vs-codex-headers-and-params.md) for fixes, pinned sources and remaining scope. Older versions of this report relied on an unversioned local source checkout; those comparisons have been replaced.

| Behavior | Official Codex | CodexManager |
| --- | --- | --- |
| Authorization | Credentials for the selected provider/account. | Resolves credentials for the routed account; local API keys are not forwarded as account tokens. |
| Client version | Compile-time package version. | Configurable runtime version, npm synchronization, and a `0.164.0` fallback. |
| Originator and User-Agent | Built from the Codex client context. | Existing runtime settings remain effective. Token-refresh and usage clients retain their dedicated standard User-Agent behavior. |
| Session affinity | Context for a single Codex conversation. | May discard account-specific affinity when changing accounts or recovering a conversation. |
| Model and service tier | Selected by the client/provider. | May be overridden by local API-key and routing policy. |
| HTTP transport | Codex Responses requests stream upstream. | Compatibility requests can be converted to upstream SSE and reconstructed as non-streaming Responses or Chat Completions for the caller. |
| Model catalog | Provider-owned Codex metadata. | Official account-pool objects pass through; managed catalogs use the gateway's configured models and neutral instruction policy. |

These are product boundaries, not differences to remove indiscriminately. New Codex fields must be evaluated against both supported OpenAI-style endpoints, streaming/non-streaming output, tools, account switching and WebSocket reuse.
