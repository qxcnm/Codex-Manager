export class TimeoutError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TimeoutError";
  }
}

export function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message = `Operation timed out after ${timeoutMs}ms`,
): Promise<T> {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    return promise;
  }

  let timeoutId: ReturnType<typeof globalThis.setTimeout> | null = null;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timeoutId = globalThis.setTimeout(() => {
      reject(new TimeoutError(message));
    }, timeoutMs);
  });

  return Promise.race([promise, timeoutPromise]).finally(() => {
    if (timeoutId !== null) {
      globalThis.clearTimeout(timeoutId);
    }
  });
}
