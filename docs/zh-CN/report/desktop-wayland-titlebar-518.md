# Linux / Wayland 标题栏按钮兼容性修复（#518）

## 问题与依据

在 Ubuntu GNOME / Wayland 下，主窗口首次显示或从托盘恢复后，原生标题栏的最小化、最大化和关闭按钮可能无响应，调整窗口或切换最大化后恢复。

项目使用延迟创建、初始隐藏的原生窗口，原先桌面锁文件解析到 Tauri 2.10.3、tauri-runtime-wry 2.10.1 和 Tao 0.34.8。该 Tao 版本包含旧的 Wayland 自绘装饰事件处理。

[Tao #1218](https://github.com/tauri-apps/tao/pull/1218) 移除了覆盖 GTK HeaderBar 的事件包装，恢复 GTK 默认的装饰及事件传播。修复提交 `07f3742b1833b64be27b1ef991e38d557d4276c9` 已随 Tao 0.36.0 发布。

## 实施范围

- 更新桌面依赖声明和锁文件，通过 Tauri 官方运行时依赖链引入修复，不直接增加另一份 Tao 依赖。
- 同步前端 `@tauri-apps/api` 的 minor 版本，满足 Tauri CLI 的 JS / Rust 版本兼容检查。
- 桌面 crate 最低 Rust 版本提升至 1.90；现有日志、单实例、自启动和窗口状态插件保持版本。
- 保留现有窗口显示、关闭到托盘、轻量模式和 WebSocket 依赖 pin。

已核对的实际依赖解析结果：

| 依赖 | 版本 |
| --- | --- |
| tauri | 2.12.5 |
| tauri-build | 2.7.4 |
| tauri-runtime | 2.12.2 |
| tauri-runtime-wry | 2.12.3 |
| tao | 0.37.2 |
| wry | 0.57.1 |
| @tauri-apps/api | 2.12.4 |

Tao 0.37.2 包含上述修复。仅将 Tauri 更新到 2.11 不足以解决问题，因为该链仍依赖 Tao 0.35。

## 自动验证

在 Windows 开发环境执行并通过：

提交前基于主线 `d3c59783` 仅应用本修复后重新验证，未包含开发工作区中其他未提交改动。

```text
cargo tree --locked --manifest-path apps/src-tauri/Cargo.toml -i tao
cargo test --locked --manifest-path apps/src-tauri/Cargo.toml --lib
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

- `cargo tree` 确认应用通过 `tauri-runtime-wry 2.12.3` 使用单一 Tao 0.37.2。
- 桌面 Rust 单元测试：96 通过，0 失败。
- 前端运行时测试：324 通过，0 失败。
- 桌面静态前端构建通过；安装的 JS API 实际版本为 2.12.4。
- 定向 `git diff --check` 通过；原有服务依赖、四项桌面插件及 WebSocket 双 pin 保留。

根 workspace 不包含 `apps/src-tauri`，本次只调整桌面依赖链，未重复运行根 `cargo test --workspace`；使用上述桌面 crate 测试覆盖本次编译及测试范围。

## 平台验收边界

Windows 编译和自动测试只能证明依赖与现有代码兼容，不能代替 Wayland 原生装饰的实际交互验收。当前开发环境没有 Ubuntu GNOME / Wayland 或 macOS 桌面，以下项目保留为人工验收项，未标记通过：

- Ubuntu GNOME / Wayland：首次显示后立即操作三个标题栏按钮；隐藏启动后通过托盘显示；关闭到托盘后再次显示；连续最小化、最大化、还原。
- Windows：原生标题栏按钮、关闭到托盘、托盘恢复、轻量模式下的重复建窗。
- macOS：红黄绿按钮、Dock Reopen、托盘唤回、全屏进出、Retina / 外接显示器上的托盘预览位置，以及应用重启。

本次 macOS 视频中的“应用接入后原生 Codex App 要求登录”属于 Codex 配置与认证检查范围，不据此认定为本标题栏缺陷，也不将升级窗口依赖视为该认证现象的修复。
