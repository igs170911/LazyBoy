import { useCallback, useEffect, useState } from "react";
import { readStorage, writeStorage } from "../storage";

const SEEN_KEY = "lazyhermes.seen";

function read(): Record<string, string> {
  try { const value = JSON.parse(readStorage(SEEN_KEY) || "{}"); return value && typeof value === "object" ? value : {}; }
  catch { return {}; }
}

/**
 * Unread activity, per bot or group chat (Grok's "Unread activity" dot): newer than the last time it was on screen.
 * Something seen for the first time counts as read, so a fresh device does not light up every row.
 */
export function useSeen(items: { id: string; last_at?: string | null }[], openId: string | null) {
  const [seen, setSeen] = useState<Record<string, string>>(read);
  useEffect(() => {
    setSeen(current => {
      let next = current;
      for (const item of items) {
        const at = item.last_at || "";
        const first = !(item.id in current);
        const viewing = item.id === openId && !document.hidden && at > (current[item.id] || "");
        if (first || viewing) { if (next === current) next = { ...current }; next[item.id] = at; }
      }
      if (next !== current) writeStorage(SEEN_KEY, JSON.stringify(next));
      return next;
    });
  }, [items, openId]);
  const isUnread = useCallback((id: string, at?: string | null) => !!at && id !== openId && at > (seen[id] || ""), [seen, openId]);
  /** Grok's "Mark as Unread": the dot comes back until the conversation is opened again. */
  const markUnread = useCallback((id: string) => setSeen(current => { const next = { ...current, [id]: "" }; writeStorage(SEEN_KEY, JSON.stringify(next)); return next; }), []);
  return { isUnread, markUnread };
}
