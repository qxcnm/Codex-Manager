import type { RequestOptions } from "../../utils/request";
import type { InvokeParams, WebRpcCaller } from "./shared";
import { asRecord } from "./shared";

function isSupportedBrowserImportFile(file: File): boolean {
  const normalizedName = String(file.name || "").trim().toLowerCase();
  return normalizedName.endsWith(".json") || normalizedName.endsWith(".txt");
}

export async function pickImportFilesFromBrowser(directory: boolean): Promise<unknown> {
  if (typeof document === "undefined") throw new Error("当前环境不支持浏览器文件选择");
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".json,.txt,application/json,text/plain";
  input.multiple = true;
  if (directory) {
    const directoryInput = input as HTMLInputElement & { directory?: boolean; webkitdirectory?: boolean };
    directoryInput.directory = true;
    directoryInput.webkitdirectory = true;
  }
  input.style.display = "none";
  document.body.appendChild(input);
  return await new Promise<unknown>((resolve, reject) => {
    let finished = false;
    const cleanup = () => {
      input.removeEventListener("change", handleChange);
      input.removeEventListener("cancel", handleCancel as EventListener);
      input.remove();
    };
    const finish = (value: unknown) => {
      if (finished) return;
      finished = true;
      cleanup();
      resolve(value);
    };
    const fail = (error: unknown) => {
      if (finished) return;
      finished = true;
      cleanup();
      reject(error);
    };
    const handleCancel = () => finish({ ok: true, canceled: true });
    const handleChange = async () => {
      try {
        const files = Array.from(input.files ?? []);
        if (!files.length) {
          handleCancel();
          return;
        }
        const importableFiles = files.filter(isSupportedBrowserImportFile);
        if (!importableFiles.length) {
          fail(new Error(directory ? "所选目录中没有可导入的 .json 或 .txt 文件" : "请选择 .json 或 .txt 文件"));
          return;
        }
        const fileEntries = await Promise.all(importableFiles.map(async (file) => {
          const content = await file.text();
          const relativePath = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
          return { content, path: relativePath || file.name };
        }));
        const nonEmptyEntries = fileEntries.filter((entry) => entry.content.trim().length > 0);
        if (!nonEmptyEntries.length) {
          fail(new Error("未在所选文件中找到可导入内容"));
          return;
        }
        const filePaths = nonEmptyEntries.map((entry) => entry.path);
        const contents = nonEmptyEntries.map((entry) => entry.content);
        const directorySourcePath = filePaths[0] || fileEntries[0]?.path || "";
        const directoryPath = directory ? directorySourcePath.split("/")[0] || directorySourcePath.split("\\")[0] || "" : "";
        finish({ ok: true, canceled: false, directoryPath, fileCount: importableFiles.length, filePaths, contents });
      } catch (error) {
        fail(error);
      }
    };
    input.addEventListener("change", handleChange);
    input.addEventListener("cancel", handleCancel as EventListener);
    input.click();
  });
}

export async function exportAccountsViaBrowser(postWebRpc: WebRpcCaller, params: Record<string, unknown> | null = null, options: RequestOptions = {}): Promise<unknown> {
  if (typeof document === "undefined") throw new Error("当前环境不支持浏览器导出");
  const selectedAccountIds = Array.isArray(params?.selectedAccountIds) ? params.selectedAccountIds.map((item) => String(item || "").trim()).filter(Boolean) : [];
  const exportMode = typeof params?.exportMode === "string" && params.exportMode.trim() ? params.exportMode.trim() : "multiple";
  const payload = asRecord(await postWebRpc<unknown>("account/exportData", { selectedAccountIds, exportMode }, options)) ?? {};
  const files = Array.isArray(payload.files) ? payload.files.map((item) => asRecord(item)).filter((item): item is Record<string, unknown> => item !== null) : [];
  for (const item of files) {
    const fileName = typeof item.fileName === "string" && item.fileName.trim() ? item.fileName.trim() : "account.json";
    const content = typeof item.content === "string" ? item.content : "";
    const blob = new Blob([content], { type: "application/json;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = fileName;
    anchor.style.display = "none";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }
  return { ok: true, canceled: false, exported: typeof payload.exported === "number" ? payload.exported : files.length, outputDir: "browser-download" };
}

export async function openInBrowserDirect(params?: InvokeParams): Promise<unknown> {
  const url = typeof params?.url === "string" ? params.url.trim() : "";
  if (!url) throw new Error("缺少浏览器跳转地址");
  if (typeof window === "undefined") throw new Error("当前环境不支持打开浏览器");
  window.open(url, "_blank", "noopener,noreferrer");
  return { ok: true };
}

export async function openExternalUrlDirect(params?: InvokeParams): Promise<unknown> {
  const url = typeof params?.url === "string" ? params.url.trim() : "";
  if (!url) throw new Error("缺少外部跳转地址");
  if (typeof window === "undefined") throw new Error("当前环境不支持打开外部链接");
  window.location.href = url;
  return { ok: true };
}

export async function showMainWindowDirect(): Promise<unknown> {
  if (typeof window !== "undefined") window.location.href = "/";
  return { ok: true };
}

export async function unsupportedOpenInFileManager(): Promise<unknown> {
  throw new Error("当前环境不支持打开本地目录");
}

export async function unsupportedOpenUpdateLogsDir(): Promise<unknown> {
  throw new Error("当前环境不支持打开更新日志目录");
}