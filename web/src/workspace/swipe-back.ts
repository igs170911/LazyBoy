import { useEffect, type RefObject } from "react";

const EDGE = 28;

/** A home-screen web app on iOS has no system back swipe, so the chat page provides its own from the left edge. */
function swipeBackSupported() {
  const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  const standalone = window.matchMedia("(display-mode: standalone)").matches || (navigator as Navigator & { standalone?: boolean }).standalone === true;
  return ios && standalone;
}

/** Drag the chat page off to the right with the finger, revealing the bot list underneath; past a third, it pops. */
export function useSwipeBack(app: RefObject<HTMLElement | null>, enabled: boolean, onBack: () => void) {
  useEffect(() => {
    const root = app.current;
    if (!enabled || !root || !swipeBackSupported()) return;
    let start: { x: number; y: number; t: number } | null = null;
    let tracking: boolean | null = null;
    let dx = 0;
    let cleanup = 0;
    const pages = () => [root.querySelector<HTMLElement>(":scope > .chat"), root.querySelector<HTMLElement>(":scope > .sidebar")] as const;
    const reset = () => {
      for (const el of pages()) if (el) { el.style.transition = ""; el.style.transform = ""; el.style.filter = ""; }
    };
    const down = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (event.touches.length !== 1 || touch.clientX > EDGE) return;
      window.clearTimeout(cleanup);
      start = { x: touch.clientX, y: touch.clientY, t: performance.now() };
      tracking = null;
      dx = 0;
    };
    const move = (event: TouchEvent) => {
      if (!start || tracking === false) return;
      const touch = event.touches[0];
      dx = touch.clientX - start.x;
      const dy = touch.clientY - start.y;
      if (tracking === null) {
        if (Math.abs(dx) > 8 && dx > Math.abs(dy)) tracking = true;
        else if (Math.abs(dy) > 8 || dx < -8) { tracking = false; return; }
        else return;
      }
      event.preventDefault();
      const [chat, list] = pages();
      const width = root.clientWidth || 1;
      const progress = Math.min(1, Math.max(0, dx / width));
      if (chat) { chat.style.transition = "none"; chat.style.transform = `translateX(${progress * width}px)`; }
      if (list) { list.style.transition = "none"; list.style.transform = `translateX(${-30 + 30 * progress}%)`; list.style.filter = `brightness(${0.7 + 0.3 * progress})`; }
    };
    const up = () => {
      if (!start) return;
      const elapsed = Math.max(1, performance.now() - start.t);
      const wasTracking = tracking;
      start = null;
      tracking = null;
      if (!wasTracking) return;
      const width = root.clientWidth || 1;
      const [chat, list] = pages();
      if (dx > width * 0.33 || (dx > 40 && dx / elapsed > 0.45)) {
        // Finish the slide from where the finger left it, then hand the pages back to the view state.
        if (chat) { chat.style.transition = ""; chat.style.transform = "translateX(100%)"; }
        if (list) { list.style.transition = ""; list.style.transform = "translateX(0)"; list.style.filter = "none"; }
        onBack();
        cleanup = window.setTimeout(reset, 450);
      } else reset();
    };
    root.addEventListener("touchstart", down, { passive: true });
    root.addEventListener("touchmove", move, { passive: false });
    root.addEventListener("touchend", up);
    root.addEventListener("touchcancel", up);
    return () => {
      window.clearTimeout(cleanup);
      reset();
      root.removeEventListener("touchstart", down);
      root.removeEventListener("touchmove", move);
      root.removeEventListener("touchend", up);
      root.removeEventListener("touchcancel", up);
    };
  }, [app, enabled, onBack]);
}
