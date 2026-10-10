# Axum/SeaORM 本地客户端与性能复验（2026-09-20）

这是同机、同探针的本地探索测量，不是生产容量或无回归验收。迁移前源码来自已有独立 worktree；当前二进制包含未提交修改。

## 版本、环境与复现

- 基线源码：`a4805dd4b5b6a64312c305c9a35554b7f334102c`；`%TEMP%/codexmanager-premigration-baseline-a4805dd4` 的 tracked 源码无修改。没有对当前工作区执行 checkout/reset/stash/clean。
- 基线首次重用旧 target 时缺少 libsqlite3-sys 生成的 bindgen.rs，构建 exit 101；改用新 target 目录并保留旧文件：在基线目录执行 `cargo build -p codexmanager-service --locked --offline --target-dir target-verify-20260920`，exit 0。
- 当前源码：同一 HEAD 加工作区未提交修改；`cargo build -p codexmanager-service -p codexmanager-web --offline`，exit 0。源文件清单 `%TEMP%/codexmanager-current-source-fingerprint-20260920.json` 的 SHA-256 为 `5C80249FEF34097F8969E49D801D73899C8625BE99A878318E977871AE45D14E`。
- 两边均为 dev profile、`debug=line-tables-only`、`incremental=false`、默认 features 和默认 SQLite 模式；编译器 `rustc 1.97.1 (8bab26f4f 2026-07-14)`，Cargo 1.97.1。不是 release 性能。
- Windows 11 build 26200，Intel64 Family 6 Model 186 Stepping 2，14 物理核/20 逻辑 CPU，系统内存 34,070,192,128 bytes。
- 探针 `scripts/migration-client-performance.py`：每轮新临时 DB、独立 fake account/key、隔离 Codex profile 设置、loopback provider；不使用生产 provider/认证。短请求并发 4；混合时再加 4 个长流 worker，provider 每条长流 1.5 秒。独立 Key 场景给每个 worker 不同 Key；产品串行锁语义未修改。
- 每场景发起请求窗口 10 秒，全部在途请求完成后计时结束，所以 RPS 分母可能大于 10 秒。三场景每轮各执行一次，共按旧→新→新→旧顺序执行 4 轮。health 每 50ms、进程资源约每 200ms 采样。性能阶段无本次 Cargo/前端构建并行。
- 精确批次入口：`pwsh -NoProfile -File "$env:TEMP/codexmanager-benchmark-20260920.ps1"`。各轮实际调用 `--duration 10 --concurrency 4 --independent-keys --build-profile dev-line-tables-only-default-features`；基线使用新 target 内二进制，当前使用工作区 target/debug 二进制。

| 文件 | SHA-256 |
| --- | --- |
| 基线 Service | `51519c0ad14f0809ec5378e1b503fe4e230623b34421014bb9ce76441978a98e` |
| 当前 Service | `5c4aa140fe9c7d1e6cd06192a6b8b855eefeceeab16dfcd666cb49ebdd0c3150` |
| 当前 Web | `2f4c9f9ba2d65d88856af7db7f61ba3cdf0724e0e3134aedf4c56945375465e2` |
| 共同 Python 探针 | `2dfabc61fbc7ff1665b58b4620f40c8741c813ab2ff1fed491168a8b9406e785` |
| Chromium 探针 | `b1e63240319f31b5b38cb3f70ea032f60beba5c502256892fb2d0220f459aab7` |

## 逐轮退出状态

| 轮次 | 总结果 / 退出码 | WS close | 强制结束 Service | SQLite integrity |
| --- | --- | --- | --- | --- |
| baseline-r1 | FAIL / 1 | 1006 | True | ok |
| current-r1 | PASS / 0 | 1000 | False | ok |
| current-r2 | PASS / 0 | 1000 | False | ok |
| baseline-r2 | FAIL / 1 | 1006 | True | ok |

基线两轮的协议正常关闭断言失败（1006），并且关闭请求后仍需强制终止；批次汇总 exit 1。负载数据在这些断言之前已收集，全部负载请求失败数为 0，但不能把基线整体验收写为 PASS。当前两轮正常关闭为 1000，初始化与正式 Service 都通过控制入口退出，未强制结束、退出码 0。

## 短请求实测值

| 场景 | 轮次 | 短/长请求数 | 实际秒数 | 短请求 RPS | P50 ms | P95 ms | P99 ms | 短/长/health 失败数 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 短请求，共享 Key | baseline-r1 | 747/0 | 10.199 | 73.239 | 48.648 | 66.77 | 77.998 | 0/0/0 |
| 短请求，共享 Key | current-r1 | 801/0 | 10.08 | 79.464 | 46.546 | 66.06 | 79.627 | 0/0/0 |
| 短请求，共享 Key | current-r2 | 696/0 | 10.199 | 68.243 | 52.756 | 88.276 | 96.255 | 0/0/0 |
| 短请求，共享 Key | baseline-r2 | 707/0 | 10.1 | 69.998 | 50.068 | 79.224 | 93.329 | 0/0/0 |
| 短/长混合，共享 Key | baseline-r1 | 13/9 | 14.968 | 0.869 | 6609.514 | 6652.382 | 6652.382 | 0/0/0 |
| 短/长混合，共享 Key | current-r1 | 12/9 | 15.642 | 0.767 | 6678.458 | 7231.009 | 7231.009 | 0/0/0 |
| 短/长混合，共享 Key | current-r2 | 12/9 | 15.912 | 0.754 | 6699.312 | 7488.589 | 7488.589 | 0/0/0 |
| 短/长混合，共享 Key | baseline-r2 | 12/9 | 15.891 | 0.755 | 5202.458 | 7303.32 | 7303.32 | 0/0/0 |
| 短/长混合，独立 Key | baseline-r1 | 867/27 | 11.556 | 75.023 | 17.648 | 148.992 | 562.86 | 0/0/0 |
| 短/长混合，独立 Key | current-r1 | 846/24 | 11.402 | 74.198 | 25.548 | 130.355 | 347.421 | 0/0/0 |
| 短/长混合，独立 Key | current-r2 | 824/24 | 10.777 | 76.463 | 27.024 | 146.481 | 402.996 | 0/0/0 |
| 短/长混合，独立 Key | baseline-r2 | 905/24 | 10.638 | 85.071 | 18.307 | 115.948 | 330.517 | 0/0/0 |

