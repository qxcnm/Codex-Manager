# Axum、Tokio、SeaORM 迁移实证记录

更新时间：2026-09-20。实现、当前源码验证和生产验收分别记录。状态以文末最新日期的六个工作包记录为准；保留早期证据与失败历史，未提供的外部环境不计为通过。

## 实现范围

### Service 存储、权限与账本

- SeaORM 业务仓储覆盖账号/Token、API Key 及 secret/quota/rollup、模型目录/价格/路由、权限 groups、用户/会话、钱包/账本、请求日志/token 统计、用量、代理配置/历史、插件、聚合 API 和账户辅助数据。
- MySQL/PostgreSQL 或显式 `CODEXMANAGER_DATABASE_URL`（包含 SQLite URL）选择 SeaORM 业务数据源。空 URL、后端不匹配等配置错误导致启动失败，不回退到另一数据库。未设置 URL 的默认桌面 SQLite 保留原有初始化和迁移兼容路径。
- SeaORM 模式的旧 Storage 参数使用无业务 schema 的兼容句柄。网络等待前释放 Service 句柄池 lease；shared_handle 保留原 SQLx pool，不重开内存数据库。
- groups、wallet/ledger 幂等扣费、reset warmup claim 和代理历史使用事务。Token 条件写回比较完整旧凭据及刷新时间；会话完成比较原 state/verifier/created_at/status。MySQL 凭据比较区分大小写，旧请求不会重建已删除账号或覆盖并发导入凭据。

### SQLite 导入与切换

- storage-transfer 只读导出 JSONL 快照，检查表/列完整性和哈希；目标必须为空，事务导入后逐表回读校验，并修复 PostgreSQL 序列。
- Service/Web/Start 转发 storage-sqlite、storage-mysql、storage-postgres features。兼容层和导入工具仍依赖 SQLite，feature 选择不表示二进制完全移除 SQLite。
- 切换设置目标 backend/URL 后需重启。Service pool 为进程级资源，不支持同进程热换 URL；导入不会自动修改在线服务配置。

### Axum 与异步调用

- 生产 Service listener、RPC、Responses、Chat Completions、SSE、WebSocket、OAuth callback 使用 Axum/Tokio；tiny_http 仅保留为测试适配器和开发依赖。
- 网关上游、响应头/响应体等待、请求锁、退避重试、Agent Identity、鉴权恢复、observability bridge 直接 async/await。网络 RPC 使用异步分派，保留原 actor 权限和返回结构。
- account models/test/warmup/proxy、usage/subscription/Token refresh、reset credits、aggregate provider、Skills/registry、profile/model catalog、plugin catalog/install/update、Codex 最新版本查询的网络阶段已异步化。
- 普通同步 RPC、数据库 facade、文件/ZIP/CPU 阶段和旧同步 API 仍有受控兼容边界。Rhai 保持同步脚本 ABI，最多 8 个脚本 worker 等待异步网络结果；未宣称全仓库没有同步 bridge。

### 取消、计费与关闭

- 响应容量在联系 provider 前预留，请求锁与活动计数保持到日志/计费完成。断连中止候选切换/重试，记录 499；已入队 usage 帧排空后再完成最终计费。
- 已获准的 Token refresh 独立完成并持久化轮换结果，先保存 refresh grant，再做可选 API key exchange。调用者取消不会丢弃已返回的轮换凭据。锁为进程内锁，数据库 CAS 防止旧值覆盖，不保证跨进程 provider refresh 单次执行。
- reset credit 消费被取消后保留结果待核对状态，拒绝再次扣次。account/usage 后台任务有注册、取消、drain 和重启支持；代理任务/检测与账号测试结束为取消状态，停服后不再新增待执行队列。
- shutdown 排空网关响应、后台任务、Token completion 与 trace 队列。trace flush 文件缓冲，不等于 fsync 持久性保证。
- HTTP runtime 随进程保持，生产 listener 与 one-shot listener 共用；停止 listener 仍关闭端口并排空响应，不销毁全局异步客户端的 keepalive 连接 driver。线程参数在首次初始化时固定。设备码回归先关闭首 listener，再允许同一条 provider TCP 连接继续完成四个认证请求和第二 listener 状态查询，已通过。
- HTTP 的 X-Request-Id 保留调用者/中间件请求标识；业务日志的内部 trace_id 对应响应 X-CodexManager-Trace-Id，两者用途不同。

## 当前源码验证

