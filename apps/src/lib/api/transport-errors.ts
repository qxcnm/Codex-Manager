/**
 * 函数 `asRecord`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * 函数 `normalizeKnownAppErrorMessage`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-14
 *
 * # 参数
 * - message: 参数 message
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeKnownAppErrorMessage(message: string): string {
  const trimmed = String(message || "").trim();
  if (!trimmed) {
    return "";
  }

  const normalized = trimmed.toLowerCase();
  if (normalized.includes("grok_credential_unauthorized")) {
    return "Grok 登录凭据已失效，请重新导入";
  }
  if (normalized.includes("grok_antibot_challenge")) {
    return "Grok 风控验证未通过，请检查固定代理出口后稍后重试";
  }
  if (normalized.includes("grok_quota_rate_limited")) {
    return "Grok 模型探测过于频繁，请稍后重试";
  }
  if (normalized.includes("grok_quota_shape_unknown")) {
    return "暂时无法确认该 Grok 账号的套餐等级，未展示未经验证的模型";
  }
  if (normalized.includes("grok_statsig_signer")) {
    return "Grok 模型探测签名服务暂时不可用";
  }
  if (
    normalized === "request or response body error" ||
    normalized === "stream read failed" ||
    normalized === "上游中途断开，未返回具体错误信息"
  ) {
    return "上游中途断开，未返回具体错误信息";
  }
  if (
    normalized === "stream idle timeout" ||
    normalized === "上游流式空闲超时" ||
    normalized.includes("stream_timeout") ||
    normalized.includes("idle timeout")
  ) {
    return "上游流式空闲超时";
  }
  if (
    normalized === "response.incomplete" ||
    normalized === "网络抖动" ||
    normalized === "stream disconnected before completion" ||
    normalized === "连接中断（可能是网络波动或客户端主动取消）"
  ) {
    return "连接中断（可能是网络波动或客户端主动取消）";
  }

  return trimmed;
}

/**
 * 函数 `resolveRpcErrorMessage`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - error: 参数 error
 *
 * # 返回
 * 返回函数执行结果
 */
export function resolveRpcErrorMessage(error: unknown): string {
  if (typeof error === "string") return normalizeKnownAppErrorMessage(error);
  const record = asRecord(error);
  if (record?.message && typeof record.message === "string") {
    return normalizeKnownAppErrorMessage(record.message);
  }
  return error ? JSON.stringify(error) : "RPC 请求失败";
}

/**
 * 函数 `resolveBusinessErrorMessage`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function resolveBusinessErrorMessage(payload: unknown): string {
  const source = asRecord(payload);
  if (!source) return "";
  const error = source.error;
  if (source.ok === false) {
    return typeof error === "string"
      ? normalizeKnownAppErrorMessage(error)
      : asRecord(error)?.message
        ? normalizeKnownAppErrorMessage(String(asRecord(error)?.message))
        : "操作失败";
  }
  if (error) {
    return typeof error === "string"
      ? normalizeKnownAppErrorMessage(error)
      : asRecord(error)?.message
        ? normalizeKnownAppErrorMessage(String(asRecord(error)?.message))
        : "";
  }
  return "";
}

/**
 * 函数 `throwIfBusinessError`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function throwIfBusinessError(payload: unknown): void {
  const message = resolveBusinessErrorMessage(payload);
  if (message) {
    throw new Error(message);
  }
}

/**
 * 函数 `unwrapRpcPayload`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - payload: 参数 payload
 *
 * # 返回
 * 返回函数执行结果
 */
export function unwrapRpcPayload<T>(payload: unknown): T {
  const responseRecord = asRecord(payload);
  if (responseRecord && "error" in responseRecord) {
    throw new Error(resolveRpcErrorMessage(responseRecord.error));
  }
  if (responseRecord && "result" in responseRecord) {
    const result = responseRecord.result as T;
    throwIfBusinessError(result);
    return result;
  }
  throwIfBusinessError(payload);
  return payload as T;
}

/**
 * 函数 `getAppErrorMessage`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - error: 参数 error
 * - fallback: 参数 fallback
 *
 * # 返回
 * 返回函数执行结果
 */
export function getAppErrorMessage(
  error: unknown,
  fallback = "操作失败"
): string {
  if (error instanceof Error) {
    const nested = getAppErrorMessage(error.message, "");
    return nested || fallback;
  }

  const businessMessage = resolveBusinessErrorMessage(error);
  if (businessMessage) return businessMessage;

  const rpcMessage = resolveRpcErrorMessage(error).trim();
  if (!rpcMessage || rpcMessage === "null" || rpcMessage === "undefined") {
    return fallback;
  }
  return rpcMessage;
}

/**
 * 函数 `isCommandMissingError`
 *
 * 作者: Codex
 *
 * 时间: 2026-04-13
 *
 * # 参数
 * - err: 参数 err
 *
 * # 返回
 * 返回函数执行结果
 */
export function isCommandMissingError(err: unknown): boolean {
  const message = getAppErrorMessage(err, "").toLowerCase();
  return (
    message.includes("unknown command") ||
    message.includes("not found") ||
    message.includes("is not a registered")
  );
}
