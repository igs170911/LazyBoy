import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { api, type AgentRow, type RoomRow, type SearchMessage, type SearchResult } from "../api";
import { Avatar } from "../avatar";
import { useDialogFocus } from "../dialog-focus";
import { CalendarClock, CornerDownLeft, FileText, LoaderCircle, MessageSquareText, Search } from "../ui/icons";
import { listTime } from "./format";
import { avatarProps } from "./Sidebar";
import { RoomAvatar } from "./Team";

export type PaletteAction = { id: string; label: string; icon: ReactNode; keys?: string; hint?: string; run: () => void };

type Item = { key: string; section: string; run: () => void; render: ReactNode; label: string };

/** The query's matches in a line of text, marked. */
function Marked({ text, query }: { text: string; query: string }) {
  const needle = query.trim().toLowerCase();
  if (!needle) return <>{text}</>;
  const parts: ReactNode[] = [];
  const lower = text.toLowerCase();
  let at = 0;
  for (let hit = lower.indexOf(needle); hit >= 0 && parts.length < 40; hit = lower.indexOf(needle, at)) {
    if (hit > at) parts.push(text.slice(at, hit));
    parts.push(<mark key={hit}>{text.slice(hit, hit + needle.length)}</mark>);
    at = hit + needle.length;
  }
  parts.push(text.slice(at));
  return <>{parts}</>;
}

const SECTION_ORDER = ["最近", "Bot", "群組對話", "動作", "訊息", "檔案", "定時任務"];

/**
 * Grok's command palette (Ctrl/⌘+K): switch bots and group chats, run an action, or search messages, files and
 * routines across every conversation and jump to the match.
 */
