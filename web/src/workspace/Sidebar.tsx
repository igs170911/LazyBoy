import { memo, useMemo, useState } from "react";
import type { AgentGroup, AgentRow, RoomRow } from "../api";
import { Avatar } from "../avatar";
import type { AvatarShape } from "../types";
import { readStorage, writeStorage } from "../storage";
import { ChevronDown, FolderPlus, Import, LayoutGrid, MessagesSquare, Network, PanelLeft, Plug, Plus, Search, Settings, Stamp, Star, X } from "../ui/icons";
import { RoomAvatar } from "./Team";
import { MenuButton, useContextMenu, type MenuItem } from "../ui/overlay";
import { listTime } from "./format";
import { eyeMotion, useMood } from "./mood";
import { activityStatus } from "./status";
import { moveGroup } from "./order";

const COLLAPSED_KEY = "lazyhermes.collapsedGroups";

export function avatarProps(agent: Pick<AgentRow, "id" | "name" | "avatar_color" | "avatar_shape" | "avatar_url" | "avatar_traits" | "avatar_expression">) {
  return { lookId: agent.id, name: agent.name, color: agent.avatar_color || undefined, shape: (agent.avatar_shape as AvatarShape | undefined) || "organic", src: agent.avatar_url || undefined,
    eyes: agent.avatar_traits, expression: agent.avatar_expression || undefined };
}

/** The hidden-bots section is folded unless this marker is in the toggled list (sections store the opposite). */
const HIDDEN_OPEN = "__hidden_open__";

function readCollapsed(): string[] {
  try { const value = JSON.parse(readStorage(COLLAPSED_KEY) || "[]"); return Array.isArray(value) ? value.filter(v => typeof v === "string") : []; }
  catch { return []; }
}

/** Where a dragged bot can land: a section of the sidebar. */
export type DropTarget = { pinned: true } | { hidden: true } | { group: string };
type Drag = { kind: "bot"; id: string } | { kind: "group"; id: string } | null;

/** Dragging is for a mouse or trackpad; on a touch screen a long press opens the menu instead. */
const canDrag = () => typeof window !== "undefined" && window.matchMedia?.("(pointer: fine)").matches;

