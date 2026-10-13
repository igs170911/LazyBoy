import { readStorage, writeStorage } from "./storage";

const ON_KEY = "lazyhermes.notify";
const MUTED_KEY = "lazyhermes.notify.muted";
export const NOTIFY_EVENT = "lazyhermes:notify-changed";

export const notifySupported = () => typeof window !== "undefined" && "Notification" in window;
export const notifyOn = () => readStorage(ON_KEY) === "on" && notifySupported() && Notification.permission === "granted";

/** Turn system notifications on (asking the browser for permission) or off. Returns whether they are on. */
export async function setNotify(on: boolean): Promise<boolean> {
  if (on && notifySupported() && Notification.permission !== "granted") {
    try { await Notification.requestPermission(); } catch { /* denied or unsupported */ }
  }
  writeStorage(ON_KEY, on ? "on" : "off");
  window.dispatchEvent(new Event(NOTIFY_EVENT));
  return notifyOn();
}

function muted(): string[] {
  try { const value = JSON.parse(readStorage(MUTED_KEY) || "[]"); return Array.isArray(value) ? value : []; } catch { return []; }
}
export const isMuted = (id: string) => muted().includes(id);
export function setMuted(id: string, mute: boolean) {
  writeStorage(MUTED_KEY, JSON.stringify(mute ? [...new Set([...muted(), id])] : muted().filter(item => item !== id)));
  window.dispatchEvent(new Event(NOTIFY_EVENT));
}

/** A system notification that opens the bot when clicked (through the service worker when there is one). */
export async function showSystem(title: string, body: string, agentId: string, icon?: string) {
  const options: NotificationOptions = { body, tag: `bot-${agentId}`, icon: icon || "/icon-192.png", badge: "/icon-192.png", data: { agentId } };
  try {
    const registration = await navigator.serviceWorker?.getRegistration();
    if (registration) { await registration.showNotification(title, options); return; }
  } catch { /* fall back to a page notification */ }
  const note = new Notification(title, options);
  note.onclick = () => { window.focus(); window.dispatchEvent(new CustomEvent("lazyhermes:open-bot", { detail: agentId })); note.close(); };
}