- 全工作区 all-features 初次回归在 RPC 失败退出，发现用量快照未完成时过早断言，以及短生命周期 listener runtime 导致后台设备码认证复用失效连接。已分别修复持久化等待和共享 runtime，保留全部业务断言。
- 修复后完整运行 `cargo test -p codexmanager-service -p codexmanager-web --all-features --offline -- --test-threads=1`：**1812 passed / 0 failed / 10 ignored，exit 0**。包含 Service lib 1638、app_settings 34、default_addr 10、e2e 1、gateway_logs 50、RPC 49、shutdown 1、Web 29。日志：`%TEMP%/codexmanager-service-web-runtime-final10.log`。
- 未受最后 runtime 修复影响的 Core（含集成）485、SeaORM 常规 33、Start 2、rusqlite 1 均通过。因此常规回归按套件合计 **2333 passed / 0 failed / 26 ignored**，不是宣称初次 workspace 命令直接 exit 0。Core 证据位于 `%TEMP%/codexmanager-workspace-final5-tests.log`；补执行的尾部套件位于 `%TEMP%/codexmanager-final5-tail-*.log`。文档测试命令 exit 0，当前无 doctest 用例。
- 26 个常规 ignored 项中，23 个数据库/监听用例已另行显式执行（网关用例覆盖三个后端，因此专项共 25 次通过）；另有 3 个 GitHub/skills.sh 实网仓库探测未执行，不计为通过。
- SeaORM MySQL/PostgreSQL 实际数据库 ignored 套件：16 passed / 0 failed，含两个 SQLite 导入、groups/ledger 并发、Token CAS、provider 领域、历史用量。日志：`%TEMP%/codexmanager-storage-all-final4.log`。
- Service 远端 Token CAS/provider 领域：4 passed；最新 runtime 的认证 listener：MySQL/PostgreSQL 各 1 passed。日志：`%TEMP%/codexmanager-auth-{mysql,postgres}-runtime-final10.log`。认证测试中的 bootstrap/login/resolve/logout 直接调用 domain API，部分权限和用户操作经真实 RPC，不能当作 Web 登录 HTTP/cookie 或生产身份验收。
- 最新源码 MySQL-only 与 PostgreSQL-only Service/Web/Start 的 no-default-features check 均 exit 0。日志：`%TEMP%/codexmanager-final10-mysql-check.log`、`%TEMP%/codexmanager-final10-postgres-check.log`。
- 生产 normal 依赖树不含 tiny_http。日志：`%TEMP%/codexmanager-production-dependencies-final.txt`。
- 最新 runtime 持续网关监听 SQLite/MySQL/PostgreSQL 各 1 passed，均 exit 0。使用生产 start_server 和隔离本地 provider，验证 IPv4/IPv6 health/metrics/request-id、RPC token 与 member 拒绝、Responses/Chat Completions 的 JSON/SSE 四路径、1.5 倍 group 计费、日志/token/quota/ledger/snapshot/wallet 回读、仅旧 SQLite 存在的 key 被选定数据源拒绝，以及 stop/start 和两地址端口关闭。日志：`%TEMP%/codexmanager-gateway-{sqlite,mysql,postgres}-runtime-final10.log`。
- 三个后端均通过 SSE 断连后立即 shutdown 的验收：上游连接取消，499 日志及第五笔 estimated-input 账本在关闭返回前写入数据库。测试按既有 X-CodexManager-Trace-Id 关联业务日志，另断言 X-Request-Id 保留调用者标识；未降低状态、金额或关闭断言。这组 fixture 不覆盖 Web 登录 HTTP/cookie、OAuth 或 WebSocket。
- 最终 `cargo fmt --all -- --check` 与 `git diff --check` 通过。代码保留在工作区，未提交或推送。

## 早期对原执行设计的缺口记录（最新状态见文末）

当前不能标记为“全部按执行设计完成”，未完成项不只是真实生产环境：

- **领域存储边界**：core 的 StorageBackend trait 目前只暴露后端类型和 health；业务仍通过按领域划分的兼容 facade 分派 SQLite/SeaORM，未完成原设计第 4、8.3、阶段 1 要求的领域存储接口统一。HTTP AppState 仍是空结构，未按第 5.3 节注入存储、client、任务通道及关闭状态。
- **runtime 与兼容层收敛**：HTTP listener 已共享进程级 runtime，但 auth、usage、account background、upstream 等仍维护独立 runtime；同步 facade、旧同步 API 和部分测试用 tiny_http/crossbeam 适配仍保留。不能把异步网络主链路完成等同于原设计第 6、阶段 7 的全面收敛与清理完成。
- **导入预演**：目前有 export/inspect/import、快照校验和事务回滚；inspect 不连接目标数据库，尚未提供原设计阶段 6 所述的目标数据库 dry-run 与专门失败记录报告。
- **性能与完整客户端验收**：未完成迁移前后吞吐、P95/P99、内存、连接数、失败率对比；本轮未完成真实 Tauri 桌面启动、完整 Web 登录/session/UI 链路和备份恢复演练。Rust Web 测试通过不能代替这些验收。
- **文档收尾**：原设计的部署、备份恢复和排障材料仍需结合上述实现与实际演练补齐。

保留桌面默认 SQLite 是原设计明确要求，不属于待删除的过渡代码。以下生产环境项目另行待验收。

## 生产环境仍待验收

当前只有隔离本地 MySQL/PostgreSQL、SQLite 和确定性本地 provider 夹具。尚未获得真实生产数据库配置、provider 配置、生产认证身份及完整目标监听地址/端口清单，因此下列验收未执行：

- 生产目标数据库迁移/备份/导入与业务回读。
- 真实 provider 的 Responses/Chat Completions、JSON/SSE、tools/tool_calls、错误恢复和断连计费。
- 生产认证登录、权限、会话撤销及真实 OAuth 流程。
- 全部生产 Service/Web/OAuth callback/反向代理监听地址、WebSocket 链路及停服关闭。

接续时提供现有配置文件路径、目标地址和端口清单即可，不把凭据贴入聊天或报告。确认目标库用途及导入目标为空；不使用其他项目的配置，不用本地 fixture 替代生产结论。

## 复验命令与隔离要求

```powershell
cargo test --workspace --all-features --offline -- --test-threads=1
cargo check -p codexmanager-service -p codexmanager-web -p codexmanager-start --no-default-features --features storage-mysql --offline
cargo check -p codexmanager-service -p codexmanager-web -p codexmanager-start --no-default-features --features storage-postgres --offline
cargo fmt --all -- --check
git diff --check
```

