---
name: "cm-imagegen"
description: "Generate or edit raster images through CodexManager's OpenAI-compatible Images API using the current Codex provider base_url and provider credentials. Use when the user asks for CodexManager image generation, API-key/provider-mode image generation, or image generation through CodexManager account pools, routing, logs, and billing."
---

# CM Image Generation Skill

Generates or edits images for the current project (for example website assets, game assets, UI mockups, product mockups, wireframes, logo design, photorealistic images, or infographics).

This skill follows the same prompt workflow, decision tree, and output expectations as the official `imagegen` skill. Image requests use the bundled CodexManager CLI and the current Codex provider's endpoint and credentials; its supported arguments are documented in `references/cli.md`.

## Top-level modes and rules

This skill has one top-level mode:

- **CodexManager CLI mode:** bundled `scripts/cm_image_gen.py` CLI. It uses the current Codex configuration and calls CodexManager's image endpoints.

The CLI exposes two subcommands:

- `generate`
- `edit`

Rules:
- Use this skill only when the user explicitly asks for CodexManager image generation or when the official built-in `image_gen` tool is unavailable in API-key/provider mode.
- Do not ask the user for a separate image API key or image base URL by default.
- Do not create one-off SDK runners.
- Never modify the official system `imagegen` skill.

CodexManager config policy:
- If `CODEXMANAGER_IMAGE_BASE_URL` is set, use it as the image request base URL.
- Otherwise, read the active `model_provider` from `$CODEX_HOME/config.toml`, including a `model_provider` override in the selected `[profiles.<profile>]` when the top-level `profile` is set.
- Read that provider's `base_url` from `[model_providers.<provider>]`.
- Read the active provider's `experimental_bearer_token` first, then the environment variable named by that provider's `env_key`. Only when neither is configured, fall back to the legacy top-level `experimental_bearer_token`, then `OPENAI_API_KEY` in `$CODEX_HOME/auth.json`.
- An explicitly configured but unset `env_key` is a configuration error; do not substitute another provider's key. Never use ChatGPT login tokens as platform keys, and do not modify `auth.json` or ask the user to switch authentication modes to use this skill.
- Use `CODEXMANAGER_IMAGE_MODEL` only for the image model override; otherwise use `gpt-image-2` for OpenAI-compatible Images providers, or `image-01` when the provider base URL targets the image_generation API (`minimax.io` / `minimaxi.com` hosts).
- Use the CLI's configured image model by default. Do not copy a different default from the official imagegen CLI, infer that Images API is unavailable from a text-only model list, or interpret a single `model_not_found` response as proof that the provider supports no image models. Report the actual endpoint/model error and check the configured image model.
- When the provider base URL targets the image_generation API, the CLI calls `POST /v1/image_generation` with `aspect_ratio`/`width`/`height`/`seed`/`n`/`prompt_optimizer`/`response_format` request fields, passes `--image` inputs as `subject_reference`, and parses `data.image_urls` / `data.image_base64` + `base_resp.status_code`; otherwise it calls the OpenAI Images `/images/generations` and `/images/edits` endpoints and parses `data[].b64_json`.
- Use `CODEXMANAGER_IMAGE_OUTPUT_DIR` only for the output directory override; otherwise save under the current working directory's `generated-images/` folder.

Save-path policy:
- CodexManager generated images are saved under the current working directory's `generated-images/` folder by default.
- If the user names a destination, use `--out-dir` and/or `--filename`, or move/copy the selected output there.
- Do not edit, create, or attach unrelated project files such as `AGENTS.md`.
- If the image is meant for the current project, keep the final selected image in `generated-images/` unless the user named another destination.
- If the image is only for preview or brainstorming, report and attach the generated PNG file from `generated-images/` so Codex App shows the file card with the Open button.
- Never report an unrelated file card as the generated image result.
- Do not overwrite an existing asset unless the user explicitly asked for replacement; otherwise create a sibling versioned filename such as `hero-v2.png` or `item-icon-edited.png`.

Shared prompt guidance lives in `references/prompting.md` and `references/sample-prompts.md`.

CLI docs/resources:
- `references/cli.md`
- `references/image-api.md`
- `references/codex-network.md`
- `scripts/cm_image_gen.py`

## When to use
- Generate a new image (concept art, product shot, cover, website hero)
- Generate a new image using one or more reference images for style, composition, or mood
- Edit an existing image (inpainting, lighting or weather transformations, background replacement, object removal, compositing, transparent background)
- Produce many assets or variants for one task

