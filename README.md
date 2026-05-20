<p align="center">
  <img src="assets/logo/logo.png" alt="CodexManager Logo" width="220" />
</p>

<h1 align="center">CodexManager</h1>

<p align="center">本地桌面端 + 服务进程的 Codex 账号管理器+网关转发</p>

<p align="center">
  <a href="docs/en/README.md">English</a>|
  <a href="docs/ru/README.md">Русский</a>|
  <a href="docs/ko/README.md">한국어</a>|
  <a href="https://github.com/qxcnm/Codex-Manager">GitHub 主仓库</a>|
  <a href="https://qxnm.top">官网</a>|
  <a href="#赞助商">赞助商</a>
</p>

<p align="center"><strong>本地桌面端 + 服务进程的 Codex 账号池管理器</strong></p>
<p align="center">统一管理账号、用量与平台 Key，并提供本地网关能力。</p>

## 认可社区
<p align="left">
  <a href="https://linux.do/t/topic/1688401" title="LINUX DO">
    <img
      src="https://cdn3.linux.do/original/4X/d/1/4/d146c68151340881c884d95e0da4acdf369258c6.png"
      alt="LINUX DO"
      width="100"
      hight="100"
    />
  </a>
</p>

## 源码说明：
> 本产品完全由本人指挥+AI打造 Codex（98%） Gemini (2%) 如果在使用过程中产生问题请友好交流，因为开源只是觉得有人能用的上，基本功能也没什么问题，不喜勿喷。
> 其次是本人没有足够的环境来验证每个包都有没有问题，本人也要上班(我只是个穷逼买不起mac之类的)，本人只保证win的桌面端的可用性，如果其他端有问题，请在充分测试后提交 Issues，有时间我自会处理
> 最后感谢各位使用者反馈的各个平台问题和参与的部分测试。


## 免责声明

- 本项目仅用于学习与开发目的。

- 使用者必须遵守相关平台的服务条款（例如 OpenAI、Anthropic）。

- 作者不提供或分发任何账号、API Key 或代理服务，也不对本软件的具体使用方式负责。

- 请勿使用本项目绕过速率限制或服务限制。

## 赞助商

感谢以下朋友与伙伴对 CodexManager 的支持。

<table>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://gzxsy.vip/register?aff=eapz">
        <img src="assets/images/sponsors/xingsiyan.jpg" alt="星思研中转站" width="120" />
      </a>
    </td>
    <td valign="top">
      <strong>星思研中转站</strong> 为 Claude Code、Codex 等模型调用场景提供稳定中转与配套服务，适合需要高可用接口、便捷接入和持续交付支持的开发者与团队。可前往<a href="https://gzxsy.vip/register?aff=eapz">官网</a>了解最新方案。
    </td>
  </tr>
</table>


