# TESTING

本文档定义 CodexManager 仓库级测试与验证基线。

目标：

- 新协作者能快速知道改动后至少要跑什么
- 避免把所有改动都一律升级成全量验证
- 让协议兼容、发布链路、设置治理这些高风险改动有固定检查入口

## 1. 基础环境

- Node.js 20
- pnpm 9
- Rust stable
- PowerShell 7+（Windows 打包 / 脚本验证）

## 2. 前端改动

适用范围：

- `apps/src/app/`
- `apps/src/components/`
- `apps/src/lib/`
- `apps/src/hooks/`

最小验证：

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
```

说明：

- `pnpm -C apps run build`：确认 Next.js 静态导出链路仍正常
- `pnpm -C apps run test:runtime`：确认运行时能力判定和桌面 / Web 能力降级逻辑未回归
- 若改动涉及运行时识别、Web RPC、桌面 / Web 差异处理，补跑第 4 节

## 3. 桌面端 / Tauri 改动

适用范围：

- `apps/src-tauri/`
- 桌面端更新、托盘、窗口、命令桥接相关改动

最小验证：

```bash
cargo test --workspace
```

补充建议：

```powershell
pwsh -NoLogo -NoProfile -File scripts/rebuild.ps1 -Bundle nsis -CleanDist
```

说明：

- 只要改了 Tauri 桥接或桌面生命周期，最好至少做一次本地桌面构建验证。

## 4. Web 运行壳 / 部署兼容改动

适用范围：

- `crates/web/`
- `apps/src/lib/api/transport.ts`
- `apps/src/components/layout/app-bootstrap.tsx`
- `apps/src/components/layout/header.tsx`
- `apps/src/components/layout/sidebar.tsx`
- Web 代理、`/api/runtime`、`/api/rpc`、部署方式相关改动

最小验证：

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
cargo test -p codexmanager-web
```

建议在启动 Web 服务后，通过浏览器或 HTTP 客户端验证 `/api/runtime`、`/api/rpc` 和关键页面；仓库已移除旧的 `scripts/tests/web_runtime_probe*.ps1` 与 `web_ui_smoke.ps1`。

说明：

- `pnpm -C apps run build`：确认前端静态导出仍可生成
- `pnpm -C apps run test:runtime`：确认前端运行时契约和能力判定保持一致
- `cargo test -p codexmanager-web`：确认 Web 壳路由与运行时探针契约

## 5. Rust 服务端改动

适用范围：

- `crates/core/`
- `crates/service/`
- `crates/start/`
- `crates/web/`

最小验证：

```bash
cargo test --workspace
```

补充建议：

```bash
cargo build -p codexmanager-service --release
cargo build -p codexmanager-web --release
cargo build -p codexmanager-start --release
```

## 6. 协议适配 / 网关改动

适用范围：

- `crates/service/src/gateway/`
- `crates/service/src/http/`
- `crates/service/src/lib.rs`

必须覆盖：

- `/v1/responses`
- `/v1/chat/completions`
- 流式 SSE
- 非流式 JSON
- `tools`
- `tool_calls`

最小验证：

```bash
cargo test --workspace
cargo test -p codexmanager-service gateway --offline -- --test-threads=1
```

说明：

- 如果本地环境不具备真实上游账号，至少要跑 Rust 测试，并将真实上游验收明确记录为未执行。
- 兼容性修复不能只验证一种客户端。

## 7. 设置项 / 环境变量 / 持久化改动

适用范围：

- `apps/src/app/settings/`
- `crates/service/src/app_settings/`
- `crates/core/src/storage/settings.rs`
- 新增 `CODEXMANAGER_*` 配置项

最小验证：

```bash
pnpm -C apps run build
cargo test --workspace
```

必须人工确认：

- 默认值是否明确
- 是否写入 `app_settings`
- 是否需要同步到运行时
- README / `CONTRIBUTING.md` / `ARCHITECTURE.md` 是否需要更新

## 8. 发布链路改动

适用范围：

- `.github/workflows/`
- `.github/actions/`
- `scripts/release/`
- `scripts/rebuild*`

最小验证：

```bash
pnpm -C apps run build
cargo test --workspace
pnpm -C apps run build:desktop
```

必须人工确认：

- workflow 输入和 README 说明一致
- 产物命名没有漂移
- prerelease / latest 行为没有漂移

## 9. 文档治理改动

适用范围：

- `README*`
- `ARCHITECTURE.md`
- `CONTRIBUTING.md`
- `CHANGELOG.md`
- `docs/`

最小验证：

- 检查链接路径是否有效
- 检查文档职责是否重复
- 检查版本号、产物名、workflow 名称是否与实际一致

## 10. 提交前最小检查建议

### 常规改动

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
cargo test -p codexmanager-web
```

### 前端页面改动

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
```

### Web 兼容 / 部署改动

```bash
pnpm -C apps run build
pnpm -C apps run test:runtime
cargo test -p codexmanager-web
```

### 协议适配改动

```bash
cargo test --workspace
cargo test -p codexmanager-service gateway --offline -- --test-threads=1
```

## 11. 结果记录约定

- 能完整执行的验证，记录为“已执行”。
- 受环境限制无法执行的验证，明确写成“未执行 + 原因”。
- 不要把“应该能过”当作“已验证”。
