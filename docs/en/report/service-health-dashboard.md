# Service health dashboard

Open **Service Health** in the sidebar. Desktop and Web users, including members, administrators and system administrators, see the same sanitized summary for their CodexManager service.

## Request observations

- The dashboard samples up to the latest 2,000 inference requests from the last 60 minutes and indicates when the sampling limit is reached. It reads existing request logs and does not call models proactively.
- Codex, Claude and unknown sources have separate request counts, sample success rates, first-response latency, error categories and user reports. Classification uses the actual routed source, not the client's protocol path. Uncertain errors remain unclassified.
- Sample success rate is successful requests divided by all sampled requests. Rate limits, local/client errors and network errors are counted separately and do not establish an official outage. First-response statistics use successful requests with recorded latency; P95 is calculated from sorted samples.
- Insufficient observations show an unknown status. Storage failures show an error rather than a healthy result. The visible page refreshes every 15 seconds and supports manual refresh.

## Incident reports

Select **Report an incident**, then choose a provider, symptom and broad region. Reports expire after 30 minutes; mark your own report resolved when service recovers. Repeated reports from the same user do not increase the count of independent reporters, and frequent changes are rate-limited.

Other users see aggregate counts and broad regions, not your report identity, and cannot resolve your reports. Existing request-log permissions remain in place. Shared responses contain no account details, keys, request content, upstream addresses or raw errors.

## Coverage

The dashboard covers this service only. It does not send health data externally or share across independent deployments. Local observations and reports can indicate a problem affecting this service, but cannot establish an official regional or global outage.
