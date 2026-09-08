import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { api } from "./api";
import { t, type MessageKey } from "./i18n";
import type { RunActivity, RunActivityEntry } from "./types";

/**
 * The observability bubble: hover (or tap, or focus) an avatar to watch the run
 * that avatar is doing, turn by turn. The trail is polled rather than streamed
 * because it is only interesting while somebody is looking at it.
 */
const POLL_MS = 1500;
const CLOSE_MS = 300;
const KEEP = 200;
const PANEL_WIDTH = 380;
const DONE = ["completed", "failed", "cancelled"];

export type RunActionId = "retry" | "screen" | "settings";

/**
 * Which buttons actually help for a failure code, best first. A bad API key is
 * fixed in settings and a lost computer is checked on screen — retrying alone
 * would only burn another turn on the same wall.
 */
const ACTIONS: Record<string, RunActionId[]> = {
  interrupted: ["screen", "retry"],
  tool_timeout: ["screen", "retry"],
  model_key: ["settings", "retry"],
  model_unknown: ["settings", "retry"],
  model_quota: ["retry", "settings"],
  model_timeout: ["retry", "settings"],
  network: ["retry", "settings"],
  computer_gone: ["screen", "retry"],
  lease_lost: ["retry"],
  unknown: ["retry"],
};

const TITLES: Record<string, MessageKey> = {
  interrupted: "errorTitleInterrupted",
  tool_timeout: "errorTitleToolTimeout",
  model_key: "errorTitleModelKey",
  model_quota: "errorTitleModelQuota",
  model_unknown: "errorTitleModelUnknown",
  model_timeout: "errorTitleModelTimeout",
  network: "errorTitleNetwork",
  computer_gone: "errorTitleComputerGone",
  lease_lost: "errorTitleLeaseLost",
  unknown: "errorTitleUnknown",
};

export function errorActions(code?: string | null): RunActionId[] {
  return ACTIONS[code || ""] || ACTIONS.unknown;
}

/** Short label for a failure code; the long sentence lives in the message body. */
export function errorTitle(code?: string | null): string {
  return t(TITLES[code || ""] || TITLES.unknown);
}

/** 1:05 / 1:02:05 — how long the run has been at it. */
export function formatElapsed(ms: number): string {
  const seconds = Number.isFinite(ms) ? Math.max(0, Math.floor(ms / 1000)) : 0;
  const pad = (value: number) => String(value).padStart(2, "0");
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds % 60)}` : `${minutes}:${pad(seconds % 60)}`;
}

/** 340ms / 6.4s / 1:05 — durations inside the trail stay glanceable. */
export function shortDuration(ms?: number | null): string {
  if (typeof ms !== "number" || !Number.isFinite(ms) || ms < 0) return "";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)}s`;
  return formatElapsed(ms);
}

function kindLabel(kind: string): string {
  if (kind === "memory") return t("monitorKindMemory");
  if (kind === "model") return t("monitorKindModel");
  if (kind === "tool") return t("monitorKindTool");
  if (kind === "retry") return t("monitorKindRetry");
  if (kind === "notice") return t("monitorKindNotice");
  return t("monitorKindRun");
}

function statusLabel(status?: string | null): string {
  if (status === "ok") return t("monitorStatusOk");
  if (status === "timed_out") return t("monitorStatusTimedOut");
  if (status === "paused") return t("monitorStatusPaused");
  if (status === "error") return t("monitorStatusError");
  return "";
}

