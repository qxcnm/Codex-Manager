# Opt-in model-route scheduling (#537)

Open **Models → Edit → Routes → Model route policy**. `legacy` (or an omitted
`capabilities.route_policy`) preserves existing Key routing. No schema migration
or new environment variable is needed; SQLite and SeaORM use the same validation.

| Policy | Hybrid Key behavior |
| --- | --- |
| `account_first` | Account branches before aggregate branches |
| `aggregate_first` | Aggregate branches before account branches |
| `weighted` | One cross-source priority order |

Within each source preference, higher `priority` runs first. Equal-priority
branches use smooth weighted round-robin for the initial selection; subsequent
fallback never repeats a failed branch. Branch identity is
`(source_kind, source_id, upstream_model)`, so multiple upstream models on the
same aggregate connection remain independent. Dragging routes assigns distinct
priorities: set equal priorities explicitly when weighted traffic sharing is wanted.

Dedicated account/aggregate Keys retain their source restriction. An explicit
`aggregateApiId` restricts the new scheduler to that connection, not merely a
preferred first connection. Model policies cannot bypass Key ownership, account
group/plan restrictions, model permissions, quota, wallet or admission checks.

All branches share one inbound deadline and a 16-transport-attempt budget,
including executor-local retries and BPS handshakes. Before delivery, connection
failures and HTTP 408/429/5xx can fall back. Ordinary 4xx, ambiguous delivery
after a submitted POST, downstream cancellation, or already-delivered content
and tool calls do not trigger model-route replay. Existing account-pool recovery
still applies inside an account branch. On a scheduled Chat Completions SSE stream,
HTTP 200 can still be followed by a structured transient error. A heartbeat or
assistant-role-only chunk does not commit the source; text, tools, usage,
finish reasons and unknown data do. This also applies when Chat is the upstream
format of a Responses request.

State is per process and effective model/tier candidate set, not per Key. It is
bounded to 512 entries, expires after 30 idle minutes and resets on restart.
Opt-in models accept at most 256 routes. Weights describe long-run first-hop
shares, not an exact per-request, cross-instance or successful-request ratio.

HTTP Responses/Chat JSON and SSE and the Responses WebSocket HTTP bridge use
the same scheduler. If the enabled catalog contains an opt-in account-pool
model, new account-only Key WebSocket sessions also use per-turn HTTP bridging
so model changes within a socket remain supported. Otherwise official native
WebSockets are retained. Reconnect existing sockets after changing this setting.
