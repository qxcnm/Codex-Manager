# CodexManager image API reference

These are the endpoints and arguments used by `scripts/cm_image_gen.py`. They are separate from the official built-in `image_gen` tool.

## OpenAI-compatible Images endpoints

- Generate: `POST <provider.base_url>/images/generations`.
- Edit: `POST <provider.base_url>/images/edits`.
- The configured base URL should already include its API prefix, normally `/v1`.
- The image model defaults to `gpt-image-2`. Use `--model` or `CODEXMANAGER_IMAGE_MODEL` only when a different image model is configured at the provider.

Shared CLI request fields:

- `prompt`: text prompt.
- `model`: image model, independent of the text model in Codex configuration.
- `n`: number of images.
- `size`: defaults to `1024x1024`.
- `response_format`: defaults to `b64_json`.
- `quality`, `background`, `output_format`: passed only when supplied.

For edits the CLI sends JSON `images: [{"image_url": "data:image/...;base64,..."}]`; each repeated `--image` adds an input. `--mask` becomes `mask.image_url`. The gateway also accepts multipart edits from other clients.

The OpenAI-compatible CLI path saves `data[].b64_json` and reports absolute file paths. It does not save URL-only responses. Image option support and limits depend on the gateway and selected upstream model; check the actual error before retrying with different options.

## MiniMax image generation API

When the provider base URL targets `minimax.io` or `minimaxi.com`, the CLI uses `POST <provider.base_url>/image_generation` for generate and edit. The default model is `image-01`.

Request fields:

- `model`, `prompt`.
- `aspect_ratio`, `width`, `height`, `seed`, `n`, `prompt_optimizer` when supplied.
- `response_format`: `url` or `base64`; the CLI converts its default `b64_json` to `base64`.
- For edit, `subject_reference` contains one `{ "type": "character", "image_file": "<data URL>" }` per `--image`.

Response fields:

- `data.image_urls`: image URL strings, downloaded by the CLI.
- `data.image_base64`: base64 image strings, decoded by the CLI.
- `metadata.success_count` / `metadata.failed_count`.
- `base_resp.status_code`: zero on success; a non-zero value is reported as an error.

OpenAI-specific `quality`, masks, `background`, and `output_format` are not sent on the MiniMax path.

For endpoint configuration, credential selection, and all CLI arguments, see `cli.md`.