/** One trail line as plain text — what the panel shows and what gets copied. */
export function trailText(entry: RunActivityEntry): string {
  if (entry.kind === "run") {
    if (entry.event === "started") return t("monitorRunStarted", { task: entry.task || "" });
    if (entry.event === "completed") return t("monitorRunCompleted", { turns: entry.turns ?? 0 });
    if (entry.event === "failed") return t("monitorRunFailed");
    if (entry.event === "waiting_input") return t("monitorRunWaiting", { reason: entry.reason || "" });
    if (entry.event === "paused") return t("monitorRunPaused", { reason: entry.reason || "" });
    if (entry.event === "retry") return t("monitorRunRetry");
    return entry.text || entry.reason || t("monitorKindRun");
  }
  if (entry.kind === "memory") {
    return entry.enabled === false ? t("monitorMemoryDisabled") : t("monitorMemoryUsed", {count:entry.memories?.length ?? 0,time:shortDuration(entry.elapsedMs)});
  }
  if (entry.kind === "model") {
    const head = t("monitorModelTurn", { turn: entry.turn ?? 0, time: shortDuration(entry.elapsedMs) });
    return entry.text ? `${head} — ${entry.text}` : head;
  }
  if (entry.kind === "tool") {
    const parts = [entry.step || entry.name || t("monitorKindTool")];
    const status = statusLabel(entry.status);
    if (status) parts.push(status);
    const time = shortDuration(entry.elapsedMs);
    if (time) parts.push(time);
    const detail = entry.snippet ? ` — ${entry.snippet}` : "";
    return `${parts.join(" · ")}${detail}`;
  }
  if (entry.kind === "retry") {
    const head = `${t("monitorRetryAttempt", { attempt: entry.attempt ?? 0 })}${entry.gaveUp ? ` · ${t("monitorGaveUp")}` : ""}`;
    return entry.error ? `${head} — ${entry.error}` : head;
  }
  return entry.text || t("monitorKindNotice");
}

function MemoryUsage({runId,activityId}:{runId:string;activityId:number}) {
  const[open,setOpen]=useState(false);
  const[items,setItems]=useState<{id:string;revision:number;content:string|null}[]|null>(null);
  const[error,setError]=useState(false);
  useEffect(()=>{
    if(!open)return;
    let stopped=false;setError(false);setItems(null);
    api<{id:string;revision:number;content:string|null}[]>(`/api/runs/${runId}/memories/${activityId}`)
      .then(result=>{if(!stopped)setItems(result)}).catch(()=>{if(!stopped)setError(true)});
    return()=>{stopped=true};
  },[open,runId,activityId]);
  return <span className="run-memory-usage" onKeyDown={e=>e.stopPropagation()}>
    <button type="button" className="outline" aria-expanded={open} onClick={e=>{e.stopPropagation();setOpen(value=>!value)}}>{t("memoryViewIncluded")}</button>
    {open&&<span className="run-memory-list">{error?t("monitorLoadFailed"):items===null?t("memoryLoadingIncluded"):items.map(item=><span className="run-memory-item" key={`${item.id}:${item.revision}`}><small>{t("memoryRevision",{revision:item.revision})}</small><span>{item.content??t("memoryHistoricalUnavailable")}</span></span>)}</span>}
  </span>
}

