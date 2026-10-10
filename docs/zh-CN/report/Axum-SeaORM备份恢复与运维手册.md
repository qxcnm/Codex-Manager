# Axum/SeaORM 备份、恢复与运维手册

更新时间：2026-09-19。本手册对应迁移执行设计的“工作包 5”，覆盖桌面 SQLite、Service SQLite/MySQL/PostgreSQL 以及离线导入。命令按当前仓库的 storage-transfer、codexmanager-service、codexmanager-web 和 codexmanager-start 实现编写。

本手册不把本地 fixture 当作生产验收。没有目标库、真实 provider、认证配置或完整端口清单时，生产项目保留 HANDOFF。归档文件和数据库快照包含账号 Token、API Key、聚合 provider secret 等敏感数据，按密钥材料管理。

## 1. 安全前提

1. 记录二进制版本、Git revision、后端、配置文件路径和监听地址。完整 URL、密码和 Token 不写入终端记录、Issue 或报告；报告只保留后端名、文件名、行数、哈希和脱敏错误码。
2. 备份和切换前停止 Service/Web/桌面端写入，等待进程退出和后台任务排空。不要在服务运行时复制 SQLite 文件。
3. SQLite 源库若有 .db-wal 或 .db-shm，先停服并完成 checkpoint；不能只复制主文件，也不能删除 WAL 文件“修复”备份。
4. 目标导入必须使用独立、确认为空的库。导入工具不会清理目标，也不会在线修改运行中的 Service 配置。
5. 保存每一步退出码和 JSON 报告。报告没有凭据不等于快照不含凭据。

## 2. SQLite 物理备份与恢复

### 2.1 备份

