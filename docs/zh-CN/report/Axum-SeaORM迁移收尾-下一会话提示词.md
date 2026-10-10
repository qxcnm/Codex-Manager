# 下一会话执行提示词

请继续完成 CodexManager 的 Axum/Tokio/Tower/SeaORM 架构迁移。直接检查、实现、测试和补齐验收，不要只做规划，不要把已经通过的主体功能重写一遍。

仓库：`C:\Users\qxnm\Data\Code\Codex-Manager`。环境为 Windows / PowerShell。交接时分支为 main，存在大量尚未提交的修改和新增文件；这些包含前面会话的实现，必须完整保留。不要 reset、stash、clean 或覆盖既有工作；没有授权时不要提交、推送或发布。用户授权继续实现和本地验证，普通可逆修改无需反复确认。

先阅读仓库根 AGENTS.md；涉及 apps 时再读 apps/AGENTS.md。然后阅读：

1. `docs/zh-CN/report/Axum-Tokio-Tower-SeaORM多数据库架构迁移执行设计.md`：原始范围、目标和完成定义，不要修改目标来迁就当前实现。
2. `docs/zh-CN/report/Axum-Tokio-Tower-SeaORM迁移进度与会话交接.md`：当前事实、测试证据、已确认缺口。
3. `docs/zh-CN/ARCHITECTURE.md` 与 `docs/zh-CN/report/环境变量与运行配置说明.md`。

## 当前已完成，优先复用

- SeaORM 业务仓储与兼容 facade 已覆盖账号/Token、API Key/quota、groups 权限、模型目录/价格/路由、用户/会话、wallet/ledger、日志/用量、代理、插件和聚合 provider 等。MySQL/Pg 真实数据库读写及事务测试已执行。
- Service 生产 listener、RPC、网关、SSE/WS、OAuth callback 已使用 Axum。生产 normal 依赖树无 tiny_http，tiny_http 仍用于测试适配。
- 网关 upstream/auth/observability 及业务网络入口已异步化，有有界任务、取消和关闭处理。同步 facade/旧 API/Rhai 等兼容边界仍在，不得宣称全仓库同步代码已清零。
- SQLite export/inspect/import、哈希与结构校验、空目标保护、事务导入、回读和 PostgreSQL 序列修复已实现，导入 MySQL/Pg 已实际通过。
- SQLite/MySQL/Pg 的真实 TCP listener 已验证两类 OpenAI endpoint 的 JSON/SSE、groups 倍率计费、断连 499 及账本、IPv4/IPv6、stop/start 和端口关闭。上游是本地 fixture，不是真实生产 provider。

## 剩余六个工作包

### 1. 统一领域存储接口与依赖注入（主要代码工作）

现状（2026-09-20 更新）：已有 `DomainStorage` 分域接口、SQLite/SeaORM adapter 和实际注入的 `AppState`，携带存储、HTTP client、任务通道、并发槽和 shutdown 状态。modelGroups 写入、requestlog/clear、apikey/delete/enable/disable 已走注入接口，API Key 写入覆盖两种 adapter 的权限及回读契约。其余业务仍大量依靠同步 facade，不等同于全部迁移完成。

- 按账号/凭据、权限/用户、API Key、日志/usage、模型/账本、设置/插件等实际领域建立可用的持久化接口；不要仅增加空 trait 或包装现有全局入口。
- SQLite 与 SeaORM 适配器接入同一领域契约，业务 handler 不直接依赖 rusqlite、SeaORM Entity/DatabaseConnection 或 SQLx pool。
- AppState 实际承载并注入后端、HTTP client/配置、任务通道与关闭状态，逐步替换隐式全局获取。
- 保持接口语义、事务边界、权限和计费不变；分域迁移，避免集中修改大型入口。
- 验收要包含后端一致性、并发事务和失败路径，证明接口实际被调用，不能仅靠编译或新类型存在。

### 2. runtime、同步 bridge 和过渡代码收敛（主要代码工作）