const BotRow = memo(function BotRow({ agent, active, onSelect, actions, unread = false, onDrag }: {
  agent: AgentRow; active: boolean; onSelect: (id: string) => void; actions: () => MenuItem[]; unread?: boolean; onDrag?: (drag: Drag) => void;
}) {
  const status = activityStatus(agent.activity);
  const busy = status.tone === "busy";
  const mood = useMood(agent.activity?.state, { lastAt: agent.last_at });
  const subtitle = agent.preview?.replace(/\s+/g, " ").trim() || agent.title || agent.description || "開始交辦第一件工作";
  const { bind, menu, opened } = useContextMenu(`${agent.name} 的選項`, actions);
  const draggable = !!onDrag && canDrag();
  return (
    <li className={`bot-row${active ? " active" : ""}${unread ? " unread" : ""}${opened ? " menu-open" : ""}`} data-tone={status.tone} {...bind}
      draggable={draggable || undefined}
      onDragStart={draggable ? event => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", agent.name); onDrag!({ kind: "bot", id: agent.id }); } : undefined}
      onDragEnd={draggable ? () => onDrag!(null) : undefined}>
      <button type="button" className="bot-row-main" aria-current={active ? "page" : undefined} aria-haspopup="menu" title={agent.name} onClick={() => onSelect(agent.id)}>
        <Avatar {...avatarProps(agent)} size={36} gaze={false} thinking={busy} mood={mood} motion={eyeMotion(agent.activity?.state)} />
        <span className="bot-row-text">
          <span className="bot-row-top">
            <strong>{agent.name}{agent.primary ? <Star className="primary-star" aria-label="主要 Bot" /> : null}</strong>
            {status.tone !== "idle" ? <span className={`status-pill ${status.tone}`}>{status.label}</span> : <time>{listTime(agent.last_at)}</time>}
            {unread ? <i className="unread-dot" aria-label="有新訊息" /> : null}
          </span>
          <span className="bot-row-sub">{subtitle}</span>
        </span>
      </button>
      {menu}
    </li>
  );
});

/** A group chat row, the way Grok lists group chats next to bots: avatar mosaic, name, latest line, who is answering. */
const RoomRowItem = memo(function RoomRowItem({ room, active, onSelect, unread = false }: { room: RoomRow; active: boolean; onSelect: (id: string) => void; unread?: boolean }) {
  const working = room.members.filter(m => room.working.includes(m.id));
  return (
    <li className={`bot-row room-row${active ? " active" : ""}${unread ? " unread" : ""}`} data-tone={working.length ? "busy" : "idle"}>
      <button type="button" className="bot-row-main" aria-current={active ? "page" : undefined} title={room.name} onClick={() => onSelect(room.id)}>
        <RoomAvatar members={room.members} size={36} />
        <span className="bot-row-text">
          <span className="bot-row-top">
            <strong>{room.name}</strong>
            {working.length ? <span className="status-pill busy">回覆中</span> : <time>{listTime(room.last_at)}</time>}
            {unread ? <i className="unread-dot" aria-label="有新訊息" /> : null}
          </span>
          <span className="bot-row-sub">{working.length ? `${working.map(m => m.name).join("、")} 正在回覆…` : room.preview || `${room.members.length} 位成員`}</span>
        </span>
      </button>
    </li>
  );
});

export function Sidebar({ agents, groups, activeId, loaded, onSelect, onCreate, onImport, onCreateGroup, onSettings, agentActions, groupActions, rooms = [], activeRoomId = null, onSelectRoom, onCreateRoom, unread, onMove, onReorderGroups, compact = false, onToggleCompact, onSearch, onConnectApps, onNew, onTemplates, onOrg, approvals = 0, onApprovals }: {
  /** Who reports to whom (Org.tsx). */
  onOrg?: () => void;
  /** Decisions waiting for the user; the inbox opens from a row at the top of the list. */
  approvals?: number;
  onApprovals?: () => void;
  /** Avatars only (Grok's compact sidebar, Ctrl/⌘+B). */
  compact?: boolean;
  onToggleCompact?: () => void;
  /** Open the full search (messages, files, routines), optionally with what was typed here. */
  onSearch?: (query?: string) => void;
  onConnectApps?: () => void;
  /** Grok's "New" button: one dialog for a new bot, a template, an import or a group chat. */
  onNew?: () => void;
  /** Grok's sidebar "Marketplace": ready bots to start from. */
  onTemplates?: () => void;
  unread?: (id: string, at?: string | null) => boolean;
  rooms?: RoomRow[];
  activeRoomId?: string | null;
  onSelectRoom?: (id: string) => void;
  onCreateRoom?: () => void;
  agents: AgentRow[];
  groups: AgentGroup[];
  activeId: string | null;
  loaded: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onImport: () => void;
  onCreateGroup: () => void;
  onSettings: () => void;
  agentActions: (agent: AgentRow) => MenuItem[];
  groupActions: (group: AgentGroup) => MenuItem[];
  /** A bot was dragged onto another section. */
  onMove?: (agent: AgentRow, target: DropTarget) => void;
  /** A section was dragged to a new place; the ids in their new order. */
  onReorderGroups?: (ids: string[]) => void;
}) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>(readCollapsed);
  const [drag, setDrag] = useState<Drag>(null);
  const [over, setOver] = useState<string | null>(null);
  const toggle = (id: string) => setCollapsed(current => {
    const next = current.includes(id) ? current.filter(item => item !== id) : [...current, id];
    writeStorage(COLLAPSED_KEY, JSON.stringify(next));
    return next;
  });
  const sections = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const visible = agents
      .filter(agent => !needle || [agent.name, agent.title, agent.description, agent.preview, ...(agent.tags || [])].some(value => value?.toLowerCase().includes(needle)))
      .sort((a, b) => (Date.parse(b.last_at || "") || 0) - (Date.parse(a.last_at || "") || 0));
    const known = new Set(groups.map(group => group.id));
    // Pinned bots sit on top; hidden ones fold away at the bottom (a search still finds them in place).
    const pinned = visible.filter(agent => agent.pinned && !agent.hidden);
    const hidden = needle ? [] : visible.filter(agent => agent.hidden);
    const listed = visible.filter(agent => !(agent.pinned && !agent.hidden) && (needle || !agent.hidden));
    const rows = groups.map(group => ({ group, agents: listed.filter(agent => agent.group_id === group.id) }));
    const loose = listed.filter(agent => !agent.group_id || !known.has(agent.group_id));
    return { rows: needle ? rows.filter(row => row.agents.length) : rows, loose, pinned, hidden, empty: !visible.length };
  }, [agents, groups, query]);

  const visibleRooms = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return rooms.filter(room => !needle || [room.name, ...room.members.map(m => m.name)].some(value => value.toLowerCase().includes(needle)));
  }, [rooms, query]);
  const createItems: MenuItem[] = [
    { label: "新增 Bot", icon: <Plus />, onSelect: onCreate },
    ...(onCreateRoom ? [{ label: "新增群組對話", icon: <MessagesSquare />, onSelect: onCreateRoom, disabled: agents.length < 2, hint: agents.length < 2 ? "需要兩個 Bot" : undefined }] : []),
    { label: "匯入 Bot", icon: <Import />, onSelect: onImport },
    { label: "新增分區", icon: <FolderPlus />, onSelect: onCreateGroup, disabled: agents.length === 0 },
  ];

  const dragBot = drag?.kind === "bot" ? agents.find(agent => agent.id === drag.id) : undefined;
  // Chrome drops a drag whose source moves while it starts, so the drop zones appear on the next frame.
  const startDrag = onMove && !query ? (next: Drag) => {
    if (next) window.setTimeout(() => setDrag(next), 0);
    else { setDrag(null); setOver(null); }
  } : undefined;
  /** Whether the dragged bot or section would change anything by landing on this section. */
  const accepts = (key: string) => {
    if (drag?.kind === "group") return key.startsWith("g:") && key !== `g:${drag.id}`;
    if (!dragBot) return false;
    if (key === "pinned") return !(dragBot.pinned && !dragBot.hidden);
    if (key === "hidden") return !dragBot.hidden;
    const group = key === "loose" ? "" : key.slice(2);
    return !!dragBot.pinned || !!dragBot.hidden || (dragBot.group_id || "") !== group;
  };
  const dropZone = (key: string) => !drag ? {} : {
    onDragOver: (event: React.DragEvent) => { if (!accepts(key)) return; event.preventDefault(); event.dataTransfer.dropEffect = "move"; if (over !== key) setOver(key); },
    onDragLeave: (event: React.DragEvent) => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setOver(current => current === key ? null : current); },
    onDrop: (event: React.DragEvent) => {
      event.preventDefault();
      if (accepts(key)) {
        if (drag.kind === "group") {
          onReorderGroups?.(moveGroup(groups.map(group => group.id), drag.id, key.slice(2)));
        } else if (dragBot) {
          onMove?.(dragBot, key === "pinned" ? { pinned: true } : key === "hidden" ? { hidden: true } : { group: key === "loose" ? "" : key.slice(2) });
        }
      }
      setDrag(null); setOver(null);
    },
  };
  const zoneClass = (key: string) => drag && accepts(key) ? (over === key ? " drop-target drop-over" : " drop-target") : "";
  const row = (agent: AgentRow, withUnread = true) => (
    <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={() => agentActions(agent)}
      unread={withUnread && !!unread?.(agent.id, agent.last_at)} onDrag={startDrag} />
  );

  return (
    <aside className={`sidebar${drag ? " is-dragging" : ""}${compact ? " compact" : ""}`} aria-label="Bot 清單">
      <header className="sidebar-head">
        <div className="brand"><img src="/lazyboy-round.svg" width={28} height={28} alt="" /><span>LazyBoy</span></div>
        {onNew ? <button type="button" className="icon-btn" aria-label="新增" title="新增 Bot 或群組對話（Ctrl Alt N）" onClick={onNew}><Plus /></button>
          : <MenuButton label="新增" icon={<Plus />} items={createItems} placement={compact ? "bottom-start" : "bottom-end"} />}
      </header>
      {compact && onSearch ? <button type="button" className="icon-btn sidebar-search-icon" aria-label="搜尋" title="搜尋（Ctrl K）" onClick={() => onSearch()}><Search /></button> : null}
      <div className="sidebar-search">
        <Search aria-hidden="true" />
        <input type="search" aria-label="搜尋 Bot" placeholder="搜尋 Bot 或群組" value={query} onChange={event => setQuery(event.target.value)}
          aria-keyshortcuts="Control+K Meta+K"
          onKeyDown={event => {
            // Enter opens the first match; Escape clears. Ctrl/⌘+K anywhere jumps here (see Workspace).
            if (event.key === "Escape") { setQuery(""); event.currentTarget.blur(); return; }
            if (event.key !== "Enter" || event.nativeEvent.isComposing || !query.trim()) return;
            event.preventDefault();
            // Best name match first: exact, then starts with, then contains; a match only in a preview comes last.
            const needle = query.trim().toLowerCase();
            const rank = (name: string) => { const n = name.toLowerCase(); return n === needle ? 0 : n.startsWith(needle) ? 1 : n.includes(needle) ? 2 : 3; };
            const hits = [
              ...visibleRooms.map(room => ({ rank: rank(room.name), open: () => onSelectRoom?.(room.id) })),
              ...[...sections.pinned, ...sections.loose, ...sections.rows.flatMap(row => row.agents)].map(bot => ({ rank: rank(bot.name), open: () => onSelect(bot.id) })),
            ].sort((a, b) => a.rank - b.rank);
            hits[0]?.open();
            setQuery(""); event.currentTarget.blur();
          }} />
        {!query ? <kbd className="search-key" aria-hidden="true">{/Mac|iPhone|iPad/.test(navigator.platform) ? "⌘K" : "Ctrl K"}</kbd> : null}
        {query ? <button type="button" className="icon-btn small" aria-label="清除搜尋" onClick={() => setQuery("")}><X /></button> : null}
      </div>
      <nav className="bot-list">
        {approvals && onApprovals ? (
          <button type="button" className="approvals-row" onClick={onApprovals} title="待你核定">
            <Stamp aria-hidden="true" /><span>待你核定</span><b>{approvals}</b>
          </button>
        ) : null}
        {!loaded ? <div className="bot-skeleton" aria-hidden="true"><i /><i /><i /></div> : null}
        {loaded && sections.empty && !(query && visibleRooms.length) ? (
          <div className="sidebar-empty">
            {query ? <p>找不到符合「{query}」的 Bot</p> : <><p>還沒有 Bot</p><button type="button" className="btn primary" onClick={onCreate}><Plus />建立第一個 Bot</button></>}
          </div>
        ) : null}
        {query.trim() && onSearch ? (
          <button type="button" className="search-everywhere" onClick={() => { onSearch(query.trim()); setQuery(""); }}>
            <Search aria-hidden="true" /><span>在所有訊息、檔案與定時任務中搜尋「{query.trim()}」</span>
          </button>
        ) : null}
        {sections.pinned.length || dragBot ? (
          <section className={`bot-section pinned-section${zoneClass("pinned")}`} {...dropZone("pinned")}>
            <div className="bot-section-head"><span>已釘選</span></div>
            {sections.pinned.length ? <ul>{sections.pinned.map(agent => row(agent))}</ul> : <p className="bot-section-empty">放開以釘選到最上面</p>}
          </section>
        ) : null}
        {visibleRooms.length && onSelectRoom ? (
          <section className="bot-section room-section">
            <div className="bot-section-head"><span>群組對話</span>{onCreateRoom ? <button type="button" className="icon-btn small" aria-label="新增群組對話" title="新增群組對話" onClick={onCreateRoom}><Plus /></button> : null}</div>
            <ul>{visibleRooms.map(room => <RoomRowItem key={room.id} room={room} active={room.id === activeRoomId} onSelect={onSelectRoom} unread={!!unread?.(room.id, room.last_at)} />)}</ul>
          </section>
        ) : null}
        {sections.loose.length || (dragBot && sections.rows.length) ? (
          <section className={`bot-section${zoneClass("loose")}`} {...dropZone("loose")}>
            {sections.rows.length ? <div className="bot-section-head"><span>未分區</span></div> : null}
            {sections.loose.length ? <ul>{sections.loose.map(agent => row(agent))}</ul> : <p className="bot-section-empty">放開以移出分區</p>}
          </section>
        ) : null}
        {sections.rows.map(({ group, agents: members }) => <GroupSection key={group.id} group={group} members={members} query={query}
          closed={collapsed.includes(group.id) && !query} onToggle={() => toggle(group.id)} actions={() => groupActions(group)}
          className={zoneClass(`g:${group.id}`)} zone={dropZone(`g:${group.id}`)} row={row}
          onDrag={startDrag && onReorderGroups && groups.length > 1 ? startDrag : undefined} />)}
        {sections.hidden.length || dragBot ? (
          <section className={`bot-section hidden-section${zoneClass("hidden")}`} {...dropZone("hidden")}>
            <div className="bot-section-head">
              <button type="button" className="bot-section-toggle" aria-expanded={collapsed.includes(HIDDEN_OPEN)} onClick={() => toggle(HIDDEN_OPEN)}>
                <ChevronDown className={collapsed.includes(HIDDEN_OPEN) ? "" : "collapsed"} aria-hidden="true" /><span>已隱藏</span><small>{sections.hidden.length}</small>
              </button>
            </div>
            {!sections.hidden.length ? <p className="bot-section-empty">放開以從側欄隱藏</p>
              : collapsed.includes(HIDDEN_OPEN) ? <ul>{sections.hidden.map(agent => row(agent, false))}</ul> : null}
          </section>
        ) : null}
      </nav>
      <footer className="sidebar-foot">
        {onTemplates ? <button type="button" className="btn ghost" onClick={onTemplates} title="Bot 範本"><LayoutGrid /><span>Bot 範本</span></button> : null}
        {onConnectApps ? <button type="button" className="btn ghost" onClick={onConnectApps} title="連接應用程式"><Plug /><span>連接應用程式</span></button> : null}
        {onOrg ? <button type="button" className="btn ghost" onClick={onOrg} title="組織架構"><Network /><span>組織架構</span></button> : null}
        <div className="sidebar-foot-row">
          <button type="button" className="btn ghost" onClick={onSettings} title="設定"><Settings /><span>設定</span></button>
          {onToggleCompact ? <button type="button" className="icon-btn" aria-label={compact ? "展開側欄" : "精簡側欄"} title={`${compact ? "展開側欄" : "精簡側欄"}（Ctrl B）`} onClick={onToggleCompact}><PanelLeft /></button> : null}
        </div>
      </footer>
    </aside>
  );
}

