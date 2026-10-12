import { memo, useMemo, useState } from "react";
import type { AgentGroup, AgentRow } from "../api";
import { Avatar } from "../avatar";
import type { AvatarShape } from "../types";
import { readStorage, writeStorage } from "../storage";
import { ChevronDown, Ellipsis, FolderPlus, Import, Plus, Search, Settings, X } from "../ui/icons";
import { MenuButton, type MenuItem } from "../ui/overlay";
import { listTime } from "./format";
import { useMood } from "./mood";
import { activityStatus } from "./status";

const COLLAPSED_KEY = "lazyhermes.collapsedGroups";

export function avatarProps(agent: Pick<AgentRow, "id" | "name" | "avatar_color" | "avatar_shape" | "avatar_url" | "avatar_traits" | "avatar_expression">) {
  return { lookId: agent.id, name: agent.name, color: agent.avatar_color || undefined, shape: (agent.avatar_shape as AvatarShape | undefined) || "organic", src: agent.avatar_url || undefined,
    eyes: agent.avatar_traits, expression: agent.avatar_expression || undefined };
}

function readCollapsed(): string[] {
  try { const value = JSON.parse(readStorage(COLLAPSED_KEY) || "[]"); return Array.isArray(value) ? value.filter(v => typeof v === "string") : []; }
  catch { return []; }
}

const BotRow = memo(function BotRow({ agent, active, onSelect, actions }: { agent: AgentRow; active: boolean; onSelect: (id: string) => void; actions: MenuItem[] }) {
  const status = activityStatus(agent.activity);
  const busy = status.tone === "busy";
  const mood = useMood(agent.activity?.state, { lastAt: agent.last_at });
  const subtitle = agent.preview?.replace(/\s+/g, " ").trim() || agent.title || agent.description || "開始交辦第一件工作";
  return (
    <li className={`bot-row${active ? " active" : ""}`} data-tone={status.tone}>
      <button type="button" className="bot-row-main" aria-current={active ? "page" : undefined} onClick={() => onSelect(agent.id)}>
        <Avatar {...avatarProps(agent)} size={36} gaze={false} thinking={busy} mood={mood} />
        <span className="bot-row-text">
          <span className="bot-row-top">
            <strong>{agent.name}</strong>
            {status.tone !== "idle" ? <span className={`status-pill ${status.tone}`}>{status.label}</span> : <time>{listTime(agent.last_at)}</time>}
          </span>
          <span className="bot-row-sub">{subtitle}</span>
        </span>
      </button>
      <MenuButton label={`${agent.name} 的選項`} icon={<Ellipsis />} className="icon-btn small bot-row-more" items={actions} />
    </li>
  );
});

export function Sidebar({ agents, groups, activeId, loaded, onSelect, onCreate, onImport, onCreateGroup, onSettings, agentActions, groupActions }: {
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
    const rows = groups.map(group => ({ group, agents: visible.filter(agent => agent.group_id === group.id) }));
    const loose = visible.filter(agent => !agent.group_id || !known.has(agent.group_id));
    return { rows: needle ? rows.filter(row => row.agents.length) : rows, loose, empty: !visible.length };
  }, [agents, groups, query]);

  const createItems: MenuItem[] = [
    { label: "新增 Bot", icon: <Plus />, onSelect: onCreate },
    { label: "匯入 Bot", icon: <Import />, onSelect: onImport },
    { label: "新增群組", icon: <FolderPlus />, onSelect: onCreateGroup, disabled: agents.length === 0 },
  ];

  return (
    <aside className="sidebar" aria-label="Bot 清單">
      <header className="sidebar-head">
        <div className="brand"><img src="/lazyboy-round.svg" width={28} height={28} alt="" /><span>LazyBoy</span></div>
        <MenuButton label="新增" icon={<Plus />} items={createItems} placement="bottom-end" />
      </header>
      <div className="sidebar-search">
        <Search aria-hidden="true" />
        <input type="search" aria-label="搜尋 Bot" placeholder="搜尋 Bot" value={query} onChange={event => setQuery(event.target.value)} />
        {query ? <button type="button" className="icon-btn small" aria-label="清除搜尋" onClick={() => setQuery("")}><X /></button> : null}
      </div>
      <nav className="bot-list">
        {!loaded ? <div className="bot-skeleton" aria-hidden="true"><i /><i /><i /></div> : null}
        {loaded && sections.empty ? (
          <div className="sidebar-empty">
            {query ? <p>找不到符合「{query}」的 Bot</p> : <><p>還沒有 Bot</p><button type="button" className="btn primary" onClick={onCreate}><Plus />建立第一個 Bot</button></>}
          </div>
        ) : null}
        {sections.loose.length ? (
          <section className="bot-section">
            {sections.rows.length ? <div className="bot-section-head"><span>未分組</span></div> : null}
            <ul>{sections.loose.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} />)}</ul>
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
                <MenuButton label={`${group.name} 群組選項`} icon={<Ellipsis />} className="icon-btn small" items={groupActions(group)} />
              </div>
              {!closed ? (members.length ? <ul>{members.map(agent => <BotRow key={agent.id} agent={agent} active={agent.id === activeId} onSelect={onSelect} actions={agentActions(agent)} />)}</ul> : <p className="bot-section-empty">還沒有 Bot，可從 Bot 選單「移到群組」加入</p>) : null}
            </section>
          );
        })}
      </nav>
      <footer className="sidebar-foot">
        <button type="button" className="btn ghost" onClick={onSettings}><Settings />設定</button>
      </footer>
    </aside>
  );
}