现状（2026-09-20 更新）：auth、usage、account background、upstream、Rhai 已复用进程级 service runtime。auth/usage/aggregate 的同步桥容量不足时返回错误，不再因 admission 失败 panic。SQLite 兼容 crate 仍有独立进程级 SQLx driver runtime，Rhai 同步 ABI、同步存储 facade、文件/CPU 阶段和测试适配器仍保留；不能把这些边界无条件删除。

- 对照原设计统一主 runtime 的所有权、任务登记、取消、drain 和同进程重启；不要在 async 路径嵌套 block_on。
- 清理确实失去调用方的同步 client、旧 bridge、HTTP 队列与适配代码；必要的文件/ZIP/CPU/Rhai 同步 ABI 继续使用有界阻塞隔离，写清保留理由。
- 测试夹具仍需的 tiny_http 不能当成生产依赖，也不要为了清零关键词破坏协议覆盖。
- 重要已修复问题：短生命周期 listener runtime 被销毁后，全局 reqwest keepalive 连接 driver 会失效，导致后台 device login 失败。当前 `http/proxy_runtime.rs::front_proxy_runtime()` 与 `lifecycle/startup.rs` 共用进程级 runtime。收敛时必须保留这一保证。
- `tests/rpc.rs` 的设备码回归会先关闭首 listener，再放行同一 provider TCP 连接上的后续请求，最后经第二 listener 查询成功；不要删除、改松或改为靠 sleep 通过。

### 3. 导入目标 dry-run 与失败报告

现状（2026-09-20 更新）：`storage-transfer` 已有 prepare-target/dry-run 及脱敏逐表失败报告。已复用项目专属私密测试配置，在新建的 MySQL/PostgreSQL 隔离库串行完成 4 个真实 dry-run/import 用例（4 passed / 0 failed / 0 ignored，exit 0），修复 MySQL INFORMATION_SCHEMA 列标签大小写与 Windows 测试源连接关闭问题。导入成功的库已非空，复验必须新建目标；生产库、生产权限和实际数据规模仍 HANDOFF。

- 增加明确的目标 dry-run，检查快照、映射、目标后端/schema、冲突与空目标要求，给出逐表统计和明确失败原因。
- dry-run 不导入业务行、不覆盖或删除源/目标数据；包括 MySQL DDL 隐式提交在内的目标修改必须避免或明确隔离，不把“执行后回滚”简单等同于完全无副作用。
- 输出结构化脱敏报告及失败记录，不包含凭据、Token、原始敏感行或完整数据库 URL。
- 覆盖非空目标、错误结构/类型、校验失败、权限/连接失败、失败后的源库完整性及重新执行；继续保留真实 MySQL/Pg 导入回读验收。

### 4. 性能及完整桌面/Web 验收

- 补充可复现的吞吐、P95/P99、内存、连接数、失败率和混合短请求/长流测试，记录硬件、并发、请求模型与基线。
- 只能使用实际测得的迁移前基线；如需旧版本，使用独立 checkout/worktree 并保持当前未提交工作不变。不能制造历史基线或凭测试数声称性能无退化。
- 实际启动 Tauri 桌面，确认无 MySQL/Pg 时默认 SQLite、本地 service 和 IPC 正常。涉及前端/桌面按 apps 规则构建和验证。
- 用真实 codexmanager-web 验证登录、cookie/session、RPC 代理、权限拒绝、退出/撤销、网关代理及 WS；普通 Next dev 或 Rust 单元测试不能替代完整 Web 模式。

### 5. 备份恢复与运维文档

- 在隔离数据上完成备份、恢复、SQLite→目标切换、失败返回原配置和数据回读演练。
- 已完成 SQLite 文件副本/JSONL 回读，以及 2026-09-20 MySQL 原生 dump/restore、PostgreSQL pg_dump/pg_restore 的新库恢复和逻辑哈希核对。下一步补实际 Service 切换/失败回退、跨主机、生产 KMS/ACL；不要把原生恢复测试当作这些验收已通过。
- 写清部署、feature 构建、启动变量、密钥/加密材料、备份恢复、升级回退及排障步骤。
- 文档记录真实执行命令、结果、证据与尚未执行项；最终逐项对应原执行设计完成定义，不以更新进度报告代替实现。

### 6. 真实生产验收（当前缺外部配置）

