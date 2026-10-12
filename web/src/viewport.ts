/**
 * Fit the app above the on-screen keyboard, while allowing browser pinch zoom.
 * With the keyboard closed the app keeps the full-screen CSS height: a home-screen web app on iOS can report a
 * visual viewport that is shorter than the screen, which would leave an empty band under the composer.
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
        root.dataset.keyboard = "";
      } else {
        root.style.removeProperty("--app-height");
        delete root.dataset.keyboard;
      }
    });
  };
  update();
  viewport?.addEventListener("resize", update);
  window.addEventListener("resize", update);
  return () => {
    cancelAnimationFrame(frame);
    viewport?.removeEventListener("resize", update);
    window.removeEventListener("resize", update);
    root.style.removeProperty("--app-height");
    delete root.dataset.keyboard;
  };
}
