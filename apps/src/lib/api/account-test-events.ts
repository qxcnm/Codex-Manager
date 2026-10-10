import { isTauriRuntime } from "./transport";

export const ACCOUNT_TEST_EVENT = "account-test-event";

export interface AccountTestEventPayload {
  testId?: string;
  type?: string;
  text?: string;
  model?: string;
  status?: string;
  imageUrl?: string;
  mimeType?: string;
  success?: boolean;
  error?: string;
}

export type AccountTestEventHandler = (payload: AccountTestEventPayload) => void;

type Unlisten = () => void;

export interface ListenAccountTestEventOptions {
  signal?: AbortSignal;
}

const ACCOUNT_TEST_EVENT_OPEN_TIMEOUT_MS = 5_000;

function createAbortError(): Error {
  if (typeof DOMException !== "undefined") {
    return new DOMException("The account test event listener was aborted", "AbortError");
  }
  const error = new Error("The account test event listener was aborted");
  error.name = "AbortError";
  return error;
}

function readAccountTestEventPayload(event: Event): AccountTestEventPayload {
  if (event instanceof CustomEvent && typeof event.detail === "object" && event.detail) {
    return event.detail as AccountTestEventPayload;
  }
  return {};
}

function readAccountTestMessagePayload(event: MessageEvent): AccountTestEventPayload {
  if (typeof event.data !== "string" || !event.data.trim()) {
    return {};
  }
  try {
    const payload = JSON.parse(event.data);
    return typeof payload === "object" && payload
      ? (payload as AccountTestEventPayload)
      : {};
  } catch {
    return {};
  }
}

export async function listenAccountTestEvent(
  testId: string,
  handler: AccountTestEventHandler,
  options: ListenAccountTestEventOptions = {},
): Promise<Unlisten> {
  const signal = options.signal;
  if (signal?.aborted) {
    throw createAbortError();
  }
  if (typeof window === "undefined") {
    return () => {};
  }

  let disposed = false;
  const emit = (payload: AccountTestEventPayload) => {
    if (!disposed && !signal?.aborted) handler(payload);
  };
  const handleWindowEvent = (event: Event) => {
    emit(readAccountTestEventPayload(event));
  };
  window.addEventListener(ACCOUNT_TEST_EVENT, handleWindowEvent);

  let eventSource: EventSource | null = null;
  let handleEventSourceEvent: ((event: MessageEvent) => void) | null = null;
  let unlistenTauri: Unlisten | null = null;
  let removeAbortListener: (() => void) | null = null;
  const cleanup = () => {
    if (disposed) return;
    disposed = true;
    removeAbortListener?.();
    removeAbortListener = null;
    window.removeEventListener(ACCOUNT_TEST_EVENT, handleWindowEvent);
    if (eventSource && handleEventSourceEvent) {
      eventSource.removeEventListener(
        ACCOUNT_TEST_EVENT,
        handleEventSourceEvent as EventListener
      );
    }
    eventSource?.close();
    unlistenTauri?.();
  };

  try {
    if (
      !isTauriRuntime() &&
      typeof EventSource !== "undefined" &&
      window.location.protocol.startsWith("http")
    ) {
      const normalizedTestId = testId.trim();
      if (!normalizedTestId) {
        throw new Error("Missing account test ID");
      }
      eventSource = new EventSource(
        `/api/events/account-test?testId=${encodeURIComponent(normalizedTestId)}`,
      );
      handleEventSourceEvent = (event: MessageEvent) => {
        emit(readAccountTestMessagePayload(event));
      };
      eventSource.addEventListener(
        ACCOUNT_TEST_EVENT,
        handleEventSourceEvent as EventListener,
      );

      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const source = eventSource;
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          window.clearTimeout(timeoutId);
          removeAbortListener?.();
          removeAbortListener = null;
          source?.removeEventListener("open", handleOpen);
          source?.removeEventListener("error", handleInitialError);
          if (error) reject(error);
          else resolve();
        };
        const timeoutId = window.setTimeout(
          () => finish(new Error("Timed out connecting to account test events")),
          ACCOUNT_TEST_EVENT_OPEN_TIMEOUT_MS,
        );
        const handleOpen = () => finish();
        const handleInitialError = () =>
          finish(new Error("Failed to connect to account test events"));
        const handleAbort = () => {
          source?.close();
          finish(createAbortError());
        };
        if (signal) {
          if (signal.aborted) {
            handleAbort();
            return;
          }
          signal.addEventListener("abort", handleAbort, { once: true });
          removeAbortListener = () =>
            signal.removeEventListener("abort", handleAbort);
        }
        source?.addEventListener("open", handleOpen);
        source?.addEventListener("error", handleInitialError);
        // The connection may have opened between construction and listener registration.
        // Re-check after listeners are attached so the RPC never starts before the SSE channel.
        if (source?.readyState === 1) {
          finish();
        }
      });
    }

    if (signal?.aborted) {
      throw createAbortError();
    }

    if (isTauriRuntime()) {
      const { listen } = await import("@tauri-apps/api/event");
      unlistenTauri = await listen<AccountTestEventPayload>(
        ACCOUNT_TEST_EVENT,
        (event) => {
          emit(event.payload || {});
        },
      );
      if (signal?.aborted) {
        throw createAbortError();
      }
    }

    if (signal) {
      const handleAbort = () => cleanup();
      signal.addEventListener("abort", handleAbort, { once: true });
      removeAbortListener = () =>
        signal.removeEventListener("abort", handleAbort);
      // AbortSignal does not replay an abort event for listeners added after
      // abort(), so close the just-created subscription if the signal raced
      // with listener registration.
      if (signal.aborted) {
        cleanup();
        throw createAbortError();
      }
    }

    return cleanup;
  } catch (error) {
    cleanup();
    throw error;
  }
}
