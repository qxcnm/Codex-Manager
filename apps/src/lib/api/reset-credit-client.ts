import type {
  ResetCredit,
  ResetCreditConsumeResult,
  ResetCreditsSnapshot,
} from "@/types";
import { invoke, withAddr } from "./transport";

const RESET_CREDIT_CONSUME_TIMEOUT_MS = 600_000;
const RESET_CREDIT_PENDING_OPERATION_KEY = "codexmanager.reset-credit.pending";
const RESET_CREDIT_DEFAULT_SERVICE_KEY = "default";
const RESET_CREDIT_PENDING_OPERATION_PREFIX = "reset_credit_pending_operation:";
const RESET_CREDIT_TERMINAL_FAILURE_PREFIX = "reset_credit_terminal_failure:";
const pendingResetCreditOperationIds = new Map<string, string>();

function normalizeResetCreditServiceAddress(serviceAddr: unknown): string {
  const scope =
    typeof serviceAddr === "string" ? serviceAddr.trim().toLowerCase() : "";
  return scope || RESET_CREDIT_DEFAULT_SERVICE_KEY;
}

function resetCreditOperationStorageKey(
  accountId: string,
  serviceAddr: unknown = "",
): string {
  const scope = normalizeResetCreditServiceAddress(serviceAddr);
  return `${RESET_CREDIT_PENDING_OPERATION_KEY}.${encodeURIComponent(scope)}.${encodeURIComponent(accountId)}`;
}

function readLocalStorageValue(key: string): string | null {
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function writeLocalStorageValue(key: string, value: string): void {
  try {
    globalThis.localStorage?.setItem(key, value);
  } catch {
    // The in-memory fallback remains authoritative for this app session.
  }
}

function removeLocalStorageValue(key: string): void {
  try {
    globalThis.localStorage?.removeItem(key);
  } catch {
    // Ignore storage cleanup failures after a terminal response.
  }
}

function isUuidV4(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      value,
    )
  );
}

function readPendingResetCreditOperationId(
  accountId: string,
  serviceAddr: unknown = "",
): string | null {
  const key = resetCreditOperationStorageKey(accountId, serviceAddr);
  const storedValue = readLocalStorageValue(key);
  if (isUuidV4(storedValue)) {
    pendingResetCreditOperationIds.set(key, storedValue);
    return storedValue;
  }
  const memoryValue = pendingResetCreditOperationIds.get(key);
  return isUuidV4(memoryValue) ? memoryValue : null;
}

function writePendingResetCreditOperationId(
  accountId: string,
  serviceAddr: unknown,
  operationId: string,
): void {
  const key = resetCreditOperationStorageKey(accountId, serviceAddr);
  pendingResetCreditOperationIds.set(key, operationId);
  writeLocalStorageValue(key, operationId);
}

function clearPendingResetCreditOperationId(
  accountId: string,
  serviceAddr: unknown = "",
): void {
  const key = resetCreditOperationStorageKey(accountId, serviceAddr);
  pendingResetCreditOperationIds.delete(key);
  removeLocalStorageValue(key);
}

function replacePendingResetCreditOperationIdIfCurrent(
  accountId: string,
  serviceAddr: unknown,
  expectedOperationId: string,
  nextOperationId: string,
): void {
  if (
    readPendingResetCreditOperationId(accountId, serviceAddr) !==
    expectedOperationId
  ) {
    return;
  }
  writePendingResetCreditOperationId(accountId, serviceAddr, nextOperationId);
}

function clearPendingResetCreditOperationIdIfCurrent(
  accountId: string,
  serviceAddr: unknown,
  expectedOperationId: string,
): void {
  if (
    readPendingResetCreditOperationId(accountId, serviceAddr) !==
    expectedOperationId
  ) {
    return;
  }
  clearPendingResetCreditOperationId(accountId, serviceAddr);
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  try {
    return JSON.stringify(error) ?? String(error);
  } catch {
    return String(error);
  }
}

function resetCreditProtocolText(error: unknown): string {
  const message = errorText(error).trim();
  const webRpcPrefix = message.match(/^RPC 请求失败（HTTP \d{3}）：\s*/);
  return webRpcPrefix ? message.slice(webRpcPrefix[0].length) : message;
}

function recoverPendingResetCreditOperationId(error: unknown): string | null {
  const protocolText = resetCreditProtocolText(error);
  if (!protocolText.startsWith(RESET_CREDIT_PENDING_OPERATION_PREFIX)) {
    return null;
  }
  const match = protocolText
    .slice(RESET_CREDIT_PENDING_OPERATION_PREFIX.length)
    .match(
      /^([0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})/i,
    );
  return isUuidV4(match?.[1]) ? match[1] : null;
}

