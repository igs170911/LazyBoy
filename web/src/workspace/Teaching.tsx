import { useEffect, useState } from "react";
import { api, type AgentRow, type Lesson, type LessonStep } from "../api";
import { Check, ChevronDown, GraduationCap, ListOrdered, LoaderCircle, MessageSquareText, Pause, Play, Trash2, X } from "../ui/icons";
import { Modal, ModalHeader, toast } from "../ui/overlay";

type TeachAction = Parameters<typeof api.teach>[1];

const SOURCE: Record<LessonStep["source"], string> = { browser: "瀏覽器", desktop: "桌面", user: "說明" };

export function TeachStartDialog({ agent, mobile, onStart, onClose }: {
  agent: Pick<AgentRow, "id" | "name">;
  mobile: boolean;
  onStart: (goal: string, trial: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const [goal, setGoal] = useState("");
  const [trial, setTrial] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function start() {
    setBusy(true); setError("");
    try { await onStart(goal.trim(), trial); onClose(); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
  }
  return (
    <Modal label="教它一件事" onClose={() => { if (!busy) onClose(); }} className="teach-dialog">
      <ModalHeader title="教它一件事" onClose={onClose} disabled={busy} />
      <p className="dialog-lead">在 {agent.name} 的電腦上照平常的方式做一次，它會記下每一步，理解你的目的後整理成技能，之後交代一句話就會照做。</p>
      <label>你要教它做什麼？（選填）
        <input value={goal} maxLength={200} disabled={busy} placeholder="例如：匯出上個月的訂單" onChange={e => setGoal(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void start(); }} autoFocus={!mobile} />
      </label>
      <label className="check-row"><input type="checkbox" checked={trial} disabled={busy} onChange={e => setTrial(e.target.checked)} />學完後讓它自己試做一次</label>
      <ul className="teach-notes">
        <li>密碼欄位只會記成 ••••，不會被存下來。</li>
        <li>操作途中可以補充說明，也可以暫停或刪掉做錯的步驟。</li>
        {mobile ? <li>手機上操作電腦比較不方便，建議用電腦教。</li> : null}
      </ul>
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" disabled={busy} onClick={onClose}>取消</button>
        <button type="button" className="primary" disabled={busy} onClick={() => void start()}>{busy ? "正在準備電腦…" : "開始示範"}</button>
      </div>
    </Modal>
  );
}

function StepShot({ lesson, step }: { lesson: Pick<Lesson, "base_url">; step: LessonStep }) {
  if (!step.shot) return null;
  // Mark what was acted on, in the screenshot's own proportions.
  const box = step.target?.box && step.target.viewport ? { box: step.target.box, size: step.target.viewport }
    : step.target?.box && step.screen ? { box: step.target.box, size: step.screen } : null;
  return (
    <span className="teach-shot">
      <img src={lesson.base_url + encodeURIComponent(step.shot)} alt="" loading="lazy" />
      {box ? <span className="teach-mark" style={{
        left: `${(box.box[0] / box.size[0]) * 100}%`, top: `${(box.box[1] / box.size[1]) * 100}%`,
        width: `${Math.max(2, (box.box[2] / box.size[0]) * 100)}%`, height: `${Math.max(2, (box.box[3] / box.size[1]) * 100)}%`,
      }} /> : null}
    </span>
  );
}

export function StepList({ lesson, onDrop }: { lesson: Pick<Lesson, "base_url" | "steps">; onDrop?: (step: LessonStep) => void }) {
  if (!lesson.steps.length) return <p className="teach-empty">還沒有步驟。在電腦上點擊、輸入或開網頁，就會出現在這裡。</p>;
  return (
    <ol className="teach-steps">
      {lesson.steps.map(step => (
        <li key={step.id} className={step.kind === "note" ? "is-note" : undefined}>
          <StepShot lesson={lesson} step={step} />
          <span className="teach-step-text">
            <small>{SOURCE[step.source] || step.source}</small>
            {step.text}
          </span>
          {onDrop ? <button type="button" className="icon-btn" aria-label={`刪除這一步：${step.text}`} title="刪除這一步" onClick={() => onDrop(step)}><Trash2 /></button> : null}
        </li>
      ))}
    </ol>
  );
}

/** Sits above the bot's computer while the human demonstrates. */
export function TeachBar({ lesson, onAction }: { lesson: Lesson; onAction: (body: TeachAction) => Promise<unknown> }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [noting, setNoting] = useState(false);
  const [note, setNote] = useState("");
  const [open, setOpen] = useState(false);
  const paused = lesson.state === "paused";
  const actions = lesson.steps.filter(step => step.kind !== "note");
  const latest = lesson.steps[lesson.steps.length - 1];

  async function act(name: string, body: TeachAction) {
    setBusy(name);
    try { await onAction(body); return true; }
    catch (err) { toast(err instanceof Error ? err.message : String(err), "error"); return false; }
    finally { setBusy(null); }
  }
  async function saveNote() {
    if (!note.trim()) return;
    if (await act("note", { action: "note", lesson_id: lesson.id, text: note.trim() })) { setNote(""); setNoting(false); }
  }

  return (
    <div className="teach-bar" role="region" aria-label="教學">
      <div className="teach-bar-row">
        <span className={`teach-live${paused ? " is-paused" : ""}`} role="status">
          <GraduationCap aria-hidden="true" />
          <strong>{paused ? "已暫停" : "教學中"}</strong> · 第 {actions.length} 步
        </span>
        <span className="teach-latest">{lesson.goal ? `「${lesson.goal}」` : ""}{latest ? ` 最新：${latest.text}` : " 開始在下方電腦操作吧"}</span>
        <div className="teach-actions">
          <button type="button" className="btn small" aria-expanded={open} onClick={() => setOpen(value => !value)}><ListOrdered />步驟<ChevronDown className={open ? "is-open" : undefined} /></button>
          <button type="button" className="btn small" aria-pressed={noting} onClick={() => setNoting(value => !value)}><MessageSquareText />補充說明</button>
          <button type="button" className="btn small" disabled={!!busy} onClick={() => void act("pause", { action: paused ? "resume" : "pause", lesson_id: lesson.id })}>
            {paused ? <Play /> : <Pause />}{paused ? "繼續" : "暫停"}
          </button>
          <button type="button" className="btn small" disabled={!!busy} onClick={() => void act("cancel", { action: "cancel", lesson_id: lesson.id })}>
            {busy === "cancel" ? <LoaderCircle className="spin" /> : <X />}取消
          </button>
          <button type="button" className="btn small primary" disabled={!!busy || !actions.length} title={actions.length ? undefined : "先在電腦上示範至少一步"}
            onClick={() => void act("finish", { action: "finish", lesson_id: lesson.id })}>
            {busy === "finish" ? <LoaderCircle className="spin" /> : <Check />}完成，請它學起來
          </button>
        </div>
      </div>
      {noting ? (
        <form className="teach-note" onSubmit={e => { e.preventDefault(); void saveNote(); }}>
          <input value={note} maxLength={500} autoFocus placeholder="例如：這裡每次都選上個月" aria-label="補充說明" onChange={e => setNote(e.target.value)} />
          <button type="submit" className="btn small primary" disabled={!note.trim() || busy === "note"}>加入</button>
        </form>
      ) : null}
      {open ? <div className="teach-panel"><StepList lesson={lesson} onDrop={step => void act("drop", { action: "drop", lesson_id: lesson.id, step: step.id })} /></div> : null}
    </div>
  );
}

/** The user's side of a demonstration in the conversation. */
export function DemoCard({ agentId, lessonId, text }: { agentId: string; lessonId: string; text: string }) {
  const [lesson, setLesson] = useState<Lesson | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!open || lesson) return;
    let live = true;
    api.lesson(agentId, lessonId).then(data => { if (live) setLesson(data.lesson); }).catch(err => { if (live) setError(err instanceof Error ? err.message : String(err)); });
    return () => { live = false; };
  }, [open, lesson, agentId, lessonId]);
  return (
    <details className="demo-card" onToggle={e => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary><GraduationCap aria-hidden="true" /><span>{text}</span><ChevronDown aria-hidden="true" /></summary>
      {error ? <p className="pane-error">{error}</p> : lesson ? <StepList lesson={lesson} /> : open ? <p className="teach-empty"><LoaderCircle className="spin" />載入步驟…</p> : null}
    </details>
  );
}
