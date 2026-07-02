# OpenAI / chatgpt.com 上游 404 间歇性问题

> 状态:已修复(2026-07-01)。已在 `decide_upstream_outcome` 中为官方 upstream 404 增加 cooldown + failover 分支。
> 影响范围:所有经过 CodexManager service 转发的 `/v1/responses`、`/v1/responses/compact`、`/v1/chat/completions` 请求。
> 现象:请求上游 OpenAI / chatgpt.com 后返回 `HTTP 404 Not Found`,`cf_ray` 字段显示经过 Cloudflare(新加坡/高雄/法兰克福节点),`request_id` 是 OpenAI 给的真实 trace id。

---

## 1. 现象

从 CodexManager service 的客户端观察到的错误信息形如:

```
Error: unexpected status 404 Not Found: {"detail":"Not Found"},
       url: http://localhost:48760/v1/responses,
       cf-ray: a13bf3cdb8ac8a13-SIN,
       request id: a252f9c7-98da-41c7-80e3-6bcd64120e25
```

- `localhost:48760` 是 CodexManager service 监听的本机端口
- `cf-ray` / `request id` 由 OpenAI 加上后透传到客户端
- **这条错误信息不含 OpenAI 的真实上游 URL** —— 客户端只看得到 service 入站 URL,看不到真正的 `https://chatgpt.com/backend-api/codex/responses`
- 现象是**间歇性的**:有时连续 5 分钟内 47 次请求全部 404,过 5 分钟又降到 2 次;某些时段 1.5 小时几乎完全静默,之后又一波密集 404
- 同一个 account 在同一时段内多次失败;同一 service 池里不同 account 也同时失败

---

## 2. 根因(基于日志证据)

### 2.1 404 来自 OpenAI / chatgpt.com 上游,不是 CodexManager service 自身

证据:

| 字段 | 含义 |
|---|---|
| `cf-ray=...-SIN/KHH/FRA` | **Cloudflare 边缘节点的响应头**。只有请求真正穿过 Cloudflare 才会带回这个值。**只有 OpenAI(chatgpt.com 的背后是 Cloudflare)能产生这个 header** |
| `request id=...` | OpenAI 给的真实请求 id,OpenAI 客服可以根据这个反查 |
| 上游 URL | service 内部 trace 记的是 `https://chatgpt.com/backend-api/codex/responses` —— **就是 OpenAI 自己的后端** |
| `elapsed_ms` | 多次失败中,有 8-15 秒的,说明请求**完整到达 OpenAI 后端**才决定 404(不是秒回,不是网络问题) |

链路:CodexManager service 的 `proxy_handler`(`crates/service/src/http/proxy_runtime.rs:115`)做的是**裸透传**:把客户端请求原方法原 header 原 body 转给上游,再把上游响应原样回给客户端。OpenAI 返回的 404 + body + `cf-ray` / `request-id` header 全部透明穿透 service 到客户端。**service 在这个路径上没有任何 status code 改写、没有 body 重写、没有 header 添加**。

### 2.2 两个不同 path 都同时被 404,说明 404 不是"endpoint 缺失"

`trace log` 中失败的 path 分布:

- `/v1/responses/compact`(精确 subpath):多次 404
- `/v1/responses`(精确路径,无 subpath):**更多**次 404

如果只是 subpath 写错或者 endpoint 真的不存在,只会有 subpath 失败、`/v1/responses` 正常。**两个 path 都失败**说明这是上游侧的**集中拒绝**,不分 path。

### 2.3 时间分布特征 —— 像间歇性 ban,不像硬 ban

把今天 trace 数据按 5 分钟桶分桶观察失败密度,关键事实:

- 同一 5 分钟桶内失败数从 2 → 47 → 2 → 28 → 51 反复波动,跨度 25 倍
- 某些时段 1.5 小时**几乎完全静默**(失败几乎为 0);某些时段 5 分钟 47 次
- `cf_ray` 后缀 KHH → SIN → FRA 不固定某个节点,**不是 IP 段 hard ban**(hard ban 的话 cf_ray 会锁死某些节点)

这种"玻璃墙"模式跟 OpenAI 已知的"间歇性反爬"特征一致:**流量模型/速率被监控**,超阈值触发降级,过一会儿恢复,反复循环。

### 2.4 service 端目前没有针对"官方 upstream + 404"的处理分支