/** A user-made section: right-click (or long-press) its name for its options; drag the name to reorder sections. */
function GroupSection({ group, members, query, closed, onToggle, actions, className, zone, row, onDrag }: {
  group: AgentGroup; members: AgentRow[]; query: string; closed: boolean; onToggle: () => void; actions: () => MenuItem[];
  className: string; zone: Record<string, unknown>; row: (agent: AgentRow) => React.ReactNode; onDrag?: (drag: Drag) => void;
}) {
  const { bind, menu } = useContextMenu(`${group.name} 分區選項`, actions);
  const draggable = !!onDrag && canDrag();
  return (
    <section className={`bot-section${className}`} {...zone}>
      <div className="bot-section-head" {...bind} draggable={draggable || undefined}
        onDragStart={draggable ? event => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", group.name); onDrag!({ kind: "group", id: group.id }); } : undefined}
        onDragEnd={draggable ? () => onDrag!(null) : undefined}>
        <button type="button" className="bot-section-toggle" aria-expanded={!closed} aria-haspopup="menu" onClick={onToggle}>
          <ChevronDown className={closed ? "collapsed" : ""} aria-hidden="true" /><span>{group.name}</span><small>{members.length}</small>
        </button>
        {menu}
      </div>
      {!closed ? (members.length ? <ul>{members.map(agent => row(agent))}</ul>
        : <p className="bot-section-empty">{query ? "" : "把 Bot 拖到這裡，或在 Bot 上按右鍵「移到分區」"}</p>) : null}
    </section>
  );
}
