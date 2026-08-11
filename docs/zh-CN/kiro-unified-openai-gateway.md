# Codex + Kiro 统一 OpenAI 网关

首版通过同一把平台 Key 和 OpenAI API 调用 Codex 与 Kiro。Kiro 核心仅合并自
MIT 许可的 [`hank9999/kiro.rs`](https://github.com/hank9999/kiro.rs)，固定上游
提交为 `b9e757e1f8c1d2f1bbfb1157e8f9002fd5c19c9c`。

## 接口

- `GET /v1/models`
- `POST /v1/responses`
- `POST /v1/chat/completions`

模型可使用精确名称 `kiro/claude-*`、`codex/gpt-*`，或统一别名 `smart`、
`coding`、`fast`、`cheap`。首版平台 Key 使用 OpenAI 格式；Claude 格式仅作为
后续协议扩展方向。

`POST /v1/responses` 省略 `stream` 时按 OpenAI 官方默认返回单个 JSON；显式
`stream: true` 时返回 SSE。网关内部仍可使用上游流并在非流式请求中完成聚合。
`smart` 的 Kiro 默认质量模型是广泛可用的 `kiro/claude-sonnet-4.5`，新模型仍可
通过精确名称调用。

## 平台 Key 策略

创建或编辑平台 Key 时可以独立配置：

- 模型白名单：每行或逗号分隔，空值表示允许全部模型；
- 平台范围：Codex + Kiro、仅 Codex 或仅 Kiro；
- Token 总额度；
- 到期时间；
- 最大并发请求数。

模型白名单和平台范围同时作用于 `/v1/models` 与实际推理请求。智能别名只在 Key
允许的平台中评分和选路；过期 Key 返回 403，并发达到上限返回 429。流式连接结束、
请求取消或传输失败后并发占位会通过请求生命周期守卫自动释放。

## 导入 Kiro 凭据

打开“管理 > Kiro 凭据”，可以：

1. 粘贴单对象或数组 JSON；
2. 同时选择多个 `.json` 文件；
3. 选择目录并递归读取其中的 JSON；
4. 先检查认证方式、字段映射、置信度、重复提示和坏记录；
5. 确认后加密导入。

内置别名无法识别的 JSON，可以打开“手动字段映射”，为 `refreshToken`、
`clientId`、`email`、区域、额度等规范字段填写源字段路径。路径支持点号嵌套写法，
例如 `tokens.refresh` 或 `profile.email`。映射可以命名保存为本机模板，并对当前多文件
导入批次统一预览后再提交。`refreshToken` 路径没有匹配到任何记录时不会写库。
预览会使用加密身份索引标记“新增”或“更新已有”；重复导入同一 Social/IdC 身份时
更新原记录的 Token，不创建重复凭据，也不会为了查重向前端返回身份哈希或密钥。

支持 Social `refreshToken`，以及 IdC `refreshToken/clientId/clientSecret`。
也支持 `region`、`authRegion`、`apiRegion`、订阅、额度和凭据级代理字段。
`authRegion` 与 `apiRegion` 会独立保存；`machineId` 会随凭据元数据恢复到 Kiro
运行时。OpenAI `web_search` 工具会转换为 Kiro MCP WebSearch 调用。
代理 URL 中的用户名会被拆出，密码与 Token、client secret 一起进入加密凭据库，
不会以明文保存到 `proxy_url`。

## 运行与故障切换

- 401/403：同一凭据强制刷新一次；失败后隔离并切换。
- 429：记录 60 秒冷却并尝试其他凭据。
- 额度耗尽：标记 `quota_exhausted` 并切换。
- 网络错误、408、5xx：退避并在响应内容输出前重试其他候选。
- 成功响应已经开始输出后，不自动重放请求，避免重复回答。
- 客户端断开后，流转发通道关闭，Kiro 响应任务随之停止。

Kiro 页面会显示成功率、最近延迟、失败次数和冷却截止时间。智能路由同时参考
能力、健康状态、额度、成功率、延迟和用户权重，并将脱敏后的候选评分和最终选择
写入请求追踪。

## 安全边界

- Codex Token、Kiro Token、client secret、代理密码和平台 API Key 使用
  AES-256-GCM 加密。
- Windows 的随机数据密钥由当前用户 DPAPI 包装；Linux 使用部署 Secret 提供的
  32 字节密钥通过 AES-256-GCM 包装。
- SQLite 仅保存密文、nonce 与非敏感路由元数据。
- 导入预览、错误和请求追踪不返回凭据明文。

升级现有数据库时，原有 Codex Token 与平台 API Key 会由一次性数据库迁移加密，
读取接口保持兼容。

只有数据库文件而没有 Windows 用户上下文或 Linux 部署 Secret 时，无法单独解出
凭据。

## 独立安装与旧库迁移

首版桌面发行名为 **CodexManager Unified**，应用标识为
`com.codexmanager.unified`，默认服务端口为 `48764`。因此它与原版
CodexManager 的安装身份、AppData 目录、SQLite、RPC Token 和监听端口相互隔离，
可以并行安装，不会共用运行状态库。

Unified 首次启动且自己的数据库为空时，会通过 SQLite 在线备份读取原版
`com.codexmanager.desktop/codexmanager.db`，复制为 Unified 独立数据库，然后在
新库上执行迁移和凭据加密。原版数据库只作为快照源，不执行新版本 schema 初始化，
后续两个应用各自写入自己的数据库。

## Linux / 云端运行

云端不运行 Tauri，也不依赖 Windows 或 WSL。使用 Docker 中的
`codexmanager-start` 同时启动 Linux `service + web`；Provider Runtime、
Canonical 协议和 SQLite 数据结构与桌面版一致。

Linux 部署必须持久注入以下任一配置，重启、升级和横向扩容时保持同一个值：

```text
CODEXMANAGER_VAULT_MASTER_KEY=<32 字节 base64 或 64 位 hex>
CODEXMANAGER_VAULT_MASTER_KEY_FILE=/run/secrets/codexmanager_vault_key
```

可以用 `openssl rand -base64 32` 生成一次，然后放入 Docker Secret、Kubernetes
Secret 或云 KMS 注入层。不要把它写入镜像、Git 或普通日志。Windows DPAPI 数据库
不能直接复制到 Linux 解密；桌面到云端通过 JSON 导入进入新的独立数据库。

推荐首版使用**单机单副本**：Linux 云主机 + Docker Compose + 持久化数据卷。
SQLite、账号冷却状态和并发计数都由同一个服务实例维护，不需要服务器安装桌面环境，
也不需要运行 WSL。示例：

```bash
export CODEXMANAGER_VAULT_MASTER_KEY="$(openssl rand -base64 32)"
# 生成后应立即保存到云 Secret；以后重启必须继续使用同一个值。
docker compose -f docker/docker-compose.all-in-one.yml up -d --build
```

默认只在宿主机发布两个独立端口：

- OpenAI 网关：`48764`（容器内 `48760`）
- 管理 Web：`48765`（容器内 `48761`）

宿主机端口可通过 `CODEXMANAGER_GATEWAY_PORT` 和 `CODEXMANAGER_WEB_PORT`
修改，因此可以与原 CodexManager 的 `48760` 并行运行。生产环境在前面放 Nginx、
Caddy 或云负载均衡并启用 HTTPS，只将管理 Web 暴露给受信网络。

首版不要对同一个 SQLite 数据卷启动多个副本。需要横向扩容时，再把持久化层迁移到
PostgreSQL、把并发租约和冷却状态迁移到 Redis；Provider Runtime 和 Canonical
协议层无需因此重写。Linux x86_64/aarch64 构建及容器发布应交给 GitHub Actions，
Windows 开发机只运行编辑、单元测试和桌面调试，不承担常驻 Linux 构建环境。
