# 运行时收敛与保留边界

本轮日期：2026-09-17。本文件说明工作包 2 的实现及复验入口；执行结果在完成验证后补录，不能将本文件本身视为通过证据。

## 执行器与关闭

- `runtime/service_runtime.rs::process_runtime` 是 Service 的进程级 Tokio runtime。HTTP、OAuth、认证、usage、账号后台、provider、插件异步网络和 SeaORM 共用它。Web main 复用同一入口；Tauri 在创建应用前将这个 handle 注册给 `tauri::async_runtime`。
- runtime 生命周期长于任一 listener。停止 listener 关闭监听端口、等待响应记账与已接纳任务；不销毁 reqwest/SQLx keepalive 连接的驱动。
- SeaORM 池在进程级执行器上创建。显式 URL 改动仍要求重新启动进程，不允许热切换或静默回退。
- 2026-09-20 二进制探针发现迁移后的独立 Service 缺少 `/__shutdown` 控制路由，旧通知无法关闭另一进程。补回路由并要求 RPC token、管理员 actor 及既有 Origin/Sec-Fetch-Site 校验；`request_shutdown` 通知携带 token。该路由位于普通请求并发槽之外，先通知 AppState 和进程关闭状态，再由既有 listener/drain 路径排空。
- OAuth listener 持有 JoinHandle，停服等待并清除登记，下次启动可重新绑定相同端口。设备码登录停服取消并等待终态，重复 login ID 被拒绝，最多 64 个并发设备码任务。one-shot listener 自行结束并不触发整个进程停服，设备码连接复用回归保持原要求。
- 插件 scheduler 改为登记的 Tokio 任务；关闭等待当前执行完成，下次启动可重新登记。Rhai 仍是同步 ABI。
- OAuth claim 取消清理也登记并排空，避免停服返回时尚有待落库的登录状态。Token refresh 的已接纳轮换凭据落库、reset credit 的不确定结果和网关 usage/log/ledger 排空沿用既有逻辑。

## 有界同步边界

- `runtime/blocking.rs` 用 8 个固定 worker 和最多 40 个已接纳任务运行普通同步 RPC、Rhai 与 scheduler。该隔离避免同步 ABI 等待异步操作时占满 Tokio blocking pool，而其内部文件/SQLite/DNS 工作又等待同一池的死锁。
- 同步调用者使用 `run_sync`，最多 32 个并发桥接。原生网络 handler 使用 async/await。current-thread 的遗留测试调用通过受限 scoped worker 等待；不新建 Tokio runtime。多线程兼容调用使用 `block_in_place`。这仍是有明确用途的同步 ABI，不代表同步代码清零。
- 2026-09-20 修复 auth、usage HTTP、aggregate 及 usage refresh 的兼容包装：将容量不足转换为调用者的 `Result` 错误，保持 reset-credit 错误的 `status=None`，不再 `expect` panic。新回归占满 32 个许可，验证认证 future 未被轮询、usage/reset/aggregate 都返回容量错误，释放后恢复调用。仅返回 bool 的旧 Token refresh 测试适配器保留明确的测试失败断言。
- 文件、ZIP、JSON/zstd、CPU、SQLite 等短同步阶段保持各自 semaphore 和 `spawn_blocking`。已有事务未因取消被重复执行。同步任务的队列等待、执行时长和失败标记只记录任务类别，不记录参数、URL、请求体或密钥。
- HTTP receiver 断开后，已接纳的同步状态修改继续完成；停服等待其结束。这里不能使用直接 abort 阻塞闭包的方式伪造取消完成。

## 删除与保留

- 删除网关中无生产调用方的 blocking client 缓存、builder、lazy slot，以及 agent identity 的废弃 blocking client 参数/无调用方同步包装。
- 代理顺序、缓存、profile 失效和 fail-closed 测试转到实际异步 client，保留通过真实本地代理请求验证路由选择的断言。
- Service 的 `reqwest/blocking` 移到 dev-dependencies；tiny_http/旧 HTTP crossbeam 适配器仅服务历史测试。跨线程兼容任务、桌面事件订阅仍有实际用途，不能为消除关键词删除。
- 桌面默认 SQLite、历史迁移、公开 RPC/API、权限与计费语义不变。
- `crates/rusqlite` 的 SQLx 兼容层仍持有独立的进程级 runtime：该底层 crate 不依赖 Service，Core、迁移工具和桌面同步存储仍需要它。删除它或反向依赖 Service 会破坏层次及连接 driver 生命周期；下一步若继续收敛应先设计向底层注入执行器的契约，而非仅替换 builder。

## 验证记录

2026-09-20（最后的关闭控制入口修复前）：`cargo test -p codexmanager-service --all-features --offline -- --test-threads=1` 最终 **1797 passed / 0 failed / 10 ignored，exit 0**。含 lib 1652、app_settings 34、default_addr 10、e2e 1、gateway_logs 50、RPC 49、shutdown 1；覆盖本次容量错误回归、OAuth/device 跨 listener、取消和停服测试。日志 `%TEMP%/codexmanager-service-final-20260920.log`。10 个 ignored 含远端 Service listener/domain 和外网仓库探测，不能计为通过；后续关闭入口修复的针对性复验见迁移交接文档最新表。

新增容量测试首次编译因 `unwrap_err` 要求 Token 响应实现 Debug 而失败（exit 101、无测试执行）；改为提取错误再断言，没有给含 Token 的响应新增 Debug。后续上述完整命令 exit 0。当前没有本轮全 workspace test、生产长时负载、真实 OAuth/provider 或 Tauri GUI 验收结果。

本轮过程中的编译失败必须保留原因及后续结果；前轮 2333 项通过数不是本轮通过数。
