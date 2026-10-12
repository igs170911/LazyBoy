import { toast } from "./ui/overlay";

const ENTRY = /src="(\/assets\/index-[^"]+\.js)"/;

/**
 * A home-screen app resumes from memory instead of reloading, so it can keep running an old build for days.
 * Each time it comes back to the foreground, compare the served page's entry bundle with the running one and offer
 * to reload. Never reloads by itself: an unsent message lives only in memory.
 */
export function watchForUpdates(): () => void {
  const running = document.querySelector<HTMLScriptElement>('script[type="module"][src*="/assets/index-"]')?.getAttribute("src");
  if (!running) return () => undefined; // dev server
  let offered = false;
  let last = 0;
  const check = async () => {
    if (document.hidden || offered || Date.now() - last < 30_000) return;
    last = Date.now();
    try {
      const html = await (await fetch("/", { cache: "no-store" })).text();
      const served = ENTRY.exec(html)?.[1];
      if (served && served !== running) {
        offered = true;
        toast("LazyBoy 有新版本", "info", { label: "更新", run: () => location.reload() }, 10 * 60_000);
      }
    } catch { /* offline: try again next time */ }
  };
  document.addEventListener("visibilitychange", check);
  window.addEventListener("focus", check);
  return () => {
    document.removeEventListener("visibilitychange", check);
    window.removeEventListener("focus", check);
  };
}
