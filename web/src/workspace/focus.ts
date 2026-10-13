import { useEffect, type MutableRefObject, type RefObject } from "react";

/** A message to bring into view once its conversation is on screen (from search); `nonce` repeats a jump. */
export type FocusTarget = { id: string; nonce: number };

const GIVE_UP_MS = 6000;

/**
 * Scroll a searched message into the middle of the conversation and flash it. Waits for the transcript to load;
 * a message that never shows up (hidden or folded into a group) is given up on quietly.
 */
export function useFocusMessage(scroller: RefObject<HTMLElement | null>, focus: FocusTarget | null | undefined, ready: unknown,
  pinned?: MutableRefObject<boolean>, onDone?: () => void) {
  useEffect(() => {
    if (!focus) return;
    const target = scroller.current?.querySelector<HTMLElement>(`[data-msg="${CSS.escape(focus.id)}"]`);
    if (!target) {
      const timer = window.setTimeout(() => onDone?.(), GIVE_UP_MS);
      return () => window.clearTimeout(timer);
    }
    if (pinned) pinned.current = false;
    target.scrollIntoView({ block: "center" });
    target.classList.remove("msg-flash");
    void target.offsetWidth;
    target.classList.add("msg-flash");
    // Not tied to this effect: clearing the focus right below must not cut the flash short.
    window.setTimeout(() => target.classList.remove("msg-flash"), 2400);
    onDone?.();
    return undefined;
  }, [focus?.nonce, ready]); // eslint-disable-line react-hooks/exhaustive-deps
}
