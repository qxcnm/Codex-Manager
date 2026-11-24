import assert from "node:assert/strict";
import fs from "node:fs/promises";
import test from "node:test";

test("main WebView allows HTML5 model and route drag-and-drop on Windows", async () => {
  const config = JSON.parse(
    await fs.readFile(
      new URL("../src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );
  const mainWindow = config.app.windows.find(
    (window) => (window.label ?? "main") === "main",
  );

  assert.ok(mainWindow, "main window configuration must exist");
  // Tauri's native file-drop handler overrides WebView2's HTML5 drag/drop.
  // The model catalog and route editor use HTML5 dragging, not native file drops.
  assert.equal(
    mainWindow.dragDropEnabled,
    false,
    "disable native file-drop interception so HTML5 drag events reach the UI",
  );
});