其他支持者：呆头呆脑、 [Wonderdch](https://github.com/Wonderdch)、[suxinwl](https://github.com/suxinwl)、[Hermit](https://github.com/HermitChen)、[Suifeng023](https://github.com/Suifeng023)、[HK-hub](https://github.com/HK-hub)


## ☕ 支持项目 (Support)

如果您觉得本项目对您有所帮助，欢迎打赏作者！
<table>
  <tr>
    <th>支付宝 (Alipay)</th>
    <th>微信支付 (WeChat)</th>
  </tr>
  <tr>
    <td align="center"><img src="assets/images/AliPay.jpg" alt="支付宝赞助码" width="220" /></td>
    <td align="center"><img src="assets/images/wechatPay.jpg" alt="微信赞助码" width="220" /></td>
  </tr>
</table>

## Star History

<a href="https://www.star-history.com/?repos=qxcnm%2FCodex-Manager&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=qxcnm/Codex-Manager&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=qxcnm/Codex-Manager&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=qxcnm/Codex-Manager&type=date&legend=top-left" />
 </picture>
</a>

## 首页导览
| 你要做什么 | 直接进入 |
| --- | --- |
| 首次启动、部署、Docker、macOS 放行 | [运行与部署指南](docs/zh-CN/report/运行与部署指南.md) |
| 配置 Codex CLI / ccswitch 接入、`auth.json` 与 `config.toml` | [运行与部署指南](docs/zh-CN/report/运行与部署指南.md#通过-ccswitch-接入) |
| 不登陆 Codex，使用 ChatGPT `/api/auth/session` 导入账号 | [不登陆 Codex 使用 ChatGPT 的 /api/auth/session 在软件中的使用](docs/zh-CN/report/不登陆Codex使用ChatGPT-auth-session导入账号.md) |
| 配置端口、代理、数据库、Web 密码、环境变量 | [环境变量与运行配置](docs/zh-CN/report/环境变量与运行配置说明.md) |
| 排查账号不命中、导入失败、挑战拦截、请求异常 | [FAQ 与账号命中规则](docs/zh-CN/report/FAQ与账号命中规则.md) |
| 排查后台任务账号跳过、禁用与停用原因 | [后台任务账号跳过说明](docs/zh-CN/report/后台任务账号跳过说明.md) |
| 插件中心最小接入、快速对接 | [插件中心最小接入说明](docs/zh-CN/report/插件中心最小接入说明.md) |
| 对接插件中心、查看接口清单、市场模式与 Rhai 接口 | [插件中心对接与接口清单](docs/zh-CN/report/插件中心对接与接口清单.md) |
| 系统全部可对接内部接口 | [系统内部接口总表](docs/zh-CN/report/系统内部接口总表.md) |
| 本地构建、打包、发版、脚本调用 | [构建发布与脚本说明](docs/zh-CN/release/构建发布与脚本说明.md) |

## 功能概览
- 账号池管理：分组、标签、排序、备注、封禁识别与封禁筛选
- 批量导入 / 导出：支持多文件导入、桌面端文件夹递归导入 JSON、按账号导出单文件
- 用量展示：支持标准 5 小时 + 7 日窗口、仅 7 日单窗口账号，以及 Code Review / Spark 等官方附加额度窗口；刷新后会统一展示各额度的剩余百分比与重置时间
- 授权登录：浏览器授权 + 手动回调解析
- 平台 Key：随机生成或自定义固定 Key、禁用、删除、模型绑定、推理等级、服务等级（跟随请求 / Fast / Flex）
- 模型管理：维护结构化模型目录、远端并入、自定义模型、`visibility` / `supportedInApi` 管理，以及桌面端 Codex 缓存同步 / Web 端缓存导出
- 聚合 API：管理第三方最小转发上游，支持创建、编辑、测试连通性、供应商名称、顺序优先级，以及按 Codex / Claude 分类展示
- 插件中心：路由为 `/plugins/`，支持内置精选、企业私有、自定义源三种市场模式，并提供插件清单、任务、日志与 Rhai 对接接口
- 设置页：支持“系统推导”按钮、单账号并发上限、上游代理、请求总超时、流式空闲超时、SSE 保活间隔，以及更保守的高并发退化策略
- 系统内部接口总表：列出当前桌面端与服务端所有可对接命令、RPC 方法、以及插件内建函数
- 本地服务：自动拉起、可自定义端口与监听地址
- 本地网关：为 Codex CLI、Gemini CLI、Claude Code 和第三方工具提供统一 OpenAI 兼容入口；Gemini 请求可转发到 `/v1/responses`，并兼容 SSE、tools、MCP、skill、请求总超时与流式空闲超时等调用链路
- 图片生成：默认按官方 Codex 行为为 `/v1/responses` 自动注入 `image_generation` tool，并支持显式 tool 透传、`/v1/images/generations` 与 `/v1/images/edits` 兼容入口，默认图片工具模型为 `gpt-image-2`

## 生态搭配

### OpenCowork

- 仓库地址：[AIDotNet/OpenCowork](https://github.com/AIDotNet/OpenCowork)
- 搭配方式：使用 OpenCowork 承接本地文件操作、多 Agent 协作、消息平台接入与桌面执行能力，再由 CodexManager 统一管理 Codex 账号、用量、平台 Key 与本地网关入口。
- 适合场景：当您希望把“执行工作台 / 办公协同”和“账号池管理 / 网关入口”拆开时，这两个项目可以形成互补组合。
- 推荐理解：**OpenCowork 更偏执行与落地，CodexManager 更偏管理与网关。**

## 截图
![仪表盘](assets/images/dashboard.png)
![账号管理](assets/images/accounts.png)
![平台 Key](assets/images/platform-key.png)
![聚合 API](assets/images/aggregate-api.png)
![插件中心](assets/images/plug.png)
![日志视图](assets/images/log.png)
![设置页](assets/images/themes.png)

## 快速开始
1. 启动桌面端，点击“启动服务”。
2. 进入“账号管理”，添加账号并完成授权。
3. 如回调失败，粘贴回调链接手动完成解析。
4. 刷新用量并确认账号状态。

## 默认数据目录
- 桌面端默认会把 SQLite 数据库写到应用数据目录下，文件名固定为 `codexmanager.db`。
- Windows：`%APPDATA%\\com.codexmanager.desktop\\codexmanager.db`
- macOS：`~/Library/Application Support/com.codexmanager.desktop/codexmanager.db`
- Linux：`~/.local/share/com.codexmanager.desktop/codexmanager.db`
- 如需调整数据库、代理、监听地址等运行配置，可继续查看 [环境变量与运行配置](docs/zh-CN/report/环境变量与运行配置说明.md)。

## 页面展示
### 桌面端
- 账号管理：集中导入、导出、刷新账号与用量，支持低配额 / 封禁筛选与重置时间展示
- 平台 Key：按模型、推理等级、服务等级绑定平台 Key，并查看调用日志
- 模型管理：桌面端修改后会自动同步本地 `~/.codex/models_cache.json`
- 插件中心：`/plugins/` 路由，内置精选 / 企业私有 / 自定义源市场切换，插件安装、启停、任务、日志、Rhai 对接
- 设置页：统一管理端口、监听地址、代理、请求超时、SSE 保活、主题、自动更新、后台行为

### Service 版
- `codexmanager-service`：提供本地 OpenAI 兼容网关
- `codexmanager-web`：提供浏览器管理页面，并承载 `/api/runtime` 与 `/api/rpc` 代理
- `codexmanager-start`：一键拉起 service + web

## 常用文档
- 版本历史：[CHANGELOG.md](docs/zh-CN/CHANGELOG.md)
- 协作约定：[CONTRIBUTING.md](docs/zh-CN/CONTRIBUTING.md)
- 架构说明：[ARCHITECTURE.md](docs/zh-CN/ARCHITECTURE.md)
- 测试基线：[TESTING.md](docs/zh-CN/TESTING.md)
- 安全说明：[SECURITY.md](docs/zh-CN/SECURITY.md)
- 文档索引：[docs/zh-CN/README.md](docs/zh-CN/README.md)

## 专题页面
| 页面 | 内容 |
| --- | --- |
| [运行与部署指南](docs/zh-CN/report/运行与部署指南.md) | 首次启动、Docker、Service 版、macOS 放行 |
| [环境变量与运行配置](docs/zh-CN/report/环境变量与运行配置说明.md) | 应用配置、代理、监听地址、数据库、Web 安全 |
| [FAQ 与账号命中规则](docs/zh-CN/report/FAQ与账号命中规则.md) | 账号命中、挑战拦截、导入导出、常见异常 |
| [不登陆 Codex 使用 ChatGPT 的 /api/auth/session 在软件中的使用](docs/zh-CN/report/不登陆Codex使用ChatGPT-auth-session导入账号.md) | 浏览器复制 ChatGPT session JSON 后，通过批量导入加入账号池 |
| [后台任务账号跳过说明](docs/zh-CN/report/后台任务账号跳过说明.md) | 后台任务过滤、禁用账号、workspace 停用原因 |
| [最小排障手册](docs/zh-CN/report/最小排障手册.md) | 快速定位服务启动、请求转发、模型刷新异常 |
| [插件中心对接与接口清单](docs/zh-CN/report/插件中心对接与接口清单.md) | 插件中心路由、市场模式、Tauri/RPC 接口、清单字段、Rhai 内建函数 |
| [构建发布与脚本说明](docs/zh-CN/release/构建发布与脚本说明.md) | 本地构建、Tauri 打包、Release workflow、脚本参数 |
| [发布与产物说明](docs/zh-CN/release/发布与产物说明.md) | 各平台发版产物、命名、是否 pre-release |
| [脚本与发布职责对照](docs/zh-CN/report/脚本与发布职责对照.md) | 各脚本负责什么、什么场景该用哪个 |
| [当前网关与 Codex 请求头和参数差异表](docs/zh-CN/report/当前网关与Codex请求头和参数差异表.md) | 当前网关参数传递、请求头和请求参数与 Codex 的对照说明 |
| [系统内部接口总表](docs/zh-CN/report/系统内部接口总表.md) | 桌面端、服务端、插件中心全部可对接内部接口 |
| [CHANGELOG.md](docs/zh-CN/CHANGELOG.md) | 最新发版内容、未发版更新与完整版本历史 |

## 目录结构
```text
.
├─ apps/                # 前端与 Tauri 桌面端
│  ├─ src/
│  ├─ src-tauri/
│  └─ dist/
├─ crates/              # Rust core/service
│  ├─ core
│  ├─ service
│  ├─ start              # Service 版本一键启动器（拉起 service + web）
│  └─ web                # Service 版本 Web UI（可内嵌静态资源 + /api/rpc 代理）
├─ docs/                # 正式文档目录
├─ scripts/             # 构建与发布脚本
└─ README.md
```

## 鸣谢与参考项目

- Codex（OpenAI）：本项目在请求链路、登录语义与上游兼容行为上参考了该项目的实现与源码结构 <https://github.com/openai/codex>
- CLIProxyAPI（CPA）：本项目在请求链路（Responses 请求转换与工具调用约定）参考其实现与约定 <https://github.com/router-for-me/CLIProxyAPI>



## 联系方式
- 公众号：七线牛马
- 微信：加微信群请加我微信 `ProsperGao`，并说明来意
- Telegram 群聊：[CodexManager TG 群](https://t.me/+OdpFa9GvjxhjMDhl)