文件 `crates/service/src/gateway/upstream/support/outcome.rs` 的 `decide_upstream_outcome` 函数是当前决定"上游失败后 service 该怎么办"的地方。它当前**只在以下情况有专属处理**:

- 官方 upstream + 429 → cooldown 45s + refresh + failover
- 官方 upstream + 401 → 直接响应(不 failover)
- 官方 upstream + 5xx 在 compact 路径 + 配额低 → cooldown + refresh + failover
- 非官方 upstream + 各种状态 → 各自对应 cooldown + failover

**`is_official_target && status.as_u16() == 404` 这个分支根本不存在**。404 走的是最后那个 `should_failover_from_cached_snapshot_value` 兜底,只看账号**配额快照**不看 status code。这意味着 OpenAI 集中 404 时 service**当前不会主动给账号打 404 专属 cooldown**,让其他账号优先尝试。

但基础设施其实已经在了:

- `crates/service/src/gateway/routing/cooldown.rs:250` 的 `mark_account_cooldown_for_status(account_id, status)` 已经存在
- `cooldown_secs_for_status(404)` 映射到 `CooldownReason::Upstream4xx`,时长 `DEFAULT_ACCOUNT_COOLDOWN_SECS = 20s`(`cooldown.rs:6,10`)
- `mark_account_cooldown_for_status` 内部按 `offense_counts` 阶梯上升,持续 404 会被 escalate

**缺的只是 `decide_upstream_outcome` 里那个调用点**。

---

## 3. 怎么用日志复现 / 验证

CodexManager 在 macOS 上有两个相关日志位置:

| 路径 | 内容 | 特征 |
|---|---|---|
| `~/Library/Logs/com.codexmanager.desktop/CodexManager.log` | Tauri 应用层 `log::warn!` / `log::error!`,中文易读 | 按天 rotate(实际只保留约 13 行,rotate 激进)|
| `~/Library/Application Support/com.codexmanager.desktop/gateway-trace.log` | 结构化 `key=value` 格式,每请求多个事件(REQUEST_START / CANDIDATE_POOL / ATTEMPT_RESULT / BRIDGE_RESULT / FAILED_REQUEST)| ~13MB,持续 append,不主动 rotate |
| `~/Library/Application Support/com.codexmanager.desktop/codexmanager.db` | SQLite 业务数据 | 业务表 `accounts` / `usage_snapshots` / `request_logs` 等 |

### 3.1 在 trace 里找一次具体失败请求的关键字段

**输入**:客户端错误里的 `cf_ray` 或 `request id`。

```bash
TRACE_LOG="$HOME/Library/Application Support/com.codexmanager.desktop/gateway-trace.log"
TARGET="a252f9c7-98da-41c7-80e3-6bcd64120e25"   # OpenAI 的 request id

# FAILED_REQUEST 这行包含上游 URL + status + elapsed_ms + cf_ray + request_id
grep -E "request_id=$TARGET|cf_ray=$TARGET" "$TRACE_LOG" | grep -E 'FAILED_REQUEST' | head -3
```

输出字段示例(`upstream_url` 是 CodexManager 看不到的、用 trace 才能找到的关键信息):

```
ts=1782809730 event=FAILED_REQUEST trace_id=trc_1782809728635_1156 key_id=gk_ffdeab292d73
  account_id=auth0|Rt9HN6985eMWCYFEHt0fYKRs::cgpt=6be464bd-135a-4711-b9c7-c3899baf6a5e|ws=org-kQaSnXe1xsgrPxz6qHOGvosX|Huawei-dev
  method=POST request_path=/v1/responses original_path=/v1/responses adapted_path=/v1/responses
  request_type=http model=gpt-5.5 reasoning=high service_tier=-
  upstream_url=https://chatgpt.com/backend-api/codex/responses
  status=404 elapsed_ms=1517 code=unknown_error error=Not Found
  [request_id=a252f9c7-98da-41c7-80e3-6bcd64120e25, cf_ray=a13bf3cdb8ac8a13-SIN]
```

### 3.2 看 5 分钟粒度的失败密度

