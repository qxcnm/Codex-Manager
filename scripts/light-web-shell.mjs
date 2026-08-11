#!/usr/bin/env node
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");

const listenAddr = normalizeListenAddr(process.env.CODEXMANAGER_LIGHT_WEB_ADDR || "127.0.0.1:48762");
const serviceAddr = stripHttp(process.env.CODEXMANAGER_SERVICE_ADDR || "127.0.0.1:48760");
const webRoot = path.resolve(process.env.CODEXMANAGER_WEB_ROOT || path.join(repoRoot, "apps", "out"));
const authorContentUrl = process.env.CODEXMANAGER_AUTHOR_CONTENT_URL || "https://author.qxnm.top/api/public/author-content";
const rpcToken = readRpcToken();

function normalizeListenAddr(raw) {
  const value = String(raw || "").trim() || "127.0.0.1:48762";
  const withoutScheme = stripHttp(value);
  const idx = withoutScheme.lastIndexOf(":");
  if (idx <= 0) return { host: "127.0.0.1", port: 48762, label: "127.0.0.1:48762" };
  const host = withoutScheme.slice(0, idx).replace(/^\[|\]$/g, "") || "127.0.0.1";
  const port = Number(withoutScheme.slice(idx + 1)) || 48762;
  return { host, port, label: `${host}:${port}` };
}

function stripHttp(value) {
  return String(value || "").trim().replace(/^https?:\/\//i, "").replace(/\/+$/, "");
}

function readTextIfExists(file) {
  try {
    const value = fs.readFileSync(file, "utf8").trim();
    return value || "";
  } catch {
    return "";
  }
}

function readRpcToken() {
  const direct = String(process.env.CODEXMANAGER_RPC_TOKEN || "").trim();
  if (direct) return direct;
  const explicitFile = String(process.env.CODEXMANAGER_RPC_TOKEN_FILE || "").trim();
  const candidates = [
    explicitFile,
    path.join(path.dirname(process.execPath), "codexmanager.rpc-token"),
    path.join(repoRoot, "codexmanager.rpc-token"),
    path.join(repoRoot, "target", "debug", "codexmanager.rpc-token"),
    path.join("D:", "CodexWork", "build", "CPA-Dashboard-target", "debug", "codexmanager.rpc-token"),
    path.join("D:", "CodexWork", "build", "CPA-Dashboard-lowmem", "debug", "codexmanager.rpc-token"),
    path.join(process.env.LOCALAPPDATA || "", "CodexManager", "codexmanager.rpc-token"),
    path.join(process.env.APPDATA || "", "CodexManager", "codexmanager.rpc-token"),
  ].filter(Boolean);
  for (const file of candidates) {
    const token = readTextIfExists(path.resolve(file));
    if (token) return token;
  }
  return "";
}

function sendJson(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
    "content-length": Buffer.byteLength(body),
  });
  res.end(body);
}

function sendText(res, status, text, type = "text/plain; charset=utf-8") {
  res.writeHead(status, { "content-type": type, "content-length": Buffer.byteLength(text) });
  res.end(text);
}

function contentType(file) {
  const ext = path.extname(file).toLowerCase();
  return {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".json": "application/json; charset=utf-8",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
    ".ico": "image/x-icon",
    ".txt": "text/plain; charset=utf-8",
    ".woff": "font/woff",
    ".woff2": "font/woff2",
  }[ext] || "application/octet-stream";
}

function safeStaticPath(urlPath) {
  let decoded = "/";
  try { decoded = decodeURIComponent(urlPath.split("?")[0] || "/"); } catch { decoded = "/"; }
  if (decoded === "/") decoded = "/index.html";
  const joined = path.resolve(webRoot, `.${decoded}`);
  if (!joined.startsWith(webRoot)) return null;
  if (fs.existsSync(joined) && fs.statSync(joined).isFile()) return joined;
  const htmlFallback = path.resolve(webRoot, "index.html");
  return fs.existsSync(htmlFallback) ? htmlFallback : null;
}

async function proxyRpc(req, res) {
  if (!rpcToken) {
    return sendJson(res, 500, {
      error: "light_shell_missing_rpc_token",
      message: "轻量壳没有找到 CODEXMANAGER_RPC_TOKEN 或 codexmanager.rpc-token，无法转发管理 RPC。",
    });
  }
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  try {
    const upstream = await fetch(`http://${serviceAddr}/rpc`, {
      method: "POST",
      headers: {
        "content-type": req.headers["content-type"] || "application/json",
        "accept": req.headers.accept || "application/json",
        "x-codexmanager-rpc-token": rpcToken,
        "x-codexmanager-rpc-actor-role": req.headers["x-codexmanager-rpc-actor-role"] || "admin",
        "x-codexmanager-rpc-actor-user-id": req.headers["x-codexmanager-rpc-actor-user-id"] || "local-light-shell",
      },
      body,
    });
    const responseBody = Buffer.from(await upstream.arrayBuffer());
    const headers = {
      "content-type": upstream.headers.get("content-type") || "application/json; charset=utf-8",
      "content-length": responseBody.length,
    };
    for (const name of ["x-codexmanager-error-code", "x-codexmanager-trace-id"]) {
      const value = upstream.headers.get(name);
      if (value) headers[name] = value;
    }
    res.writeHead(upstream.status, headers);
    res.end(responseBody);
  } catch (error) {
    sendJson(res, 502, {
      error: "light_shell_rpc_proxy_failed",
      message: `无法连接 codexmanager-service：${error?.message || String(error)}`,
    });
  }
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url || "/", `http://${listenAddr.label}`);
  if (req.method === "GET" && url.pathname === "/api/runtime") {
    return sendJson(res, 200, {
      mode: "web-gateway",
      rpcBaseUrl: "/api/rpc",
      authorContentUrl,
      canManageService: false,
      canSelfUpdate: false,
      canAutoStart: false,
      canCloseToTray: false,
      canOpenLocalDir: false,
      canUseBrowserFileImport: true,
      canUseBrowserDownloadExport: true,
      unsupportedReason: null,
    });
  }
  if (req.method === "POST" && url.pathname === "/api/rpc") {
    return proxyRpc(req, res);
  }
  if (req.method !== "GET" && req.method !== "HEAD") {
    return sendText(res, 405, "Method Not Allowed");
  }
  const file = safeStaticPath(url.pathname);
  if (!file) {
    return sendText(res, 404, "当前轻量壳没有找到 apps/out，请先运行 pnpm -C apps run build:desktop。", "text/plain; charset=utf-8");
  }
  const data = fs.readFileSync(file);
  res.writeHead(200, {
    "content-type": contentType(file),
    "cache-control": file.endsWith("index.html") ? "no-store" : "public, max-age=3600",
    "content-length": data.length,
  });
  if (req.method === "HEAD") res.end(); else res.end(data);
});

server.listen(listenAddr.port, listenAddr.host, () => {
  console.log(`CodexManager light shell listening on http://${listenAddr.label}`);
  console.log(`Frontend root: ${webRoot}`);
  console.log(`RPC proxy: /api/rpc -> http://${serviceAddr}/rpc`);
  console.log(`RPC token: ${rpcToken ? "loaded" : "missing"}`);
});