上一会话尚未获得生产数据库、真实 provider、生产认证配置及完整端口清单。先完成不依赖这些信息的工作，同时只询问现有配置文件路径、目标地址和端口，不让用户在聊天中贴密钥。

- 覆盖真实数据库读写、真实 provider 的 Responses/Chat、JSON/SSE、tools、错误/取消计费。
- 覆盖生产登录、权限与会话撤销、OAuth、Service/Web/callback/反向代理所有实际监听端口和 WS。
- 不使用其他项目凭据，不擅自覆盖生产数据，不把 fixture 或本地 Docker 验收称为生产通过。
- 缺失配置时只将这部分准确标记 HANDOFF，不要因此停止其余五包；代码缺口不能归因于缺生产凭据。

## 必须保持的已验证约束

- 默认桌面 SQLite 是原设计要求，保留它不属于未完成项；不要删除 SQLite 历史迁移。
- 显式 DATABASE_URL（含 sqlite URL）或 MySQL/Pg 后端使用 SeaORM。空/不匹配配置启动失败，不静默回退写 SQLite。数据库切换需重启，禁止未经设计的双写或在线切换。
- HTTP X-Request-Id 与内部 X-CodexManager-Trace-Id 用途不同，业务日志按后者关联。
- Token refresh 已接受的轮换结果在调用者取消后仍需 CAS 落库；先保存 grant 再 exchange，不能覆盖并发导入、不能重建已删除账号。
- reset credit 取消后的不确定结果不能触发第二次扣次；日志、usage、账本幂等和 groups 倍率保持一致。
- 断连后的 queued usage 帧、最终日志与计费须排空；stop/start 后不遗留 running/checking/queued 状态或未释放端口。

## 已有验证与工具

这些是交接时的证据，不自动代表你修改后的源码已通过：

- 前轮 Service/Web 全套：1812 passed，0 failed，10 ignored，exit 0。日志 `%TEMP%/codexmanager-service-web-runtime-final10.log`。2026-09-20 Service 单独全套已为 1797 passed / 0 failed / 10 ignored，exit 0；本轮其他最新结果以迁移交接文档末节为准，不能累加历次重复用例。
- 未受最后 runtime 修复影响的 Core/集成 485、SeaORM 33、Start 2、rusqlite 1 已通过；常规套件合计 2333 passed、26 ignored，是分套件汇总，不能写成最初 workspace 命令直接成功。
- 数据库/监听专项共 25 次通过；覆盖常规 ignored 中的 23 个用例（网关另跑三个后端）。另有 3 个 GitHub/skills.sh 实网探测未跑。
- `%TEMP%/codexmanager-storage-all-final4.log`、`codexmanager-service-remote-*-final5.log`、`codexmanager-{gateway,auth}-*-runtime-final10.log` 保存相应证据。
- 最新 MySQL-only/Pg-only Service/Web/Start check、fmt 和 diff check 均通过。
- 私密测试配置可能仍在 `%TEMP%/codexmanager-seaorm-acceptance-final2.json`，脚本可能在 `%TEMP%/codexmanager-run-remote-acceptance.ps1`。先验证存在和当前容器状态；只安全加载，禁止打印值。已执行过的 import/auth 库非空，重验应建立新的独立 fixture 库并保留旧数据。