```bash
TRACE_LOG="$HOME/Library/Application Support/com.codexmanager.desktop/gateway-trace.log"

# 统计今天 12:00 之后,按 5 分钟桶的 FAILED_REQUEST 数(过滤出 404 类)
# 把 ts= 后面数字除以 300 向下取整,bucket 5 分钟
awk -v cut=1782694800 '
  /^ts=/ { ts=$1; gsub("ts=", "", ts); if (ts+0 < cut) next }
  /event=FAILED_REQUEST/ && /status=404/ {
    bucket = int((ts+0)/300)*300
    total[bucket]++
  }
  END {
    for (b in total) {
      cmd = "date -r "b" +%H:%M"; cmd | getline t; close(cmd)
      printf "%s %d\n", t, total[b]
    }
  }' "$TRACE_LOG" | sort
```

输出(示例)如果某天持续高失败率意味着 OpenAI 集中 block 该时段:

```
12:25 36
12:30 47
12:35 27
...
18:30 27
18:35 28
...
21:25 1
```

### 3.3 区分"账号维度 block"和"IP 维度 block"

`trace log` 里 `CANDIDATE_POOL` 事件列出每个请求的候选账号池,`ATTEMPT_RESULT` 记录每次实际尝试的结果:

```bash
TRACE_LOG="$HOME/Library/Application Support/com.codexmanager.desktop/gateway-trace.log"

# 看今天 trace 里每个 account 的 REQUEST_START 总数 vs 404 失败数
echo "===REQUEST_START counts==="
grep 'event=REQUEST_START' "$TRACE_LOG" | \
  grep -oE 'account_id=[^ ]+' | sort | uniq -c

echo ""
echo "===FAILED_REQUEST status=404 counts==="
grep 'event=FAILED_REQUEST' "$TRACE_LOG" | grep 'status=404' | \
  grep -oE 'account_id=[^ ]+' | sort | uniq -c
```

**两个 account 都大量 404** —— 倾向 IP/客户端指纹维度。
**只有一个 account 大量 404 另一个正常** —— 倾向账号维度。

---

## 4. 现状相关注意点

### 4.1 CodexManager 客户端看不到的信息

trace log 里有的、但客户端错误信息里**没有**的:

| 字段 | 客户端是否看到 | 备注 |
|---|---|---|
| 客户端请求 URL(`http://localhost:48760/v1/responses`) | ✅ 看到 | service 入口 |
| **上游 URL**(`https://chatgpt.com/backend-api/codex/responses`) | ❌ 看不到 | service 透传,没加到 response header / body |
| `cf-ray` | ✅ 看到 | 从 OpenAI 响应 header 透传到客户端 header |
| `request id` | ✅ 看到 | 同上 |
| `account_id` | ❌ 看不到 | CodexManager 内部状态,不上行 |
| `model` / `reasoning` | ❌ 看不到 | CodexManager 内部状态,不上行 |
| `elapsed_ms` | ❌ 看不到 | CodexManager 内部 metrics |
| `trace_id`(CodexManager 自己的)| ❌ 看不到 | 可以反向 grep log |

如果需要让客户端更快定位:**可以在 `proxy_handler` 失败分支加 response header**(例如 `X-CodexManager-Upstream-Url` / `X-CodexManager-Trace-Id`),具体行为取决于客户端(比如 Codex CLI)是否会解析这些 header。

### 4.2 不同客户端的覆盖

- **Codex CLI**:能看到客户端 URL + cf_ray + request id,看不到上游 URL / trace_id
- **Tauri 内调用**:同上(走同一个 `proxy_handler`)
- **浏览器(Web 模式)**:走 `codexmanager-web` 的 RPC 包装,客户端错误信息可能不同

### 4.3 已知未解决的相邻问题

服务侧 `cargo test --workspace` 有 4 个测试失败,**与本 issue 完全无关,不在本 issue 修复范围内**,只是顺手标记一下:

- `aggregate_api::tests::read_aggregate_api_secret_uses_auth_type_projection`:no such column `preferred`
- `gateway::upstream::attempt_flow::postprocess::tests::chatgpt_challenge_on_last_candidate_retries_without_same_account_failover`:reqwest DNS error
- `http::proxy_runtime::tests::hybrid_responses_websocket_returns_426`:no such column `workspace_name`
- `quota::read::tests::quota_capacity_updates_return_config_from_same_storage_handle`:no such column `preferred`

这 4 个失败的根因是测试基础设施假设的 schema 跟当前 migrations 不一致(常见的是测试初始化用了 fixture 但 fixture 没更新到最新 schema)。**它们在仓库长期存在,与 OpenAI 404 问题无关。**

