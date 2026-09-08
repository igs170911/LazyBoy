// Live session feed. The API keeps a durable, ordered event cursor per session
// and streams it as server-sent events, so the browser does not have to guess
// when the transcript changed. This module owns only the transport half:
// subscribe, decode, drop replays, and collapse a burst into one refresh.

export type SessionEventKind =
  | "message.created"
  | "run.started"
  | "run.paused"
  | "run.failed"
  | "run.completed"
  | "session.cleared";

/**
 * Kinds the app listens for. An SSE source cannot subscribe to "everything", so
 * a kind added on the server needs to appear here to be delivered live; the
 * caller's safety poll still picks anything new up within a few seconds.
 */
export const SESSION_EVENT_TYPES: SessionEventKind[] = [
  "message.created",
  "run.started",
  "run.paused",
  "run.failed",
  "run.completed",
  "session.cleared",
];

export interface SessionEvent {
  kind: string;
  /** Server cursor (`events.seq`); 0 when the source sent no id. */
  id: number;
  payload: Record<string, unknown>;
}

/** The slice of `EventSource` this module uses, so tests can fake the socket. */
export interface EventSourceLike {
  addEventListener(type: string, listener: (event: { data?: string; lastEventId?: string }) => void): void;
  close(): void;
}

export interface LiveFeed {
  /** Stop listening. Safe to call more than once. */
  close(): void;
}

export function sessionEventsUrl(sessionId: string): string {
  return `/api/sessions/${encodeURIComponent(sessionId)}/events`;
}

/**
 * Open the event stream for one session. `onStatus` reports whether the live
 * path is up, which lets the caller decide how often it needs to poll as a
 * backup; it stays silent about the browser's own reconnect attempts.
 */
export function subscribeToSession(
  sessionId: string,
  onEvent: (event: SessionEvent) => void,
  options: {
    source?: (url: string) => EventSourceLike;
    onStatus?: (connected: boolean) => void;
  } = {},
): LiveFeed {
  const open = options.source ?? ((url: string) => new EventSource(url) as unknown as EventSourceLike);
  const source = open(sessionEventsUrl(sessionId));
  // A reconnect replays from the last id the browser saw, and the server replays
  // from its cursor, so the same event can legitimately arrive twice. Applying
  // it twice is only harmless work, but the cursor is free and keeps the caller
  // honest about doing real work once per event.
  let cursor = 0;
  const receive = (kind: string) => (event: { data?: string; lastEventId?: string }) => {
    const id = Number(event.lastEventId);
    if (Number.isFinite(id) && id > 0) {
      if (id <= cursor) return;
      cursor = id;
    }
    let payload: Record<string, unknown> = {};
    try {
      const parsed: unknown = event.data ? JSON.parse(event.data) : {};
      if (parsed && typeof parsed === "object") payload = parsed as Record<string, unknown>;
    } catch {
      // A malformed frame is a dropped frame: the next event, or the poll,
      // delivers the same state.
    }
    onEvent({ kind, id: Number.isFinite(id) ? id : 0, payload });
  };
  for (const kind of SESSION_EVENT_TYPES) source.addEventListener(kind, receive(kind));
  source.addEventListener("open", () => options.onStatus?.(true));
  source.addEventListener("error", () => options.onStatus?.(false));
  let closed = false;
  return {
    close() {
      if (closed) return;
      closed = true;
      source.close();
    },
  };
}

export interface Coalescer {
  /** Ask for a run; a request inside an open window joins the pending one. */
  kick(): void;
  /** Forget a pending run, e.g. because the session was switched away from. */
  cancel(): void;
}

/**
 * Collapse a burst of "something changed" into a single run. One turn can move
 * several events at once, and each of them would otherwise fetch the whole
 * transcript again.
 */
export function createCoalescer(
  run: () => void,
  windowMs: number,
  schedule: (callback: () => void, ms: number) => number = (callback, ms) => setTimeout(callback, ms) as unknown as number,
  dismiss: (handle: number) => void = handle => clearTimeout(handle),
): Coalescer {
  let handle: number | null = null;
  return {
    kick() {
      if (handle !== null) return;
      handle = schedule(() => {
        handle = null;
        run();
      }, windowMs);
    },
    cancel() {
      if (handle === null) return;
      dismiss(handle);
      handle = null;
    },
  };
}