- SeaORM 实际数据库套件：设置 CODEXMANAGER_TEST_MYSQL_URL、CODEXMANAGER_TEST_POSTGRES_URL、CODEXMANAGER_TEST_IMPORT_MYSQL_URL、CODEXMANAGER_TEST_IMPORT_POSTGRES_URL，运行 `cargo test -p codexmanager-storage-seaorm --all-features --lib -- --ignored --test-threads=1`。导入库必须独立且为空；成功后保留数据，复验需新空库。
- 认证 listener：分别设置专用空认证库 URL，以独立进程运行 `cargo test -p codexmanager-service --all-features --test remote_auth_listener mysql_remote_auth_permission_listener -- --ignored --exact`；PostgreSQL 测试名为 postgres_remote_auth_permission_listener。
- 持续网关监听：设置 CODEXMANAGER_TEST_GATEWAY_BACKEND 为 sqlite/mysql/postgres，每个后端独立进程运行 `cargo test -p codexmanager-service --all-features --test remote_gateway -- --ignored --test-threads=1`。远端后端使用专用 CODEXMANAGER_TEST_MYSQL_URL 或 CODEXMANAGER_TEST_POSTGRES_URL，SQLite 自动建立独立临时库。
- 禁止共享 fixture 并行使用同一库；保留测试数据，不清理用户业务库；不把含凭据 URL 写入命令行、报告或日志。

## 2026-09-19 续接会话：六个工作包逐项证据

本节覆盖本次续接实际改动和验证，优先于上文 2026-09-17 的“尚未闭合”描述。状态使用“已实现/本地已验证/部分完成/HANDOFF”，不把本地 fixture、编译成功或路由存在当作生产通过。

| 工作包 | 本次实现 | 已执行证据 | 当前缺口 |
| --- | --- | --- | --- |
| 1. 领域存储接口与依赖注入 | 新增 `DomainStorage` 及 accounts/access/api-keys/observability/catalog-billing/settings-plugins 分域 trait；SQLite 与 SeaORM adapter 实现；`StorageBackend::domain()` 暴露契约；`AppState` 以 `OnceCell<Arc<dyn DomainStorage>>` 捕获一次存储，携带共享 HTTP client、RPC 并发槽、任务广播通道和 shutdown 状态；storage RPC 通过该状态读取。 | `cargo test -p codexmanager-core storage::traits::tests --offline -- --test-threads=1`：2 passed/0 failed；`cargo test -p codexmanager-service http::state::tests::injected_domain_storage_is_captured_once_and_checked --offline -- --exact --test-threads=1`：1 passed/0 failed；Service/Web/Start all-features check exit 0。 | 仍有大量旧同步 facade 和按领域兼容函数；尚未完成所有写入 handler 的统一 DomainStorage 化，也未删除兼容 SQLite 入口。工作包 1 为**部分完成**。 |
| 2. runtime/同步桥收敛 | HTTP、auth completion、account background、upstream 等 runtime 入口统一复用进程级 runtime；移除失效的旧 upstream client re-export；保留 `run_sync`/blocking bridge 作为受控兼容层。 | `cargo check -p codexmanager-service -p codexmanager-web -p codexmanager-start --all-features --offline`：exit 0；本地二进制探针的服务启动、停止和 graceful shutdown 均完成。 | Rhai/同步脚本 ABI、数据库 facade、文件/CPU 阶段和测试适配器仍存在；尚未证明全仓库无同步桥，也未做生产长时关闭和线程/连接基线。工作包 2 为**部分完成**。 |
| 3. import dry-run/失败报告 | `storage-transfer dry-run` 增加目标只读 schema/catalog/列类型/唯一键/行数扫描、未知非空表拒绝、稳定失败码和逐表统计；报告不输出 URL、凭据或原始行值；修正 WAL 下用文件字节比较造成的误报。 | `cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --test-threads=1`：9 passed/0 failed/4 ignored；覆盖 schema/type/非空目标/未知表/checksum/rollback/re-execution；`cargo check -p codexmanager-storage-seaorm --all-features --offline`：exit 0；CLI 不存在 SQLite 目标时返回结构化连接失败且 `target_modified=false`。 | MySQL/PostgreSQL 真实目标测试因无独立配置保持 ignored/HANDOFF；prepare-target 的 DDL 仍可能有数据库副作用，生产目标回读未执行。工作包 3 为**本地 SQLite 已验证，远端目标 HANDOFF**。 |
| 4. 性能与桌面/Web 验收 | 修复本地探针的 loopback WebSocket URL、官方 backend fixture 路径和 JSON-shaped text/plain RPC 解析；保留短流/长流/health、资源和请求失败率指标。 | `pnpm.cmd -C apps run test:runtime`：217 passed/0 failed；`pnpm.cmd -C apps run build:desktop`：exit 0；`cargo check --manifest-path apps/src-tauri/Cargo.toml --offline`：exit 0；重建后的真实 Service/Web 二进制 loopback SQLite fixture 报告 `%TEMP%/codexmanager-migration-current8.json`：PASS。报告验证 Responses 与 Chat Completions JSON/SSE、Responses WebSocket 两个事件、Web 登录/session/权限/登出/禁用会话及 SQLite 回读；短流 103 requests、97.594 RPS、P50/P95/P99 16.669/30.176/39.803 ms、0 failures；混合短/长 4/4 requests、短请求 P95 6604.003 ms、长流 P95 4971.194 ms、0 failures；峰值 RSS 约 58 MB、线程 51、TCP 13。 | 没有迁移前同硬件/同 fixture 的可比 baseline，不能声明无回归；混合长流下短请求尾延迟较高，需基线和真实流量复测；WebSocket fixture 事件已回读但 close code 记录为 1006，需真实 provider/反向代理确认关闭语义；未启动实际 Tauri GUI，也未执行生产浏览器、OAuth、tools/tool_calls 和真实 provider。工作包 4 为**本地二进制已验证，完整/生产验收 HANDOFF**。 |
| 5. 备份/恢复/运维文档 | 新增[《Axum/SeaORM 备份、恢复与运维手册》](Axum-SeaORM备份恢复与运维手册.md)，覆盖 SQLite WAL 物理备份、JSONL inspect/prepare-target/dry-run/import、切换、失败回退、密钥保护、升级回退和排障；同步记录 9/0/4 transfer 证据和 MySQL/PostgreSQL 缺口。 | 文档命令与实现入口逐项核对；transfer/preflight 过滤测试 9 passed/0 failed/4 ignored；文档明确快照含敏感数据且未自动 KMS 加密。 | 未在生产执行真实备份、恢复、KMS/ACL、保留周期、销毁证明和跨主机回读；本地文档和 fixture 不能替代演练。工作包 5 为**文档实现完成，运维演练 HANDOFF**。 |
| 6. 生产验收 | 新增[`scripts/production-acceptance.ps1`](../../../scripts/production-acceptance.ps1)，检查受控配置文件的必需变量、占位符、MySQL 后端和 URL、地址格式、可选 token 文件及监听可达性，不打印值或 secret；缺配置或语义不完整时输出 JSON `HANDOFF` 并以退出码 2 结束，配置和启用的监听器均通过时才输出 `READY_FOR_AUTHENTICATED_ACCEPTANCE`。 | 脚本静态检查并入本次工作区；没有调用生产环境，也没有伪造 provider/DB/认证结果。 | 当前没有生产 DB URL、provider、认证身份或完整监听地址清单；真实 DB 迁移/回读、OAuth、WebSocket、代理和停服验收全部待配置。工作包 6 为**HANDOFF**。 |

