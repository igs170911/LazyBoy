/** Cancel both the request and body read, without requiring new AbortSignal APIs. */
export async function withRequestTimeout<T>(
  parent: AbortSignal | null | undefined,
  timeout: number,
  run: (signal: AbortSignal) => Promise<T>,
): Promise<T> {
  const controller = new AbortController();
  const abort = () => controller.abort(parent?.reason);
  if (parent?.aborted) abort();
  else parent?.addEventListener("abort", abort, { once: true });
  const timer = window.setTimeout(() => {
    controller.abort(new DOMException("連線逾時，請重試", "TimeoutError"));
  }, timeout);
  try { return await run(controller.signal); }
  finally {
    window.clearTimeout(timer);
    parent?.removeEventListener("abort", abort);
  }
}
