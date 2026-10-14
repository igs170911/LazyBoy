/** Chrome and Edge offer installing the app once, early, through `beforeinstallprompt`; keep it for the settings page. */
type InstallPrompt = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: "accepted" | "dismissed" }> };

let pending: InstallPrompt | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach(listener => listener());

window.addEventListener("beforeinstallprompt", event => { event.preventDefault(); pending = event as InstallPrompt; changed(); });
window.addEventListener("appinstalled", () => { pending = null; changed(); });

export function installAvailable() { return pending !== null; }

export function onInstallChange(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }

/** Shows the browser's install dialog; true when the person installed the app. */
export async function installApp() {
  const prompt = pending;
  if (!prompt) return false;
  pending = null; changed();
  await prompt.prompt();
  return (await prompt.userChoice).outcome === "accepted";
}

export function runningAsApp() {
  return matchMedia("(display-mode: standalone)").matches || (navigator as Navigator & { standalone?: boolean }).standalone === true;
}
