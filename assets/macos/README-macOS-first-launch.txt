CodexManager macOS first launch

This build is not notarized with an Apple Developer account yet.
If macOS says the app is damaged or refuses to open it, use one of the methods below.

Recommended:
1. Open the DMG.
2. Drag CodexManager.app into Applications.
3. Double-click "Open CodexManager.command".

Manual command:
xattr -dr com.apple.quarantine /Applications/CodexManager.app

Fallback:
- Right-click CodexManager.app and choose Open once.

---

CodexManager macOS 首次启动说明

当前构建暂未使用 Apple Developer 账号完成公证。
如果 macOS 提示“已损坏”或无法打开，请按下面任一方式处理。

推荐做法：
1. 打开 DMG。
2. 将 CodexManager.app 拖到“应用程序”。
3. 双击执行 "Open CodexManager.command"。

手动命令：
xattr -dr com.apple.quarantine /Applications/CodexManager.app

兜底方式：
- 右键 CodexManager.app，选择“打开”一次。
