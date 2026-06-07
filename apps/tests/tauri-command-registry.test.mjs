import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(import.meta.dirname, "..", "..");
const appsRoot = path.join(repoRoot, "apps");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

async function listSourceFiles(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files = await Promise.all(
    entries.map(async (entry) => {
      const entryPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if ([".next", "node_modules", "out"].includes(entry.name)) {
          return [];
        }
        return listSourceFiles(entryPath);
      }
      return /\.(ts|tsx)$/.test(entry.name) ? [entryPath] : [];
    }),
  );
  return files.flat();
}

function extractStaticInvokedCommands(source) {
  return Array.from(
    source.matchAll(/invoke(?:First)?(?:<[^>]+>)?\(\s*(?:\[\s*)?["']([^"']+)["']/g),
  ).map((match) => match[1]);
}

test("前端静态调用的 Tauri commands 都已注册", async () => {
  const sourceFiles = await listSourceFiles(path.join(appsRoot, "src"));
  const registrySource = await readSource("src-tauri/src/commands/registry.rs");
  const commands = [
    ...new Set(
      (
        await Promise.all(
          sourceFiles.map(async (file) => extractStaticInvokedCommands(await fs.readFile(file, "utf8"))),
        )
      ).flat(),
    ),
  ].sort();

  assert.ok(commands.length > 0, "未从 apps/src 读取到静态 invoke command");
  for (const command of commands) {
    assert.match(
      registrySource,
      new RegExp(`::${command}\\b`),
      `${command} missing from Tauri invoke registry`,
    );
  }
});
