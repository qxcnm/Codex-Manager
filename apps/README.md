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
- 模型管理页不会自动覆盖 `~/.codex/models_cache.json`；直连模式由 Codex 官方维护模型目录与缓存，网关模式使用 CodexManager 独立生成的 `model_catalog_json`。
- 平台模式页提供“切换后重载 Codex 后台”开关；默认开启，只针对使用目标 `CODEX_HOME` 的 app-server，前台 Codex CLI 不会被终止。
- 因此桌面端不再显示“导出到本地 Codex 缓存”按钮。

### Web 部署

- 必须通过 `codexmanager-web` 提供页面壳与 `/api/runtime`、`/api/rpc` 代理。
- 只启动前端静态页面，或者只跑一个普通 Next 开发服务器，不足以支撑完整管理页面。
- Web 端保留“导出到本地 Codex 缓存”作为显式兼容操作；正常的直连与网关切换都不依赖该文件。

## 当前前端重点

- 模型管理页维护结构化模型目录，并区分 `supportedInApi` 与 `visibility`。
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