本次还运行了 `cargo fmt --all -- --check` 和 `git diff --check`；前者 exit 0，后者仅有仓库既有 LF/CRLF 提示，没有内容错误。工作区仍保留所有未提交修改；本次没有 commit、push、reset、stash 或清理动作。综合结论仍是：六个工作包均有继续实现，但不能表述为“全部按执行设计完成”，生产与远端数据库项必须待真实配置后单独验收。

## 2026-09-19 继续收尾：领域写入与本地恢复演练

本节记录在上一节基础上继续执行的实际改动和命令证据。它只收窄已完成的局部边界，不改变六个工作包的部分完成和 HANDOFF 结论。

| 工作包 | 本次继续实现/复核 | 命令证据与结果 | 未执行项、阻塞和下一步 |
| --- | --- | --- | --- |
| 1. 领域存储接口与依赖注入 | 为 `ObservabilityStore` 增加 `clear_request_logs`，为 `ApiKeysStore` 增加 `delete_api_key`；SQLite/SeaORM adapter 均实现；`storage_async` 将 `requestlog/clear` 和 `apikey/delete` 接入注入的 `DomainStorage`，保留旧同步 facade 作为兼容路径；成员删除继续通过注入的 owner 查询校验归属，管理员缺少 `id` 时保持旧错误语义。 | `cargo test -p codexmanager-service rpc_dispatch::storage_async::tests --offline -- --test-threads=1`：2 passed / 0 failed / 0 ignored，1,652 filtered，exit 0；覆盖两个新 handler 的真实 SQLite adapter 回读。此前本轮 `cargo test -p codexmanager-core storage::traits::tests` 为 2 passed / 0 failed；`cargo test -p codexmanager-storage-seaorm ... transfer` 为 9 passed / 0 failed / 4 ignored。 | 其他 API Key、settings、account、usage、plugin 等写入仍有同步 facade；未完成所有写入 handler 的统一迁移，也未删除兼容入口。工作包 1 仍为**部分完成**；下一步继续按写入风险逐项迁移并补 handler 回读测试。 |
| 2. runtime 与同步桥收敛 | 复核 auth、usage、account background、upstream、Rhai 和 blocking bridge：生产网络 runtime 已复用进程级 runtime；`run_sync`、Rhai 同步 ABI、数据库 facade、文件/CPU 阶段和测试适配器仍是受控兼容边界，没有进行无证据的删除。 | 源码复核确认 `process_runtime`/`front_proxy_runtime` 为共享进程 runtime；本轮未发现可安全删除且不影响兼容契约的额外边界。 | 尚未证明全仓库无同步 bridge，也未做生产长时关闭、线程和连接基线。工作包 2 仍为**部分完成**。 |
| 3. import dry-run | 保持上一节的 SQLite dry-run 结论。 | `cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --test-threads=1`：9 passed / 0 failed / 4 ignored，exit 0。 | MySQL/PostgreSQL 独立配置仍缺失，真实 prepare-target/dry-run/import/目标回读继续 HANDOFF；不能用 SQLite 结果替代远端目标。 |
| 4. 性能与客户端验收 | 本轮未修改性能探针，也未把已有 current8 报告升级为 baseline 或生产验收。 | 继续以 `%TEMP%\codexmanager-migration-current8.json` 的本地二进制报告为已知证据；迁移前可比 baseline、混合长流尾延迟复测和 WebSocket close code 1006 的真实链路确认仍未完成。 | 未启动实际 Tauri GUI，未执行生产浏览器、OAuth、tools/tool_calls 或真实 provider。工作包 4 仍为**本地已验证，完整验收 HANDOFF**。 |
| 5. 备份、恢复与运维 | 在隔离临时目录执行 SQLite 文件副本备份、恢复、哈希比对，再对备份副本和恢复副本分别执行 `storage-transfer export` 并比较 JSONL 快照哈希；结果已补入[备份恢复与运维手册](Axum-SeaORM备份恢复与运维手册.md)。 | 临时目录 `%TEMP%\codexmanager-backup-restore-5f232b05a0d54b3399b063afa531a6e6`；备份/恢复文件哈希均为 `2A2ED2C00562EE370FCA2AD2121278DC1968EEFE001F94353BA33A7DAF5F0DEB`；两次 export 均 exit 0，JSONL 哈希均为 `B7B796A3BBFD710660F150692D966FC9CE26E62CB763770A02B496EFAC69E54D`。 | 服务 HTTP health/readback 未执行，进程启动被当前执行环境策略拦截；没有生产 KMS/ACL、保留周期、销毁证明、跨主机恢复或真实数据库演练。工作包 5 为**本地副本恢复回读已验证，生产运维演练 HANDOFF**。 |
| 6. 生产验收 | 按受控脚本检查生产验收入口。 | `scripts/production-acceptance.ps1` 在缺少配置时输出 `result=HANDOFF`、`secrets_loaded=false`，退出码 2；未打印或记录任何 secret。 | 没有真实 DB/provider/认证/完整监听配置，因此生产迁移、回读、OAuth、WebSocket、代理和停服验收均未执行。工作包 6 仍为**HANDOFF**。 |

