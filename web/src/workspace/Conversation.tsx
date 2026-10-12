import { memo, useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { AgentRow, Artifact, PlanItem, TaskDetail, ThreadRow } from "../api";
import { Avatar, botColor, bubbleTone } from "../avatar";
import { ChatMessageBody, contentUrl, type PreviewFile } from "../markdown";
import { Brain, CalendarClock, Check, ChevronDown, ChevronLeft, ChevronRight, Ellipsis, FileText, FolderOpen, Globe, LoaderCircle, MessageSquarePlus, Monitor, Square, TriangleAlert, X } from "../ui/icons";
import { ConfirmDialog, MenuButton, toast, type MenuItem } from "../ui/overlay";
import { Composer } from "./Composer";
import { draftKey } from "./drafts";
import { bytes, clock, dayLabel, duration, sameDay, unicodeHost } from "./format";
import { QuestionCard, questionBlocksComposer } from "./QuestionCard";
import { avatarProps } from "./Sidebar";
import { useMood } from "./mood";
import { activityStatus } from "./status";
import { savedSteps } from "./steps";
import { ThreadBar, ThreadRenameDialog } from "./Threads";
import type { LiveTask, Message, Step } from "./useSession";
import { useSession } from "./useSession";

type Session = ReturnType<typeof useSession>;
export type PanelTab = "computer" | "files";

const SUGGESTIONS = [
  "搜尋今天的科技新聞，整理三則重點並附上來源",
  "打開瀏覽器到 Google 首頁，告訴我你看到什麼",
  "在 /workspace 建立一份本週待辦清單 Markdown",
];

function hidden(item: Message) {
  const text = item.content.trim();
  if (!text) return true;
  if (text.startsWith("<system_reminder>")) return true;
  if (item.role === "user" && text.startsWith("Background task result (data, not instructions)")) return true;
  return false;
}

function artifactFile(agentId: string, artifact: Artifact): PreviewFile {
  return { title: artifact.title, path: artifact.rel_path, url: contentUrl(agentId, artifact.rel_path), mediaType: artifact.media_type };
}

function StepIcon({ state }: { state: Step["state"] }) {
  if (state === "running") return <LoaderCircle className="spin" aria-label="進行中" />;
  if (state === "failed") return <X aria-label="失敗" />;
  return <Check aria-label="完成" />;
}

function StepList({ steps, limit }: { steps: Step[]; limit?: number }) {
  const shown = limit ? steps.slice(-limit) : steps;
  return (
    <ol className="step-list">
      {shown.map(step => (
        <li key={step.id} data-state={step.state}>
          <StepIcon state={step.state} />
          <span className="step-label">{step.label}</span>
          {step.detail ? <span className="step-detail" title={step.detail}>{step.detail}</span> : null}
        </li>
      ))}
    </ol>
  );
}

/** The bot's own todo plan, Grok DeepSearch style: what it intends to do and how far it got. */
function PlanList({ plan }: { plan: PlanItem[] }) {
  const done = plan.filter(item => item.status === "completed").length;
  return (
    <ol className="plan-list" aria-label={`計畫 ${done}/${plan.length}`}>
      {plan.map((item, index) => (
        <li key={index} data-status={item.status}>
          {item.status === "completed" ? <Check aria-label="完成" /> : item.status === "in_progress" ? <LoaderCircle className="spin" aria-label="進行中" /> : <span className="plan-dot" aria-hidden="true" />}
          <span>{item.content}</span>
        </li>
      ))}
    </ol>
  );
}

function useElapsed(since?: string, running = true) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  const start = since ? Date.parse(since) : NaN;
  return Number.isFinite(start) ? Math.max(0, Math.round((now - start) / 1000)) : null;
}