export function CommandPalette({ agents, rooms, actions, initialQuery = "", onClose, onOpenBot, onOpenRoom, onOpenMessage, onOpenFile, onOpenRoutine }: {
  initialQuery?: string;
  agents: AgentRow[];
  rooms: RoomRow[];
  actions: PaletteAction[];
  onClose: () => void;
  onOpenBot: (id: string) => void;
  onOpenRoom: (id: string) => void;
  onOpenMessage: (hit: SearchMessage) => void;
  onOpenFile: (agentId: string, path: string, title: string, mediaType?: string) => void;
  onOpenRoutine: (agentId: string) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState(initialQuery);
  const [result, setResult] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [index, setIndex] = useState(0);
  useDialogFocus(root, onClose);
  useEffect(() => { input.current?.focus(); }, []);

  // Messages, files and routines come from the server; bots and groups filter at once on what is already loaded.
  useEffect(() => {
    const text = query.trim();
    if (!text) { setResult(null); setSearching(false); return; }
    setSearching(true);
    let alive = true;
    const timer = window.setTimeout(() => {
      api.search(text).then(data => { if (alive) setResult(data); }).catch(() => { if (alive) setResult(null); }).finally(() => { if (alive) setSearching(false); });
    }, 140);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [query]);

  const items = useMemo<Item[]>(() => {
    const text = query.trim();
    const needle = text.toLowerCase();
    const done = (run: () => void) => () => { onClose(); run(); };
    const rows: Item[] = [];
    const botItem = (agent: AgentRow, section: string): Item => ({
      key: `bot:${agent.id}`, section, label: agent.name, run: done(() => onOpenBot(agent.id)),
      render: <><Avatar {...avatarProps(agent)} size={24} gaze={false} /><span className="palette-main"><strong><Marked text={agent.name} query={text} /></strong>{agent.title ? <small>{agent.title}</small> : null}</span>{agent.last_at ? <time>{listTime(agent.last_at)}</time> : null}</>,
    });
    const roomItem = (room: RoomRow, section: string): Item => ({
      key: `room:${room.id}`, section, label: room.name, run: done(() => onOpenRoom(room.id)),
      render: <><RoomAvatar members={room.members} size={24} /><span className="palette-main"><strong><Marked text={room.name} query={text} /></strong><small>{room.members.map(m => m.name).join("、")}</small></span>{room.last_at ? <time>{listTime(room.last_at)}</time> : null}</>,
    });
    const shown = actions.filter(action => !needle || action.label.toLowerCase().includes(needle) || action.hint?.toLowerCase().includes(needle));
    if (!needle) {
      // Nothing typed: the latest conversations first, then what you can do.
      const recent = [...agents.filter(a => !a.hidden).map(a => ({ at: a.last_at || "", item: botItem(a, "最近") })), ...rooms.map(r => ({ at: r.last_at || "", item: roomItem(r, "最近") }))]
        .sort((a, b) => (Date.parse(b.at) || 0) - (Date.parse(a.at) || 0)).slice(0, 6).map(row => row.item);
      rows.push(...recent);
    }
    rows.push(...shown.map(action => ({
      key: `action:${action.id}`, section: "動作", label: action.label, run: done(action.run),
      render: <><span className="palette-icon">{action.icon}</span><span className="palette-main"><strong><Marked text={action.label} query={text} /></strong>{action.hint ? <small>{action.hint}</small> : null}</span>{action.keys ? <kbd>{action.keys}</kbd> : null}</>,
    })));
    if (needle) {
      const fromServer = new Set((result?.bots || []).map(bot => bot.id));
      rows.push(...agents.filter(a => fromServer.has(a.id) || [a.name, a.title].some(v => v?.toLowerCase().includes(needle))).slice(0, 8).map(a => botItem(a, "Bot")));
      const serverRooms = new Set((result?.rooms || []).map(room => room.id));
      rows.push(...rooms.filter(r => serverRooms.has(r.id) || r.name.toLowerCase().includes(needle) || r.members.some(m => m.name.toLowerCase().includes(needle))).slice(0, 6).map(r => roomItem(r, "群組對話")));
      for (const hit of result?.messages || []) {
        const agent = hit.kind === "bot" ? agents.find(a => a.id === hit.owner_id) : undefined;
        const room = hit.kind === "room" ? rooms.find(r => r.id === hit.owner_id) : undefined;
        rows.push({
          key: `msg:${hit.owner_id}:${hit.message_id}`, section: "訊息", label: hit.snippet, run: done(() => onOpenMessage(hit)),
          render: <>
            {agent ? <Avatar {...avatarProps(agent)} size={24} gaze={false} /> : room ? <RoomAvatar members={room.members} size={24} /> : <span className="palette-icon"><MessageSquareText /></span>}
            <span className="palette-main">
              <strong className="palette-snippet"><Marked text={hit.snippet} query={text} /></strong>
              <small>{[hit.owner_name, hit.thread_title && hit.thread_title !== "主要對話" ? hit.thread_title : "", hit.speaker && hit.speaker !== hit.owner_name ? hit.speaker : ""].filter(Boolean).join(" · ")}</small>
            </span>
            {hit.at ? <time>{listTime(hit.at)}</time> : null}
          </>,
        });
      }
      for (const file of result?.files || []) rows.push({
        key: `file:${file.agent_id}:${file.path}`, section: "檔案", label: file.title, run: done(() => onOpenFile(file.agent_id, file.path, file.title, file.media_type)),
        render: <><span className="palette-icon"><FileText /></span><span className="palette-main"><strong><Marked text={file.title} query={text} /></strong><small>{file.agent_name} · {file.path}</small></span>{file.updated_at ? <time>{listTime(file.updated_at)}</time> : null}</>,
      });
      for (const routine of result?.routines || []) rows.push({
        key: `routine:${routine.id}`, section: "定時任務", label: routine.title, run: done(() => onOpenRoutine(routine.agent_id)),
        render: <><span className="palette-icon"><CalendarClock /></span><span className="palette-main"><strong><Marked text={routine.title} query={text} /></strong><small>{routine.agent_name} · {routine.enabled ? "已啟用" : "已暫停"}</small></span></>,
      });
    }
    return SECTION_ORDER.flatMap(section => rows.filter(row => row.section === section));
  }, [query, result, agents, rooms, actions, onClose, onOpenBot, onOpenRoom, onOpenMessage, onOpenFile, onOpenRoutine]);

  useEffect(() => { setIndex(0); }, [query]);
  const current = Math.min(index, Math.max(0, items.length - 1));
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${current}"]`)?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown") { event.preventDefault(); setIndex(i => Math.min(items.length - 1, i + 1)); }
    else if (event.key === "ArrowUp") { event.preventDefault(); setIndex(i => Math.max(0, i - 1)); }
    else if (event.key === "Enter") { event.preventDefault(); items[current]?.run(); }
  };

  let lastSection = "";
  return createPortal(
    <div className="palette-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <div ref={root} className="palette" role="dialog" aria-modal="true" aria-label="搜尋與指令">
        <div className="palette-input">
          {searching ? <LoaderCircle className="spin" aria-hidden="true" /> : <Search aria-hidden="true" />}
          <input ref={input} value={query} onChange={event => setQuery(event.target.value)} onKeyDown={onKeyDown}
            placeholder="搜尋 Bot、群組、訊息、檔案，或輸入指令…" aria-label="搜尋與指令" role="combobox" aria-expanded="true"
            aria-controls="palette-list" aria-activedescendant={items[current] ? `palette-item-${current}` : undefined} />
          <kbd>Esc</kbd>
        </div>
        <div ref={list} className="palette-list" id="palette-list" role="listbox" aria-label="結果">
          {items.map((item, i) => {
            const head = item.section !== lastSection ? item.section : "";
            lastSection = item.section;
            return (
              <div key={item.key} role="presentation">
                {head ? <div className="palette-section" role="presentation">{head}</div> : null}
                <div id={`palette-item-${i}`} data-index={i} role="option" aria-selected={i === current} className={`palette-item${i === current ? " active" : ""}`}
                  onMouseMove={() => { if (i !== current) setIndex(i); }} onClick={item.run}>
                  {item.render}
                  {i === current ? <CornerDownLeft className="palette-enter" aria-hidden="true" /> : null}
                </div>
              </div>
            );
          })}
          {query.trim() && !searching && !items.length ? <p className="palette-empty">找不到「{query.trim()}」</p> : null}
        </div>
        <footer className="palette-foot"><span><kbd>↑</kbd><kbd>↓</kbd> 選擇</span><span><kbd>Enter</kbd> 開啟</span><span><kbd>Esc</kbd> 關閉</span></footer>
      </div>
    </div>,
    document.body,
  );
}