本轮新增代码和文档仍在工作区，未 commit、未 push；没有执行 reset、checkout、stash 或 clean。综合结论仍为：领域写入边界有进一步收敛，本地 SQLite 备份恢复和 JSONL 回读已得到可核对证据，但六个工作包没有全部完成，远端数据库、完整客户端、生产运维和生产验收必须在受控配置具备后继续。

## 2026-09-20 继续收尾：真实远端导入、运行时桥、关闭握手与客户端验收

以下是本日最新事实，覆盖并修正前文相同工作包的旧状态描述；旧失败日志保留，不把一次失败改写成成功。

| 工作包 | 已实现内容 | 命令证据 | 未执行项、阻塞和下一步 |
| --- | --- | --- | --- |
| 1. 领域存储接口与依赖注入 | `apikey/enable`、`apikey/disable` 和 `apikey/delete` 统一使用注入的 `DomainStorage`，成员先校验 user owner，管理员保留缺少 id 的旧错误语义；SQLite/SeaORM 均回读验证。 | Service 定向矩阵覆盖两种 adapter、管理员/成员/越权/未登录/空 id/缺失 id/删除 owner：5 passed；与 WebSocket 定向组合同命令 **8 passed / 0 failed / 0 ignored，exit 0**。 | 其他 API Key、settings、account、usage、plugin 写入仍有同步 facade；工作包 1 仍**部分完成**。 |
| 2. runtime 与同步桥收敛 | auth、usage、aggregate 及 usage refresh 的同步兼容包装在桥容量耗尽时返回 Result 错误，不再 `expect` panic；新增受保护 Service `GET /__shutdown`，要求 RPC token、管理员 actor 和既有来源校验，通知不占普通请求槽；Web `/__quit` 探针使用有效登录会话。Responses/Web gateway 收到 Close 后限时 flush，避免 1006。 | HTTP/runtime 定向 **201 passed / 0 failed / 0 ignored，exit 0**；其中关闭入口权限/饱和槽测试 22 条。Web **31 passed / 0 failed / 0 ignored，exit 0**。新二进制直连和 Web 转发 WS close code 均 1000；关闭三进程均 HTTP 200、exit 0、`forced_termination=false`。 | `crates/rusqlite` 仍有底层独立 SQLx runtime；Rhai、同步 facade、文件/CPU 和测试适配器仍是有理由的兼容边界。未证明全仓库同步 bridge 清零。 |
| 3. import dry-run | 修复 MySQL INFORMATION_SCHEMA 查询的稳定列别名；远端 SQLite 源只读回读在删除 Windows fixture 前等待连接关闭。 | 新建隔离库串行运行 `cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --ignored --test-threads=1`：**4 passed / 0 failed / 0 ignored，exit 0**。覆盖 MySQL/Pg dry-run、重复预演、空目标保护、正式 import、非空目标拒绝和回读；目标库与数据库名见 `%TEMP%/codexmanager-transfer-targets-20260920.json`。初次 **2 passed / 2 failed / exit 101** 原因已记录于 `%TEMP%/codexmanager-transfer-real-initial-20260920.log`。 | 本地隔离库不代表生产库；生产权限/大数据量/真实在线切换仍 HANDOFF。 |
| 4. 性能与客户端验收 | 新增实际 Chromium 流程脚本，验证登录、首次引导、cookie/tab session、API Key 页面真实 RPC、退出后的 401；性能探针新增独立 Key 场景、binary/script hash、退出码、强制终止标识。补回失落的 `/__shutdown` 后完成正常停服。 | Chromium 145.0.7632.6：**7 项通过、0 page error**；Web HTTP/RPC **12 项通过**；真实二进制 Responses/Chat JSON/SSE、直连/Web WS close 1000。旧基线独立 worktree `a4805dd4...` 新 target 构建 exit 0。旧→新→新→旧四轮：旧版两轮因 WS 1006/强制终止 exit 1；当前两轮 exit 0。当前共享 Key 混合短请求 P95 两轮中位数 7359.799ms，基线 6977.851ms，约 +5.47%；独立 Key 混合短请求 P95 两轮中位数当前 138.418ms、基线 132.470ms。详细报告：[本地客户端与性能复验](Axum-SeaORM本地客户端与性能复验-2026-09-20.md)，原始 JSON 在 `%TEMP%/codexmanager-*-20260920.json`。 | dev 构建、每版本两轮、loopback fake provider，不能给生产容量或无回归结论；共享 Key 长流尾延迟仍秒级。未启动 Tauri GUI（没有现成 src-tauri exe，未为此改变构建流程）；未执行真实 OAuth/provider tools/tool_calls、生产反向代理。工作包 4 **本地验证完成，完整验收 HANDOFF**。 |
| 5. 备份/恢复与运维 | 在隔离 MySQL/Pg 导入库完成原生备份恢复：MySQL `mysqldump` 全结构/有序数据比对；Pg `pg_dump` custom + `pg_restore --exit-on-error --no-owner`，按 INSERT/sequence setval 比对；业务设置实际回读。 | 2 backend PASS / 0 failed / exit 0；报告 `%TEMP%/codexmanager-native-restore-f696249af1/report.json`。MySQL backup/逻辑哈希 `1D0945FA...19F93`，Pg 逻辑哈希 `A9846255...0F80D6`，两者源/恢复一致。两个专属测试容器已停止，库和备份保留。 | 只验证隔离本机目标；Service 切换/失败回退、跨主机、崩溃 WAL、KMS/ACL、保留销毁、真实生产恢复仍 HANDOFF。 |
| 6. 生产验收 | 运行受控配置预检；未读取或打印 secret。 | `scripts/production-acceptance.ps1 -ConfigPath .\codexmanager.production.env` 输出 `HANDOFF`，缺配置，exit 2。 | 没有生产 DB/provider/认证/监听配置；生产 DB、OAuth、真实 provider、tools/tool_calls、反向代理和生产关闭均未执行，仍 HANDOFF。 |