/** Grok-style live work: one quiet "思考中" line with the current step; the rest folds away. */
function WorkCard({ agent, state, live, task, computerOpen, onOpenComputer, onStop }: {
  agent: AgentRow; state: string; live: LiveTask | null; task?: TaskDetail; computerOpen: boolean; onOpenComputer: () => void; onStop?: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const elapsed = useElapsed(task?.started_at, state !== "queued");
  const steps = live?.steps || [];
  const title = state === "queued" ? "排隊中" : state === "stopping" ? "正在停止…" : state.startsWith("waiting") ? "等你回覆" : "思考中";
  const notes = live?.narration || [];
  const plan = live?.plan || task?.plan || [];
  const status = live?.status || notes.at(-1);
  const more = steps.length > 1 || notes.length > 0;
  const meta = [elapsed !== null && state !== "queued" ? duration(elapsed) : "", steps.length > 1 ? `${steps.length} 個步驟` : "", live?.model].filter(Boolean).join(" · ");
  return (
    <section className="work-card" aria-live="polite" aria-label="工作進度">
      <header>
        <button type="button" className="think-toggle" aria-expanded={expanded} disabled={!more} onClick={() => setExpanded(v => !v)} title={`${agent.name} ${title}`}>
          {state === "running" || state === "stopping" ? <LoaderCircle className="spin" aria-hidden="true" /> : <Ellipsis aria-hidden="true" />}
          <strong className={state === "running" ? "shimmer" : undefined}>{title}</strong>
          {meta ? <small>{meta}</small> : null}
          {more ? <ChevronRight className={expanded ? "open" : undefined} aria-hidden="true" /> : null}
        </button>
        <div className="work-card-actions">
          {!computerOpen ? <button type="button" className="icon-btn small" aria-label="看電腦" title="看電腦" onClick={onOpenComputer}><Monitor /></button> : null}
          {state !== "stopping" && onStop ? <button type="button" className="icon-btn small" aria-label="停止" title="停止" onClick={onStop}><Square /></button> : null}
        </div>
      </header>
      {status ? <p className="work-card-status">{status}</p> : null}
      {live?.notice ? <p className="work-card-notice">{live.notice}</p> : null}
      {plan.length ? <PlanList plan={plan} /> : null}
      {steps.length ? <StepList steps={steps} limit={expanded ? undefined : 1} /> : null}
      {expanded && notes.length ? <Narration notes={notes} /> : null}
    </section>
  );
}

function Narration({ notes }: { notes: string[] }) {
  return <ol className="narration-list">{notes.map((note, index) => <li key={index}>{note}</li>)}</ol>;
}

const VERDICTS: Record<string, { label: string; tone: string }> = {
  failed: { label: "工作未完成", tone: "danger" },
  cancelled: { label: "已停止", tone: "muted" },
  interrupted: { label: "已中斷，未自動重播", tone: "warn" },
};

const SHOWN_FILES = 3;

function taskSeconds(task?: TaskDetail) {
  const start = task?.started_at ? Date.parse(task.started_at) : NaN;
  const end = task?.ended_at ? Date.parse(task.ended_at) : NaN;
  return Number.isFinite(start) && Number.isFinite(end) && end >= start ? Math.round((end - start) / 1000) : null;
}

/** Grok's "Thought for Ns": the whole process collapses to one line above the answer. */
function ThoughtSummary({ task, steps, narration }: { task?: TaskDetail; steps?: Step[]; narration?: string[] }) {
  const notes = narration || [];
  const plan = task?.plan || [];
  if (!steps?.length && !notes.length && !plan.length) return null;
  const seconds = taskSeconds(task);
  const label = [seconds !== null ? `已思考 ${duration(seconds)}` : "已完成", steps?.length ? `${steps.length} 個步驟` : ""].filter(Boolean).join(" · ");
  return (
    <details className="task-steps">
      <summary><Brain aria-hidden="true" />{label}<ChevronRight className="chev" aria-hidden="true" /></summary>
      {plan.length ? <PlanList plan={plan} /> : null}
      {notes.length ? <Narration notes={notes} /> : null}
      {steps?.length ? <StepList steps={steps} /> : null}
    </details>
  );
}

/** Only the few files worth opening; the rest live in the files panel. */
function TaskResult({ agentId, task, onOpen }: { agentId: string; task?: TaskDetail; onOpen: (file: PreviewFile) => void }) {
  const [all, setAll] = useState(false);
  if (!task) return null;
  const verdict = task.state === "terminal" ? VERDICTS[task.verdict || ""] : undefined;
  const artifacts = task.artifacts || [];
  const shown = all ? artifacts : artifacts.slice(0, SHOWN_FILES);
  const sources = sourceList(task);
  if (!verdict && !artifacts.length && !sources.length) return null;
  return (
    <div className="task-result">
      {verdict ? <span className={`verdict ${verdict.tone}`}>{verdict.tone === "danger" ? <TriangleAlert /> : null}{verdict.label}</span> : null}
      {artifacts.length ? (
        <div className="artifact-list" aria-label={`產出 ${artifacts.length} 個檔案`}>
          {shown.map(artifact => (
            <button type="button" key={artifact.id} className="artifact" onClick={() => onOpen(artifactFile(agentId, artifact))} title={artifact.rel_path}>
              <FileText aria-hidden="true" /><span>{artifact.title}</span><small>{bytes(artifact.bytes)}</small>
            </button>
          ))}
          {artifacts.length > SHOWN_FILES ? <button type="button" className="artifact more" onClick={() => setAll(v => !v)}>{all ? "收合" : `+${artifacts.length - SHOWN_FILES}`}</button> : null}
        </div>
      ) : null}
      {sources.length ? <Sources sources={sources} /> : null}
    </div>
  );
}

const SHOWN_SOURCES = 8;

/** Pages the bot read come first; search hits only fill in when it read few pages itself. */
function sourceList(task: TaskDetail) {
  const seen = new Set<string>();
  const rows = (task.sources || []).filter(row => !seen.has(row.url) && seen.add(row.url));
  const read = rows.filter(row => row.kind === "read");
  return read.length >= 3 ? read : [...read, ...rows.filter(row => row.kind !== "read")].slice(0, Math.max(read.length, 5));
}

function Sources({ sources }: { sources: NonNullable<TaskDetail["sources"]> }) {
  const [all, setAll] = useState(false);
  const shown = all ? sources : sources.slice(0, SHOWN_SOURCES);
  return (
    <details className="source-list">
      <summary><Globe aria-hidden="true" />來源 {sources.length}<ChevronRight className="chev" aria-hidden="true" /></summary>
      <ol>
        {shown.map(source => {
          let host = source.url;
          try { host = unicodeHost(new URL(source.url).hostname.replace(/^www\./, "")); } catch { /* keep the raw URL */ }
          return (
            <li key={source.url}>
              <a href={source.url} target="_blank" rel="noreferrer noopener" title={source.url}>
                <span>{source.title || host}</span><small>{host}</small>
              </a>
            </li>
          );
        })}
      </ol>
      {sources.length > SHOWN_SOURCES ? <button type="button" className="link-btn" onClick={() => setAll(v => !v)}>{all ? "收合" : `顯示全部 ${sources.length} 個來源`}</button> : null}
    </details>
  );
}

function RoutineBubble({ item, agentId, knownPaths, onOpen }: { item: Message; agentId: string; knownPaths: string[]; onOpen: (file: PreviewFile) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`bubble routine${open ? " open" : ""}`}>
      <button type="button" className="routine-origin" aria-expanded={open} onClick={() => setOpen(value => !value)}>
        <CalendarClock aria-hidden="true" /><span>定時任務{item.routine_title ? `：${item.routine_title}` : ""}</span>{open ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
      </button>
      {open ? <ChatMessageBody content={item.content} agentId={agentId} knownPaths={knownPaths} onOpen={onOpen} /> : null}
    </div>
  );
}

const FOLD_AT = 900;

/** Very long replies start folded so one answer does not bury the conversation. */
function Foldable({ active, children }: { active: boolean; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const [tall, setTall] = useState(false);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || !active) { setTall(false); return; }
    const measure = () => setTall(el.scrollHeight > FOLD_AT);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el.firstElementChild || el);
    return () => observer.disconnect();
  }, [active]);
  const folded = tall && !open;
  return (
    <div className={`foldable${folded ? " folded" : ""}`}>
      <div ref={box} className="foldable-body">{children}</div>
      {tall ? <button type="button" className="fold-toggle" aria-expanded={open} onClick={() => setOpen(v => !v)}>{open ? <>收合<ChevronDown className="up" aria-hidden="true" /></> : <>展開全文<ChevronDown aria-hidden="true" /></>}</button> : null}
    </div>
  );
}