## 两轮中位数与资源采样

下表的分位数是两轮实测分位数的中位数，不是合并样本后重新计算的分位数；RSS/TCP 是两轮中较高的采样峰值，仅统计 Service 进程。

| 场景 | 版本 | 短请求 RPS | 短 P95/P99 ms | 长 P95/P99 ms | health P95/P99 ms | RSS 峰值 MiB | 线程峰值 | TCP 峰值 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 短请求，共享 Key | baseline | 71.619 | 72.997/85.663 | - | 1.584/5.962 | 49.85 | 81 | 20 |
| 短请求，共享 Key | current | 73.853 | 77.168/87.941 | - | 1.202/6.105 | 53.50 | 48 | 9 |
| 短/长混合，共享 Key | baseline | 0.812 | 6977.851/6977.851 | 6976.561/6976.561 | 19.093/20.947 | 53.53 | 91 | 32 |
| 短/长混合，共享 Key | current | 0.760 | 7359.799/7359.799 | 7347.373/7347.373 | 22.023/23.836 | 61.92 | 57 | 13 |
| 短/长混合，独立 Key | baseline | 80.047 | 132.470/446.688 | 1838.927/1929.354 | 1.741/7.085 | 63.36 | 109 | 39 |
| 短/长混合，独立 Key | current | 75.331 | 138.418/375.208 | 1845.758/1898.207 | 1.550/2.353 | 73.53 | 72 | 20 |

共享 Key 混合场景的短请求 P95：基线两轮中位数 6977.851 ms，当前 7359.799 ms，当前高 5.5%。这一场景每轮仅约十余个短请求，不能据此作稳定 P99 或生产退化幅度结论，也不能宣布“性能无退化”。
源码中 request gate 按 Key/path/model 串行化；两版共享 Key 场景都出现秒级排队，而独立 Key 场景回到毫秒级。这与串行锁影响一致，但不是唯一瓶颈的证明。本轮未为了降低数字删除锁或修改计费/路由语义。后续应使用 release、更长窗口/更多重复和 phase tracing，区分锁等待、SQLite/同步 facade、最终日志和计费耗时。

## 实际 Web 与 Chromium

- 新构建的 Service/Web 和静态导出 UI：`--web-bin target/debug/codexmanager-web.exe --web-root apps/out --browser --duration 0`，报告 `%TEMP%/codexmanager-web-browser-final-20260920.json`，PASS、exit 0。
- Chromium 145.0.7632.6（headless，真实浏览器）：7 项交互检查通过，0 未捕获异常；包含匿名重定向、首次引导关闭、表单登录与 React 页面、cookie/tab session、API Key 页面经真实 RPC 回读、点击退出后的 RPC 401。浏览器记录 13 次 RPC 200 和 1 次预期退出后 401。没有 mock HTTP transport，也不是 Next 静态测试服务器。
- HTTP Web 检查 12 项通过，包含错误密码、成员权限拒绝、成员自己 Key 的启用/禁用/删除及 SQLite 回读、他人 Key 拒绝、退出和禁用用户会话撤销。Service 和 Web 各验证 Responses/Chat Completions JSON/SSE 四路径及 Responses WebSocket，两侧 close code 均 1000。
- 初始化 Service、正式 Service 和 Web 均由控制请求退出：HTTP 200、exit 0、forced_termination=false，SQLite integrity=ok。它只证明本地 fixture 此次关闭，长时/生产任务 drain 另行验收。
- 浏览器截图：`C:\Users\qxnm\AppData\Local\Temp\codexmanager-client-acceptance-1_hfjjjj/browser-api-keys.png`；fixture 与报告保留。

最初浏览器探针在首次引导遮罩下等待退出按钮而超时；补上真实引导关闭，并使用源码中的 `/apikeys/` 导航。首轮还暴露 Service 缺失关闭控制路由、Web 匿名退出请求跟随重定向误读 200 的问题。已补 Service 的受保护关闭入口、跨进程通知 token，探针 Web 退出使用 fixture 管理员 session 并禁止跟随重定向。修复前报告保留为 FAIL，没有将强制终止改称正常退出。

## 范围和未完成项

- 这是 dev 构建和短窗口探索测量；每版本只有两轮，未控制机器上所有其他任务和版本元数据后台同步，也没有 release 长时压力、真实网络/TLS/provider、生产反向代理或跨平台样本。采样可能漏过瞬时峰值。
- 共享 Key 的混合流尾延迟仍高，独立 Key 场景也存在波动；工作包 4 保持部分完成。旧版本协议/关闭失败与其负载指标分开记录。
- Tauri 原生 GUI/IPC、真实 OAuth、真实 provider 的 tools/tool_calls、生产监听/认证/反向代理 WS、生产容量和 KMS/ACL 验收未执行，仍 HANDOFF。Rust 协议回归中的 tools/tool_calls 不代表真实 provider 验收。

原始四轮 JSON、退出码清单 `%TEMP%/codexmanager-benchmark-exits-20260920.json`、构建日志和 source fingerprint 为本轮证据。没有提交、推送或清理工作区。