本日最终 Rust/构建检查：`cargo test -p codexmanager-service --all-features --offline -- --test-threads=1` **1797 passed / 0 failed / 10 ignored，exit 0**（前述关闭入口之后另有 HTTP/runtime 201 条针对性复验）；`cargo test -p codexmanager-core --offline -- --test-threads=1` **485 passed / 0 failed / 0 ignored，exit 0**；`cargo test -p codexmanager-start --offline -- --test-threads=1` **2 passed / 0 failed / 0 ignored，exit 0**；`cargo test -p codexmanager-web --offline -- --test-threads=1` **31/0/0，exit 0**；SeaORM 常规 **37/0/18 ignored，exit 0**；SQLite 远端网关 **1/0/0，exit 0**；`cargo check --workspace --all-features --offline`、`cargo build -p codexmanager-service -p codexmanager-web --offline`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0；`pnpm.cmd -C apps run test:runtime` **217/0/0**、`pnpm.cmd -C apps run build:desktop` exit 0。代码和文档仍未提交、未推送；未执行 reset、checkout、stash、clean。

## 2026-09-20 继续迁移：账号自服务与 API Key 归属写入

本节记录上一节之后实际完成的下一组领域写入迁移。它扩大了工作包 1 的已验证范围，但不改变六个工作包整体仍为“部分完成/HANDOFF”的结论。

| 工作包 | 本次已实现内容 | 命令证据 | 未执行项、阻塞和下一步 |
| --- | --- | --- | --- |
| 1. 领域存储接口与依赖注入 | `AccessStore` 增加资料更新、用户创建/更新/删除和带旧哈希条件的密码更新；`ApiKeysStore` 增加归属保存；`CatalogBillingStore` 增加钱包确保/账本调整。SQLite/SeaORM adapter 均实现。`accountManager/profile/update`、`accountManager/password/change`、`accountManager/apiKeyOwners/set`、`accountManager/users/create`、`accountManager/users/update`、`accountManager/users/delete`、`accountManager/wallet/topUp`、`accountManager/wallet/setAvailable` 已接入 `storage_async`，保留无 `AppState` 桌面入口的兼容边界；密码校验、重复用户名、管理员保护、成员状态、初始钱包、归属校验、冻结额度、账本调整和回读均保留。 | 定向 `storage_mutation_tests`：**8 passed / 0 failed / 0 ignored / 1658 filtered，exit 0**；Service 全量：lib 1659、app_settings 34、default_addr 10、e2e 1、gateway_logs 50、rpc 49、shutdown_flag 1，另有远端认证 2 和远端网关 1 ignored，合计 **1804 passed / 0 failed / 10 ignored，exit 0**。`cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check` 均 exit 0。 | API Key 创建/模型更新、settings、account、usage、plugin 及 aggregate/quota/system 写入仍使用旧同步 facade；继续按边界迁移并补适配器回读测试。工作包 1 仍为**部分完成**。 |
| 2. runtime 与同步桥收敛 | 本次没有删除新的兼容桥；账号写入复用现有注入存储，不新增同步连接路径。 | 复用本次 Service 全量测试和 workspace check；未发现因本次写入迁移引入新的 runtime 失败。 | Rhai、同步 facade、文件/CPU 阶段、测试适配器和底层独立 runtime 仍保留；继续做桥边界审计，不能宣称同步 bridge 清零。工作包 2 仍为**部分完成**。 |
| 3. import dry-run | 本次未修改 transfer 实现。 | 继续采用此前真实隔离 MySQL/PostgreSQL dry-run/import **4 passed / 0 failed / 0 ignored，exit 0** 及 SQLite 9/0/4 证据；本次未重复执行。 | 生产权限、大数据量、在线切换和生产目标回读仍无配置，保持 HANDOFF。 |
| 4. 性能与客户端验收 | 本次未修改前端、桌面或性能探针。 | 继续采用此前 Chromium、WebSocket close 1000、基线对比和本地二进制报告；本次仅运行 Rust/service 范围检查，因此未重复 `pnpm` 验证。 | 实际 Tauri GUI、真实 OAuth/provider、tools/tool_calls、生产反向代理及容量结论仍未执行；共享 Key 混合长流尾延迟仍需真实流量复测。工作包 4 为**本地验证完成，完整验收 HANDOFF**。 |
| 5. 备份、恢复与运维 | 本次未修改运维实现。 | 继续采用此前隔离 SQLite 副本恢复回读和 MySQL/Pg 原生备份恢复 **2 backend PASS / 0 failed** 证据；本次未重复执行。 | Service 切换/失败回退、跨主机、崩溃 WAL、KMS/ACL、保留销毁和生产恢复仍未执行。工作包 5 为**本地/隔离环境已验证，生产运维 HANDOFF**。 |
| 6. 生产验收 | 本次未调用生产环境。 | `scripts/production-acceptance.ps1` 此前在缺配置时返回 `HANDOFF`、exit 2；本次没有伪造配置或结果。 | 生产 DB/provider/认证/监听配置仍缺失，生产 DB、OAuth、真实 provider、代理和停服验收继续 HANDOFF。 |