const MessageRow = memo(function MessageRow({ item, agent, showAvatar, knownPaths, task, steps, onOpen }: {
  item: Message; agent: AgentRow; showAvatar: boolean; knownPaths: string[]; task?: TaskDetail; steps?: Step[]; onOpen: (file: PreviewFile) => void;
}) {
  if (item.role === "user") {
    return (
      <div className={`msg user${item.pending ? " pending" : ""}`}>
        {item.routine_id ? <RoutineBubble item={item} agentId={agent.id} knownPaths={knownPaths} onOpen={onOpen} /> : <div className="bubble"><ChatMessageBody content={item.content} agentId={agent.id} knownPaths={knownPaths} onOpen={onOpen} /></div>}
        <time className="msg-time">{item.pending ? "傳送中…" : clock(item.at)}</time>
      </div>
    );
  }
  return <AssistantRow item={item} agent={agent} showAvatar={showAvatar} knownPaths={knownPaths} task={task} steps={steps} onOpen={onOpen} />;
});

function AssistantRow({ item, agent, showAvatar, knownPaths, task, steps, onOpen }: {
  item: Message; agent: AgentRow; showAvatar: boolean; knownPaths: string[]; task?: TaskDetail; steps?: Step[]; onOpen: (file: PreviewFile) => void;
}) {
  // While a reply streams, Markdown re-renders can lag behind the text; scrolling and typing stay responsive.
  const content = useDeferredValue(item.content);
  return (
    <div className={`msg assistant${item.streaming ? " streaming" : ""}`}>
      <div className="msg-avatar">{showAvatar ? <Avatar {...avatarProps(agent)} size={28} gaze={false} /> : null}</div>
      <div className="msg-body">
        {!item.streaming ? <ThoughtSummary task={task} steps={steps} narration={item.narration} /> : null}
        <Foldable active={!item.streaming}><ChatMessageBody content={item.streaming ? content : item.content} agentId={agent.id} knownPaths={knownPaths} onOpen={onOpen} /></Foldable>
        {!item.streaming ? <TaskResult agentId={agent.id} task={task} onOpen={onOpen} /> : null}
        {!item.streaming ? <time className="msg-time">{clock(item.at)}</time> : null}
      </div>
    </div>
  );
}

