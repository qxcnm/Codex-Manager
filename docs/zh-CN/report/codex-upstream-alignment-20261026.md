# Codex 上游兼容核查（2026-10-26）

本次核查以最新稳定版 [Codex rust-v0.164.0](https://github.com/openai/codex/releases/tag/rust-v0.164.0) 为基线，提交为 `6aaac694803004a63c6f393d153c96d22521a6b9`；同时确认 npm `@openai/codex` 的 latest 为 `0.164.0`。核查范围是 CodexManager 已有的认证、用量、网关、配置、模型目录和本地技能集成，不代表复制全部 Codex 新功能。

## 本次调整

| 范围 | 原有问题 | 调整 |
| --- | --- | --- |
| Responses HTTP / WebSocket | 请求字段白名单会删除新版 `stream_options` 和 `access_programs`。 | 保留这两个字段，并同步 HTTP、WebSocket 和 HTTP 转 WebSocket 路径；选择是否有权限仍由上游校验。 |
| 连续 WebSocket 对话 | 初始握手头覆盖后续消息的 `client_metadata`，导致新一轮使用旧元数据。 | 每帧元数据优先，握手头只补缺失值，兼容旧客户端。 |
| OAuth 令牌刷新 | 使用表单和固定的 `openid profile email` scope。 | 对齐官方 JSON 刷新请求，不缩减登录时取得的授权范围；继续保留标准 User-Agent、代理、错误识别和轮换令牌持久化行为。 |
| 技能清单 | 手写逐行解析无法正确处理 YAML 多行描述，嵌套元数据可能覆盖顶层名称/描述。 | 使用完整 YAML 解析，兼容多行、引号及官方的有限散文修复，校验必填描述和 64 字符名称上限。 |
| 用户技能目录 | 只展示 `$CODEX_HOME/skills`。 | 同时识别服务运行用户的 `~/.agents/skills`，以只读条目展示；同名不同路径并存，同一物理目录去重，安装及卸载范围不变。 |
| 历史可见性修复 | 只扫描普通 `.jsonl`，漏掉新版压缩历史。 | 支持 `.jsonl.zst`，保留压缩格式和原始备份，使用官方维护/写入协调锁避免与 Codex 争用文件。 |
| 内置客户端版本 | 离线或版本同步失败时回落到 `0.155.0`。 | 基线更新为 `0.164.0`，保留已有的运行设置和 npm 自动同步。 |

## 已核对，无需变更

- `wire_api = "responses"`、provider ID/显示名称、现有 Bearer token、模型目录及 WebSocket 配置仍有效。
- 官方账号池模型对象保持原样透传，包括未知新字段；托管目录继续使用中立指令，不在缺少官方元数据时启用 Responses Lite。
- 官方历史数据库仍为 `state_5.sqlite`。
- 两个工作区锁定的 WebSocket fork revision 与上游一致，无需升级依赖版本。
- 登录授权 scope、AgentIdentity 注册/签名结构、用量账号/地区头及 Luna Reserve 选择头已对照源码。
- 保留旧客户端使用的中立顶层 `instructions` 兼容字段；新版 Codex 本身通过 `input` 组织指令。

## 边界与后续事项

- 网关的账号轮换、模型/等级覆盖和会话恢复是项目能力，不能直接替换成单账号 Codex 客户端实现。
- 新 `x-codex-routing-hint` 是模型/等级预路由提示，直接透传可能与网关改写后的模型不一致，本次未新增该头。
- 新 Personal Access Token / AgentIdentity JWT 登录模式尚未扩展到现有账号导入接口；用量附加窗口的 `normal_model_slug` 元数据目前未展示。
- 技能导入继续要求显式安全名称；官方允许缺少名称时使用目录名，本项目保留现有导入约定。
- 压缩历史的输入文件和解压内容各限 64 MiB，解码窗口最多 128 MiB。损坏、超限、维护锁忙、活跃写入或源文件变化时跳过相关操作并报告警告；已有普通历史文件优先于同名压缩文件。

固定提交的源码链接及更详细的集成边界见[英文核查记录](../../en/report/gateway-vs-codex-headers-and-params.md)。后续升级应先更新固定版本证据和协议回归，再调整客户端版本号，避免仅修改版本宣告。

## 验证记录

以下命令均已执行成功：

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace --no-fail-fast` | 首轮全量 2,841 项通过，19 项按原有配置忽略；包含 Responses、Chat Completions、SSE、tools、RPC 和 Web 服务回归。 |
| `cargo test -p codexmanager-service --lib codex_ --locked` | 后续技能目录及压缩历史调整后的专项 227 项通过，3 项按原有配置忽略。 |
| `cargo test -p codexmanager-service --lib codex_profile::rollout::tests --locked --offline` | 最终文件写回细节调整后，5 项压缩历史回归再次通过，包含等长度源内容变化校验。 |
| `cargo check --manifest-path apps/src-tauri/Cargo.toml -p codexmanager-service --locked --offline` | 独立桌面工作区的服务依赖编译通过，两份锁文件只新增 YAML 解析所需依赖。 |
| `pnpm.cmd -C apps run test:runtime` | 319 项通过。 |
| `pnpm.cmd -C apps run build:desktop` | 静态导出构建通过。 |
| `cargo fmt --all -- --check` / `git diff --check` | 格式和空白检查通过。 |

真实账号刷新、线上推理请求和完整 Tauri 安装包构建未执行。本次认证和协议验证使用本地 HTTP/WebSocket fixtures，历史修复验证仅操作临时目录；未修改真实用户历史或账号凭据。