本次新增代码和文档仍未提交、未推送；没有执行 `reset`、`checkout`、`stash` 或 `clean`。下一阶段优先继续迁移 account、settings、API Key 创建/模型更新、usage/quota、aggregate 和 plugin 的剩余写入，并为每个注入 handler 保留 SQLite/SeaORM 真实回读证据。

## 2026-09-20 继续收尾：API Key 创建与模型更新注入迁移

本节记录本次在 HTTP Service 路径完成的 API Key 写入切片。旧同步/桌面 RPC 入口仍作为兼容边界存在，因此工作包 1 继续标记为部分完成。

| 工作包 | 本次已实现内容 | 命令证据与结果 | 未执行项、阻塞和下一步 |
| --- | --- | --- | --- |
| 1. 领域存储接口与依赖注入 | `DomainStorage::api_keys()` 新增查询 Key/secret、创建和配置更新契约；SQLite 提供创建/更新原子事务，SeaORM 提供带 owner 的事务创建和配置更新。`apikey/create`、`apikey/updateModel` 在 `storage_async` 中调用注入存储，覆盖 secret、quota、account group filter、owner/wallet、model、routing、protocol、profile、upstream 和 headers；成员归属、管理员字段、模型能力与旧错误语义保留。 | `cargo test -p codexmanager-service --all-features --offline --lib api_key_create_update_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1666 filtered，exit 0**；`cargo test -p codexmanager-service --all-features --offline --lib api_key_failure_and_routing_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1668 filtered，exit 0**。两组均覆盖 SQLite/SeaORM 回读、成员只能操作自己的 Key、管理员字段成员不可修改、重复 custom key、image-only 拒绝、quota 清除/更新、protocol/profile/routing 回读及失败回滚。 | 账户剩余写入、settings、usage/quota、aggregate、plugin 和其他同步 facade 仍未全部迁移；桌面旧入口仍保留。工作包 1 为**部分完成**，下一步继续按领域补最小契约和回读测试。 |
| 2. runtime 与同步桥收敛 | 本次只增加注入存储调用和事务适配，没有删除 Rhai、同步 facade、文件/CPU 或测试适配器边界。 | 复用 Service 定向测试；未发现本次 API Key 写入引入的 runtime 失败。 | 仍未证明全仓库同步 bridge 清零；工作包 2 继续**部分完成**。 |
| 3. import dry-run | 本次未修改 transfer。 | 沿用此前隔离 MySQL/PostgreSQL dry-run/import **4 passed / 0 failed / 0 ignored** 证据，本次未重复执行。 | 生产目标、生产权限、在线切换和大数据量仍 HANDOFF。 |
| 4. 性能与客户端验收 | 本次未修改前端、桌面或性能探针。 | 本次只执行 Rust Service 定向/全量及 workspace 检查，未执行 pnpm、Tauri 或浏览器验收。 | 真实 Tauri、OAuth、provider、tools/tool_calls、生产反向代理和容量结论继续 HANDOFF。 |
| 5. 备份/恢复与运维 | 本次未修改运维实现。 | 沿用此前隔离 SQLite/原生 MySQL/Pg 恢复证据，本次未重复执行。 | 生产切换/失败回退、KMS/ACL、跨主机、崩溃 WAL、保留销毁和真实恢复继续 HANDOFF。 |
| 6. 生产验收 | 本次未调用生产环境。 | 未读取或伪造生产 DB/provider/认证/监听配置。 | 工作包 6 因受控配置缺失继续 HANDOFF。 |

复跑完整 `rpc_dispatch::storage_mutation_tests` 模块：**12 passed / 0 failed / 0 ignored / 1658 filtered，exit 0**。本次全量 Service 命令按要求执行：`cargo test -p codexmanager-service --all-features --offline -- --test-threads=1` 在 lib 目标报告 **1662 passed / 1 failed / 7 ignored，exit 101**，唯一失败是 `http::proxy_runtime::tests::official_responses_websocket_rebases_tool_output_on_next_account`；该测试单独重跑为 **1 passed / 0 failed / 0 ignored / 1669 filtered，exit 0**。后续源码调查确认是 failover 排队的 usage refresh 覆盖权威限额状态，不是单纯时序波动；该次完整套件仍按 exit 101 保留，且 Cargo 在 lib 失败后未继续后续 integration targets。