function readTerminalResetCreditFailure(error: unknown): string | null {
  const protocolText = resetCreditProtocolText(error);
  if (!protocolText.startsWith(RESET_CREDIT_TERMINAL_FAILURE_PREFIX)) {
    return null;
  }
  return (
    protocolText
      .slice(RESET_CREDIT_TERMINAL_FAILURE_PREFIX.length)
      .trim() || "reset credit operation failed"
  );
}

function createResetCreditOperationId(): string {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  if (typeof cryptoApi?.getRandomValues !== "function") {
    throw new Error("secure random values are unavailable");
  }
  const bytes = cryptoApi.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return [
    hex.slice(0, 4).join(""),
    hex.slice(4, 6).join(""),
    hex.slice(6, 8).join(""),
    hex.slice(8, 10).join(""),
    hex.slice(10).join(""),
  ].join("-");
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function optionalNumber(value: unknown): number | null {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) ? number : null;
}

function normalizeResetCredit(value: unknown): ResetCredit {
  const source = asRecord(value);
  return {
    id: optionalString(source.id),
    status: optionalString(source.status),
    resetType: optionalString(source.resetType ?? source.reset_type),
    grantedAt: optionalNumber(source.grantedAt ?? source.granted_at),
    expiresAt: optionalNumber(source.expiresAt ?? source.expires_at),
    redeemedAt: optionalNumber(source.redeemedAt ?? source.redeemed_at),
    rawStatus: optionalString(source.rawStatus ?? source.raw_status),
  };
}

function normalizeResetCreditsSnapshot(value: unknown): ResetCreditsSnapshot {
  const source = asRecord(value);
  const credits = Array.isArray(source.credits)
    ? source.credits.map(normalizeResetCredit)
    : [];
  return {
    availableCount: optionalNumber(
      source.availableCount ?? source.available_count,
    ),
    credits,
    nextExpiresAt: optionalNumber(
      source.nextExpiresAt ?? source.next_expires_at,
    ),
  };
}

function normalizeConsumeResult(value: unknown): ResetCreditConsumeResult {
  const source = asRecord(value);
  return {
    consumed: source.consumed === true,
    usageRefreshed:
      source.usageRefreshed === true || source.usage_refreshed === true,
    snapshot:
      source.snapshot == null
        ? null
        : normalizeResetCreditsSnapshot(source.snapshot),
    warning: optionalString(source.warning),
  };
}

export const resetCreditClient = {
  async get(accountId: string): Promise<ResetCreditsSnapshot> {
    const result = await invoke<unknown>(
      "service_usage_reset_credits",
      withAddr({ accountId }),
    );
    return normalizeResetCreditsSnapshot(result);
  },

  async consume(accountId: string): Promise<ResetCreditConsumeResult> {
    const requestParams = withAddr({ accountId });
    const serviceAddr =
      typeof requestParams.addr === "string" ? requestParams.addr : "";
    const pendingOperationId = readPendingResetCreditOperationId(
      accountId,
      serviceAddr,
    );
    const operationId = pendingOperationId || createResetCreditOperationId();
    // Keep the id until a terminal service response is received. A user retry
    // after an unknown response must address the same durable operation.
    writePendingResetCreditOperationId(accountId, serviceAddr, operationId);
    try {
      const result = await invoke<unknown>(
        "service_usage_reset_credit_consume",
        { ...requestParams, operationId },
        {
          // A redemption is not safe to replay after a timeout because the first request may
          // already have consumed a credit. Allow the service transaction to finish instead.
          retries: 0,
          timeoutMs: RESET_CREDIT_CONSUME_TIMEOUT_MS,
        },
      );
      const normalized = normalizeConsumeResult(result);
      clearPendingResetCreditOperationIdIfCurrent(
        accountId,
        serviceAddr,
        operationId,
      );
      return normalized;
    } catch (error) {
      const terminalFailure = readTerminalResetCreditFailure(error);
      if (terminalFailure) {
        clearPendingResetCreditOperationIdIfCurrent(
          accountId,
          serviceAddr,
          operationId,
        );
        throw new Error(terminalFailure);
      } else {
        const recoveredOperationId = recoverPendingResetCreditOperationId(error);
        if (recoveredOperationId) {
          replacePendingResetCreditOperationIdIfCurrent(
            accountId,
            serviceAddr,
            operationId,
            recoveredOperationId,
          );
        }
      }
      throw error;
    }
  },
};

export {
  createResetCreditOperationId,
  readPendingResetCreditOperationId,
  clearPendingResetCreditOperationId,
  normalizeConsumeResult as normalizeResetCreditConsumeResult,
  normalizeResetCreditsSnapshot,
};
