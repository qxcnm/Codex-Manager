# Codex 图片生成支持 TODO

日期：2026-04-26

## 目标

为 CodexManager 增加图片生成能力。实现顺序优先对齐官方 Codex 的 `image_generation` tool 主链路，再补 OpenAI Images API 兼容入口。

## 维护规则

- 每完成一个功能点，把对应复选框从 `[ ]` 改成 `[x]`。
- 如果一个 TODO 被拆成多次提交，只有完成验收项后才打勾。
- 新发现的子任务追加到对应章节，不把未完成工作隐藏在已勾选项里。

## TODO

- [x] 1. 梳理当前网关图片能力缺口
  - 确认当前 `CodexManager` 是否会过滤 `tools[].type = "image_generation"`。
  - 确认 `/v1/responses`、`/v1/chat/completions` 的请求改写是否保留未知 tool 字段。
  - 确认 SSE 聚合层是否会丢弃 `image_generation_call`。
  - 输出改动点小结：哪些模块要改、哪些不用改。
  - 验收：有明确源码定位和实现边界。

- [x] 2. 支持官方 Codex `image_generation` tool 透传
  - 请求侧允许：
    ```json
    {
      "type": "image_generation",
      "output_format": "png"
    }
    ```
  - `/v1/responses` 原样带给 `chatgpt.com/backend-api/codex/responses`。
  - 先不默认自动注入，只保证客户端显式传入时可用。
  - 验收：构造 `/v1/responses` 测试，请求 body 中 `image_generation` 未被删除。

- [x] 3. 支持 `image_generation_call` 响应解析
  - 识别 `response.output_item.done` 里的：
    ```json
    {
      "type": "image_generation_call",
      "id": "ig_...",
      "status": "completed",
      "revised_prompt": "...",
      "result": "<base64>"
    }
    ```
  - 非流式聚合时保留这类 output item。
  - 日志/trace 保留类型，但不完整打印 base64 图片内容。
  - 验收：单测输入 Codex SSE，聚合结果保留 `image_generation_call`。

- [x] 4. 支持 Chat Completions 图片返回转换
  - 非流式输出增加 `choices.0.message.images`。
  - 流式输出增加 `choices.0.delta.images`。
  - 图片使用 data URL：`data:image/png;base64,...`。
  - 验收：覆盖 `response.output_item.done` 转 `message.images` / `delta.images`。

- [x] 5. 可选支持 partial image 流式事件
  - 识别 `response.image_generation_call.partial_image`。
  - 从 `partial_image_b64` 生成 data URL。
  - 对同一个 `item_id` 去重，避免 partial 和 done 重复输出。
  - 验收：连续 partial + done，相同图片只输出一次。

- [x] 6. 新增 `/v1/images/generations`
  - 路由：`POST /v1/images/generations`。
  - 接收 OpenAI Images API 参数：
    - `prompt`
    - `model`
    - `size`
    - `quality`
    - `background`
    - `output_format`
    - `output_compression`
    - `partial_images`
    - `response_format`
    - `stream`
  - 内部转成 `/v1/responses + image_generation tool`。
  - 默认主模型使用配置项，默认值可参考 CPA 的 `gpt-5.4-mini`。
  - 默认图片工具模型：`gpt-image-2`。
  - 验收：`POST /v1/images/generations` 返回 OpenAI Images 格式。

- [x] 7. 新增 `/v1/images/edits`
  - 路由：`POST /v1/images/edits`。
  - 支持 multipart：
    - `image`
    - `image[]`
    - `mask`
    - `prompt`
  - 支持 JSON：
    ```json
    {
      "prompt": "...",
      "images": [
        {
          "image_url": "data:image/png;base64,..."
        }
      ],
      "mask": {
        "image_url": "data:image/png;base64,..."
      }
    }
    ```
  - 暂不支持 `file_id`，返回清晰错误。
  - 验收：JSON edit 和 multipart edit 各有测试覆盖。

- [x] 8. Images API 输出包装
  - 非流式返回：
    ```json
    {
      "created": 123,
      "data": [
        {
          "b64_json": "..."
        }
      ]
    }
    ```
  - `response_format = "url"` 时返回 data URL：
    ```json
    {
      "url": "data:image/png;base64,..."
    }
    ```
  - 可透出：
    - `revised_prompt`
    - `usage`
    - `size`
    - `quality`
    - `background`
    - `output_format`
  - 验收：`b64_json` 和 `url` 两种 response format 都通过。