## When not to use
- Extending or matching an existing SVG/vector icon set, logo system, or illustration library inside the repo
- Creating simple shapes, diagrams, wireframes, or icons that are better produced directly in SVG, HTML/CSS, or canvas
- Making a small project-local asset edit when the source file already exists in an editable native format
- Any task where the user clearly wants deterministic code-native output instead of a generated bitmap
- When the user explicitly wants the official built-in `image_gen` workflow and that tool is available

## Decision tree

Think about two separate questions:

1. **Intent:** is this a new image or an edit of an existing image?
2. **Execution strategy:** is this one asset or many assets/variants?

Intent:
- If the user wants to modify an existing image while preserving parts of it, treat the request as **edit**.
- If the user provides images only as references for style, composition, mood, or subject guidance, treat the request as **generate**.
- If the user provides no images, treat the request as **generate**.

Edit semantics:
- Edit mode is for images already visible in the conversation context, images generated earlier in the thread, or local image files whose paths are available.
- If a local file is the edit target, pass its absolute path with `--image`.
- For edits, preserve invariants aggressively and save non-destructively by default.

Execution strategy:
- For one asset, issue one CLI call.
- For many assets or variants, issue one CLI call per requested asset or variant.

Assume the user wants a new image unless they clearly ask to change an existing one.

## Workflow
1. Decide the intent: `generate` or `edit`.
2. Decide whether the output is preview-only or meant to be consumed by the current project.
3. Decide the execution strategy: single asset vs repeated calls.
4. Collect inputs up front: prompt(s), exact text (verbatim), constraints/avoid list, and any input images.
5. For every input image, label its role explicitly:
   - reference image
   - edit target
   - supporting insert/style/compositing input
6. If the user asked for a photo, illustration, sprite, product image, banner, or other explicitly raster-style asset, use CodexManager image generation rather than substituting SVG/HTML/CSS placeholders. If the request is for an icon, logo, or UI graphic that should match existing repo-native SVG/vector/code assets, prefer editing those directly instead.
7. Augment the prompt based on specificity:
   - If the user's prompt is already specific and detailed, normalize it into a clear spec without adding creative requirements.
   - If the user's prompt is generic, add tasteful augmentation only when it materially improves output quality.
8. Call `scripts/cm_image_gen.py`.
9. Inspect outputs and validate: subject, style, composition, text accuracy, and invariants/avoid items.
10. Iterate with a single targeted change, then re-check.
11. For preview-only work, report and attach the generated PNG file from `generated-images/` so Codex App shows the file card with the Open button.
12. For project-bound work, keep the selected artifact in `generated-images/` or move/copy it only to a user-named destination, then update consuming code or references if needed.
13. For batches, persist only the selected finals in the workspace unless the user explicitly asked to keep discarded variants.
14. Keep the final response minimal. Do not print the final prompt, validation details, file size, JSON output, model, base URL, or absolute path unless the user explicitly asks for them.

## Commands

Generate:

```powershell
python "$HOME\.codex\skills\cm-imagegen\scripts\cm_image_gen.py" generate --prompt "<final prompt>"
```

Edit:

```powershell
python "$HOME\.codex\skills\cm-imagegen\scripts\cm_image_gen.py" edit --image "<absolute image path>" --prompt "<edit prompt>"
```

Useful options:

```powershell
--size 1024x1024
--response-format b64_json
--out-dir "D:\path\to\output"
--filename "asset-name.png"
--quality high
--background transparent
--output-format png
```

After a successful call:
- Parse the printed JSON.
- Use the first absolute `paths[]` item as the final image path unless the user requested multiple outputs.
- Do not call a local image viewing tool for the final answer. In Codex App this can create a broken gray preview placeholder for CLI-created local images.
- Attach/report the generated PNG file itself from `generated-images/` so Codex App shows a normal image file card with the Open button.
- Do not create a markdown image/link, HTML image tag, or fake preview block.
- Do not attach or report unrelated files such as `AGENTS.md`.
- The normal saved path should be the current working directory's `generated-images/<filename>` file, but do not print the path in the final response unless the user asks.
- Do not print the final prompt, prompt spec, validation result, file size, model, base URL, raw JSON, or command output in the final response unless the user asks.
- The final response should be only a short completion sentence plus the generated PNG file card.
- For edits, also keep track of the source image path.

## Prompt augmentation

Reformat user prompts into a structured, production-oriented spec. Make the user's goal clearer and more actionable, but do not blindly add detail.

Treat this as prompt-shaping guidance, not a closed schema. Use only the lines that help, and add a short extra labeled line when it materially improves clarity.

### Specificity policy

Use the user's prompt specificity to decide how much augmentation is appropriate:

- If the prompt is already specific and detailed, preserve that specificity and only normalize/structure it.
- If the prompt is generic, you may add tasteful augmentation when it will materially improve the result.

