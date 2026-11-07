# Codex compatibility follow-up, 2026-11-06

At the time of the original review, the stable GitHub release and npm package were both verified as **0.167.0**. This review compares pinned commit [`086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`](https://github.com/openai/codex/tree/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a) with the previous 0.164 baseline, `6aaac694803004a63c6f393d153c96d22521a6b9`.

The original research and historical tests below cover the complete working tree then at `D:\Data\Code\Codex-Manager`, including pre-existing 0.164 compatibility changes. A later isolated commit transferred only this review's five corrections. The current integration also includes those earlier changes and background model discovery. Each recorded result applies to its own tested source, rather than the current integrated tree or an already published binary.

## Integration scope and historical evidence

The current integration starts from main at `746d938a`, retains published fixes, and includes the original workspace's previously uncommitted 0.164 compatibility changes alongside [background model discovery (#515)](model-discovery-background.md). The earlier changes are detailed in the [historical 0.164 review](gateway-vs-codex-headers-and-params.md).

The integrated default Codex User-Agent is **0.164.0**. Refresh requests use JSON without the fixed scope that could narrow the original grant. Preservation of `stream_options`, `access_programs`, `reasoning.context`, `include`, and per-frame `client_metadata`, along with skill parsing, read-only skill discovery, and compressed-history repair, is included. Updating the fallback version does not establish complete 0.167 compatibility.

The 2,169-test result and six successful online generations for the original workspace, plus the 2,207-test result for the earlier isolated tree based on `08ccc932`, are retained below as historical evidence. They must not be presented as validation of the current integrated tree.

## Findings and corrections

The HTTP and WebSocket top-level request declarations retain the same 15 and 17 fields between these tags. Most additions concern workspace switching, connection routing, Guardian metadata, and provider capabilities rather than universally required inference parameters.

| Area | Correction |
| --- | --- |
| WebSocket `stream` | Preserve explicit native values; default older omitted values to true. HTTP-to-WS always requests upstream streaming, including when downstream output is aggregated. This field already existed in 0.164. |
| Native WS identity headers | Emit canonical `session-id` and `thread-id`, preserving separate session, thread and request identities and existing cross-account stripping. Accept legacy incoming session spelling. |
| Window identity | Preserve a valid incoming window ID even when its thread prefix differs from cache/session affinity. Invalid header values and account changes still use the existing fallback rules. |
| Token refresh | Do not automatically replay a rotating refresh grant after an ambiguous transport failure. A single refresh attempt sends one POST. |
| Selected workspace | Codex can retain the old ID token after changing its access token and selected account. Import and subsequent metadata resolution now use the access-token workspace only when both tokens identify the same user and the selected account matches the access-token claim. AgentIdentity also uses the confirmed workspace's FedRAMP flag; conflicting cached identities are registered anew rather than relabeled. |

The workspace issue is directly exposed by the new upstream switching feature. The other corrections address pre-existing gaps found during this review.

Previously misimported accounts require reimporting the official auth file with `tokens.account_id`; incomplete historical records are not automatically reassigned. The refresh fix removes immediate replay within one ambiguous attempt, not later scheduled refreshes or user retries.

## Additions and remaining boundaries

The upstream findings apply to the pinned tags. These implementation boundaries remain relevant to the current integration and do not establish support for unimplemented upstream features.

- `current_workspace_id`, `login_hint`, and `connection` were added for workspace-login flows. Ordinary login must not add them indiscriminately. This change accepts switched auth files; it does not introduce a switching UI.
- `ChatGptRoutingAffinity` adds connection-owned routing pins from `__oailb` / `__cflb` handshake cookies. This is not implemented here. Endpoint, expiration, account, proxy, reconnect, and HTTP/WS ownership need a separate design. The infrastructure-cookie allowlist already existed in 0.164; unrestricted cookie forwarding is not equivalent.
- `guardian_recorded_items` and Guardian response assessments are new. Existing generic metadata and native event forwarding retain unknown data, but that does not establish full Guardian routing support.
- `capabilities.cyber_access_programs` controls provider support for explicit `access_programs.cyber`; it does not grant authorization. The integrated implementation preserves explicit request fields without enabling capabilities automatically.
- GCP `project_id`, `location`, and `credential_file` are new provider settings, outside this ChatGPT account-pool correction.
- Optional `refresh_reason` diagnostics already existed in 0.164 and remain omitted. Conditional `x-codex-guardian`, `x-openai-memgen-request`, and `x-codex-routing-hint` also predate 0.167 and remain outside the current controlled header list. Routing hints must agree with the final model/tier; request-role headers must correspond to actual behavior.
- New account-list fields (`can_access_with_session`, `sso_connection_name`, `is_deactivated`, `is_fedramp_compliant_workspace`) support the workspace feature; no UI expansion is included.
- Gateway User-Agent generation and npm version synchronization differ from the official compile-time client identity. HTTP conditionally preserves the incoming originator, while native WS uses the global originator. The integrated fallback is 0.164; changing a version string does not establish protocol parity.
- The HTTP helper still derives `thread-id` from `x-client-request-id`. Official clients normally use the same value, but custom clients can differ. Instruction promotion, model/tier overrides, Chat Completions adaptation, account selection and session recovery remain intentional gateway behavior.
- The FedRAMP correction applies to the existing AgentIdentity path. Ordinary OAuth Bearer fallback still lacks complete FedRAMP routing; this is not full cross-mode FedRAMP support. The main plan-resolution path already prefers the access token and is unchanged.

Protocol compatibility cannot guarantee avoidance of account restrictions. Client source does not expose the complete server-side decision process; missing optional diagnostics or differences in request shape do not prove the cause of a 401/403/429 response.

## Pinned evidence

- [Request structures](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/codex-api/src/common.rs), [client construction](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/client.rs), [identity headers](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/codex-api/src/requests/headers.rs)
- [Workspace switching](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/auth/manager/workspace_switching.rs), [login fields](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/server.rs), [OAuth requests](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/login/src/oauth/client.rs)
- [Turn metadata](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/core/src/responses_metadata.rs), [routing pins](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/http-client/src/chatgpt_routing_affinity.rs), [cookie boundaries](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/http-client/src/chatgpt_cloudflare_cookies.rs)
- [Provider capabilities](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/model-provider-info/src/capabilities.rs), [provider settings](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/model-provider-info/src/lib.rs)
- [Full tag comparison](https://github.com/openai/codex/compare/rust-v0.164.0...rust-v0.167.0), [Chinese report](../../zh-CN/report/codex-upstream-alignment-20261106.md)

## Historical validation of the complete original workspace

These results were obtained before isolating the commit on `08ccc932`. The tested source included the pre-existing 0.164 changes as well as this review's corrections.

| Command | Final result |
| --- | --- |
| `cargo test -p codexmanager-service --locked --offline --no-fail-fast` | Exit 0; 2,169 passed, 0 failed, 11 ignored under existing configuration. Includes 1,993 library tests, 75 gateway end-to-end tests, 50 RPC tests, and 51 other service tests. |
| `cargo fmt --all -- --check` | Exit 0. |
| `git diff --check` | Exit 0. |

The original review's Rust changes were confined to the service package, so its complete suite was run. Coverage includes Responses / Chat Completions, SSE / non-streaming output, tools, HTTP / WS, account switching, authentication and usage. Two assertions discovered in the first run still required the old WS behavior; they were updated against the pinned upstream contract before the successful full rerun above.

New tests use local HTTP / WS fixtures and temporary or in-memory databases, including a fully delivered refresh grant with a lost response, both workspace-switch directions, cross-user rejection, and re-registration of conflicting cached identities. Authentication and protocol changes also received independent static cross-review.

Local fixtures cannot establish online outcomes; the authorized Free-account checks are recorded below. Production FedRAMP routing and server-side restriction behavior remain unverified. Frontend and installer builds were not run in this historical validation because the review did not modify the frontend or desktop shell. These results do not establish the final commit's test status.

## Historical validation of the earlier isolated commit

The earlier isolated tree, based on `08ccc932` and containing only this review's five corrections, completed the following validation:

| Command | Final result |
| --- | --- |
| `cargo test -p codexmanager-service --locked --offline --no-fail-fast --target-dir D:/Data/Code/Codex-Manager/target` | Exit 0; 2,207 passed, 0 failed, 11 ignored under existing configuration. Includes 2,018 library tests, 88 gateway end-to-end tests, 50 RPC tests and 51 other service tests. |
| `cargo fmt --all -- --check` | Exit 0. |
| `git diff --check`, `git diff --cached --check` | Exit 0. |

That historical tree retained the then-current main-branch fixes and added only this review's corrections. Conflict resolution preserved WebSocket control-frame tests, and the new lost-response refresh test accepted both form and JSON encoding. Its staged-file credential checks found no matches, original worktree hashes remained unchanged, and no frontend/installer artifacts or release tag were produced. The current integration has a broader scope and requires its own checks.

The online checks below still apply only to the identified original-workspace binary; they have not been repeated against the current integrated tree.

## Historical authorized Free-account checks

On 2026-11-06, 22:23–22:27 Asia/Shanghai, the service built from the complete original workspace, including the pre-existing 0.164 changes, was tested with a separate temporary database, CODEX_HOME and API key. It was not built from the isolated publication tree. The normal account database and original credential file were left untouched. Binary SHA-256: `af88ce2c861e4dfb2830bbd7241dae4d5ff4bb9961a7d0c6ab26599686e344b3`.

| Check | Observed result |
| --- | --- |
| Import and usage | One Free account imported; the final usage lookup succeeded and showed available quota. |
| Account model catalog | A single direct GET with the existing access token returned five models; `gpt-6-luna` was selected. This did not validate the application's `account/fetchModels` exchange path. |
| Responses, non-streaming | HTTP 200, `completed`, exact `CM_RESPONSES_OK` output. |
| Responses, SSE tools | HTTP 200, `response.completed`, expected function name and arguments. |
| Chat Completions, non-streaming | HTTP 200, exact `CM_CHAT_OK` output. |
| Chat Completions, SSE tools | HTTP 200, `finish_reason=tool_calls`, `[DONE]`, expected function name and arguments. |
| Native WebSocket, two turns | Both turns reached `response.completed`; the second used the first response's `previous_response_id`. Exact outputs were `CM_WS_OK` and `CM_WS_SECOND_OK`. |

All six generations succeeded, reporting 480 tokens in total. Tool checks validated generated call structures without executing external business tools. The final local account state was `active`, with plan `free`. Credentials rotated during the test; the latest access, ID and refresh tokens were exported, matched against the database, and saved separately in the user's Downloads directory as `codexmanager-free-verified-20261106.json`. The original file was preserved; subsequent imports should use the new file because rotation can invalidate the old refresh token. The temporary API key was deleted and the test service stopped.

Earlier attempts encountered a region restriction, then TLS handshake failures after the user changed networks. These checks succeeded after the user confirmed restored connectivity and the isolated service rebuilt its connections. Those earlier failures are not evidence of an account ban. The successful requests establish current access for this account and build, not long-term account safety, access to every model, the complete login flow, or production restriction behavior.
