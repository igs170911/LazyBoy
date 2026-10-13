import { useEffect, useRef, useState } from "react";
import type { ThreadRow } from "../api";
import { Ellipsis, Eraser, MessageSquarePlus, Pencil, Trash2 } from "../ui/icons";
import { MenuButton, Modal, ModalHeader, type MenuItem } from "../ui/overlay";

export function ThreadBar({ threads, threadId, workingId, onOpen, onCreate, onRename, onClear, onDelete }: {
  threads: ThreadRow[];
  threadId: string;
  workingId?: string | null;
  onOpen: (id: string) => void;
  onCreate: () => void;
  onRename: (thread: ThreadRow) => void;
  onClear: (thread: ThreadRow) => void;
  onDelete: (thread: ThreadRow) => void;
}) {
  const strip = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Scroll only the strip: scrollIntoView would also scroll the app sideways while this page sits off screen on a phone.
    const row = strip.current, tab = row?.querySelector<HTMLElement>("[aria-selected=true]")?.parentElement;
    if (row && tab) {
      const left = tab.offsetLeft - row.offsetLeft, right = left + tab.offsetWidth;
      if (left < row.scrollLeft) row.scrollLeft = left;
      else if (right > row.scrollLeft + row.clientWidth) row.scrollLeft = right - row.clientWidth;
    }
  }, [threadId]);
  return (
    <div className="thread-bar">
      <div className="thread-strip" ref={strip} role="tablist" aria-label="對話串">
        {threads.map(thread => {
          const active = thread.id === threadId;
          const items: MenuItem[] = [
            { label: "重新命名", icon: <Pencil />, onSelect: () => onRename(thread) },
            { label: "清除這段對話", icon: <Eraser />, onSelect: () => onClear(thread), disabled: !thread.message_count },
            ...(thread.main ? [] : ["separator" as const, { label: "刪除對話", icon: <Trash2 />, danger: true, onSelect: () => onDelete(thread) }]),
          ];
          const working = thread.id === workingId || thread.running;
          return (
            <div key={thread.id} className={`thread-tab${active ? " active" : ""}`}>
              <button type="button" role="tab" aria-selected={active} title={thread.preview || thread.title} onClick={() => onOpen(thread.id)}>
                {working ? <i className="thread-dot busy" aria-label="工作中" /> : thread.queued ? <i className="thread-dot queued" aria-label="排隊中" /> : null}
                <span>{thread.title}</span>
              </button>
              {active ? <MenuButton label={`「${thread.title}」選項`} icon={<Ellipsis />} items={items} className="thread-menu" /> : null}
            </div>
          );
        })}
      </div>
      <button type="button" className="thread-new" onClick={onCreate} aria-label="新對話" title="開新對話"><MessageSquarePlus /></button>
    </div>
  );
}

export function ThreadRenameDialog({ thread, onClose, onSave }: { thread: ThreadRow; onClose: () => void; onSave: (title: string) => Promise<void> }) {
  const [title, setTitle] = useState(thread.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function save() {
    if (!title.trim() || busy) return;
    setBusy(true); setError("");
    try { await onSave(title.trim()); onClose(); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
  }
  return (
    <Modal label="重新命名對話" onClose={() => { if (!busy) onClose(); }} className="group-dialog">
      <form onSubmit={event => { event.preventDefault(); void save(); }}>
        <ModalHeader title="重新命名對話" onClose={onClose} disabled={busy} />
        <label className="field">
          <span>對話名稱</span>
          <input autoFocus value={title} maxLength={80} onChange={event => setTitle(event.target.value)} onFocus={event => event.currentTarget.select()} />
        </label>
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="dialog-actions">
          <button type="button" disabled={busy} onClick={onClose}>取消</button>
          <button type="submit" className="primary" disabled={busy || !title.trim()}>{busy ? "儲存中…" : "儲存"}</button>
        </div>
      </form>
    </Modal>
  );
}
