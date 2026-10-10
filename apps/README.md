# apps 前端与桌面端说明

`apps/` 是 CodexManager 的前端工作区，承载浏览器管理页面与 Tauri 桌面壳。

## 技术栈

- Next.js App Router
- TypeScript
- Tailwind CSS v4
- shadcn/ui
- TanStack Query
- Zustand
- Tauri v2

## 目录结构

```text
apps/
├─ src/                # Web UI、页面、hooks、API client、store
├─ src-tauri/          # Tauri 桌面端壳、Rust 命令、打包配置
├─ public/             # 静态资源
├─ tests/              # Playwright 与导航回归测试
└─ out/                # 静态导出产物
```

## 常用命令

```powershell
pnpm install
pnpm dev
pnpm dev:desktop
pnpm run build:desktop
pnpm exec playwright test
```

说明：

- `pnpm dev`：启动前端开发服务器。
- `pnpm dev:desktop`：启动前端 + Tauri 桌面端。
- `pnpm run build:desktop`：桌面端静态导出检查，也是前端改动的默认验证命令。
- `pnpm exec playwright test`：执行端到端回归。

## Web 与桌面端差异

### 桌面端

- 通过 Tauri `invoke` 调用本地命令，不走浏览器 `fetch` IPC。
- 模型管理页的“应用模型”在未勾选时把完整目录写入 `gateway-models.json`，存在勾选项时只写入所选模型，并在目标 `CODEX_HOME/config.toml` 更新 `model_catalog_json`；它不要求平台密钥，也不会改写现有认证、模型提供方或网关地址。
- 应用时不按 enabled、`supportedInApi`、visibility 或文本生成能力过滤，`gpt-image-2`、两个 Image 2.5 模型和明确勾选的隐藏模型也能进入 Codex 模型列表。操作不会终止 Codex 后台进程；关闭并重新打开 Codex 后读取新目录。
- 模型管理页不提供写入或下载 `~/.codex/models_cache.json` 的入口；Codex 通过 `model_catalog_json` 指向的 `gateway-models.json` 读取已应用的模型列表。
- 平台模式页支持 OpenAI 账号直连、Codex/Compatible Responses 聚合 API 直连和 CodexManager 本地网关三种接入方式；聚合 API 直连只接纳可映射为 Responses `base_url` 的 API-key 条目。
- 平台模式页提供“切换后重载 Codex 后台”开关；默认开启，只匹配使用目标 `CODEX_HOME` 的 app-server，不会终止前台 Codex CLI。

### Web 部署

- 必须通过 `codexmanager-web` 提供页面壳与 `/api/runtime`、`/api/rpc` 代理。
- 只启动前端静态页面，或者只跑一个普通 Next 开发服务器，不足以支撑完整管理页面。
- Web 端通过同一 service RPC 执行模型应用和价格同步；Codex profile 切换统一通过“平台模式选择”页完成。

## 当前前端重点

- 模型管理页维护唯一的模型目录 V2，来源只有 `builtin` / `custom`，并原子保存 model、整数价格阶梯、routes、permission groups 和 instructions policy。
- 模型列表显示 enabled、origin、price status、instructions mode 和 route 状态；hidden 模型只在显式筛选中出现。
- 当前内置目录共 11 条记录：GPT-6 Astra/Sol/Luna、GPT-5.6 Sol/Terra/Luna、GPT-5.5、GPT Image 2 和两个 Image 2.5 变体共 10 款模型默认可见；只有 `codex-auto-review` 默认隐藏。
- `gpt-5.4` 与 `gpt-5.4-mini` 已于 2026-08-31 从 ChatGPT 登录的 Codex 退役，`gpt-5.2` 也已在该登录方式下 deprecated；通用 API 模型不受此清理影响。revision 9 会删除未定制的对应 builtin；用户编辑、价格阶梯、非默认 route 或 permission group/API Key 关联仍会保留，并转为 custom 管理。
- 管理员可手动“同步价格”：`basellm` 为主来源，`models.dev` 用于补全和回退；单一来源失败仍可继续，两个来源都失败才报错。未勾选模型时同步整个目录并保留 `price_status=custom`；勾选后只同步所选模型，明确勾选的模型允许外部 `estimated` 价格覆盖已有自定义价格；未知 slug 会被拒绝且不会写入。同步不会修改模型字段、routes 或 `user_edited`。只有两个来源都健康且非空，并确认此前同步的 external `estimated` 价格已经撤销或失效时，服务才会把价格置为 `missing`，同时自动移出显式 permission groups；单一来源失败时不会清除旧价格或组关联。
- 聚合 API 页面只管理连接、密钥、余额和具体 V2 route 测试，不提供供应商 `/models` 同步、模型池或模板导入。
- 平台密钥页默认优先展示 `supportedInApi = true` 的模型。
- 所有主要列表页的“操作”列都已做右侧冻结，横向滚动时不会丢失操作入口。
- 页面切换使用 keep-alive 缓存与整区加载遮罩，减少桌面端与 Web 版回访时的重载体感。
- 首次接入引导会展示 `auth.json` 与 `config.toml` 示例，帮助用户把 Codex CLI / ccswitch 接入到本地网关。
- 设置页网关配置包含上游代理、请求总超时、流式空闲超时与 SSE 保活间隔。

## 开发约定

- 新增桌面命令后，必须同步更新 `src/lib/api/` 下的调用封装。
- 与桌面端 IPC 交互时，优先使用统一 transport，不要直接写裸 `fetch()`。
- 前端交互改动完成后，至少验证一条关键路径；默认先跑 `pnpm run build:desktop`。

## 相关文档

- 根项目说明：[../README.md](../README.md)
- 中文文档索引：[../docs/zh-CN/README.md](../docs/zh-CN/README.md)
- 运行与部署指南：[../docs/zh-CN/report/运行与部署指南.md](../docs/zh-CN/report/运行与部署指南.md)
- 环境变量与运行配置：[../docs/zh-CN/report/环境变量与运行配置说明.md](../docs/zh-CN/report/环境变量与运行配置说明.md)
- 模型目录 V2：[../docs/zh-CN/report/模型目录V2管理与计费说明.md](../docs/zh-CN/report/模型目录V2管理与计费说明.md)
