/**
 * Keep the app on the visible part of the screen while the on-screen keyboard is open, the way a native chat does:
 * the header stays put, the conversation shortens, and the composer rides on top of the keyboard.
 *
 * iOS answers a focused field by scrolling the whole page up under the keyboard. The app is therefore pinned to the
 * visual viewport (its height and its offset from the top) instead of the page, so that scroll moves nothing.
 * With the keyboard closed the app keeps the full-screen CSS height; pinch zoom (scale ≠ 1) is left alone.
 */
export function watchViewport(): () => void {
  const viewport = window.visualViewport;
  const root = document.documentElement;
  let frame = 0;
  const update = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const keyboard = !!viewport && viewport.scale === 1 && viewport.height < root.clientHeight - 120;
      if (keyboard) {
        root.style.setProperty("--app-height", `${Math.round(viewport.height)}px`);
        root.style.setProperty("--app-top", `${Math.round(viewport.offsetTop)}px`);
        root.dataset.keyboard = "";
      } else {
        root.style.removeProperty("--app-height");
        root.style.removeProperty("--app-top");
        delete root.dataset.keyboard;
        // iOS can leave the page scrolled after the keyboard closes; the app has no page scroll of its own.
        if (window.scrollY) window.scrollTo(0, 0);
      }
    });
  };
  update();
  viewport?.addEventListener("resize", update);
  viewport?.addEventListener("scroll", update);
  window.addEventListener("resize", update);
  return () => {
    cancelAnimationFrame(frame);
    viewport?.removeEventListener("resize", update);
    viewport?.removeEventListener("scroll", update);
    window.removeEventListener("resize", update);
    root.style.removeProperty("--app-height");
    root.style.removeProperty("--app-top");
    delete root.dataset.keyboard;
  };
}
