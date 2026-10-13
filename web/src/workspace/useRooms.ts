import { useCallback, useEffect, useState } from "react";
import { api, type RoomRow } from "../api";

/** Group chats for the sidebar: loaded once, then kept fresh while the page is visible. */
export function useRooms() {
  const [rooms, setRooms] = useState<RoomRow[]>([]);
  const refresh = useCallback(async () => {
    try { setRooms((await api.rooms()).rooms); } catch { /* keep the last listing */ }
  }, []);
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, 5000);
    const visible = () => { if (!document.hidden) void refresh(); };
    document.addEventListener("visibilitychange", visible);
    return () => { window.clearInterval(timer); document.removeEventListener("visibilitychange", visible); };
  }, [refresh]);
  return { rooms, refresh, setRooms };
}
