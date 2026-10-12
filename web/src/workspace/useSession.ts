import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AgentActivity, type AgentFile, type PlanItem, type TaskDetail, type TeamEvent, type ThreadRow, type TranscriptItem } from "../api";
import { readStorage, writeStorage } from "../storage";
import { randomUUID } from "../uuid";
import { stepDetail, stepLabel } from "./steps";

export type Step = { id: string; name: string; label: string; detail: string; state: "running" | "done" | "failed" };
export type LiveTask = { taskId: string; status: string; steps: Step[]; narration?: string[]; model?: string; notice?: string; plan?: PlanItem[] };
export type Message = TranscriptItem & { pending?: boolean };

const IDLE: AgentActivity = { state: "idle", running: false, queued: false, observed_at_ms: 0, active_task_ids: [] };
const lives = new Map<string, LiveTask>();
const lastEvents = new Map<string, number>();
/** Last transcript seen per bot thread, shown at once when it is reopened while the fresh copy loads. */
const transcripts = new Map<string, Message[]>();

export const BUSY_STATES = new Set(["queued", "running", "waiting_input", "waiting_approval", "stopping"]);
const THREADS_KEY = "lazyhermes.threads";

function storedThreads(): Record<string, string> {
  try { return JSON.parse(readStorage(THREADS_KEY) || "{}") as Record<string, string>; } catch { return {}; }
}

/** Where the bot's single computer is busy, relative to the thread on screen. */
export function threadState(activity: AgentActivity, threadId: string): { here: string; elsewhere: boolean } {
  const owner = activity.thread_id || (BUSY_STATES.has(activity.state) && activity.state !== "queued" ? threadId : null);
  if (owner === threadId) return { here: activity.state, elsewhere: false };
  const queuedHere = activity.queued_thread_ids ? activity.queued_thread_ids.includes(threadId) : !owner && activity.state === "queued";
  return { here: queuedHere ? "queued" : "idle", elsewhere: !!owner };
}

function mergeTranscript(current: Message[], incoming: TranscriptItem[]): Message[] {
  const byId = new Map(current.map(item => [item.id, item]));
  const merged: Message[] = incoming.map(item => {
    const mine = item.id ? byId.get(item.id) : undefined;
    // The stream may already be ahead of the snapshot that was read before the last delta.
    if (mine && mine.streaming && item.streaming && mine.content.length > item.content.length) return mine;
    if (mine && mine.content === item.content && mine.streaming === item.streaming && mine.at === item.at) return mine;
    return item;
  });
  const sentTexts = new Set(incoming.filter(item => item.role === "user").map(item => item.content.trim()));
  const known = new Set(incoming.map(item => item.id));
  // A snapshot read before the first delta must not erase text that is already streaming.
  for (const item of current) if (item.streaming && item.id && !known.has(item.id)) merged.push(item);
  for (const item of current) if (item.pending && !sentTexts.has(item.content.trim())) merged.push(item);
  return merged.length === current.length && merged.every((item, i) => item === current[i]) ? current : merged;
}

