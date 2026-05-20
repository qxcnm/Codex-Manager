# Docs 目录说明
`docs/` 是 CodexManager 的正式治理文档目录，不再视为临时文件仓。

目标：
- 让结构治理、决策记录、发布说明、运行手册都进入仓库主线
- 让新协作者不依赖口头交接也能找到正确文档

## 文档职责边界
- 根目录 `README.md` 与多语言 `docs/*/README.md`：首页导航、项目概览、快速开始。
- 根目录 `CHANGELOG.md`：版本历史与未发版更新的唯一事实源。
- `report/*`：运行、排障、兼容性、FAQ 类手册。
- `release/*`：构建、发版、产物和发布流程说明。
- `docs/plan/*` / `docs/decision/*`：长期治理、实施计划与决策记录。

## 赞助商

感谢以下赞助商对 CodexManager 的支持。

<table>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://gzxsy.vip/register?aff=eapz">
        <img src="../../assets/images/sponsors/xingsiyan.jpg" alt="星思研中转站" width="120" />
      </a>
    </td>
    <td valign="top">
      <strong>星思研中转站</strong> 为 Claude Code、Codex、Gemini 等模型调用场景提供稳定中转与配套服务，适合需要高可用接口、便捷接入和持续交付支持的开发者与团队。可前往<a href="https://gzxsy.vip/register?aff=eapz">官网</a>了解最新方案。
    </td>
  </tr>
</table>

其他支持者：[Wonderdch](https://github.com/Wonderdch)、[suxinwl](https://github.com/suxinwl)、[Hermit](https://github.com/HermitChen)、[Suifeng023](https://github.com/Suifeng023)、[HK-hub](https://github.com/HK-hub)

## 文档首页
| 如果你现在要做什么 | 建议先看 |
| --- | --- |
| 跑起来项目、部署 service/web、处理 macOS 首启 | [运行与部署指南](report/运行与部署指南.md) |
| 配置 Codex CLI / ccswitch 的 `auth.json` 与 `config.toml` | [运行与部署指南](report/运行与部署指南.md#通过-ccswitch-接入) |
| 不登陆 Codex，使用 ChatGPT `/api/auth/session` 导入账号 | [不登陆 Codex 使用 ChatGPT 的 /api/auth/session 在软件中的使用](report/不登陆Codex使用ChatGPT-auth-session导入账号.md) |
| 配环境变量、数据库、端口、代理、监听地址 | [环境变量与运行配置说明](report/环境变量与运行配置说明.md) |
| 排查账号不命中、导入异常、挑战拦截 | [FAQ 与账号命中规则](report/FAQ与账号命中规则.md) |
| 排查后台任务账号跳过、禁用与停用原因 | [后台任务账号跳过说明](report/后台任务账号跳过说明.md) |
| 插件中心最小接入、快速对接 | [插件中心最小接入说明](report/插件中心最小接入说明.md) |
| 系统内部接口总表、全部可对接入口 | [系统内部接口总表](report/系统内部接口总表.md) |
| 本地打包、工作流发版、下载产物 | [构建发布与脚本说明](release/构建发布与脚本说明.md) |

## 目录分工

### `docs/plan/`
用于保存实施计划、治理清单、阶段性 TODO。

### `docs/decision/`
用于保存决策记录和 ADR。

### `release/`
用于保存发布说明、回滚方案、发版验收记录，以及构建发布手册。

### `report/`
用于保存扫描、排障、运行、兼容性、FAQ 类报告和手册。

## 推荐入口

### 运行与使用
| 文档 | 作用 |
| --- | --- |
| [运行与部署指南.md](report/运行与部署指南.md) | 首次启动、Service 版、Docker、macOS 首启 |
| [环境变量与运行配置说明.md](report/环境变量与运行配置说明.md) | 统一查看所有运行配置、默认值与作用 |
| [FAQ与账号命中规则.md](report/FAQ与账号命中规则.md) | 常见问题、账号命中与日志排障 |
| [不登陆Codex使用ChatGPT-auth-session导入账号.md](report/不登陆Codex使用ChatGPT-auth-session导入账号.md) | 通过 ChatGPT `/api/auth/session` JSON 批量导入账号 |
| [当前网关与Codex官方请求参数对照表.md](report/当前网关与Codex官方请求参数对照表.md) | 当前网关实际出站参数、目标 Codex 参数，以及两者差异的对照表 |
| [原生Codex线程锚点优先级说明.md](report/原生Codex线程锚点优先级说明.md) | 说明原生 Codex、`prompt_cache_key` 与兼容兜底锚点的优先级规则 |
| [后台任务账号跳过说明.md](report/后台任务账号跳过说明.md) | 后台任务过滤、禁用账号、workspace 停用原因 |
| [最小排障手册.md](report/最小排障手册.md) | 快速定位最常见启动与转发问题 |
| [插件中心最小接入说明.md](report/插件中心最小接入说明.md) | 插件中心最小接入字段、接口和 Rhai 最小函数 |
| [当前网关与Codex请求头和参数差异表.md](report/当前网关与Codex请求头和参数差异表.md) | 当前网关参数传递、请求头和请求参数与 Codex 的对照说明 |
| [插件中心对接与接口清单.md](report/插件中心对接与接口清单.md) | 插件中心接入方式、市场模式、RPC/Tauri 命令、清单字段、Rhai 接口清单 |
| [系统内部接口总表.md](report/系统内部接口总表.md) | 系统全部可对接内部接口、Tauri/RPC 对照、插件内建函数 |

### 发布与构建
| 文档 | 作用 |
| --- | --- |
| [构建发布与脚本说明.md](release/构建发布与脚本说明.md) | 本地构建、脚本参数、workflow 入口 |
| [发布与产物说明.md](release/发布与产物说明.md) | 各平台产物、命名和发布结果说明 |
| [脚本与发布职责对照.md](report/脚本与发布职责对照.md) | 脚本职责边界和使用场景 |

### 治理与决策
| 文档 | 作用 |
| --- | --- |
| [账号模式与额度分发锁定策略.md](report/账号模式与额度分发锁定策略.md) | 说明账号系统、额度分发、平台 Key 归属和成员钱包扣费的锁定规则 |

## 提交规则

### 应提交到 Git 的文档
- 对未来协作者仍有参考价值
- 会影响后续开发、测试、发布或排障方式
- 可以作为项目长期事实源的一部分

### 不建议提交到 Git 的文档
- 临时草稿
- 个人过程笔记
- 一次性中间产物
- 本地试验记录

## 忽略规则
当前仓库会忽略以下文档：
- `docs/**/*.tmp.md`
- `docs/**/*.local.md`

如果是正式文档，不要使用上述后缀。

## 命名建议
推荐格式：

```text
长期保留文档：主题.md
一次性报告：yyyyMMddHHmmssfff_主题.md
```

## 维护约定
- 新增重要治理文档时，优先放到 `docs/`，不要继续堆进 README。
- 版本历史继续写入 `CHANGELOG.md`。
- 架构总览继续维护在 `ARCHITECTURE.md`。
- 协作规范继续维护在 `CONTRIBUTING.md`。
- 不要把未发版更新同时写进多个长文档；需要对外说明时，优先补 `CHANGELOG.md`，README 只保留摘要和入口。

## 联系方式
- Telegram 群聊：[CodexManager TG 群](https://t.me/+OdpFa9GvjxhjMDhl)
