# SECURITY

## Support range

CodexManager We are still iterating rapidly, but we will try our best to deal with security issue reports within a reasonable range.

Priority issues include:

- Risk of leakage of account token, RPC token, and platform key
- Web Access password bypass
- Unauthorized access to local gateway
- Sensitive log leaks
- High-risk remote exploitation issues affecting desktop, service, and web terminals

## Reporting method

Please do not directly disclose reproducible sensitive vulnerability details, valid tokens or account data in public issues.

It is recommended to contact the maintainer privately via:

- GitHub Warehouse owner home page contact information
- Established private communication channels

If contact can only be made through Issues for the time being, please only describe the scope of impact and do not include:

- valid access token
- refresh token
- id token
- RPC token
- API key plain text
- Directly reusable attack scripts

## What we want to receive

Please try to provide:

1. Scope of influence
2. Reproduction steps
3. Trigger condition
4. expected results vs. actual results
5. Do you need to log in / Do you need local network conditions?
6. Log fragments or screenshots (note desensitization)

## Sensitive information handling conventions

Before submitting logs, screenshots, and configurations, please desensitize the following content:

- `Authorization` Head
- `Cookie`
- `Set-Cookie`
- `access_token`
- `refresh_token`
- `id_token`
- `CODEXMANAGER_RPC_TOKEN`
- Any directly reusable agent account or platform key

## Security constraints in the repository

- Do not submit real tokens, passwords, cookies, and API keys to the repository.
- Do not post sensitive values ​​that can be used directly in README, Issue, PR, or documents.
- When adding new logs, priority is given to recording structured context and plain text sensitive data is not output directly.
- When adding setting items, distinguish between "configurations suitable for persistence" and "sensitive values ​​that can only be provided in environment variables".

## Current known boundaries

- Account tokens are encrypted in SQLite with AES-256-GCM. On macOS and Windows the master key prefers the operating-system credential store, and it can also be supplied through `CODEXMANAGER_TOKEN_ENCRYPTION_KEY_FILE` or `CODEXMANAGER_TOKEN_ENCRYPTION_KEY`.
- The `codexmanager.token-key` fallback used when no credential store is available only reduces the risk of the database file being copied by itself. It does not protect against compromise of the whole data directory or logged-in OS account. Production service and Docker deployments should use a separate secret manager.
- Existing tokens cannot be recovered if the master key is lost. Never commit the key or include it in logs or the same unprotected backup as the database.
- The project defaults to local deployment and self-hosted usage scenarios, and does not promise fully automatic security hardening when exposed to the public network.
- If `0.0.0.0` monitoring is enabled or Web/service is exposed to the LAN or public network, the deployer needs to bear the risk of network exposure at its own risk and cooperate with:
  - Strong password
  - Reverse proxy access control
  - network boundary restrictions

## Response Principles

- Issues that can be identified will be reproduced as much as possible and their impact levels assessed.
- After the repair is completed, it will be released in an appropriate version, and sensitive details will not be exposed in advance through public channels.
- If the problem does not belong to the repository itself, but to improper deployment configuration, a boundary description will also be given.
