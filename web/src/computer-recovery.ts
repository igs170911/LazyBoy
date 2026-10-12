import {useCallback, useEffect, useRef, useState} from "react";

const ISSUES = {
  viewerOffline: "網路已離線，恢復後會自動重新連線",
  viewerRetryNeeded: "畫面連不上，請按重新連線",
  viewerReconnecting: "正在重新連線電腦畫面…",
  viewerDisconnected: "畫面連線中斷，正在等待恢復…",
  viewerNoResponse: "電腦畫面沒有回應，正在重新連線…",
} as const;
type MessageKey = keyof typeof ISSUES;

const RETRY_DELAY = 8000;
const RETRY_COOLDOWN = 15000;
const STABLE_CONNECTION = 30000;

/** Give noVNC time to reconnect itself; never race an explicit power operation. */
export function useComputerRecovery(
  agentId: string | null,
  visible: boolean,
  busy: boolean,
  ensure: () => Promise<void>,
  reconnect: () => Promise<void>,
  viewerGeneration: number,
) {
  const [issue, setIssue] = useState<MessageKey | "">("");
  const [connected, setConnected] = useState(false);
  const callbacks = useRef({ensure, reconnect, busy});
  callbacks.current = {ensure, reconnect, busy};
  const manual = useRef<(() => Promise<void>) | null>(null);
  useEffect(() => { setConnected(false); }, [agentId, visible, viewerGeneration, busy]);
  useEffect(() => {
    setIssue("");
    if (!agentId || !visible) return;
    let heartbeat = Date.now(), lastAttempt = -Infinity, attempts = 0, pending = false;
    let state = "connecting", stateSince = Date.now(), stopped = false, wasBusy = false;
    let online = navigator.onLine;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const clearTimer = () => { if (timer) clearTimeout(timer); timer = undefined; };
    const recover = async (explicit = false) => {
      if (stopped || pending || document.hidden || callbacks.current.busy) return;
      if (!online) { setIssue("viewerOffline"); return; }
      if (explicit) { attempts = 0; lastAttempt = -Infinity; }
      if (attempts >= 2) { setIssue("viewerRetryNeeded"); return; }
      if (Date.now() - lastAttempt < RETRY_COOLDOWN) return;
      clearTimer();
      pending = true; attempts++; lastAttempt = Date.now(); heartbeat = Date.now();
      state = "connecting"; stateSince = Date.now();
      setConnected(false);
      setIssue("viewerReconnecting");
      try { await callbacks.current.reconnect(); }
      catch { if (!stopped) setIssue("viewerRetryNeeded"); }
      finally { pending = false; }
    };
    const retryManually = () => recover(true);
    manual.current = retryManually;
    const message = (event: MessageEvent) => {
      if (event.origin !== window.location.origin || event.data?.type !== "lazyboy:viewer-status") return;
      const frame = document.querySelector<HTMLIFrameElement>("iframe.desktop-frame");
      if (!frame || event.source !== frame.contentWindow) return;
      const next = event.data.state;
      if (next !== "connected" && next !== "connecting" && next !== "disconnected" && next !== "disconnecting" && next !== "paused") return;
      heartbeat = Date.now();
      if (next === "paused") {
        // The viewer released the stream while nobody was looking; it reconnects itself when shown.
        state = next; stateSince = Date.now(); clearTimer(); setConnected(false); setIssue(""); return;
      }
      if (state !== next) { state = next; stateSince = Date.now(); }
      if (callbacks.current.busy) { clearTimer(); return; }
      if (!online) { setConnected(false); setIssue("viewerOffline"); return; }
      setConnected(next === "connected");
      if (next === "connected") {
        // A brief successful handshake is not a stable connection.
        if (Date.now() - stateSince >= STABLE_CONNECTION) attempts = 0;
        setIssue(""); clearTimer();
      } else if (next === "disconnected" || next === "disconnecting") {
        setIssue(attempts >= 2 ? "viewerRetryNeeded" : "viewerDisconnected");
        if (!timer && attempts < 2) timer = setTimeout(() => { timer = undefined; void recover(); }, RETRY_DELAY);
      }
    };
    const interval = setInterval(() => {
      if (callbacks.current.busy) {
        wasBusy = true; heartbeat = Date.now(); stateSince = Date.now(); clearTimer(); setIssue(""); return;
      }
      if (wasBusy) {
        wasBusy = false; heartbeat = Date.now(); stateSince = Date.now(); return;
      }
      if (state === "paused" && Date.now() - heartbeat <= STABLE_CONNECTION) return;
      if (state === "connected" && Date.now() - heartbeat <= STABLE_CONNECTION) {
        if (Date.now() - stateSince >= STABLE_CONNECTION) attempts = 0;
        return;
      }
      if (Date.now() - heartbeat > STABLE_CONNECTION || (state !== "connected" && Date.now() - stateSince > 20000)) {
        setConnected(false);
        setIssue(attempts >= 2 ? "viewerRetryNeeded" : "viewerNoResponse");
        void recover();
      }
    }, 5000);
    const onOnline = () => {
      online = true;
      if (state !== "connected" || Date.now() - heartbeat > STABLE_CONNECTION) void recover();
      else setIssue("");
    };
    const onOffline = () => { online = false; clearTimer(); setConnected(false); setIssue("viewerOffline"); };
    // Background tabs throttle the viewer's reports; give it a fresh grace period when the tab returns.
    const onVisible = () => { if (!document.hidden) { heartbeat = Date.now(); stateSince = Date.now(); } };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("message", message);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    // The single opening/wake path also handles the inline handover viewer.
    void callbacks.current.ensure().catch(() => { if (!stopped) setIssue("viewerRetryNeeded"); });
    return () => {
      stopped = true; clearInterval(interval); clearTimer();
      if (manual.current === retryManually) manual.current = null;
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("message", message);
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, [agentId, visible]);
  const retry = useCallback(async () => { await manual.current?.(); }, []);
  return {issue: issue ? ISSUES[issue] : "", retry, connected};
}