export function useSession(agentId: string | null, onAgentChanged: () => void) {
  const [transcript, setTranscript] = useState<Message[]>([]);
  const [activity, setActivity] = useState<AgentActivity>(IDLE);
  const [live, setLiveState] = useState<LiveTask | null>(null);
  const [tasks, setTasks] = useState<Record<string, TaskDetail>>({});
  const [files, setFiles] = useState<AgentFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [threads, setThreads] = useState<ThreadRow[]>([]);
  const [selection, setSelection] = useState<Record<string, string>>(storedThreads);
  const threadId = agentId ? selection[agentId] || agentId : "";
  const current = useRef(agentId);
  current.current = agentId;
  const currentThread = useRef(threadId);
  currentThread.current = threadId;
  const changed = useRef(onAgentChanged);
  changed.current = onAgentChanged;
  const timers = useRef<Record<string, number>>({});
  const owner = useRef("");
  useEffect(() => {
    if (!owner.current) return;
    transcripts.delete(owner.current);
    transcripts.set(owner.current, transcript.filter(item => !item.pending));
    while (transcripts.size > 24) transcripts.delete(transcripts.keys().next().value!);
  }, [transcript]);

  const setLive = useCallback((id: string, update: (live: LiveTask | null) => LiveTask | null) => {
    const next = update(lives.get(id) || null);
    if (next) lives.set(id, next); else lives.delete(id);
    if (current.current === id) setLiveState(next);
  }, []);

  const selectThread = useCallback((id: string, thread: string) => {
    setSelection(cur => {
      if ((cur[id] || id) === thread) return cur;
      const next = { ...cur, [id]: thread };
      if (thread === id) delete next[id];
      writeStorage(THREADS_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  const reloadThreads = useCallback(async (id: string) => {
    try {
      const data = await api.threads(id);
      if (current.current !== id) return;
      setThreads(data.threads || []);
      const chosen = currentThread.current;
      if (chosen !== id && data.threads && !data.threads.some(row => row.id === chosen)) selectThread(id, id);
    } catch { /* keep the last listing */ }
  }, [selectThread]);

  const reloadTranscript = useCallback(async (id: string) => {
    const thread = currentThread.current;
    try {
      const data = await api.agent(id, thread);
      if (current.current !== id || currentThread.current !== thread) return;
      setTranscript(cur => mergeTranscript(cur, data.transcript || []));
      setActivity(data.activity || IDLE);
      setLoadError("");
    } catch (err) {
      if (current.current !== id || currentThread.current !== thread) return;
      // A thread deleted elsewhere falls back to the main conversation.
      if (thread !== id && err instanceof Error && /not found|找不到|404/i.test(err.message)) { selectThread(id, id); return; }
      setLoadError(err instanceof Error ? err.message : String(err));
    } finally {
      if (current.current === id && currentThread.current === thread) setLoading(false);
    }
  }, [selectThread]);

  const refreshActivity = useCallback(async (id: string) => {
    try {
      const next = await api.activity(id);
      if (current.current === id) setActivity(previous => JSON.stringify(previous.question) === JSON.stringify(next.question) && previous.state === next.state && previous.latest_task_id === next.latest_task_id && previous.queued === next.queued ? previous : next);
    } catch { /* the next event or poll retries */ }
  }, []);

  const loadTask = useCallback(async (taskId: string) => {
    try {
      const detail = await api.getTask(taskId);
      setTasks(cur => JSON.stringify(cur[taskId]) === JSON.stringify(detail) ? cur : { ...cur, [taskId]: detail });
    } catch { /* deleted or not yet visible */ }
  }, []);

  const reloadFiles = useCallback(async (id: string) => {
    try {
      const data = await api.agentFiles(id);
      if (current.current === id) setFiles(data.files || []);
    } catch { /* keep the last listing */ }
  }, []);

  const debounce = useCallback((key: string, run: () => void, ms = 120) => {
    window.clearTimeout(timers.current[key]);
    timers.current[key] = window.setTimeout(run, ms);
  }, []);

  const handle = useCallback((id: string, event: TeamEvent) => {
    const p = event.payload || {};
    const taskId = event.task_id || "";
    lastEvents.set(id, Math.max(lastEvents.get(id) || 0, event.id));
    const eventThread = typeof p.thread_id === "string" ? p.thread_id : "";
    const here = !eventThread || eventThread === currentThread.current;
    const ensure = (live: LiveTask | null): LiveTask => live && live.taskId === taskId ? live : { taskId, status: "", steps: [] };
    switch (event.kind) {
      case "runtime": {
        const type = String(p.type || "");
        if (type === "message_delta") {
          if (!here) break;
          const key = `delta:${String(p.id || taskId)}`;
          const delta = String(p.delta || "");
          if (!delta) break;
          setTranscript(cur => {
            const index = cur.findIndex(item => item.id === key);
            if (index < 0) return [...cur, { id: key, role: "assistant", content: delta, at: new Date().toISOString(), streaming: true }];
            const next = [...cur];
            next[index] = { ...next[index], content: next[index].content + delta, streaming: true };
            return next;
          });
        } else if (type === "narration" && taskId) {
          // Text written before a tool call is progress narration; the reply restarts after it.
          const text = String(p.text || "").trim();
          if (!text) break;
          setLive(id, live => { const base = ensure(live); return { ...base, narration: [...(base.narration || []), text].slice(-40) }; });
          if (here) {
            const key = `delta:${String(p.id || taskId)}`;
            setTranscript(cur => cur.map(item => item.id === key ? { ...item, content: "", narration: [...(item.narration || []), text] } : item));
          }
        } else if (type === "tool_started" && taskId) {
          const name = String(p.name || "");
          setLive(id, live => {
            const base = ensure(live);
            const step: Step = { id: String(p.id || randomUUID()), name, label: stepLabel(name), detail: stepDetail(String(p.detail || "")), state: "running" };
            return { ...base, steps: [...base.steps.filter(item => item.id !== step.id), step].slice(-60) };
          });
        } else if (type === "tool_finished" && taskId) {
          setLive(id, live => {
            const base = ensure(live);
            const stepId = String(p.id || "");
            let index = base.steps.findIndex(item => item.id === stepId);
            for (let i = base.steps.length - 1; index < 0 && i >= 0; i--) if (base.steps[i].state === "running" && base.steps[i].name === p.name) index = i;
            if (index < 0) return base;
            const steps = [...base.steps];
            const failed = p.success === false;
            steps[index] = { ...steps[index], state: failed ? "failed" : "done", detail: (failed && stepDetail(String(p.summary || ""))) || steps[index].detail || stepDetail(String(p.detail || "")) };
            return { ...base, steps };
          });
        } else if (type === "plan" && taskId) {
          const plan = Array.isArray(p.plan) ? p.plan as PlanItem[] : [];
          setLive(id, live => ({ ...ensure(live), plan }));
        } else if (type === "user_progress" && taskId) {
          const message = String(p.message || "").trim();
          if (message) setLive(id, live => ({ ...ensure(live), status: message }));
        } else if (type === "steering" && taskId) {
          setLive(id, live => ({ ...ensure(live), notice: "已把你的補充送進目前的工作" }));
        } else if (type === "question" || type === "turn_ended") {
          void refreshActivity(id);
          if (type === "turn_ended") {
            setLive(id, live => live && live.taskId === taskId ? { ...live, steps: live.steps.map(step => step.state === "running" ? { ...step, state: "done" } : step) } : live);
            if (taskId) void loadTask(taskId);
            debounce("transcript", () => void reloadTranscript(id));
            debounce("files", () => void reloadFiles(id), 300);
            debounce("threads", () => void reloadThreads(id), 300);
            changed.current();
          }
        }
        break;
      }
      case "m0.progress":
        if (taskId && typeof p.message === "string") setLive(id, live => ({ ...ensure(live), status: String(p.message) }));
        void refreshActivity(id);
        break;
      case "model_routed":
        if (taskId) setLive(id, live => ({ ...ensure(live), model: [p.provider, p.model].filter(Boolean).join(" · ") }));
        break;
      case "model_fallback":
        if (taskId) setLive(id, live => ({ ...ensure(live), notice: `模型連線不穩，改用備援：${String(p.model || "")}` }));
        break;
      case "m0.artifact":
        if (taskId) debounce(`task:${taskId}`, () => void loadTask(taskId), 200);
        debounce("files", () => void reloadFiles(id), 300);
        break;
      case "user_message":
      case "reply":
      case "question_answered":
        if (here) debounce("transcript", () => void reloadTranscript(id));
        void refreshActivity(id);
        if (event.kind !== "question_answered") { changed.current(); debounce("threads", () => void reloadThreads(id), 300); }
        break;
      case "conversation_cleared":
        if (here) {
          setTranscript([]);
          void reloadTranscript(id);
        }
        debounce("threads", () => void reloadThreads(id), 100);
        changed.current();
        break;
      case "thread_updated":
        debounce("threads", () => void reloadThreads(id), 100);
        break;
      case "thread_deleted":
        if (eventThread && eventThread === currentThread.current) selectThread(id, id);
        debounce("threads", () => void reloadThreads(id), 100);
        changed.current();
        break;
      case "skills_changed":
        window.dispatchEvent(new CustomEvent("lazyboy-skills-changed", { detail: { agent: id, saved: p.saved === true, changed: p.changed === true, at: typeof p.at === "string" ? p.at : "" } }));
        if (p.saved === true && Array.isArray(p.skill_ids) && p.skill_ids.length) window.dispatchEvent(new CustomEvent("lazyboy-skills-saved", { detail: { agent: id, count: p.skill_ids.length } }));
        changed.current();
        break;
      case "error":
        void refreshActivity(id);
        debounce("transcript", () => void reloadTranscript(id));
        break;
      default:
        break;
    }
  }, [debounce, loadTask, refreshActivity, reloadFiles, reloadThreads, reloadTranscript, selectThread, setLive]);

  useEffect(() => {
    if (!agentId) return;
    const key = `${agentId}:${threadId}`;
    owner.current = key;
    const cached = transcripts.get(key);
    setTranscript(cached || []);
    setLoading(!cached);
    setLoadError("");
    void reloadTranscript(agentId);
  }, [agentId, threadId, reloadTranscript]);

  useEffect(() => {
    if (!agentId) return;
    const id = agentId;
    setActivity(IDLE);
    setLiveState(lives.get(id) || null);
    setFiles([]);
    setThreads([]);
    void reloadFiles(id);
    void reloadThreads(id);
    let source: EventSource | null = null;
    let poll = 0;
    const connect = () => {
      if (source || document.hidden) return;
      const since = lastEvents.get(id);
      source = new EventSource(`/api/agents/${encodeURIComponent(id)}/events${since ? `?since=${since}` : ""}`);
      source.onmessage = message => {
        try {
          const data = JSON.parse(message.data) as { events?: TeamEvent[] };
          for (const event of data.events || []) handle(id, event);
        } catch { /* ignore malformed frames */ }
      };
      source.onopen = () => { void refreshActivity(id); };
    };
    const disconnect = () => { source?.close(); source = null; };
    const visibility = () => {
      if (document.hidden) disconnect();
      else { connect(); void reloadTranscript(id); }
    };
    connect();
    poll = window.setInterval(() => { if (!document.hidden) void refreshActivity(id); }, 4000);
    document.addEventListener("visibilitychange", visibility);
    return () => {
      disconnect();
      window.clearInterval(poll);
      document.removeEventListener("visibilitychange", visibility);
      Object.values(timers.current).forEach(timer => window.clearTimeout(timer));
      timers.current = {};
    };
  }, [agentId, handle, refreshActivity, reloadFiles, reloadThreads, reloadTranscript]);

  // Fetch result details for the task-backed replies on screen.
  useEffect(() => {
    const ids = transcript.filter(item => item.id?.startsWith("delta:") && !item.streaming).map(item => item.id!.slice(6)).slice(-12);
    for (const taskId of ids) if (!tasks[taskId]) void loadTask(taskId);
  }, [transcript, tasks, loadTask]);

  const placement = threadState(activity, threadId);
  const busy = BUSY_STATES.has(placement.here);
  const thread = threads.find(row => row.id === threadId) || null;
  const workingThread = activity.thread_id && activity.thread_id !== threadId ? threads.find(row => row.id === activity.thread_id) || null : null;
  const activeTaskId = activity.active_task_ids?.[0] || null;
  useEffect(() => {
    if (activeTaskId && !tasks[activeTaskId]) void loadTask(activeTaskId);
  }, [activeTaskId, tasks, loadTask]);

  const send = useCallback(async (text: string) => {
    const id = current.current;
    if (!id) return;
    const local: Message = { id: `local:${randomUUID()}`, role: "user", content: text, at: new Date().toISOString(), pending: true };
    setTranscript(cur => [...cur, local]);
    try {
      const result = await api.send(id, text, currentThread.current);
      if (result.steered) setLive(id, live => live ? { ...live, notice: "已把你的補充送進目前的工作" } : live);
      void refreshActivity(id);
      await reloadTranscript(id);
    } catch (err) {
      setTranscript(cur => cur.filter(item => item.id !== local.id));
      throw err;
    }
  }, [refreshActivity, reloadTranscript, setLive]);

  const stop = useCallback(async () => {
    const id = current.current;
    if (!id) return;
    setActivity(cur => ({ ...cur, state: cur.running || cur.queued ? "stopping" : cur.state }));
    try { await api.stop(id); }
    finally { void refreshActivity(id); }
  }, [refreshActivity]);

  const refresh = useCallback(() => {
    const id = current.current;
    if (!id) return;
    void reloadTranscript(id);
    void refreshActivity(id);
    void reloadFiles(id);
    void reloadThreads(id);
  }, [refreshActivity, reloadFiles, reloadThreads, reloadTranscript]);

  const openThread = useCallback((thread: string) => {
    const id = current.current;
    if (id) selectThread(id, thread);
  }, [selectThread]);

  const createThread = useCallback(async (title = "") => {
    const id = current.current;
    if (!id) return;
    const row = await api.createThread(id, title);
    setThreads(cur => [cur[0], row, ...cur.slice(1)].filter(Boolean));
    selectThread(id, row.id);
    void reloadThreads(id);
  }, [reloadThreads, selectThread]);

  const renameThread = useCallback(async (thread: string, title: string) => {
    const id = current.current;
    if (!id) return;
    const row = await api.renameThread(id, thread, title);
    setThreads(cur => cur.map(item => item.id === row.id ? row : item));
  }, []);

  const deleteThread = useCallback(async (thread: string) => {
    const id = current.current;
    if (!id) return;
    await api.deleteThread(id, thread);
    if (currentThread.current === thread) selectThread(id, id);
    setThreads(cur => cur.filter(item => item.id !== thread));
    void reloadThreads(id);
  }, [reloadThreads, selectThread]);

  const clear = useCallback(async (thread?: string) => {
    const id = current.current;
    if (!id) return;
    const target = thread || currentThread.current;
    await api.clearConversation(id, target);
    if (target === currentThread.current) setTranscript([]);
    void reloadTranscript(id);
    void reloadThreads(id);
  }, [reloadThreads, reloadTranscript]);

  return {
    transcript, activity, busy, here: placement.here, elsewhere: placement.elsewhere, live, tasks, files, loading, loadError,
    threadId, thread, threads, workingThread, openThread, createThread, renameThread, deleteThread, clear,
    send, stop, refresh, refreshActivity, reloadFiles,
  };
}
