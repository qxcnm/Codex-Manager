# Responses WebSocket 依赖维护说明

## 当前锁定

Responses WebSocket 由服务端工作区和 `apps/src-tauri` 独立工作区共同编译。两个工作区必须使用同一组依赖来源：

- `tokio-tungstenite`：OpenAI fork revision `0e5b2d73aa18dd9f0a50ee9ff199d5aef7594186`（版本线 `0.28`）。
- `tungstenite`：OpenAI fork revision `4fffad30fe373adbdcffab9545e9e9bf4f2fc19f`（版本线 `0.27`）。
- `tungstenite` 的 `deflate` feature 保持开启；Responses WebSocket 客户端默认协商 `permessage-deflate`，并把消息/帧上限保持在 256 MiB。

这些 revision 与官方 Codex Responses WebSocket 客户端当前使用的压缩配置保持一致。仓库同时保留无压缩兼容路径：如果上游在握手阶段以 `400`/`426` 或明确的扩展拒绝信息拒绝 `permessage-deflate`，客户端只重新建立一次不带扩展的握手；其他握手错误不触发该回退。

## 更新流程

升级任一 fork 时，必须在同一个变更中完成以下步骤：

1. 先核对官方 Codex `responses_websocket` 实现和其 lockfile 中的依赖 revision，以及 fork 的变更记录。
2. 同步修改根 `Cargo.toml`、`apps/src-tauri/Cargo.toml`、`Cargo.lock` 和 `apps/src-tauri/Cargo.lock`。
3. 执行 `bash scripts/ci/check-websocket-pins.sh`，确认两个工作区的 patch 与 lockfile 一致。
4. 执行 `cargo fmt --all -- --check`、Responses WebSocket 定向测试、`cargo test --workspace --no-fail-fast`。
5. 安装前端依赖后执行 `pnpm -C apps run build:desktop`、`cargo test -p codexmanager-web --no-fail-fast`，再执行至少一个 Tauri 目标的 bundle 构建。
6. 检查压缩协商成功和“上游拒绝压缩后无压缩重试”两条回归路径；在所有支持的上游都确认兼容前，保留无压缩回退。

禁止只更新一个工作区、只更新 lockfile，或在未通过上述验证时改用浮动 git branch/tag。若官方 Codex 切换 fork revision、上游修复了握手兼容性，或本仓库升级 `tokio-tungstenite`/`tungstenite` 的主版本，应重新评估回退条件、帧大小限制和代理路径，并在 CI 中保留上述同步检查。

## CI 保障

`.github/workflows/ci.yml` 在 Pull Request 和 `main` push 上执行：

- 根工作区格式检查、`cargo check --workspace --all-targets` 和 service WebSocket 回归测试；
- 前端静态构建与 `codexmanager-web` 测试；
- macOS arm64 Tauri bundle 构建；
- 两个 Cargo 工作区的 revision/lockfile 同步检查。

这样可以在依赖 revision、前端静态资源或 Tauri 独立工作区发生漂移时尽早发现，而不会把运行时兼容性依赖隐藏在本机环境中。
