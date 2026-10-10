# 受控生产配置

`codexmanager.env.example` 是可审计的配置模板，不包含数据库密码、RPC token 或 provider 凭据。部署时复制为同目录外的 `codexmanager.env`（或由 secret manager 注入），替换所有 `REPLACE_ME_*` 占位符，并把 `CODEXMANAGER_RPC_TOKEN_FILE` 指向只允许服务账户读取的 token 文件。

此模板面向独立的 `codexmanager-service` + `codexmanager-web` + `codexmanager-start` 发布包。它把 service、Web 和 OAuth callback 绑定到回环地址，由反向代理负责唯一外部入口；如果部署拓扑需要直接对外监听，必须在验收记录中明确端口、TLS、访问控制和 `CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR` 的理由。

先用脱敏校验脚本检查变量名、占位符、MySQL 配置、地址格式和可选监听端口：

```powershell
pwsh -NoProfile -File scripts/production-acceptance.ps1 `
  -ConfigPath .\config\production\codexmanager.env `
  -ServiceAddress 127.0.0.1:48760 `
  -WebAddress 127.0.0.1:48761 `
  -ProbeListeners `
  -RequireTokenFile
```

脚本只输出状态、缺失变量、脱敏语义错误和监听结果，不打印任何配置值或 token 文件路径。启用 `-RequireTokenFile` 时 token 文件必须存在且非空；启用 `-ProbeListeners` 时所有指定监听器必须可达。`READY_FOR_AUTHENTICATED_ACCEPTANCE` 只表示配置已经通过静态和监听检查；真实数据库读写、provider Responses/Chat、OAuth、Web session、反向代理和停服验收仍必须在目标环境完成并单独记录。

MySQL 目标构建必须给 Service、Web、Start 同时启用 `storage-mysql`：

```powershell
cargo build -p codexmanager-service -p codexmanager-web -p codexmanager-start `
  --release --no-default-features --features storage-mysql --locked
```

启动前确认：目标库为空或已完成批准的切换，RPC token 文件权限正确，反向代理只暴露 Web 端口，并且没有把此文件或数据库 URL 写入日志、工单或验收报告。
