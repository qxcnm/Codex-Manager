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

const ACCOUNT_TEST_EVENT_OPEN_TIMEOUT_MS = 5_000;

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
  handler: AccountTestEventHandler
): Promise<Unlisten> {
  if (typeof window === "undefined") {
    return () => {};
  }

  const handleWindowEvent = (event: Event) => {
    handler(readAccountTestEventPayload(event));
  };
  window.addEventListener(ACCOUNT_TEST_EVENT, handleWindowEvent);

  let eventSource: EventSource | null = null;
  let handleEventSourceEvent: ((event: MessageEvent) => void) | null = null;
  let unlistenTauri: Unlisten | null = null;
  const cleanup = () => {
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
        handler(readAccountTestMessagePayload(event));
      };
      eventSource.addEventListener(
        ACCOUNT_TEST_EVENT,
        handleEventSourceEvent as EventListener,
      );

      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const source = eventSource;
        let timeoutId: number | undefined;
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          if (timeoutId !== undefined) {
            window.clearTimeout(timeoutId);
          }
          source?.removeEventListener("open", handleOpen);
          source?.removeEventListener("error", handleInitialError);
          if (error) reject(error);
          else resolve();
        };
        const handleOpen = () => finish();
        const handleInitialError = () =>
          finish(new Error("Failed to connect to account test events"));
        source?.addEventListener("open", handleOpen);
        source?.addEventListener("error", handleInitialError);
        timeoutId = window.setTimeout(
          () => finish(new Error("Timed out connecting to account test events")),
          ACCOUNT_TEST_EVENT_OPEN_TIMEOUT_MS,
        );
        // The connection may have opened between construction and listener registration.
        // Re-check after listeners are attached so the RPC never starts before the SSE channel.
        if (source?.readyState === 1) {
          finish();
        }
      });
    }

    if (isTauriRuntime()) {
      const { listen } = await import("@tauri-apps/api/event");
      unlistenTauri = await listen<AccountTestEventPayload>(
        ACCOUNT_TEST_EVENT,
        (event) => {
          handler(event.payload || {});
        },
      );
    }

    return cleanup;
  } catch (error) {
    cleanup();
    throw error;
  }
}
