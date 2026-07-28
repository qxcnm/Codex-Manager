import { isTauri } from "@tauri-apps/api/core";

const MAX_LOG_TEXT_LENGTH = 2_000;
const SAFE_EVENT_PATTERN = /[^a-z0-9_.-]+/gi;

function sanitizeLogText(value: string): string {
  return value
    .replace(/[\r\n\t]+/g, " ")
    .replace(/\bBearer\s+[^\s,;]+/gi, "Bearer <redacted>")
    .replace(/\bsk-[a-z0-9_-]{8,}\b/gi, "<redacted-api-key>")
    .replace(
      /("(?:access_token|refresh_token|id_token|authorization|cookie)"\s*:\s*")[^"]*"/gi,
      "$1<redacted>\"",
    )
    .replace(/([?&](?:code|state|token|key|secret)=)[^&\s]+/gi, "$1<redacted>")
    .slice(0, MAX_LOG_TEXT_LENGTH);
}

function normalizeEventName(event: string): string {
  const normalized = event.trim().replace(SAFE_EVENT_PATTERN, "_").slice(0, 96);
  return normalized || "unknown";
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return `${error.name}: ${error.message}`;
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

async function writePersistentLog(
  level: "error" | "warn",
  message: string,
): Promise<void> {
  if (!isTauri()) {
    return;
  }
  try {
    const logger = await import("@tauri-apps/plugin-log");
    await logger[level](message);
  } catch {
    // Logging must never throw into the application error path.
  }
}

export function reportClientError(event: string, error: unknown): void {
  const message = `event=frontend_${normalizeEventName(event)} error=${sanitizeLogText(
    errorMessage(error),
  )}`;
  console.error(message);
  void writePersistentLog("error", message);
}

export function reportClientWarning(event: string, error: unknown): void {
  const message = `event=frontend_${normalizeEventName(event)} error=${sanitizeLogText(
    errorMessage(error),
  )}`;
  console.warn(message);
  void writePersistentLog("warn", message);
}
