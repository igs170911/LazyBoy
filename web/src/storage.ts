// Preferences must never prevent opening a conversation. Keep failed writes for
// this tab when storage is blocked/full; do not persist chat or secret contents.
const fallback = new Map<string, string | null>();

export function readStorage(key: string): string | null {
  if (fallback.has(key)) return fallback.get(key) ?? null;
  try { return window.localStorage.getItem(key); }
  catch { return null; }
}

export function writeStorage(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
    fallback.delete(key);
  } catch { fallback.set(key, value); }
}

export function removeStorage(key: string): void {
  try {
    window.localStorage.removeItem(key);
    fallback.delete(key);
  } catch { fallback.set(key, null); }
}