Allowed augmentations:
- composition or framing hints
- polish level or intended-use hints
- practical layout guidance
- reasonable scene concreteness that supports the stated request

Not allowed augmentations:
- extra characters or objects that are not implied by the request
- brand names, slogans, palettes, or narrative beats that are not implied
- arbitrary side-specific placement unless the surrounding layout supports it

## Use-case taxonomy (exact slugs)

Classify each request into one of these buckets and keep the slug consistent across prompts and references.

Generate:
- photorealistic-natural — candid/editorial lifestyle scenes with real texture and natural lighting.
- product-mockup — product/packaging shots, catalog imagery, merch concepts.
- ui-mockup — app/web interface mockups and wireframes; specify the desired fidelity.
- infographic-diagram — diagrams/infographics with structured layout and text.
- logo-brand — logo/mark exploration, vector-friendly.
- illustration-story — comics, children's book art, narrative scenes.
- stylized-concept — style-driven concept art, 3D/stylized renders.
- historical-scene — period-accurate/world-knowledge scenes.

Edit:
- text-localization — translate/replace in-image text, preserve layout.
- identity-preserve — try-on, person-in-scene; lock face/body/pose.
- precise-object-edit — remove/replace a specific element (including interior swaps).
- lighting-weather — time-of-day/season/atmosphere changes only.
- background-extraction — transparent background / clean cutout.
- style-transfer — apply reference style while changing subject/scene.
- compositing — multi-image insert/merge with matched lighting/perspective.
- sketch-to-render — drawing/line art to photoreal render.

## Shared prompt schema

Use the following labeled spec as shared prompt scaffolding:

```text
Use case: <taxonomy slug>
Asset type: <where the asset will be used>
Primary request: <user's main prompt>
Input images: <Image 1: role; Image 2: role> (optional)
Scene/backdrop: <environment>
Subject: <main subject>
Style/medium: <photo/illustration/3D/etc>
Composition/framing: <wide/close/top-down; placement>
Lighting/mood: <lighting + mood>
Color palette: <palette notes>
Materials/textures: <surface details>
Text (verbatim): "<exact text>"
Constraints: <must keep/must avoid>
Avoid: <negative constraints>
```

Notes:
- `Asset type` and `Input images` are prompt scaffolding, not dedicated CLI flags.
- `Scene/backdrop` refers to the visual setting. It is not the same as the CLI `background` parameter, which controls output transparency behavior.

Augmentation rules:
- Keep it short.
- Add only the details needed to improve the prompt materially.
- For edits, explicitly list invariants (`change only X; keep Y unchanged`).
- If any critical detail is missing and blocks success, ask a question; otherwise proceed.

## Examples

### Generation example (hero image)
```text
Use case: product-mockup
Asset type: landing page hero
Primary request: a minimal hero image of a ceramic coffee mug
Style/medium: clean product photography
Composition/framing: wide composition with usable negative space for page copy if needed
Lighting/mood: soft studio lighting
Constraints: no logos, no text, no watermark
```

### Edit example (invariants)
```text
Use case: precise-object-edit
Asset type: product photo background replacement
Primary request: replace only the background with a warm sunset gradient
Constraints: change only the background; keep the product and its edges unchanged; no text; no watermark
```

## Prompting best practices
- Structure prompt as scene/backdrop -> subject -> details -> constraints.
- Include intended use (ad, UI mock, infographic) to set the mode and polish level.
- Use camera/composition language for photorealism.
- Only use SVG/vector stand-ins when the user explicitly asked for vector output or a non-image placeholder.
- Quote exact text and specify typography + placement.
- For tricky words, spell them letter-by-letter and require verbatim rendering.
- For multi-image inputs, reference images by index and describe how they should be used.
- For edits, repeat invariants every iteration to reduce drift.
- Iterate with single-change follow-ups.
- If the prompt is generic, add only the extra detail that will materially help.
- If the prompt is already detailed, normalize it instead of expanding it.

More principles: `references/prompting.md`.
Copy/paste specs: `references/sample-prompts.md`.

## Guidance by asset type
Asset-type templates (website assets, game assets, wireframes, logo) are consolidated in `references/sample-prompts.md`.

## Reference map
- `references/prompting.md`: shared prompting principles.
- `references/sample-prompts.md`: shared copy/paste prompt recipes.
- `references/cli.md`: CLI usage reference.
- `references/image-api.md`: API/CLI parameter reference.
- `references/codex-network.md`: network/sandbox troubleshooting.
- `scripts/cm_image_gen.py`: CodexManager CLI implementation.