最终检查：`cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check` 均 **exit 0**。本次没有修改前端或桌面代码，未执行前端/桌面测试；不能把此前结果冒充本次验证。全部代码和文档仍未提交、未推送；没有执行 `reset`、`checkout`、`stash`、`clean`、`commit` 或 `push`。

## 2026-09-20 继续收尾：API Key 事务、插件启停与完整回归

本阶段实际完成的实现：

| 范围 | 实现与边界 | 回读/错误覆盖 |
| --- | --- | --- |
| API Key 创建 | SQLite 缺失 owner 的错误语义统一为“用户不存在”；创建在 owner 校验前写入的 Key、secret、quota、profile、account group filter 和 wallet 关联由同一事务保护。 | SQLite/SeaORM 均验证缺失成员失败后 Key 数量、hash secret 和 wallet 均无残留；重复 custom key、image-only、成员归属和管理员保护沿用并回读。 |
| API Key 更新 | SeaORM `update_with_owner` 在事务内锁 Key、校验 owner 并更新 model、routing、protocol、profile、upstream、headers、quota；旧无 owner 兼容调用保留。 | 定向创建/更新与失败/routing 两组各 2 passed；协议/profile/routing/quota 回读通过。 |
| Plugin enable/disable | `SettingsPluginsStore` 新增状态更新和 interval schedule repair；SQLite/SeaORM adapter 实现，`storage_async` 通过注入 `DomainStorage` 调用，支持 `pluginId` 与 `plugin_id`。 | SQLite/SeaORM 各验证成员拒绝、缺失参数、禁用状态、启用状态和 `next_run_at` 回读；旧同步 facade 保留。 |
| 完整套件失败修复 | usage refresh 失败不再覆盖已确认的 limited/`usage_limit_exhausted` reason；latency fake proxy 改为按 302 一次或 204 warmup+10 samples 阻塞接收，移除 500ms 非阻塞竞态。 | WebSocket 用量限制用例通过；proxy testing 59/0/0 通过；完整套件最终全绿。 |

本阶段命令证据：

- `cargo test -p codexmanager-service --all-features --offline --lib api_key_create_update_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1666 filtered，exit 0**。
- `cargo test -p codexmanager-service --all-features --offline --lib api_key_failure_and_routing_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1668 filtered，exit 0**。
- `cargo test -p codexmanager-service --all-features --offline --lib plugin_status_and_schedule_readback -- --test-threads=1`：**2 passed / 0 failed / 0 ignored / 1670 filtered，exit 0**。
- `cargo test -p codexmanager-service --all-features --offline account::proxy_testing:: -- --test-threads=1`：**59 passed / 0 failed / 0 ignored / 1613 filtered，exit 0**。
- 修复前第一次完整 Service 命令：**1664 passed / 1 failed / 7 ignored，exit 101**；失败是 `account::proxy_testing::latency::tests::latency_test_reports_redirect_without_following_it`，实际 `status_code=None`、期望 `Some(302)`。单独重跑曾通过，但源码调查确认是 fake proxy 500ms 非阻塞接收竞态，随后改为按请求数阻塞接收。
- 修复后完整 `cargo test -p codexmanager-service --all-features --offline -- --test-threads=1`：lib **1665 passed / 0 failed / 7 ignored，exit 0**；app_settings **34/0/0**、default_addr **10/0/0**、e2e **1/0/0**、gateway_logs **50/0/0**、rpc **49/0/0**、shutdown_flag **1/0/0** 均 exit 0；远端认证 **0 passed / 2 ignored**、远端网关 **0 passed / 1 ignored**。
- `cargo check --workspace --all-features --offline`、`cargo fmt --all -- --check`、`git diff --check`：均 **exit 0**；diff check 仅输出既有 LF/CRLF 警告。

失败调查的完整结论：之前 WebSocket 用例中的 `usage_refresh_connection` 是 failover 排队的后台 refresh 在 fixture 上失败后覆盖了已确认的 usage-limit 状态，现已只保留失败事件而保护权威限额 reason；本次 latency 失败是测试 fake proxy 的短轮询窗口，现已消除其调度竞态。两处都保留了原业务断言，没有删断言或盲加 sleep。完整套件最终成功，首次失败日志仍保留在 `%TEMP%/codexmanager-service-full-20260920-final.log` 和 `%TEMP%/codexmanager-service-full-20260920-final2.log`。

本阶段没有修改前端、桌面或 Web transport，故未执行 `pnpm.cmd -C apps run test:runtime`、`build:desktop` 或 Tauri/浏览器验收；没有生产 DB/provider/认证/监听配置，未执行生产验收。六个工作包状态继续为：工作包 1 部分完成；工作包 2 保留 Rhai、同步 facade、文件/CPU、测试适配器和独立 runtime 兼容边界；工作包 3 生产目标 HANDOFF；工作包 4 本地客户端/性能已验证但真实 Tauri/OAuth/provider/tools/tool_calls/生产代理 HANDOFF；工作包 5 隔离恢复已验证但生产运维 HANDOFF；工作包 6 HANDOFF。代码和文档仍未提交、未推送，且没有执行 `reset`、`checkout`、`stash`、`clean`、`commit` 或 `push`。