function clockOf(createdAt: string): string {
  const date = new Date(createdAt);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function logDump(snapshot: RunActivity | null, entries: RunActivityEntry[]): string {
  const turn = snapshot?.turnLimit
    ? t("monitorTurnOf", { turn: snapshot.turn ?? 0, limit: snapshot.turnLimit })
    : t("monitorTurn", { turn: snapshot?.turn ?? 0 });
  const head = [
    snapshot?.runId ?? "",
    snapshot?.status ?? "",
    snapshot?.turn != null ? turn : "",
    snapshot?.step ?? "",
    snapshot?.elapsedMs != null ? t("monitorElapsed", { time: formatElapsed(snapshot.elapsedMs) }) : "",
  ].filter(Boolean).join(" · ");
  const trail = entries.map(entry => `${clockOf(entry.createdAt)} ${trailText(entry)}`);
  const error = snapshot?.error ? `\n[${snapshot.error.code}] ${snapshot.error.raw}` : "";
  return [head, ...trail].join("\n") + error;
}

async function copyLog(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/**
 * Wraps an avatar (or any small trigger) in a live readout of the run behind
 * it. Hover opens it and it closes ~300ms after the pointer leaves; a click
 * pins it so the panel survives reaching for the copy button; Esc lets go.
 */
export function RunProbe({ runId, align = "start", label, children }: { runId?: string | null; align?: "start" | "end"; label?: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [snapshot, setSnapshot] = useState<RunActivity | null>(null);
  const [entries, setEntries] = useState<RunActivityEntry[]>([]);
  const [stale, setStale] = useState(false);
  const [copied, setCopied] = useState<boolean | null>(null);
  const [box, setBox] = useState<{ left: number; top: number } | null>(null);
  const anchorRef = useRef<HTMLSpanElement | null>(null);
  const panelRef = useRef<HTMLSpanElement | null>(null);
  const listRef = useRef<HTMLSpanElement | null>(null);
  const lastId = useRef(0);
  const followTail = useRef(true);
  const linger = useRef<number | null>(null);
  const pinnedRef = useRef(false);

  useEffect(() => {
    pinnedRef.current = pinned;
  }, [pinned]);

  useEffect(() => {
    lastId.current = 0;
    setSnapshot(null);
    setEntries([]);
    setStale(false);
  }, [runId]);

  const place = useCallback(() => {
    const node = anchorRef.current;
    if (!node) return;
    const rect = node.getBoundingClientRect();
    const width = Math.min(PANEL_WIDTH, window.innerWidth - 24);
    const edge = align === "end" ? rect.right - width : rect.left;
    const height=Math.min(panelRef.current?.getBoundingClientRect().height||260,window.innerHeight-24);
    const above=rect.top>window.innerHeight-rect.bottom;
    const next={
      left:Math.max(12,Math.min(edge,window.innerWidth-12-width)),
      top:Math.max(12,Math.min(above?rect.top-height-8:rect.bottom+8,window.innerHeight-height-12)),
    };
    setBox(current=>current?.left===next.left&&current?.top===next.top?current:next);
  }, [align]);

  useEffect(() => {
    if (!open || !runId) return;
    let stopped = false;
    let timer = 0;
    const load = async () => {
      try {
        const after = lastId.current > 0 ? `?after=${lastId.current}` : "";
        const next = await api<RunActivity>(`/api/runs/${runId}/activity${after}`);
        if (stopped) return;
        setStale(false);
        setSnapshot(next);
        setEntries(current => {
          if (lastId.current <= 0) return next.activity.slice(-KEEP);
          const seen = new Set(current.map(entry => entry.id));
          return [...current, ...next.activity.filter(entry => !seen.has(entry.id))].slice(-KEEP);
        });
        for (const entry of next.activity) lastId.current = Math.max(lastId.current, entry.id);
        if (DONE.includes(next.status)) window.clearInterval(timer);
      } catch {
        if (!stopped) setStale(true);
      }
    };
    timer = window.setInterval(() => void load(), POLL_MS);
    void load();
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [open, runId]);

  useEffect(() => {
    if (!open) return;
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll",place,true);
    const observer=new ResizeObserver(place);
    if(panelRef.current)observer.observe(panelRef.current);
    return () => {window.removeEventListener("resize",place);window.removeEventListener("scroll",place,true);observer.disconnect()};
  }, [open, place]);

  useEffect(() => {
    const list = listRef.current;
    if (open && followTail.current && list) list.scrollTop = list.scrollHeight;
  }, [entries, open]);

  const hoverIn = useCallback(() => {
    if (linger.current) window.clearTimeout(linger.current);
    linger.current = null;
    setOpen(true);
  }, []);

  const hoverOut = useCallback(() => {
    if (pinnedRef.current) return;
    if (linger.current) window.clearTimeout(linger.current);
    linger.current = window.setTimeout(() => setOpen(false), CLOSE_MS);
  }, []);

  useEffect(
    () => () => {
      if (linger.current) window.clearTimeout(linger.current);
    },
    [],
  );

  const onKeyDown = (event: ReactKeyboardEvent<HTMLSpanElement>) => {
    if (event.key === "Escape") {
      setPinned(false);
      setOpen(false);
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setPinned(current => !current);
      setOpen(true);
    }
  };

  const turnText =
    snapshot?.status === "queued"
      ? t("monitorQueued")
      : snapshot?.turn != null
        ? snapshot.turnLimit
          ? t("monitorTurnOf", { turn: snapshot.turn, limit: snapshot.turnLimit })
          : t("monitorTurn", { turn: snapshot.turn })
        : "—";

  return (
    <span
      className={`run-probe ${runId ? "" : "inactive"}`}
      ref={anchorRef}
      tabIndex={runId ? 0 : -1}
      aria-expanded={open}
      aria-label={label || t("monitorTitle")}
      onMouseEnter={runId ? hoverIn : undefined}
      onMouseLeave={runId ? hoverOut : undefined}
      onClick={
        runId
          ? () => {
              setOpen(true);
              setPinned(current => !current);
            }
          : undefined
      }
      onKeyDown={runId ? onKeyDown : undefined}
    >
      {children}
      {open ? (
        <span
          ref={panelRef}
          className={`run-monitor ${pinned ? "pinned" : ""}`}
          style={box ? { left: box.left, top: box.top, width: Math.min(PANEL_WIDTH, window.innerWidth - 24) } : { display: "none" }}
          role="dialog"
          aria-label={t("monitorTitle")}
          onMouseEnter={hoverIn}
          onMouseLeave={hoverOut}
        >
          <span className="run-monitor-head">
            <b>{t("monitorTitle")}</b>
            <span className="run-monitor-turn">
              {turnText}
              {snapshot?.elapsedMs != null ? ` · ${t("monitorElapsed", { time: formatElapsed(snapshot.elapsedMs) })}` : ""}
            </span>
          </span>
          <span className={`run-monitor-step ${snapshot?.error ? "bad" : ""}`}>
            {snapshot?.status==="completed"?t("monitorCompletedLabel"):snapshot?.status==="cancelled"?t("monitorCancelledLabel"):snapshot?.error?.headline||(snapshot?.step?t("monitorNow",{step:snapshot.step}):entries.length?t("monitorRecordedLabel"):t("monitorEmpty"))}
          </span>
          <span
            className="run-monitor-list"
            ref={listRef}
            onScroll={event => {
              const node = event.currentTarget;
              followTail.current = node.scrollHeight - node.scrollTop - node.clientHeight < 32;
            }}
          >
            {entries.length === 0 ? <span className="run-monitor-empty">{stale ? t("monitorLoadFailed") : t("monitorEmpty")}</span> : null}
            {entries.map(entry => (
              <span className={`run-line kind-${entry.kind} ${entry.status && entry.status !== "ok" ? `is-${entry.status}` : ""}`} key={entry.id}>
                <span className="run-line-clock">{clockOf(entry.createdAt)}</span>
                <span className="run-line-kind">{kindLabel(entry.kind)}</span>
                <span className="run-line-text">{trailText(entry)}{entry.kind==="memory"&&Boolean(entry.memories?.length)&&runId&&<MemoryUsage runId={runId} activityId={entry.id}/>}</span>
              </span>
            ))}
          </span>
          {snapshot?.error ? (
            <span className="run-monitor-error">
              <b>{`[${snapshot.error.code}] ${errorTitle(snapshot.error.code)}`}</b>
              <code>{snapshot.error.raw}</code>
            </span>
          ) : null}
          <span className="run-monitor-foot">
            <button
              type="button"
              onClick={() =>
                void copyLog(logDump(snapshot, entries)).then(ok => {
                  setCopied(ok);
                  window.setTimeout(() => setCopied(null), 1800);
                })
              }
            >
              {copied === null ? t("monitorCopy") : copied ? t("monitorCopied") : t("clipboardWriteBlocked")}
            </button>
            <small>{pinned ? t("monitorPinned") : t("monitorPinHint")}</small>
            <button
              type="button"
              className="run-monitor-close"
              aria-label={t("monitorClose")}
              onClick={() => {
                setPinned(false);
                setOpen(false);
              }}
            >
              ×
            </button>
          </span>
        </span>
      ) : null}
    </span>
  );
}
