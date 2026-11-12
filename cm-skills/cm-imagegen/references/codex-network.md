# Codex network approvals / sandbox notes

This file covers network access for `scripts/cm_image_gen.py`.

Network policy varies by environment. Follow the current environment's rules.

## Why am I asked to approve image generation calls?
The CLI contacts the configured provider endpoint, which may be a local CodexManager gateway or a remote provider. Network restrictions may apply to either. Credentials come from the active provider configuration or legacy `auth.json` API key as described in `cli.md`; a separate official OpenAI API key is not required for CodexManager gateway use.

## Important note about approvals vs network
- `--ask-for-approval never` suppresses approval prompts.
- It does **not** by itself enable network access.
- In `workspace-write`, network access still depends on your Codex configuration (for example `[sandbox_workspace_write] network_access = true`).

## How do I reduce repeated approval prompts?
If you trust the repo and want fewer prompts, use a configuration or profile that both:
- enables network for the sandbox mode you plan to use
- sets an approval policy that matches your risk tolerance

Example `~/.codex/config.toml` pattern:

```toml
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = true
```

If you want quieter automation after network is enabled, you can choose a stricter approval policy, but do that intentionally and with care.

## Safety note
Enabling network and reducing approvals lowers friction, but increases risk if you run untrusted code or work in an untrusted repository.
