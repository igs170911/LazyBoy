import { useEffect, useState } from "react";
import { api, type MemoryState } from "./api";
import { Pencil, Plus, Trash2, X } from "./ui/icons";
import { toast } from "./ui/overlay";

const SECTIONS: { target: "user" | "memory"; title: string; hint: string }[] = [
  { target: "user", title: "關於你", hint: "Bot 記得的你：偏好、身分、習慣" },
  { target: "memory", title: "Bot 記住的事", hint: "工作上學到的事：環境、做法、專案位置" },
];

/** What a bot remembers between conversations, as plain cards you can fix, delete or add to. */
export function MemoryPanel({ agentId, agentName }: { agentId: string; agentName: string }) {
  const [state, setState] = useState<MemoryState | null>(null);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<{ target: string; index: number; text: string } | null>(null);
  const [adding, setAdding] = useState<{ target: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.memory(agentId).then(setState).catch(err => setError(err instanceof Error ? err.message : String(err))); }, [agentId]);
  const save = async (target: "user" | "memory", entries: string[], done: string) => {
    setBusy(true); setError("");
    try { setState(await api.saveMemory(agentId, target, entries, state?.[target].entries)); setEditing(null); setAdding(null); toast(done, "ok"); }
    catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      // The bot changed this list meanwhile: show its current version so the edit can be made on top of it.
      void api.memory(agentId).then(setState).catch(() => undefined);
    }
    finally { setBusy(false); }
  };
  if (!state) return <div className="memory-panel">{error ? <p className="pane-error" role="alert">{error}</p> : <p className="muted">讀取記憶…</p>}</div>;
  return (
    <div className="memory-panel">
      <p className="muted">{agentName} 會在每次對話時參考這些記憶。改錯的、過時的直接刪掉；修改從下一次對話開始生效。</p>
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      {SECTIONS.map(({ target, title, hint }) => {
        const list = state[target];
        const full = list.used / list.limit;
        return (
          <section key={target} className="memory-section">
            <header>
              <strong>{title}</strong><small>{hint}</small>
              <span className={`memory-usage${full > .9 ? " full" : ""}`} title="Hermes 對這類記憶的字數上限">{list.used.toLocaleString()} / {list.limit.toLocaleString()} 字</span>
            </header>
            {!list.entries.length ? <p className="memory-empty">還沒有記憶。</p> : null}
            <ul>
              {list.entries.map((entry, index) => editing?.target === target && editing.index === index ? (
                <li key={index} className="memory-entry editing">
                  <textarea autoFocus rows={3} value={editing.text} maxLength={2000} onChange={event => setEditing({ ...editing, text: event.target.value })} />
                  <div className="memory-actions">
                    <button type="button" className="outline" disabled={busy} onClick={() => setEditing(null)}>取消</button>
                    <button type="button" className="primary" disabled={busy || !editing.text.trim()} onClick={() => void save(target, list.entries.map((item, i) => i === index ? editing.text : item), "記憶已更新")}>儲存</button>
                  </div>
                </li>
              ) : (
                <li key={index} className="memory-entry">
                  <p>{entry}</p>
                  <div className="memory-tools">
                    <button type="button" className="icon-btn small" aria-label="修改這條記憶" title="修改" disabled={busy} onClick={() => { setAdding(null); setEditing({ target, index, text: entry }); }}><Pencil /></button>
                    <button type="button" className="icon-btn small" aria-label="刪除這條記憶" title="刪除" disabled={busy} onClick={() => void save(target, list.entries.filter((_, i) => i !== index), "已刪除這條記憶")}><Trash2 /></button>
                  </div>
                </li>
              ))}
            </ul>
            {adding?.target === target ? (
              <div className="memory-entry editing">
                <textarea autoFocus rows={2} value={adding.text} maxLength={2000} placeholder={target === "user" ? "例如：我習慣先看結論再看細節" : "例如：專案放在 /workspace/shop"} onChange={event => setAdding({ ...adding, text: event.target.value })} />
                <div className="memory-actions">
                  <button type="button" className="outline" disabled={busy} onClick={() => setAdding(null)}><X />取消</button>
                  <button type="button" className="primary" disabled={busy || !adding.text.trim()} onClick={() => void save(target, [...list.entries, adding.text], "已新增記憶")}>新增</button>
                </div>
              </div>
            ) : <button type="button" className="link-btn memory-add" onClick={() => { setEditing(null); setAdding({ target, text: "" }); }}><Plus />新增一條</button>}
          </section>
        );
      })}
    </div>
  );
}
