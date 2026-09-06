import { useEffect, useRef, useState } from "react";
import { Avatar } from "./avatar";
import { CallAudio } from "./call-audio";
import { t } from "./i18n";
import type { AvatarShape, Bot } from "./types";

export type CallPhase = "connecting" | "listening" | "speaking" | "working";

type ComputerEvent = { status?: string; step?: string | null; takeover?: boolean };
type ServerEvent =
  | { type: "ready"; callId?: string; voice?: string }
  | { type: "transcript"; role: "user" | "assistant"; text: string; final?: boolean }
  | { type: "speech"; state: "started" | "stopped" }
  | { type: "computer"; status?: string; step?: string | null; takeover?: boolean }
  | { type: "error"; message: string };

function wsUrl(sessionId: string): string {
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${location.host}/api/sessions/${sessionId}/call`;
}

function phaseLabel(phase: CallPhase, computer: ComputerEvent | null): string {
  if (computer?.takeover) return t("takeOverToContinue");
  if (phase === "connecting") return t("callConnecting");
  if (phase === "speaking") return t("speaking");
  if (phase === "working" || computer?.status === "running" || computer?.status === "queued") {
    return t("workingOnComputer");
  }
  return t("listening");
}

export function PhoneIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
      <path d="M6.5 3.8c.5-1 1.6-1.4 2.6-1l2 1c.8.4 1.2 1.3 1 2.2l-.6 2.2a2 2 0 0 0 .6 1.9l3.8 3.8c.5.5 1.3.7 1.9.6l2.2-.6c.9-.2 1.8.2 2.2 1l1 2c.4 1 .1 2.1-.9 2.6-2.3 1.2-7.2 1-11.6-3.4S4.1 7.6 5.3 5.3Z" />
    </svg>
  );
}

export function CallOverlay({
  bot,
  sessionId,
  takeover,
  onHangUp,
  onTakeOver,
}: {
  bot: Bot;
  sessionId: string;
  takeover: boolean;
  onHangUp: () => void;
  onTakeOver: () => void;
}) {
  const [phase, setPhase] = useState<CallPhase>("connecting");
  const [caption, setCaption] = useState("");
  const [error, setError] = useState("");
  const [elapsed, setElapsed] = useState(0);
  const [computer, setComputer] = useState<ComputerEvent | null>(null);
  const audioRef = useRef<CallAudio | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const hanging = useRef(false);

  function hangUp() {
    hanging.current = true;
    audioRef.current?.stop();
    audioRef.current = null;
    socketRef.current?.close();
    socketRef.current = null;
    onHangUp();
  }

  function interrupt() {
    audioRef.current?.interrupt();
    setPhase("listening");
  }

  useEffect(() => {
    hanging.current = false;
    let disposed = false;
    const started = Date.now();
    const tick = window.setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 1000);
    const audio = new CallAudio({
      onCapture: (pcm) => {
        if (socketRef.current?.readyState === WebSocket.OPEN) socketRef.current.send(pcm);
      },
      onError: (message) => setError(message),
    });
    audioRef.current = audio;
    const socket = new WebSocket(wsUrl(sessionId));
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;
    socket.onopen = () => {
      void audio.start().catch((err) => {
        if (disposed) return;
        audio.stop();
        socket.close();
        setError(err instanceof Error && /NotAllowedError|PermissionDenied/.test(err.name + err.message) ? t("micDenied") : t("micFailed"));
      });
      setPhase("listening");
    };
    socket.onmessage = (event) => {
      if (typeof event.data !== "string") {
        audio.play(event.data as ArrayBuffer);
        return;
      }
      let payload: ServerEvent;
      try { payload = JSON.parse(event.data) as ServerEvent; } catch { return; }
      if (payload.type === "speech") {
        if (payload.state === "started") {
          audio.interrupt();
          setPhase("listening");
        } else setPhase("listening");
        return;
      }
      if (payload.type === "transcript") {
        setCaption(payload.text);
        if (payload.role === "assistant") setPhase(payload.final ? "listening" : "speaking");
        return;
      }
      if (payload.type === "computer") {
        setComputer(payload);
        if (payload.status === "running" || payload.status === "queued" || payload.takeover) setPhase("working");
        return;
      }
      if (payload.type === "error") setError(payload.message);
    };
    socket.onerror = () => { if (!disposed && !hanging.current) setError(t("callFailed")); };
    socket.onclose = () => {
      audio.stop();
      if (!disposed && !hanging.current) setError((previous) => previous || t("callFailed"));
    };
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") { event.preventDefault(); hangUp(); }
      if (event.key === " " && !event.repeat && event.target === document.body) {
        event.preventDefault();
        interrupt();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => {
      disposed = true;
      hanging.current = true;
      window.clearInterval(tick);
      window.removeEventListener("keydown", onKey);
      audio.stop();
      socket.close();
    };
  }, [sessionId, bot.id]);

  const minutes = Math.floor(elapsed / 60);
  const seconds = String(elapsed % 60).padStart(2, "0");
  const showTakeover = takeover || Boolean(computer?.takeover);

  return (
    <div className="call-overlay" role="dialog" aria-label={t("call")}>
      <div className="call-card" data-testid="call-view">
        <div className="call-kicker">{t("call")}</div>
        <Avatar lookId={bot.id} name={bot.name} color={bot.avatarColor} shape={bot.avatarShape as AvatarShape} active online size={72} />
        <strong className="call-name">{bot.name}</strong>
        <div className={`call-phase ${phase}`}>{phaseLabel(phase, computer)}</div>
        <p className="call-caption">{caption || t("callPrompt")}</p>
        {error ? <p className="call-error">{error}</p> : null}
        <div className="call-time">{minutes}:{seconds}</div>
        <div className="call-actions">
          <button type="button" className="call-interrupt" onClick={interrupt}>{t("interrupt")}</button>
          <button type="button" className="call-hangup" onClick={hangUp}>{t("hangUp")}</button>
        </div>
        {showTakeover ? (
          <button type="button" className="primary call-takeover" onClick={onTakeOver}>{t("takeOverNow")}</button>
        ) : null}
        <p className="call-hint">{t("callShortcuts")}</p>
      </div>
    </div>
  );
}