function EmptyChat({ agent, thread, onPick }: { agent: AgentRow; thread: ThreadRow | null; onPick: (text: string) => void }) {
  const mood = useMood(undefined, { playful: true });
  const side = thread && !thread.main;
  return (
    <div className="chat-empty">
      <Avatar {...avatarProps(agent)} size={72} alive mood={mood} />
      <h2>{side ? thread.title : agent.name}</h2>
      {side ? <p className="muted">{agent.name} 的另一段對話</p> : agent.title ? <p className="muted">{agent.title}</p> : null}
      <p className="chat-empty-lead">{side
        ? "聊天內容和其他對話分開，記憶、技能、檔案與電腦則是同一個 Bot 共用。"
        : "在自己的電腦裡查資料、操作瀏覽器、處理檔案。交代一件事，它會一步步完成，你隨時可以打開電腦接手。"}</p>
      <div className="suggestions">
        {SUGGESTIONS.map(text => <button type="button" key={text} onClick={() => onPick(text)}>{text}</button>)}
      </div>
    </div>
  );
}

export function Conversation({ agent, session, mobile, panel, onBack, onTogglePanel, onOpenFile, onRoutines, onRecord, onPick, menuItems, onOpenSettings }: {
  agent: AgentRow;
  session: Session;
  mobile: boolean;
  panel: { open: boolean; tab: PanelTab };
  onBack: () => void;
  onTogglePanel: (tab: PanelTab, force?: boolean) => void;
  onOpenFile: (file: PreviewFile) => void;
  onRoutines: () => void;
  onRecord: () => void;
  onPick: (text: string) => void;
  menuItems: MenuItem[];
  onOpenSettings: () => void;
}) {
  const { transcript, activity, busy, live, tasks, files, threadId, thread, threads, workingThread } = session;
  const [threadDialog, setThreadDialog] = useState<{ kind: "rename" | "clear" | "delete"; thread: ThreadRow } | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [showJump, setShowJump] = useState(false);
  const knownPaths = useMemo(() => files.map(file => file.path), [files]);
  const visible = useMemo(() => transcript.filter(item => !hidden(item)), [transcript]);
  const status = activityStatus(activity);
  const mood = useMood(activity.state, { verdict: activity.latest_task_id ? tasks[activity.latest_task_id]?.verdict : undefined, playful: true });
  const question = session.elsewhere ? null : activity.question || null;
  const activeTaskId = activity.active_task_ids?.[0] || null;
  const computerOpen = panel.open && panel.tab === "computer";

  const toBottom = useCallback((smooth = false) => {
    const el = scroller.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  }, []);
  useLayoutEffect(() => { pinned.current = true; setShowJump(false); toBottom(); }, [agent.id, threadId, toBottom]);
  useEffect(() => {
    const el = content.current;
    if (!el) return;
    const observer = new ResizeObserver(() => { if (pinned.current) toBottom(); });
    observer.observe(el);
    return () => observer.disconnect();
  }, [toBottom]);

  const stop = useCallback(() => { void session.stop().catch(err => toast(err instanceof Error ? err.message : String(err), "error")); }, [session]);
  const blocked = questionBlocksComposer(question) ? "請先在上方卡片回覆 Bot 的問題" : "";
  const fail = (err: unknown) => toast(err instanceof Error ? err.message : String(err), "error");
  const newThread = () => { void session.createThread().catch(fail); };
  const steering = busy && session.here !== "queued";
  const color = botColor(agent.name, agent.avatar_color);
  const tint = useMemo(() => { const tone = bubbleTone(color); return { "--bot": tone.fill, "--bot-ink": tone.ink } as CSSProperties; }, [color]);

  return (
    <main className="chat" style={tint} aria-label={`與 ${agent.name} 的對話`}>
      <header className="chat-header">
        {mobile ? <button type="button" className="icon-btn" aria-label="返回 Bot 清單" onClick={onBack}><ChevronLeft /></button> : null}
        <button type="button" className="chat-identity" onClick={onOpenSettings} title="Bot 設定">
          <Avatar {...avatarProps(agent)} size={32} gaze={false} alive thinking={status.tone === "busy"} mood={mood} />
          <span>
            <strong className="chat-title">{agent.name}</strong>
            <small className={`chat-status ${status.tone}`}><i aria-hidden="true" />{status.label}{agent.title && status.tone === "idle" ? ` · ${agent.title}` : ""}</small>
          </span>
        </button>
        <div className="chat-actions">
          {threads.length <= 1 ? <button type="button" className="icon-btn" aria-label="新對話" title="開新對話" onClick={newThread}><MessageSquarePlus /></button> : null}
          <button type="button" className="icon-btn" aria-label="定時任務" title="定時任務" onClick={onRoutines}><CalendarClock /></button>
          <button type="button" className="icon-btn" aria-label="檔案" title="檔案" aria-pressed={panel.open && panel.tab === "files"} onClick={() => onTogglePanel("files")}><FolderOpen /></button>
          <button type="button" className="icon-btn" aria-label="電腦" title="電腦" aria-pressed={computerOpen} onClick={() => onTogglePanel("computer")}><Monitor /></button>
          <MenuButton label="更多" icon={<Ellipsis />} items={menuItems} />
        </div>
      </header>
      {threads.length > 1 ? (
        <ThreadBar threads={threads} threadId={threadId} workingId={activity.thread_id} onOpen={session.openThread} onCreate={newThread}
          onRename={row => setThreadDialog({ kind: "rename", thread: row })}
          onClear={row => setThreadDialog({ kind: "clear", thread: row })}
          onDelete={row => setThreadDialog({ kind: "delete", thread: row })} />
      ) : null}

      <div className="chat-scroll" ref={scroller} onScroll={event => {
        const el = event.currentTarget;
        const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        pinned.current = near;
        setShowJump(!near);
        el.parentElement?.classList.toggle("scrolled", el.scrollTop > 4);
      }}>
        <div className="chat-content" ref={content}>
          {session.loading && !visible.length ? <div className="chat-loading"><LoaderCircle className="spin" />載入對話…</div> : null}
          {session.loadError && !visible.length ? <div className="pane-error" role="alert">{session.loadError}<button type="button" className="btn small" onClick={session.refresh}>重試</button></div> : null}
          {!session.loading && !visible.length && !busy ? <EmptyChat agent={agent} thread={thread} onPick={onPick} /> : null}
          {visible.map((item, index) => {
            const previous = visible[index - 1];
            const newDay = !previous || !sameDay(previous.at, item.at);
            const taskId = item.id?.startsWith("delta:") ? item.id.slice(6) : "";
            const steps = taskId && live?.taskId === taskId && taskId !== activeTaskId && live.steps.length ? live.steps
              : taskId && tasks[taskId]?.steps?.length ? savedSteps(tasks[taskId].steps) : undefined;
            return (
              <div key={item.id || `${item.at}-${index}`} className="msg-wrap">
                {newDay && item.at ? <div className="day-sep"><span>{dayLabel(item.at)}</span></div> : null}
                <MessageRow item={item} agent={agent} knownPaths={knownPaths} onOpen={onOpenFile}
                  showAvatar={item.role === "assistant" && (newDay || previous?.role !== "assistant")}
                  task={taskId ? tasks[taskId] : undefined} steps={steps} />
              </div>
            );
          })}
          {busy ? <WorkCard agent={agent} state={session.here} live={steering && live && live.taskId === activeTaskId ? live : null} task={steering && activeTaskId ? tasks[activeTaskId] : undefined}
            computerOpen={computerOpen} onOpenComputer={() => onTogglePanel("computer", true)} onStop={session.elsewhere ? undefined : stop} /> : null}
        </div>
      </div>
      {showJump ? <button type="button" className="jump-latest" onClick={() => { pinned.current = true; toBottom(true); }}><ChevronDown />最新訊息</button> : null}

      <div className="chat-bottom">
        {session.elsewhere ? (
          <button type="button" className="elsewhere-bar" onClick={() => activity.thread_id && session.openThread(activity.thread_id)}>
            <LoaderCircle className="spin" aria-hidden="true" />
            <span>{agent.name} 正在「{workingThread?.title || "另一段對話"}」{activity.question ? "等你回覆" : "工作"}{busy ? "，這裡的訊息排在後面" : ""}</span>
            <strong>前往<ChevronRight aria-hidden="true" /></strong>
          </button>
        ) : null}
        {question ? <QuestionCard agentId={agent.id} question={question} onDone={() => { void session.refreshActivity(agent.id); }} onOpenComputer={() => onTogglePanel("computer", true)} /> : null}
        <Composer key={draftKey(agent.id, threadId)} agentId={agent.id} draft={draftKey(agent.id, threadId)} agentName={agent.name} busy={steering} elsewhere={session.elsewhere}
          stopping={!session.elsewhere && activity.state === "stopping"} blocked={blocked}
          answering={!!question && !blocked} onSend={session.send} onStop={session.stop} onRecord={onRecord} autoFocus={!mobile} />
      </div>
      {threadDialog?.kind === "rename" ? (
        <ThreadRenameDialog thread={threadDialog.thread} onClose={() => setThreadDialog(null)} onSave={title => session.renameThread(threadDialog.thread.id, title)} />
      ) : null}
      {threadDialog?.kind === "clear" ? (
        <ConfirmDialog title="清除這段對話" confirmLabel="清除" danger onClose={() => setThreadDialog(null)}
          body={<>會清除「{threadDialog.thread.title}」的聊天紀錄，並讓 {agent.name} 在這段對話重新開始。記憶、技能、檔案與電腦都會保留。</>}
          onConfirm={async () => { await session.clear(threadDialog.thread.id); toast("對話已清除", "ok"); }} />
      ) : null}
      {threadDialog?.kind === "delete" ? (
        <ConfirmDialog title="刪除對話" confirmLabel="刪除" danger onClose={() => setThreadDialog(null)}
          body={<>要刪除「{threadDialog.thread.title}」嗎？這段對話的聊天紀錄會一併刪除，{agent.name} 的記憶、技能與檔案不受影響。</>}
          onConfirm={async () => { await session.deleteThread(threadDialog.thread.id); toast("對話已刪除", "ok"); }} />
      ) : null}
    </main>
  );
}