~~~powershell
$ErrorActionPreference = 'Stop'
$run = Join-Path $env:TEMP ('codexmanager-cutover-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $run | Out-Null
$source = 'C:\data\codexmanager.db'
$backup = Join-Path $run 'codexmanager.db.pre-cutover.bak'
if (Test-Path $backup) { throw "backup already exists: $backup" }
if ((Test-Path "$source-wal") -or (Test-Path "$source-shm")) {
  throw 'SQLite WAL/SHM sidecar exists; stop service and checkpoint before copying'
}
Copy-Item -LiteralPath $source -Destination $backup
$sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
$backupHash = (Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash
if ($sourceHash -ne $backupHash) { throw 'backup hash mismatch' }
[pscustomobject]@{
  operation = 'sqlite-physical-backup'
  source_file = [IO.Path]::GetFileName($source)
  backup_file = [IO.Path]::GetFileName($backup)
  sha256 = $backupHash
  source_was_stopped = $true
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'physical-backup.json')
~~~

把备份和报告放在受控目录；跨主机传输时使用部署方的磁盘/对象存储加密和访问控制。

### 2.2 恢复

恢复前停服。活动路径已有文件时先改名保留，避免覆盖唯一副本：

~~~powershell
$failed = "$source.failed-$(Get-Date -Format yyyyMMdd-HHmmss)"
if (Test-Path $source) { Move-Item -LiteralPath $source -Destination $failed }
Copy-Item -LiteralPath $backup -Destination $source
$restoredHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
if ($restoredHash -ne $backupHash) { throw 'restored hash mismatch' }
~~~

使用原 CODEXMANAGER_DB_PATH（或桌面默认数据目录）启动，先做健康检查，再只读核对账号、API Key、最近日志和设置。不要在核对前删除 failed 文件。生产恢复还需目标环境业务回读；本地拷贝成功不代表生产恢复通过。

## 3. SQLite 快照、目标预检和导入

storage-transfer 的 export 只读源库，inspect 只检查快照，dry-run 连接目标读取 schema/列类型/唯一键/目标行数但不插入业务行，import 要求目标表为空并在事务中导入。prepare-target 可能执行 MySQL DDL，不能视为无副作用 dry-run。

### 3.1 导出和检查

~~~powershell
$snapshot = Join-Path $run 'codexmanager.snapshot.jsonl'
cargo run -p codexmanager-storage-seaorm --offline --bin storage-transfer -- export $source $snapshot
if ($LASTEXITCODE -ne 0) { throw 'storage-transfer export failed' }
cargo run -p codexmanager-storage-seaorm --offline --bin storage-transfer -- inspect $snapshot
if ($LASTEXITCODE -ne 0) { throw 'snapshot inspect failed' }
Get-FileHash -LiteralPath $snapshot -Algorithm SHA256 |
  ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'snapshot-file-hash.json')
~~~

默认 feature 带 SQLite，需要时可显式加 --features sqlite。快照原文不要放入公共日志、聊天或终端录屏。

### 3.2 准备目标 schema

目标 URL 放进受控 env 文件或进程环境，不放命令行历史；以下不会打印 URL：

~~~powershell
$env:CODEXMANAGER_STORAGE_BACKEND = 'mysql' # 或 postgres、sqlite
$env:CODEXMANAGER_DATABASE_URL = $env:CM_TARGET_DATABASE_URL
$env:CODEXMANAGER_DB_MAX_CONNECTIONS = '10'
$env:CODEXMANAGER_DB_ACQUIRE_TIMEOUT_MS = '5000'
cargo run -p codexmanager-storage-seaorm --offline --features mysql --bin storage-transfer -- prepare-target
if ($LASTEXITCODE -ne 0) { throw 'target schema preparation failed' }
~~~

PostgreSQL 把 feature 改为 postgres；显式 SQLite URL 使用 sqlite。目标 URL 为空或后端不匹配必须失败，不能静默回退到本地 SQLite。schema DDL 失败后由 DBA 检查和清理，不能依赖事务回滚覆盖 MySQL DDL 副作用。

### 3.3 dry-run、导入和回读

~~~powershell
cargo run -p codexmanager-storage-seaorm --offline --features mysql --bin storage-transfer -- dry-run $snapshot |
  Tee-Object -FilePath (Join-Path $run 'dry-run.json')
if ($LASTEXITCODE -ne 0) { throw 'target dry-run failed' }
# 只有 success=true、failures=[] 且目标为空时继续：
cargo run -p codexmanager-storage-seaorm --offline --features mysql --bin storage-transfer -- import $snapshot |
  Tee-Object -FilePath (Join-Path $run 'import.json')
if ($LASTEXITCODE -ne 0) { throw 'target import failed' }
~~~

把 mysql 改为目标后端 feature。成功 import 的 target_modified=true 是预期结果；失败报告应指出 snapshot、schema、目标非空、冲突、权限或连接问题，且不写原始行值。导入后必须用目标配置启动 Service，执行健康、登录/授权、API Key、读写和网关最小回读。

保护条件包括：校验头/表顺序/行宽/SHA-256 失败不写目标；缺表、列类型或长度不匹配、必填列无映射、唯一键冲突、目标已有行时拒绝；有数据的未知表阻止导入；源 SQLite 不被 import 改写；事务失败报告为 import_failed_transaction_rolled_back 时仍要检查目标行数。

## 4. SQLite → MySQL/PostgreSQL 切换

1. 维护窗口停写，记录版本、源文件哈希和监听地址。
2. 做物理备份，保存 SHA-256，确认没有未处理 WAL/SHM。
3. export、inspect，保存逐表行数和快照哈希。
4. 独立空目标运行 prepare-target。
5. 同一目标运行 dry-run 两次；均无 failures 且 target_modified=false 才能继续。
6. import 并保存脱敏报告；重复 import 应因目标非空拒绝。
7. 保留源 SQLite，启动编译相同 storage feature 的 Service/Web/Start。URL/后端变更必须完整重启，不支持热切换。
8. 回读健康、用户/会话、Token、API Key/quota、模型、日志/用量、wallet/ledger 和受控网关请求。生产 provider、OAuth、Web cookie/session、WS 和反向代理另行验收。
9. 观察维护窗口错误率、连接数和后台任务终态后才归档旧源库。

证据目录建议包含 physical-backup.json、snapshot-file-hash.json、dry-run.json、import.json、service-startup.log、health-and-readback.txt 和脱敏配置记录。

## 5. 失败回退

| 失败点 | 立即动作 | 回退依据 |
| --- | --- | --- |
| 停止/备份前失败 | 不改配置、不删源库 | 原进程和源文件仍是权威 |
| export/inspect 失败 | 保留源库，修复权限/磁盘后重导出 | 源文件 SHA 和备份报告 |
| prepare-target 失败 | 不改 Service URL，由 DBA 检查 DDL | 源库未导入；目标不能直接当空库 |
| dry-run 失败 | 不执行 import，修复 schema/权限或重建隔离目标 | dry-run JSON 失败码和逐表报告 |
| import 失败 | 停止切换，保留目标和报告，确认事务回滚/目标行数 | 物理备份和快照 |
| 新后端启动/回读失败 | 停止新 Service，恢复原后端、URL、DB_PATH，重启原版本 | 原配置、源 hash、旧端口健康 |
| 旧版本也失败 | 不覆盖 failed-时间戳文件，按迁移规则使用更早备份 | 每次备份 SHA、版本和迁移日志 |

回退是“停服、恢复原配置、重启”的离线动作。不要在同一进程双写，不要删除迁移记录、手改 schema 或清空目标来制造成功。

## 6. 构建、变量和密钥

Service 模式三个包必须编译相同驱动：

~~~powershell
cargo build -p codexmanager-service -p codexmanager-web -p codexmanager-start --release --no-default-features --features storage-mysql --offline
# PostgreSQL 将 storage-mysql 改为 storage-postgres
~~~

启动前准备同目录 codexmanager.env 或系统环境变量，至少包括 STORAGE_BACKEND、DATABASE_URL、DB_MAX_CONNECTIONS、DB_ACQUIRE_TIMEOUT_MS、SERVICE_ADDR、WEB_ADDR、RPC_TOKEN 或 RPC_TOKEN_FILE、可选 WEB_ROOT、UPSTREAM_BASE_URL 和代理/超时设置。只给 Service 编译远端驱动不会给 Web/Start 增加驱动。

优先使用 CODEXMANAGER_RPC_TOKEN_FILE，文件只给运行账户读取；不要把 Token 放启动参数或日志。Web 密码由设置页写入 app_settings 的 web.auth.password_hash。

当前存储/导入会原样迁移账号 Token、API Key secret、provider secret、代理认证字段和相关设置。仓库没有把 URL、JSONL 或业务 secret 自动转换成由独立 master key 解密的备份格式；快照诊断脱敏不等于快照加密。用 OS ACL、磁盘/卷加密、受控对象存储和短期凭据保护 DB、.bak、JSONL、env 和 token 文件。生产 KMS 接入、备份销毁证明、保留周期和密钥轮换未在仓库自动化，属于部署方 HANDOFF。

## 7. 升级和回退

升级前保存版本、Git SHA、配置路径、后端和源库/备份 SHA。在隔离副本先跑 inspect、dry-run、最小启动和回读；生产升级采用停服，保留旧二进制和源库。新版本迁移或回读失败时停新版本，恢复旧二进制和原配置；SQLite 用迁移前 .bak，SeaORM 目标按 DBA 备份恢复。不要让旧版本连接不兼容的 schema，除非该版本明确支持回退；保留失败日志和目标状态。

## 8. 快速排障

- 后端/URL 不匹配：检查 env 文件位置、系统 env 优先级和三包 feature；显式 URL 不会静默回退 SQLite。
- target_configuration_invalid：检查 backend 拼写、URL 非空和 scheme/feature 一致性，不粘贴完整 URL。
- target_connection_or_authentication_failed：检查主机、端口、TLS、权限、防火墙；先用独立客户端做无写入连接检查。
- target_schema_preparation_failed：保留目标和报告，由 DBA 检查 DDL/权限；MySQL DDL 可能已提交。
- snapshot_checksum_mismatch：删除损坏副本，从源库重新导出，不手改 JSONL。
- target_not_empty/唯一键冲突：保留目标证据，使用新空目标重试，不清库掩盖重复导入。
- 启动读到旧数据：核对三包 feature、可执行文件同目录配置和完整重启。
- RPC 401/403：核对 token 文件权限和地址，不把 token 放 URL/日志。
- 健康成功但网关失败：继续检查 upstream、代理、账号状态、请求日志、错误码和 provider。
- 恢复出现迁移错误：保留失败文件和旧备份，不删除迁移表或手改版本号。
- SQLite 文件锁：确认没有第二进程打开 DB，检查连接和 blocking 上限，停服后再备份。

## 9. 本轮证据和未执行项

本表保留前次会话的实际命令；2026-09-20 的新增演练见第 10 节，文档存在不替代演练：

| 检查 | 命令/范围 | 结果 |
| --- | --- | --- |
| SQLite 快照导入/校验回归 | cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --test-threads=1 | 已执行：9 passed、0 failed、4 ignored，exit 0；以本轮完整 transfer/preflight 过滤结果为准 |
| dry-run schema 失败保护 | cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer::preflight_tests::dry_run_missing_and_malformed_schema_remains_unmodified -- --exact --test-threads=1 | 已执行：1 passed、0 failed，exit 0 |
| CLI JSON 报告入口 | cargo run -p codexmanager-storage-seaorm --offline --bin storage-transfer -- inspect <snapshot> | 已执行：空快照返回 success=true、tables=[]、target_modified=false，exit 0；测试文件为临时文件，未写入仓库 |
| 完整 transfer/preflight 过滤测试 | cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --test-threads=1 | 已执行：9 passed、0 failed、4 ignored，exit 0；包含未知目标表 dry-run 拒绝和 WAL 字节稳定性回归 |
| 隔离 SQLite 物理备份/恢复与回读 | 临时副本复制、SHA-256、`storage-transfer export` | 已执行：备份与恢复文件哈希均为 `2A2ED2C00562EE370FCA2AD2121278DC1968EEFE001F94353BA33A7DAF5F0DEB`；源副本和恢复副本 export 均 exit 0，JSONL 哈希均为 `B7B796A3BBFD710660F150692D966FC9CE26E62CB763770A02B496EFAC69E54D`；服务 HTTP health 未执行，进程启动被当前执行环境策略拦截 |
| MySQL/PostgreSQL 切换 | 独立空库 + prepare-target/dry-run/import + Service 回读 | 2026-09-20 已完成隔离库 dry-run/import/仓储回读，见下节；实际 Service 切换及失败回退仍 HANDOFF |
| 生产备份恢复/密钥管理 | 生产配置、KMS、完整端口和 provider | HANDOFF：本地 fixture 不可替代 |
| Tauri/Web 完整恢复验收 | 实际桌面与浏览器链路 | HANDOFF：Rust 单测不可替代 |

相关入口：[环境变量与运行配置说明](环境变量与运行配置说明.md)、[运行与部署指南](运行与部署指南.md)、[最小排障手册](最小排障手册.md)、[迁移执行设计](Axum-Tokio-Tower-SeaORM多数据库架构迁移执行设计.md)。当前实现边界见 crates/storage-seaorm/src/transfer.rs 和 src/transfer/preflight.rs。

## 10. 2026-09-20 隔离数据库导入和原生备份恢复

本轮找到了项目专属的私密测试配置，启动原先已停止的两个 CodexManager 测试容器；仅在新建数据库操作，未使用其他项目容器或生产配置。测试后两个容器恢复停止状态，源库、失败尝试、成功目标及备份均保留。

- 实际导入命令：安全加载新建目标 URL 后运行 `cargo test -p codexmanager-storage-seaorm --all-features --offline --lib transfer -- --ignored --test-threads=1`，**4 passed / 0 failed / 0 ignored，exit 0**。MySQL/Pg 各验证未迁移目标拒绝、迁移后两次只读预演、目标业务行仍为空、正式导入、非空目标拒绝和源/目标设置回读；另有各一个完整快照导入回读用例。
- 初次运行 **2 passed / 2 failed，exit 101**：MySQL 元数据列标签需显式别名；PostgreSQL 用例在已回读后删除源文件时遇到 Windows 锁。修复未降低数据断言，源回读连接现显式等待关闭。日志 `%TEMP%/codexmanager-transfer-real-initial-20260920.log` 与 `codexmanager-transfer-real-20260920.log`。
- 原生恢复：`pwsh -NoProfile -File "$env:TEMP/codexmanager-backup-restore-20260920.ps1"`，**2 后端 PASS / 0 failed，exit 0**。MySQL 使用 `mysqldump --single-transaction --hex-blob --order-by-primary` 导出，再导入另一新空库；PostgreSQL 使用 custom-format `pg_dump` 和 `pg_restore --exit-on-error --no-owner`。两者都从恢复库实际回读业务设置。
- MySQL 原库/恢复库的完整结构及有序数据 dump 哈希均为 `1D0945FA5BE57D5468645CCF7ABACD3E9E77D2E041837BEC5132E3EBB9719F93`。PostgreSQL 全部 INSERT 行及 sequence setval 值按行排序后的哈希均为 `A9846255CF19628124884BD7456E59D8E2735DCDD8BB2510624E02F2320F80D6`；忽略 pg_dump 每次随机生成的 restrict 标记，未忽略业务值。此项不等于 PostgreSQL 二进制备份字节相同或完整权限元数据相同。
- 原生备份 SHA-256、源/恢复库名及范围见 `%TEMP%/codexmanager-native-restore-f696249af1/report.json`，执行日志 `codexmanager-native-restore-20260920.log`。初次辅助脚本在 MySQL 回读 SQL 的 PowerShell 转义上 exit 1，修正列名引用后使用另一新库完成；失败库保留。

仍未执行：生产/跨主机恢复、Service 切换及失败回退、崩溃中 WAL 恢复、真实 KMS 加解密、专门 ACL 收紧及访问拒绝验证、备份保留/销毁证明、恢复后的真实 provider 和 OAuth。备份含隔离 fixture secret，放在本机临时目录并保留不等于生产保密控制已通过。
