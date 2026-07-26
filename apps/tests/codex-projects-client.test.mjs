import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "codex-projects-client.ts",
);

async function loadClientModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-codex-projects-client-"),
  );
  const tempFile = path.join(tempDir, "codex-projects-client.mjs");
  await fs.writeFile(
    path.join(tempDir, "transport.mjs"),
    "export async function invoke(command, params) { globalThis.__codexProjectsInvokeCalls ??= []; globalThis.__codexProjectsInvokeCalls.push({ command, params }); return globalThis.__codexProjectsInvokeResult ?? {}; }\n",
    "utf8",
  );
  await fs.writeFile(
    tempFile,
    compiled.outputText.replace("./transport", "./transport.mjs"),
    "utf8",
  );
  return import(pathToFileURL(tempFile).href);
}

const client = await loadClientModule();

test("normalizes desktop project payloads and filters missing paths", () => {
  assert.deepEqual(
    client.normalizeCodexProjectList({
      items: [
        {
          path: " /work/repo ",
          name: " Repo ",
          added_at: 123,
          available: true,
        },
        { name: "missing path" },
      ],
    }),
    {
      items: [
        {
          path: "/work/repo",
          name: "Repo",
          addedAt: 123,
          available: true,
        },
      ],
    },
  );

  assert.deepEqual(
    client.normalizeCodexProjectAddResult({
      canceled: false,
      added: true,
      project: { path: "/work/repo", available: true },
    }),
    {
      canceled: false,
      added: true,
      project: {
        path: "/work/repo",
        name: "/work/repo",
        addedAt: 0,
        available: true,
      },
    },
  );
});

test("desktop project methods use app-shell commands without service parameters", async () => {
  globalThis.__codexProjectsInvokeCalls = [];

  globalThis.__codexProjectsInvokeResult = { items: [] };
  await client.codexProjectsClient.list();
  globalThis.__codexProjectsInvokeResult = { canceled: true, added: false };
  await client.codexProjectsClient.add();
  globalThis.__codexProjectsInvokeResult = { removed: true };
  await client.codexProjectsClient.remove("/work/repo");
  globalThis.__codexProjectsInvokeResult = {
    path: "/work/repo",
    action: "resume",
    target: "cli",
    codex_home: "/home/user/.codex",
  };
  const launchResult = await client.codexProjectsClient.launch({
    path: "/work/repo",
    action: "resume",
  });

  assert.deepEqual(launchResult, {
    path: "/work/repo",
    action: "resume",
    target: "cli",
    codexHome: "/home/user/.codex",
  });

  assert.deepEqual(globalThis.__codexProjectsInvokeCalls, [
    { command: "app_codex_projects_list", params: undefined },
    { command: "app_codex_project_add", params: undefined },
    { command: "app_codex_project_remove", params: { path: "/work/repo" } },
    {
      command: "app_codex_project_launch",
      params: { path: "/work/repo", action: "resume" },
    },
  ]);
});
