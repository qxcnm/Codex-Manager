import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

test("toast notifications do not block page controls while actions stay clickable", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src", "components", "ui", "sonner.tsx"),
    "utf8",
  );

  assert.match(source, /toaster group pointer-events-none/);
  assert.match(source, /cn-toast pointer-events-none/);
  assert.match(source, /actionButton:[\s\S]*?pointer-events-auto/);
  assert.match(source, /cancelButton:[\s\S]*?pointer-events-auto/);
  assert.match(source, /closeButton:[\s\S]*?pointer-events-auto/);
  assert.match(source, /\.\.\.toastOptions\?\.classNames/);
});
