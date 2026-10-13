import { memo, useMemo, useState } from "react";
import type { AgentGroup, AgentRow, RoomRow } from "../api";
import { Avatar } from "../avatar";
import type { AvatarShape } from "../types";
import { readStorage, writeStorage } from "../storage";
import { ChevronDown, Ellipsis, FolderPlus, Import, MessagesSquare, Plus, Search, Settings, X } from "../ui/icons";
import { RoomAvatar } from "./Team";
import { MenuButton, type MenuItem } from "../ui/overlay";
import { listTime } from "./format";
import { useMood } from "./mood";
import { activityStatus } from "./status";

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

const BotRow = memo(function BotRow({ agent, active, onSelect, actions, unread = false }: { agent: AgentRow; active: boolean; onSelect: (id: string) => void; actions: MenuItem[]; unread?: boolean }) {
  const status = activityStatus(agent.activity);
  const busy = status.tone === "busy";
  const mood = useMood(agent.activity?.state, { lastAt: agent.last_at });
  const subtitle = agent.preview?.replace(/\s+/g, " ").trim() || agent.title || agent.description || "開始交辦第一件工作";
  return (
    <li className={`bot-row${active ? " active" : ""}${unread ? " unread" : ""}`} data-tone={status.tone}>
      <button type="button" className="bot-row-main" aria-current={active ? "page" : undefined} onClick={() => onSelect(agent.id)}>
        <Avatar {...avatarProps(agent)} size={36} gaze={false} thinking={busy} mood={mood} />
        <span className="bot-row-text">
          <span className="bot-row-top">
            <strong>{agent.name}</strong>
            {status.tone !== "idle" ? <span className={`status-pill ${status.tone}`}>{status.label}</span> : <time>{listTime(agent.last_at)}</time>}
            {unread ? <i className="unread-dot" aria-label="有新訊息" /> : null}
          </span>
          <span className="bot-row-sub">{subtitle}</span>
        </span>
      </button>
      <MenuButton label={`${agent.name} 的選項`} icon={<Ellipsis />} className="icon-btn small bot-row-more" items={actions} />
    </li>
  );
});

/** A group chat row, the way Grok lists group chats next to bots: avatar mosaic, name, latest line, who is answering. */
const RoomRowItem = memo(function RoomRowItem({ room, active, onSelect, unread = false }: { room: RoomRow; active: boolean; onSelect: (id: string) => void; unread?: boolean }) {
  const working = room.members.filter(m => room.working.includes(m.id));
  return (
    <li className={`bot-row room-row${active ? " active" : ""}${unread ? " unread" : ""}`} data-tone={working.length ? "busy" : "idle"}>
      <button type="button" className="bot-row-main" aria-current={active ? "page" : undefined} onClick={() => onSelect(room.id)}>
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

export function Sidebar({ agents, groups, activeId, loaded, onSelect, onCreate, onImport, onCreateGroup, onSettings, agentActions, groupActions, rooms = [], activeRoomId = null, onSelectRoom, onCreateRoom, unread }: {
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
}) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>(readCollapsed);
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

  return (
    <aside className="sidebar" aria-label="Bot 清單">
      <header className="sidebar-head">
        <div className="brand"><img src="/lazyboy-round.svg" width={28} height={28} alt="" /><span>LazyBoy</span></div>
        <MenuButton label="新增" icon={<Plus />} items={createItems} placement="bottom-end" />
      </header>
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
        {!loaded ? <div className="bot-skeleton" aria-hidden="true"><i /><i /><i /></div> : null}
        {loaded && sections.empty ? (
          <div className="sidebar-empty">
            {query ? <p>找不到符合「{query}」的 Bot</p> : <><p>還沒有 Bot</p><button type="button" className="btn primary" onClick={onCreate}><Plus />建立第一個 Bot</button></>}
          </div>
        ) : null}
        {sections.pinned.length ? (
          <section className="bot-section pinned-section">
            <div className="bot-section-head"><span>已釘選</span></div>
            <ul>{sections.pinned.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} unread={!!unread?.(agent.id, agent.last_at)} />)}</ul>
          </section>
        ) : null}
        {visibleRooms.length && onSelectRoom ? (
          <section className="bot-section room-section">
            <div className="bot-section-head"><span>群組對話</span>{onCreateRoom ? <button type="button" className="icon-btn small" aria-label="新增群組對話" title="新增群組對話" onClick={onCreateRoom}><Plus /></button> : null}</div>
            <ul>{visibleRooms.map(room => <RoomRowItem key={room.id} room={room} active={room.id === activeRoomId} onSelect={onSelectRoom} unread={!!unread?.(room.id, room.last_at)} />)}</ul>
          </section>
        ) : null}
        {sections.loose.length ? (
          <section className="bot-section">
            {sections.rows.length ? <div className="bot-section-head"><span>未分區</span></div> : null}
            <ul>{sections.loose.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} unread={!!unread?.(agent.id, agent.last_at)} />)}</ul>
          </section>
        ) : null}
        {sections.rows.map(({ group, agents: members }) => {
          const closed = collapsed.includes(group.id) && !query;
          return (
            <section className="bot-section" key={group.id}>
              <div className="bot-section-head">
                <button type="button" className="bot-section-toggle" aria-expanded={!closed} onClick={() => toggle(group.id)}>
                  <ChevronDown className={closed ? "collapsed" : ""} aria-hidden="true" /><span>{group.name}</span><small>{members.length}</small>
                </button>
                <MenuButton label={`${group.name} 分區選項`} icon={<Ellipsis />} className="icon-btn small" items={groupActions(group)} />
              </div>
              {!closed ? (members.length ? <ul>{members.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} unread={!!unread?.(agent.id, agent.last_at)} />)}</ul> : <p className="bot-section-empty">還沒有 Bot，可從 Bot 選單「移到分區」加入</p>) : null}
            </section>
          );
        })}
        {sections.hidden.length ? (
          <section className="bot-section hidden-section">
            <div className="bot-section-head">
              <button type="button" className="bot-section-toggle" aria-expanded={collapsed.includes(HIDDEN_OPEN)} onClick={() => toggle(HIDDEN_OPEN)}>
                <ChevronDown className={collapsed.includes(HIDDEN_OPEN) ? "" : "collapsed"} aria-hidden="true" /><span>已隱藏</span><small>{sections.hidden.length}</small>
              </button>
            </div>
            {collapsed.includes(HIDDEN_OPEN) ? <ul>{sections.hidden.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} />)}</ul> : null}
          </section>
        ) : null}
      </nav>
      <footer className="sidebar-foot">
        <button type="button" className="btn ghost" onClick={onSettings}><Settings />設定</button>
      </footer>
    </aside>
  );
}
