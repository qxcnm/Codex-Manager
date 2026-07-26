"use client";

import { ServiceInitializationResult } from "@/types";

const LOOPBACK_PROXY_HINT = "若开启全局代理，请将 localhost/127.0.0.1/::1 设为直连";

/**
 * 函数 `asRecord`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
function asRecord(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === "object" && !Array.isArray(payload)
    ? (payload as Record<string, unknown>)
    : {};
}

/**
 * 函数 `normalizeServiceAddr`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - raw: 参数 raw
 *
 * # 返回
 * 返回函数执行结果
 */
export function normalizeServiceAddr(raw: string): string {
  const trimmed = String(raw || "").trim();
  if (!trimmed) {
    throw new Error("请输入端口或地址");
  }

  let value = trimmed;
  if (value.startsWith("http://")) {
    value = value.slice("http://".length);
  }
  if (value.startsWith("https://")) {
    value = value.slice("https://".length);
  }
  value = value.split("/")[0];

  if (/^\d+$/.test(value)) {
    return `localhost:${value}`;
  }

  const [host, port] = value.split(":");
  if (!port) return value;
  if (host === "127.0.0.1" || host === "0.0.0.0") {
    return `localhost:${port}`;
  }
  return value;
}

/**
 * 函数 `readInitializeResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function readInitializeResult(payload: unknown): ServiceInitializationResult {
  const source = asRecord(payload);
  const version = typeof source.version === "string" ? source.version : "";
  const userAgent =
    typeof source.userAgent === "string"
      ? source.userAgent
      : typeof source.user_agent === "string"
        ? source.user_agent
        : "";
  const codexHome =
    typeof source.codexHome === "string"
      ? source.codexHome
      : typeof source.codex_home === "string"
        ? source.codex_home
        : "";
  const platformFamily =
    typeof source.platformFamily === "string"
      ? source.platformFamily
      : typeof source.platform_family === "string"
        ? source.platform_family
        : "";
  const platformOs =
    typeof source.platformOs === "string"
      ? source.platformOs
      : typeof source.platform_os === "string"
        ? source.platform_os
        : "";
  return { version, userAgent, codexHome, platformFamily, platformOs };
}

/**
 * 函数 `isExpectedInitializeResult`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function isExpectedInitializeResult(payload: unknown): boolean {
  const result = readInitializeResult(payload);
  return result.userAgent.includes("codex_cli_rs/") && result.codexHome.length > 0;
}

/**
 * 函数 `formatServiceError`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - error: 参数 error
 *
 * # 返回
 * 返回函数执行结果
 */
export function formatServiceError(error: unknown): string {
  const raw =
    error && typeof error === "object" && "message" in error
      ? error.message
      : String(error || "");
  const text = String(raw || "").trim();
  if (!text) return "未知错误";

  const normalized = text
    .split("\n")[0]
    .trim()
    .replace(/^initialize task failed:\s*/i, "")
    .replace(/^service_initialize task failed:\s*/i, "")
    .replace(/^service_start task failed:\s*/i, "")
    .replace(/^service_stop task failed:\s*/i, "")
    .trim();

  const lower = normalized.toLowerCase();
  if (lower.includes("timed out")) return "连接超时";
  if (lower.includes("connection refused") || lower.includes("actively refused")) {
    return "连接被拒绝";
  }
  if (lower.includes("empty response")) {
    return "服务返回空响应（可能启动未完成、已异常退出或端口被占用）";
  }
  if (lower.includes("port is in use") || lower.includes("unexpected service responded")) {
    return `端口已被占用或响应来源不是 CodexManager 服务（${LOOPBACK_PROXY_HINT}）`;
  }
  if (lower.includes("missing user_agent") || lower.includes("missing useragent")) {
    return `响应缺少服务标识（疑似非 CodexManager 服务，${LOOPBACK_PROXY_HINT}）`;
  }
  if (
    lower.includes("unexpected rpc response") ||
    lower.includes("expected value at line 1 column 1") ||
    lower.includes("invalid chunked body")
  ) {
    return `响应格式异常（疑似非 CodexManager 服务，${LOOPBACK_PROXY_HINT}）`;
  }
  if (lower.includes("no address resolved")) return "地址解析失败";
  if (lower.includes("addr is empty")) return "地址为空";
  if (lower.includes("invalid service address")) return "地址不合法";
  return normalized.length > 120 ? `${normalized.slice(0, 120)}...` : normalized;
}

export function isServicePortConflictError(error: string | null | undefined): boolean {
  const normalized = String(error || "").toLowerCase();
  return (
    normalized.includes("端口已被占用") ||
    normalized.includes("port is in use") ||
    normalized.includes("unexpected service responded") ||
    normalized.includes("疑似非 codexmanager 服务") ||
    normalized.includes("响应来源不是 codexmanager 服务") ||
    normalized.includes("服务返回空响应")
  );
}
