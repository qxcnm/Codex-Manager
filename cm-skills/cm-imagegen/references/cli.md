# CodexManager image CLI reference (`scripts/cm_image_gen.py`)

Use the bundled CLI for both supported operations:

- `generate`: create an image from a prompt.
- `edit`: edit one or more local images, with an optional mask.

The CLI uses Python 3.10+ and the standard library only. Python 3.11+ uses the full standard TOML parser; Python 3.10 supports simple unquoted configuration tables.

## Endpoint and credentials

The default configuration directory is `$CODEX_HOME`, or `~/.codex` when unset. The CLI reads `model_provider` from `config.toml`; a provider override in the selected `[profiles.<profile>]` takes precedence when the top-level `profile` is set. It sends requests to that provider's `base_url`.

Credential order:

1. The active provider's `experimental_bearer_token`.
2. The environment variable named by the active provider's `env_key`. A configured but unset variable is an error.
3. The legacy top-level `experimental_bearer_token`.
4. `OPENAI_API_KEY` in `auth.json`, for legacy API key configurations.

ChatGPT login tokens are not platform keys. The CLI does not modify `auth.json` or `config.toml`; gateway configurations can preserve ChatGPT sign-in or omit `auth.json` entirely. A CodexManager platform key in `experimental_bearer_token` is sufficient.

## Commands

PowerShell:

```powershell
$codexConfigDir = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HOME '.codex' }
$imageCli = Join-Path $codexConfigDir 'skills/cm-imagegen/scripts/cm_image_gen.py'
python $imageCli generate --prompt 'A cozy alpine cabin at dawn'
python $imageCli edit --image 'D:/assets/input.png' --prompt 'Replace only the background with a warm sunset'
```

Use `python3` on systems where that is the installed interpreter name.

## Defaults and options

- Model: `gpt-image-2` for OpenAI-compatible Images endpoints; `image-01` for MiniMax endpoints. `--model` takes precedence over `CODEXMANAGER_IMAGE_MODEL`.
- Size: `1024x1024`; use `--size` to override.
- Response format: `b64_json`. The OpenAI-compatible path saves `data[].b64_json`; URL responses are not saved by that path. MiniMax supports `url` and `base64` and converts `b64_json` to `base64`.
- Output directory: `generated-images/` under the current working directory, overridden by `CODEXMANAGER_IMAGE_OUTPUT_DIR` or `--out-dir`.
- Filename: generated automatically, overridden by `--filename`. Existing paths receive a timestamp suffix.
- Timeout: 600 seconds, overridden by `--timeout`.
- Shared optional arguments: `--n`, `--quality`, `--background`, `--output-format png|jpeg|webp`.
- Edit arguments: repeat `--image` for multiple images; add `--mask` for a local mask.
- MiniMax arguments: `--aspect-ratio`, `--width`, `--height`, `--seed`, `--prompt-optimizer true|false`.
- `CODEXMANAGER_IMAGE_BASE_URL` overrides the provider's base URL while retaining the credential selection above.

This CLI has no `generate-batch`, `--dry-run`, `--out`, `--input-fidelity`, or prompt augmentation flags. Put augmented prompts in `--prompt`; use repeated CLI calls for multiple assets.

## Results and errors

Successful calls print JSON with `ok`, the effective `model`, and absolute output `paths[]`. Credentials are never included. Use the saved file as the image result.

An HTTP error includes the upstream response body. A `model_not_found` response can indicate a wrong image model, gateway configuration, or upstream availability; it does not by itself establish that all image generation is unsupported. Check the actual requested model and Images configuration before changing the model. The text model catalog is not a complete image-capability probe.

## Local regression tests

```powershell
python -m unittest discover -s cm-skills/cm-imagegen/tests -v
```

Tests use temporary configuration and a local mock HTTP server. They do not contact any provider or require an account.
