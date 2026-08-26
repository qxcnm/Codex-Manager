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
  if (
    !isTauriRuntime() &&
    typeof EventSource !== "undefined" &&
    window.location.protocol.startsWith("http")
  ) {
    eventSource = new EventSource("/api/events/account-test");
    handleEventSourceEvent = (event: MessageEvent) => {
      handler(readAccountTestMessagePayload(event));
    };
    eventSource.addEventListener(
      ACCOUNT_TEST_EVENT,
      handleEventSourceEvent as EventListener
    );
  }

  let unlistenTauri: Unlisten | null = null;
  if (isTauriRuntime()) {
    const { listen } = await import("@tauri-apps/api/event");
    unlistenTauri = await listen<AccountTestEventPayload>(
      ACCOUNT_TEST_EVENT,
      (event) => {
        handler(event.payload || {});
      },
    );
  }

  return () => {
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
}