---

## 5. 修复方向(待评估,未实施)

按"投入小 / 风险小 / 收益清晰"评估:

### 5.1 启用 404 cooldown(已实施)

文件:`crates/service/src/gateway/upstream/support/outcome.rs`

已在 `is_official_target && status.as_u16() == 429` 分支后新增官方 404 分支:

```rust
if is_official_target && status.as_u16() == 404 {
    super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
    log::warn!(...);
    log_gateway_result(
        Some(url),
        status.as_u16(),
        Some("upstream not-found (likely OpenAI block)"),
    );
    return from_follow_up_action(follow_up_action(true, has_more_candidates));
}
```

效果:OpenAI 集中 block 时该账号进入 20s cooldown,自动 failover 到其他账号;其他账号也 404 时逐个 cooldown,避免同一账号反复撞墙。最后一个候选无退路时仍把 404 回传客户端,保持原有语义。

**局限**:如果 404 是**纯 IP 维度**的硬拦截(同 service 出口 IP 下所有账号同时被 block),单纯 per-account cooldown 只能把“连续撞墙”变成“逐个撞墙”,并不能绕过 IP 封禁。此时需要配合代理池(`CODEXMANAGER_PROXY_LIST`)或全局 backoff。

对应单测:
- `official_status_404_with_more_candidates_triggers_failover`
- `official_status_404_on_last_candidate_keeps_upstream_response`

### 5.2 failure rate monitor(改动中)

加一条 CodexManager `log::warn!`,把 trace 里"上游 URL + status + elapsed_ms + cf_ray + account"全打出来,以后 grep `[UPSTREAM_FAIL]` 立刻看到上下文,不必去翻 13MB 的 gateway-trace.log。

### 5.3 proxy_handler 加 response header(改动小)

在 `crates/service/src/http/proxy_runtime.rs:115` 的失败分支加 `X-CodexManager-Upstream-Url` 等 header,便于客户端(以及将来调试)直接看到上游 URL。是否能被 Codex CLI 提取出来取决于客户端实现。

### 5.4 配置层面:`CODEXMANAGER_UPSTREAM_BASE_URL`

默认 `https://chatgpt.com/backend-api/codex`(写死在 `crates/service/src/gateway/upstream/config.rs:4`)。

可以通过环境变量或 app setting 改成 `https://api.openai.com/v1` 之类的别地址,但 `normalize_upstream_base_url`(`config.rs:11`)只白名单了 `chatgpt.com` / `chat.openai.com`,换了**不能保证 Codex 协议兼容**,需要小流量验证。

仓库里有一个 `UPSTREAM_FALLBACK_BASE_URL`(`config.rs:8`,默认 `None`),**主上游失败切到 fallback upstream** 的能力目前**没有看到主动用**。这个改造会比较显形的"降级",但 Codex 协议兼容性同样要确认。

### 5.5 全局 throttle(改动大)

观察连续 N 次失败的频率,自动降频 outgoing request,模仿 OpenAI 的"窗口式准入"。

风险:让所有用户都感受到限速(包括没受影响的请求)。

---

## 6. 文件 / 文件位置汇总

- CodexManager 前端代理:`crates/service/src/http/proxy_runtime.rs:115` (`proxy_handler`)
- CodexManager 上游结果决策:`crates/service/src/gateway/upstream/support/outcome.rs` (`decide_upstream_outcome`)
- CodexManager cooldown 体系:`crates/service/src/gateway/routing/cooldown.rs`
- CodexManager upstream 配置:`crates/service/src/gateway/upstream/config.rs`
- CodexManager trace 日志:`crates/service/src/gateway/observability/...`(具体写日志的位置,grep `event=FAILED_REQUEST` 可以反推到源码)
- macOS 客户端日志:`~/Library/Logs/com.codexmanager.desktop/CodexManager.log`
- macOS trace 日志:`~/Library/Application Support/com.codexmanager.desktop/gateway-trace.log`

---

## 7. 文档元信息

- 写入时间:2026-06-29
- 更新时间:2026-07-01
- 类型:已知 issue 的故障诊断与修复记录
- 与代码改动的提交:`crates/service/src/gateway/upstream/support/outcome.rs` 增加官方 upstream 404 cooldown/failover 分支