- [x] 9. 模型列表补 `gpt-image-2`
  - 模型列表加入 `gpt-image-2`。
  - 限制它只用于 `/v1/images/generations` 和 `/v1/images/edits`。
  - 如果用户拿 `gpt-image-2` 调 `/v1/chat/completions` 或 `/v1/responses`，返回明确错误。
  - 验收：模型列表可见，普通 chat 使用会被拒绝。

- [x] 10. 配置开关
  - 增加配置项：
    ```text
    codex_image_generation_enabled
    codex_image_generation_auto_inject_tool
    codex_image_generation_main_model
    codex_image_generation_tool_model
    ```
  - 默认建议：
    ```text
    codex_image_generation_enabled = true
    codex_image_generation_auto_inject_tool = false
    codex_image_generation_tool_model = gpt-image-2
    ```
  - 验收：关闭开关后 `/v1/images/*` 返回清晰错误。

- [x] 11. 安全与日志处理
  - 不在普通日志里完整打印 base64 图片。
  - request log 里截断或标记：`<base64 image omitted>`。
  - 限制请求体大小，尤其是 edits multipart。
  - 错误返回保持 OpenAI 风格。
  - 验收：大图、非法 base64、缺 prompt、缺 image 都有明确错误。

- [x] 12. 测试覆盖
  - 单测覆盖：
    - `image_generation` tool 透传
    - `image_generation_call` 聚合
    - Chat Completions 图片转换
    - Images generation 非流式
    - Images generation 流式 partial
    - Images edits JSON
    - Images edits multipart
    - `gpt-image-2` 普通 chat 禁用
  - 集成测试覆盖：
    - 本地 mock upstream 返回 Codex SSE，网关输出 OpenAI Images 格式。
  - 验收：相关 Rust 测试通过。

## 建议执行顺序

1. 先做 `1 -> 2 -> 3 -> 4`，打通官方 Codex 图片生成主链路。
2. 再做 `6 -> 8 -> 12`，让 `/v1/images/generations` 可用。
3. 最后做 `7 -> 9 -> 10 -> 11`，补编辑、模型列表、开关和安全细节。

## 当前状态

已完成 `1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11 -> 12`：

- 请求侧：显式传入的 `tools[].type = "image_generation"` 和对象形式 `tool_choice` 会保留给 Codex backend。
- 响应侧：SSE 聚合保留 `image_generation_call` output item。
- Chat Completions 兼容输出：非流式写入 `choices.0.message.images`，流式写入 `choices.0.delta.images`。
- Images API：`/v1/images/generations` 会转成内部 `/v1/responses + image_generation tool`。
- Images Edits：`/v1/images/edits` 支持 JSON `image_url` / `images[]` / `mask.image_url`，也支持 multipart `image` / `image[]` / `mask`，暂不支持 `file_id` 并返回清晰错误。
- Images 输出：支持 `b64_json` 和 `url`，并透出 `revised_prompt`、`usage`、`size`、`quality`、`background`、`output_format`。
- Partial image：支持 `response.image_generation_call.partial_image`，并对 partial/done 相同图片做去重。
- 模型列表：本地 `/v1/models` 会补 `gpt-image-2`，普通 `/v1/chat/completions` 和 `/v1/responses` 直接使用 `gpt-image-2` 会被拒绝。
- 配置项：支持 `CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED`、`CODEXMANAGER_CODEX_IMAGE_GENERATION_AUTO_INJECT_TOOL`、`CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL`、`CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL`；默认开启并自动注入 `image_generation` tool，默认主模型 `gpt-6-luna`，默认图片工具模型 `gpt-image-2`。
- 设置页：上述 4 个图片配置项已注册到环境变量覆盖目录，可在 Settings 的环境变量页修改；保存后按 runtime 配置热重载生效。
- 安全与日志：trace 日志会将 `data:image/*;base64,...` 的图片载荷替换为 `<base64 image omitted>`；request log 不记录请求体；JSON edits 会本地拒绝非法 base64 data URL、缺 prompt、缺 image 和 `file_id`。
- 测试覆盖：新增 mock upstream 集成测试，覆盖 `/v1/images/generations` → `/v1/responses + image_generation tool` → Codex SSE → OpenAI Images JSON 的完整网关路径。
- 当前 TODO 已全部完成。