常用验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace --all-features --offline -- --test-threads=1
cargo check -p codexmanager-service -p codexmanager-web -p codexmanager-start --no-default-features --features storage-mysql --offline
cargo check -p codexmanager-service -p codexmanager-web -p codexmanager-start --no-default-features --features storage-postgres --offline
git diff --check
```

按实际改动补专项、真实数据库和客户端验收。单次 Cargo 作业完成编译后可并行独立数据库测试，但避免多个 Cargo 重复构建和同库 fixture 并发。异步测试等待业务终态，不以 mock provider 已返回等同于业务已落库；失败不能靠删断言或盲加等待消除。

请按六包建立实施顺序，能独立的子任务可分给 agent，连续推进实现。完成后更新迁移记录，列出每包的实际修改、验证和剩余 HANDOFF。只有原设计完成条件全部满足，才能回答“全部按文档完成”。

## 2026-09-20 最新交接覆盖

本日已完成真实隔离 MySQL/PostgreSQL dry-run/import（4/0/0）、原生备份恢复回读（2 backend PASS）、Service 全套回归（1797/0/10）、Web 31/0/0、HTTP/runtime 201/0/0、SQLite 远端网关 1/0/0、Chromium Web 7 项、workspace check/build/fmt/diff check，以及前端 runtime 217/0/0 和 desktop build。新增的受保护 `/__shutdown` 与 Web `/__quit` 流程均正常退出；WebSocket close code 已为 1000。

性能基线来自独立 worktree `a4805dd4b5b6a64312c305c9a35554b7f334102c` 的新 target 构建。旧版两轮因 1006 和强制终止退出 1；当前两轮通过。共享 Key 混合长流短请求 P95 中位数实测约 +5.47%，不能写成无回归；详细结果见 `Axum-SeaORM本地客户端与性能复验-2026-09-20.md`。当前工作包 1/2 仍部分完成，工作包 4 为本地验证但完整验收 HANDOFF，工作包 5 生产运维 HANDOFF，工作包 6 生产 HANDOFF。未启动实际 Tauri GUI，未执行真实 OAuth/provider/tools/tool_calls，因没有生产配置不执行生产验收。

## 2026-09-20 最新交接覆盖：下一阶段从剩余写入继续

上一阶段已将以下写入切到注入式 `DomainStorage`：

- `accountManager/profile/update`：通过 `AccessStore` 更新资料。
- `accountManager/password/change`：通过 `AccessStore` 按旧哈希条件更新密码，保留密码校验和并发更新语义。
- `accountManager/apiKeyOwners/set`：通过 `ApiKeysStore` 保存归属，并通过 `CatalogBillingStore` 确保钱包。
- `accountManager/users/create`、`accountManager/users/update`、`accountManager/users/delete`：通过 `AccessStore` 创建并更新用户、初始化成员钱包、执行用户级联删除。
- `accountManager/wallet/topUp`、`accountManager/wallet/setAvailable`：通过 `CatalogBillingStore` 确保钱包并写入手工账本，保留冻结额度和管理员/用户归属校验。
- SQLite 与 SeaORM adapter 都已实现上述契约；成员状态、管理员权限、归属校验、错误路径和回读均有 Service 定向测试覆盖。

本阶段证据：

- `cargo test -p codexmanager-service --lib rpc_dispatch::storage_mutation_tests --offline -- --test-threads=1`：**8 passed / 0 failed / 0 ignored / 1658 filtered，exit 0**。
- `cargo test -p codexmanager-service --all-features --offline -- --test-threads=1`：**1804 passed / 0 failed / 10 ignored，exit 0**；lib 1659、app_settings 34、default_addr 10、e2e 1、gateway_logs 50、rpc 49、shutdown_flag 1，远端认证 2 和远端网关 1 ignored。
- `cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check`：均 **exit 0**。本次未修改前端或桌面代码，因此不重复运行前端 runtime/build；此前通过结果仍以迁移进度交接文档为准。

下一阶段按以下顺序继续，完成一项就补对应 adapter 回读测试和本节证据：

1. 账户相关剩余写入，确认 account、登录/令牌边界与 `AccessStore` 的最小契约，继续保留成员/管理员边界。
2. API Key 创建、模型更新，补齐 `ApiKeysStore` 的最小领域契约；钱包充值/设置已完成，继续复核其与 API Key 归属、扣费的联动边界。
3. app settings、usage/quota、aggregate 和 plugin 写入，确认哪些需要独立领域方法，哪些必须保留兼容 facade。
4. 每批完成后运行最窄 Service 定向测试，再运行 Service 全量；最终仍需 workspace check、fmt check 和 diff check。

当前六包状态必须保持真实：工作包 1 仍部分完成；工作包 2 仍有 Rhai、同步 facade、文件/CPU 和测试适配器兼容边界；工作包 3 已有隔离 MySQL/PostgreSQL 4/0/0 和 SQLite 9/0/4 证据但生产目标 HANDOFF；工作包 4 已完成本地客户端/性能复验但实际 Tauri、真实 provider/OAuth、tools/tool_calls 和生产代理 HANDOFF；工作包 5 已有隔离恢复证据但生产运维 HANDOFF；工作包 6 因无受控生产配置保持 HANDOFF。不得把本地 fixture、编译成功或 ignored 用例写成生产完成。

工作区继续保留所有未提交修改；不得执行 `reset`、`checkout`、`stash`、`clean`、`commit` 或 `push`。缺少生产配置只阻塞生产验收，不能阻塞上述本地迁移和测试。

## 2026-09-20 最新续接：API Key 创建与模型更新已接入注入存储

本次继续完成工作包 1 中 API Key 写入的一个完整切片，范围是 HTTP Service 的 `apikey/create` 与 `apikey/updateModel`。桌面/旧同步 RPC 兼容入口仍保留，不能据此宣称所有入口都已完成迁移。

- `DomainStorage::api_keys()` 增加 Key 查询、secret 查询、创建和配置更新契约；`ApiKeyCreate`/`ApiKeyConfigPatch` 保留归属、分组过滤、quota、路由、协议、profile、upstream、headers 和 model 字段。
- SQLite 增加创建和更新的事务边界；SeaORM 增加带 owner 的事务创建和配置更新。创建时同一事务写入 Key、secret、quota、account group filter，并校验成员归属、成员状态和钱包；重复 id/hash/custom key 或后续失败会回滚，不留下半成品。
- 注入式 Service handler 保留成员只能操作自己 Key、管理员字段成员不可修改、文本模型能力校验、image-only 拒绝、旧错误语义，以及 aggregate routing 清除 group 的行为。更新后的 protocol/profile/routing/quota 字段通过 adapter 回读核对。

本次实际验证：

- `cargo test -p codexmanager-service --all-features --offline --lib api_key_create_update_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1666 filtered，exit 0**。
- `cargo test -p codexmanager-service --all-features --offline --lib api_key_failure_and_routing_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1668 filtered，exit 0**。
- 复跑完整 `rpc_dispatch::storage_mutation_tests` 模块：**12 passed / 0 failed / 0 ignored / 1658 filtered，exit 0**。
- 上述定向组覆盖成员自有 Key、越权拒绝、管理员字段保护、重复 custom key、image-only 拒绝、quota 清除/更新、protocol/profile/routing 回读和失败不留半成品；SQLite 与 SeaORM adapter 均实际回读。
- 按要求运行 `cargo test -p codexmanager-service --all-features --offline -- --test-threads=1`：Service lib **1662 passed / 1 failed / 7 ignored，exit 101**。唯一失败为既有 `http::proxy_runtime::tests::official_responses_websocket_rebases_tool_output_on_next_account`，与本次 API Key 代码无直接关联；单独重跑同测试为 **1 passed / 0 failed / 0 ignored / 1669 filtered，exit 0**。后续源码调查确认是 failover 排队的 usage refresh 覆盖了权威限额状态，不是单纯时序波动；该次完整套件仍按 exit 101 保留，修复及最终回归见本节后文。
- `cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check` 均 **exit 0**；本次没有修改前端或桌面代码，未把此前的 pnpm/Tauri 结果冒充本次验证。

六个工作包状态保持真实：工作包 1 仍为**部分完成**，下一步继续迁移 settings、usage/quota、aggregate、plugin 及剩余 account 写入；工作包 2 的 Rhai、同步 facade、文件/CPU 和测试适配器兼容边界仍保留；工作包 3 只有隔离 MySQL/PostgreSQL dry-run/import 证据，生产目标仍 HANDOFF；工作包 4 只有本地客户端/性能证据，真实 Tauri、OAuth、provider、tools/tool_calls 和生产代理仍 HANDOFF；工作包 5 只有隔离备份恢复证据，生产切换、KMS/ACL、跨主机和真实恢复仍 HANDOFF；工作包 6 因缺少受控生产 DB/provider/认证/监听配置继续 HANDOFF。

本节涉及的代码和文档仍保留在工作区，未提交、未推送；没有执行 `reset`、`checkout`、`stash`、`clean`。下一会话从剩余写入继续，并对每个注入 handler 保留 SQLite/SeaORM 回读证据。

## 2026-09-20 本阶段收尾：API Key 事务、插件启停与完整回归

本阶段继续在 HTTP Service 注入式路径收窄了两组写入边界，并修正了两类完整套件中实际暴露的问题：

- API Key 创建失败时，SQLite 将缺失 owner 统一为“用户不存在”；新增的 SQLite/SeaORM 回读测试让 owner 校验发生在 Key、secret、quota、profile 和 wallet 写入之后，确认失败会回滚全部关联行。
- SeaORM API Key 更新新增 `update_with_owner` 事务边界：锁定 Key、在同一事务内校验 owner，再应用 model/routing/protocol/profile/upstream/headers/quota 更新，避免校验与写入之间的并发窗口。原有无 owner 的兼容调用仍保留。
- `SettingsPluginsStore` 增加插件状态更新和任务 schedule repair 契约；`plugin/enable`、`plugin/disable` 已由 `storage_async` 调用注入 `DomainStorage`，SQLite/SeaORM 均通过状态和任务 `next_run_at` 回读。成员拒绝、`pluginId`/`plugin_id` 参数及缺失参数也有覆盖；旧同步 facade 仍保留。

失败调查和修复证据：

- 先前 WebSocket 用量限制用例的 `usage_refresh_connection` 覆盖了已确认的 `usage_limit_exhausted`。根因是 failover 异步排队的 usage refresh 在 fixture 上连接失败后写入状态，而不是单纯时序波动；现在保留失败事件，但不会覆盖已确认的 limited/usage-limit reason。
- 本阶段第一次完整套件还暴露了 `account::proxy_testing::latency::tests::latency_test_reports_redirect_without_following_it`：fake proxy 的非阻塞 `accept` 只轮询 500ms，完整套件调度下可能在 warmup 连接前退出，导致 `status_code=None`。测试 fixture 已按实际请求数使用阻塞接收（302 一次，204 为 warmup 加十次采样），未增加业务等待或放宽断言。

本阶段实际命令和结果：

- API Key 创建/更新：2 passed / 0 failed / 0 ignored / 1666 filtered，exit 0；失败/routing：2 / 0 / 0 / 1668，exit 0。
- 插件启停定向测试：2 passed / 0 failed / 0 ignored / 1670 filtered，exit 0；proxy testing 回归：59 passed / 0 failed / 0 ignored / 1613 filtered，exit 0。
- 修复前的第一次完整 Service 命令保留为 1664 passed / 1 failed / 7 ignored，exit 101（latency fixture）；对应测试单独重跑通过。修复后再次运行 `cargo test -p codexmanager-service --all-features --offline -- --test-threads=1`：lib 1665 passed / 0 failed / 7 ignored，exit 0；app_settings 34、default_addr 10、e2e 1、gateway_logs 50、rpc 49、shutdown_flag 1 均 exit 0；远端认证 2、远端网关 1 保持 ignored。
- `cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0。diff check 只有仓库既有 LF/CRLF 提示。
- 本阶段未修改前端、桌面或 Web transport，未执行 pnpm runtime/build、Tauri GUI、真实 OAuth/provider/tools/tool_calls 或生产代理验收；此前结果不冒充本阶段证据。

六个工作包仍按真实边界记录：工作包 1 为部分完成，继续迁移 settings、usage/quota、aggregate、plugin 和剩余 account 写入；工作包 2 的 Rhai、同步 facade、文件/CPU、测试适配器和底层独立 runtime 兼容边界仍保留；工作包 3 只有隔离 MySQL/PostgreSQL dry-run/import 证据，生产目标 HANDOFF；工作包 4 只有本地客户端/性能证据，真实 Tauri、OAuth、provider、tools/tool_calls 和生产代理 HANDOFF；工作包 5 只有隔离备份恢复证据，生产切换、KMS/ACL、跨主机和真实恢复 HANDOFF；工作包 6 因缺少受控生产 DB/provider/认证/监听配置继续 HANDOFF。工作区保持未提交、未推送，且未执行 `reset`、`checkout`、`stash`、`clean`、`commit` 或 `push`。
