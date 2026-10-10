# CONTRIBUTING

默认语言：简体中文

其他语言入口：

- English: [docs/en/CONTRIBUTING.md](docs/en/CONTRIBUTING.md)
- Русский: [docs/ru/Руководство-для-участников.md](docs/ru/Руководство-для-участников.md)
- 한국어: [docs/ko/기여-가이드.md](docs/ko/기여-가이드.md)

本文档用于约束 CodexManager 的日常协作方式，目标是让新协作者能在尽量少的口头交接下完成开发、验证、提交和发版。

## 1. 项目定位

CodexManager 不是单一前端项目，也不是单一 Rust 服务项目。当前仓库同时包含：

- 桌面端：`apps/` + `apps/src-tauri/`
- 本地服务：`crates/service`
- Web 壳：`crates/web`
- Service 启动器：`crates/start`
- 数据与存储底座：`crates/core`
- 构建 / 发版脚本：`scripts/`
- GitHub Actions 发布链路：`.github/workflows/`

因此提交前必须先判断你改动属于哪个边界，避免把多个职责直接堆进同一个文件。

治理文档入口：

- `README.md`：项目介绍与快速开始
- `ARCHITECTURE.md`：结构边界与运行关系
- `TESTING.md`：仓库级验证基线
- `SECURITY.md`：安全问题与敏感信息处理规则
- `docs/zh-CN/README.md`：治理文档目录与提交规则

## 2. 开发环境

### 2.1 必备工具

- Node.js 20
- pnpm 9
- Rust stable
- Windows 本地打包需要 PowerShell 7+
- Tauri 打包需要对应平台依赖

### 2.2 安装依赖

```bash
pnpm -C apps install
cargo test --workspace
```

### 2.3 常用本地命令

前端：

```bash
pnpm -C apps run dev
pnpm -C apps run build
pnpm -C apps run build:desktop
pnpm -C apps run test:runtime
pnpm -C apps run test:navigation
```

Rust：

```bash
cargo test --workspace
cargo build -p codexmanager-service --release
cargo build -p codexmanager-web --release
cargo build -p codexmanager-start --release
```

桌面端打包：

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist -Portable
```

## 3. 提交边界

### 3.1 按职责改文件

优先遵守以下边界：

- 前端页面、交互、状态：`apps/src/`
- 桌面壳、托盘、窗口、Tauri command：`apps/src-tauri/src/`
- 服务端 HTTP / RPC / Gateway / 协议适配：`crates/service/src/`
- 数据库迁移、存储基础设施：`crates/core/`
- 发布与构建脚本：`scripts/`、`.github/workflows/`

### 3.2 当前高风险文件

以下文件已明显偏大，修改时必须克制追加总控逻辑：

- `apps/src/app/settings/page.tsx`
- `apps/src/app/logs/page.tsx`
- `apps/src/app/aggregate-api/page.tsx`
- `apps/src/app/page.tsx`
- `apps/src-tauri/src/lib.rs`
- `crates/service/src/lib.rs`
- `crates/service/src/gateway/`
- `.github/workflows/release-all.yml`

### 3.3 大文件预警阈值

达到以下阈值时，不应默认继续往里堆逻辑，而应优先评估拆分：

- JavaScript / TypeScript：超过 `500` 行开始预警，超过 `800` 行必须说明为什么不拆
- Rust：超过 `400` 行开始预警，超过 `700` 行必须说明为什么不拆
- Workflow / YAML：超过 `250` 行开始预警，超过 `400` 行必须说明为什么不拆
- Markdown 说明文档：超过 `300` 行开始预警，优先下沉到 `docs/` 子文档

说明：

- “开始预警”表示提交前应主动判断是否继续拆职责
- “必须说明为什么不拆”表示提交说明或 PR 描述中要明确给出理由
- 这些阈值是长期维护约束，不是一次性清理指标

### 3.4 禁止项

- 不要顺手在总入口继续堆设置项、事件绑定或协议分支。
- 不要把 README 当 changelog 长期维护。
- 不要在没有验证的情况下顺手改脚本、workflow、版本号。
- 不要回退自己未创建的用户改动。
- 不要把 release workflow 里的内联脚本再次复制展开，优先复用 `scripts/release/`。

## 4. 提交前检查

### 4.1 最小检查清单

按改动范围至少执行以下内容：

前端改动：

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
pnpm -C apps run test:navigation
```

Rust / 服务端改动：

```bash
cargo test --workspace
```

桌面端 / 打包链路改动：

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -DryRun
```

### 4.2 协议适配相关改动

如果改了以下路径，必须补最小回归验证：

- `crates/service/src/gateway/`
- `crates/service/src/http/`
- `crates/service/src/lib.rs`

最低要覆盖：

- `/v1/chat/completions`
- `/v1/responses`
- 流式返回
- 非流式返回
- `tool_calls` / tools 相关路径

### 4.3 设置项相关改动

如果新增设置页字段、环境变量或持久化配置，必须同时确认：

- 默认值是否明确
- 是否需要写入 `app_settings`
- 是否影响桌面端 / service / web 三端行为
- README 或专用文档是否需要更新

## 5. 提交信息与 PR 约定

### 5.1 提交信息

当前仓库以中文提交说明为主，要求：

- 一次提交只解决一类问题
- 标题直接描述结果，不写空话
- 不要把多个不相关改动塞进同一提交

### 5.2 PR 描述最低要求

PR 至少写清：

- 改了哪些文件
- 解决什么问题
- 影响哪些平台或接口
- 跑了哪些验证
- 有无未覆盖风险

## 6. 发版前检查

每次发版前必须确认：

1. `CHANGELOG.md` 已更新。
2. `README.md` 与 `docs/en/README.md` 当前版本入口一致。
3. 根 `Cargo.toml`、`apps/src-tauri/Cargo.toml`、`apps/src-tauri/tauri.conf.json` 版本一致。
4. release workflow 输入说明、脚本参数说明、实际 workflow 保持一致。
5. 高风险兼容路径至少完成一轮本地验证。
6. 若改动了产物命名或发布类型逻辑，必须验证 `prerelease` 与 tag 行为。

## 7. 文档维护规则

长期维护约定如下：

- `README.md` / `docs/en/README.md` 负责项目介绍、快速开始、入口说明。
- `CHANGELOG.md` 负责版本历史。
- `ARCHITECTURE.md` 负责结构边界与运行关系。
- `CONTRIBUTING.md` 负责协作规则与提交前检查。

不要再把版本历史、架构说明、发布细则全部堆回 README。

## 8. 遇到大改动时的处理方式

满足以下任一情况，建议先拆任务再提交：

- 同时涉及前端、桌面端、服务端三个边界
- 同时改协议适配、设置持久化、发布链路
- 需要重命名产物、修改 workflow、调整版本策略
- 需要拆分高风险大文件

建议顺序：

1. 先补测试或验证脚本
2. 再做重构或结构调整
3. 最后补文档与版本说明
