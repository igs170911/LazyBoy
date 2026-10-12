import { useLayoutEffect, useRef, type RefObject } from "react";

const dialogs: HTMLElement[] = [];
const selector = 'button, a[href], input, select, textarea, iframe, [tabindex]';
function controls(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(selector)].filter(element =>
    element.tabIndex >= 0 && !element.matches(':disabled') &&
    element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden"
  );
}

/** Only the top dialog owns focus. Nested previews restore their parent. */
export function useDialogFocus(ref: RefObject<HTMLElement | null>, onEscape?: () => void, active = true) {
  const previous = useRef(document.activeElement as HTMLElement | null);
  const wasActive = useRef(false);
  if (active && !wasActive.current) previous.current = document.activeElement as HTMLElement | null;
  wasActive.current = active;
  const escape = useRef(onEscape);
  escape.current = onEscape;
  useLayoutEffect(() => {
    const root = ref.current;
    if (!root || !active) return;
    dialogs.push(root);
    const isTop = () => dialogs.filter(element => element.isConnected).at(-1) === root;
    const focusFirst = () => (controls(root)[0] || root).focus({ preventScroll: true });
    const tabIndex = root.getAttribute("tabindex");
    if (tabIndex === null) root.tabIndex = -1;
    if (!root.contains(document.activeElement)) focusFirst();
    const onFocus = (event: FocusEvent) => {
      if (isTop() && !root.contains(event.target as Node)) focusFirst();
    };
    const onKey = (event: KeyboardEvent) => {
      if (!isTop() || event.defaultPrevented) return;
      if (event.key === "Escape" && escape.current) {
        event.preventDefault();
        event.stopPropagation();
        escape.current();
      }
      if (event.key !== "Tab") return;
      const targets = controls(root);
      const index = targets.indexOf(document.activeElement as HTMLElement);
      if (!targets.length || index < 0 || (event.shiftKey ? index === 0 : index === targets.length - 1)) {
        event.preventDefault();
        (targets[event.shiftKey ? targets.length - 1 : 0] || root).focus();
      }
    };
    document.addEventListener("focusin", onFocus);
    document.addEventListener("keydown", onKey, true);
    return () => {
      dialogs.splice(dialogs.indexOf(root), 1);
      document.removeEventListener("focusin", onFocus);
      document.removeEventListener("keydown", onKey, true);
      if (tabIndex === null) root.removeAttribute("tabindex");
      if (previous.current?.isConnected) previous.current.focus({ preventScroll: true });
    };
  }, [ref, active]);
}
