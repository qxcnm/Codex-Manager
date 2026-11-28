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

## MiniMax H3 text-to-image

Select `MiniMax-H3-Image` or `MiniMax-H3-Image-Highspeed` with `generate --model` or `CODEXMANAGER_IMAGE_MODEL`.
Use the base URL for the credential region, `https://api.minimax.io/v1` or `https://api.minimax.cn/v1`.
The CLI sends one POST to `<provider.base_url>/images/generations` with a timeout of at least 600 seconds.

- Set `--size WIDTHxHEIGHT` or `--aspect-ratio X:Y`. Omitting both lets H3 choose the dimensions.
- Set `--n` from 1 to 10. Use `--output-format` for the encoded file format.
- The default `--response-format b64_json` saves inline images. With `--response-format url`, the CLI downloads each successful image.
- For a mixed response, `paths` contains the saved images and `errors` lists each failed item's zero-based index and provider error. The CLI includes the `request_id` for troubleshooting.

## Legacy MiniMax image generation API

For other models, when the provider base URL targets `minimax.io` or `minimaxi.com`, the CLI uses `POST <provider.base_url>/image_generation` for generate and edit. The default model is `image-01`.

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
