import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type AgentRow, type Approval, type OrgBot, type OrgChart, type TeamPerson, type TeamTemplate } from "../api";
import { Avatar } from "../avatar";
import { ChatMessageBody } from "../markdown";
import { Check, ChevronDown, ChevronRight, Crown, MessagesSquare, Network, Plus, ShieldCheck, Stamp, Star, Trash2, X } from "../ui/icons";
import { Modal, ModalHeader, toast } from "../ui/overlay";
import { ago } from "./format";
import { avatarProps } from "./Sidebar";
import { ROOM_MAX } from "./Team";

const STATE_LABEL: Record<OrgBot["state"], string> = { working: "工作中", waiting: "等你回答", queued: "排隊中", idle: "閒置" };
const message = (err: unknown) => err instanceof Error ? err.message : String(err);

function Face({ agents, id, size = 28 }: { agents: AgentRow[]; id: string; size?: number }) {
  const agent = agents.find(row => row.id === id);
  return agent ? <Avatar {...avatarProps(agent)} size={size} gaze={false} /> : <span className="org-face-blank" style={{ width: size, height: size }} />;
}

/** The bots under `id` (all levels), so a reporting line can never be set into a loop. */
function below(bots: OrgBot[], id: string) {
  const found = new Set<string>();
  let frontier = [id];
  while (frontier.length) {
    frontier = bots.filter(bot => frontier.includes(bot.reports_to_id) && !found.has(bot.id)).map(bot => bot.id);
    frontier.forEach(item => found.add(item));
  }
  return found;
}

function BotNode({ bot, chart, agents, selected, onSelect }: { bot: OrgBot; chart: OrgChart; agents: AgentRow[]; selected: string | null; onSelect: (id: string | null) => void }) {
  const children = chart.bots.filter(item => item.reports_to_id === bot.id);
  const open = selected === bot.id;
  return (
    <li className="org-node" data-state={bot.state}>
      <button type="button" className={`org-card${open ? " is-open" : ""}`} aria-expanded={open} onClick={() => onSelect(open ? null : bot.id)}>
        <Face agents={agents} id={bot.id} size={34} />
        <span className="org-card-text">
          <strong>{bot.name}{bot.primary ? <Star className="org-badge star" aria-label="主要 Bot" /> : null}{bot.can_manage ? <ShieldCheck className="org-badge" aria-label="有團隊管理權限" /> : null}</strong>
          <small>{[bot.title, bot.team].filter(Boolean).join(" · ") || "未設定職稱"}</small>
          {bot.current ? <em>{bot.current.task}</em> : null}
        </span>
        <span className="org-state"><i aria-hidden="true" />{bot.state === "queued" ? `排隊 ${bot.queued}` : STATE_LABEL[bot.state]}</span>
        {bot.approvals.length ? <span className="org-pending" title="等你核定">{bot.approvals.length}</span> : null}
      </button>
      {children.length ? (
        <ul className="org-children">
          {children.map(child => <BotNode key={child.id} bot={child} chart={chart} agents={agents} selected={selected} onSelect={onSelect} />)}
        </ul>
      ) : null}
    </li>
  );
}

function NodePanel({ bot, chart, onChanged, onOpenBot }: { bot: OrgBot; chart: OrgChart; onChanged: () => void; onOpenBot: (id: string) => void }) {
  const [busy, setBusy] = useState(false);
  const blocked = useMemo(() => below(chart.bots, bot.id), [chart.bots, bot.id]);
  async function save(fields: { reports_to?: string; can_manage?: boolean }) {
    setBusy(true);
    try { await api.updateAgent(bot.id, fields); onChanged(); }
    catch (err) { toast(message(err), "error"); }
    finally { setBusy(false); }
  }
  return (
    <div className="org-panel" aria-label={`${bot.name} 的組織設定`}>
      <header><strong>{bot.name}</strong><small>{bot.title}</small></header>
      {bot.waiting_for ? <p className="org-wait">等你回答：{bot.waiting_for}</p> : null}
      {bot.current ? <p><b>正在做</b>{bot.current.task}<small>（{bot.current.from}）</small></p> : null}
      <label>向誰報告
        <select value={bot.reports_to_id} disabled={busy} onChange={e => void save({ reports_to: e.target.value })}>
          <option value="">你（董事長）</option>
          {chart.bots.filter(item => item.id !== bot.id && !blocked.has(item.id)).map(item => <option key={item.id} value={item.id}>{item.name}{item.title ? `（${item.title}）` : ""}</option>)}
        </select>
      </label>
      <label className="org-toggle">
        <input type="checkbox" checked={bot.can_manage} disabled={busy} onChange={e => void save({ can_manage: e.target.checked })} />
        <span><b>團隊管理權限</b><small>可以替你建立團隊、新增 Bot，調整其他 Bot 的職稱、人設、報告對象與分區。不能刪除 Bot。</small></span>
      </label>
      {bot.recent.length ? (
        <details>
          <summary>最近完成 {bot.recent.length} 件</summary>
          <ul className="org-recent">{bot.recent.map((item, i) => <li key={i}><b>{item.task}</b><small>{item.from} · {ago(item.at)}{item.verdict === "failed" ? " · 未完成" : ""}</small>{item.result ? <span>{item.result}</span> : null}</li>)}</ul>
        </details>
      ) : null}
      <button type="button" className="outline" onClick={() => onOpenBot(bot.id)}><MessagesSquare />打開對話</button>
    </div>
  );
}

function ApprovalItems({ items, onSaved }: { items: string[]; onSaved: (items: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  async function save(next: string[]) {
    setBusy(true);
    try { onSaved((await api.setApprovalItems(next)).approval_items); setDraft(""); }
    catch (err) { toast(message(err), "error"); }
    finally { setBusy(false); }
  }
  return (
    <section className="org-items" aria-label="董事長核定事項">
      <h3><Stamp aria-hidden="true" />只有你能決定的事</h3>
      <p>Bot 遇到這些事會先送到「待你核定」，等你核准才做；隊友或主管說可以都不算數。</p>
      <ul>
        {items.map(item => <li key={item}>{item}<button type="button" aria-label={`移除 ${item}`} disabled={busy} onClick={() => void save(items.filter(other => other !== item))}><X /></button></li>)}
        <li className="org-items-add">
          <input value={draft} maxLength={40} placeholder="新增事項，例如：退款" disabled={busy} aria-label="新增核定事項"
            onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && draft.trim()) { e.preventDefault(); void save([...items, draft.trim()]); } }} />
          <button type="button" aria-label="加入" disabled={busy || !draft.trim()} onClick={() => void save([...items, draft.trim()])}><Plus /></button>
        </li>
      </ul>
    </section>
  );
}

/** Who reports to whom, what each bot is doing now, and what only the user decides. */
export function OrgDialog({ agents, onClose, onOpenBot, onCreateTeam }: { agents: AgentRow[]; onClose: () => void; onOpenBot: (id: string) => void; onCreateTeam: () => void }) {
  const [chart, setChart] = useState<OrgChart | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const load = useCallback(() => api.org().then(setChart).catch(err => setError(message(err))), []);
  useEffect(() => {
    void load();
    const timer = window.setInterval(() => { if (!document.hidden) void load(); }, 5000);
    return () => window.clearInterval(timer);
  }, [load]);
  const top = chart?.bots.filter(bot => !bot.reports_to_id) || [];
  const current = chart?.bots.find(bot => bot.id === selected);
  const working = chart?.bots.filter(bot => bot.state === "working").length || 0;
  return (
    <Modal label="組織架構" onClose={onClose} className="org-dialog">
      <ModalHeader title={<span className="org-title"><Network aria-hidden="true" />組織架構</span>} onClose={onClose} />
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      {!chart ? <p className="muted">讀取中…</p> : (
        <div className="org-layout">
          <div className="org-tree-wrap">
            <div className="org-root"><Crown aria-hidden="true" /><span><strong>你</strong><small>董事長 · 唯一最終核定者</small></span>
              <span className="org-summary">{chart.bots.length} 個 Bot · {working} 個工作中</span></div>
            <ul className="org-tree">
              {top.map(bot => <BotNode key={bot.id} bot={bot} chart={chart} agents={agents} selected={selected} onSelect={setSelected} />)}
            </ul>
            <div className="org-actions">
              <button type="button" className="primary" onClick={onCreateTeam}><Plus />建立團隊</button>
              <small>點一個 Bot 可以改它向誰報告、給它團隊管理權限。</small>
            </div>
          </div>
          <aside className="org-side">
            {current ? <NodePanel bot={current} chart={chart} onChanged={() => void load()} onOpenBot={id => { onClose(); onOpenBot(id); }} /> : null}
            <ApprovalItems items={chart.approval_items} onSaved={items => setChart(value => value ? { ...value, approval_items: items } : value)} />
          </aside>
        </div>
      )}
    </Modal>
  );
}

const BLANK: TeamTemplate = { key: "blank", name: "", about: "自己決定主管與成員。", lead: { name: "", title: "主管" }, members: [{ name: "", title: "" }] };

function PersonRow({ person, onChange, onRemove, label }: { person: TeamPerson; onChange: (person: TeamPerson) => void; onRemove?: () => void; label: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="team-person">
      <div className="team-person-row">
        <input aria-label={`${label}名字`} placeholder="名字" value={person.name} maxLength={40} onChange={e => onChange({ ...person, name: e.target.value })} />
        <input aria-label={`${label}職稱`} placeholder="職稱" value={person.title || ""} maxLength={60} onChange={e => onChange({ ...person, title: e.target.value })} />
        <button type="button" className="icon-btn" aria-label={open ? "收起人設" : "編輯人設"} aria-expanded={open} onClick={() => setOpen(v => !v)}>{open ? <ChevronDown /> : <ChevronRight />}</button>
        {onRemove ? <button type="button" className="icon-btn" aria-label={`移除${label}`} onClick={onRemove}><Trash2 /></button> : null}
      </div>
      {open ? <textarea aria-label={`${label}人設`} rows={3} maxLength={4000} placeholder="負責什麼、怎麼做、交付什麼" value={person.persona || ""} onChange={e => onChange({ ...person, persona: e.target.value })} /> : null}
    </div>
  );
}

/** Build a team in one go: pick a template, adjust the people, choose who the lead answers to. */
export function TeamBuilderDialog({ agents, onClose, onCreated }: { agents: AgentRow[]; onClose: () => void; onCreated: (result: Awaited<ReturnType<typeof api.createTeam>>) => void }) {
  const [templates, setTemplates] = useState<TeamTemplate[]>([]);
  const [picked, setPicked] = useState<TeamTemplate | null>(null);
  const [name, setName] = useState("");
  const [lead, setLead] = useState<TeamPerson>({ name: "" });
  const [members, setMembers] = useState<TeamPerson[]>([]);
  const [reportsTo, setReportsTo] = useState("");
  const [chat, setChat] = useState(true);
  const [daily, setDaily] = useState(false);
  const [primary, setPrimary] = useState(false);
  const [manage, setManage] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => { api.org().then(data => setTemplates(data.templates)).catch(err => setError(message(err))); }, []);
  function choose(template: TeamTemplate) {
    const taken = new Set(agents.map(agent => agent.name));
    // A name already used by another bot gets a number, so a template can be used twice.
    const fresh = (person: TeamPerson) => {
      if (!person.name || !taken.has(person.name)) return { ...person };
      let n = 2; while (taken.has(`${person.name} ${n}`)) n++;
      return { ...person, name: `${person.name} ${n}` };
    };
    setPicked(template); setName(template.name); setLead(fresh(template.lead)); setMembers(template.members.map(fresh));
    const company = template.key === "company";
    setPrimary(company); setManage(company); setDaily(company); setChat(template.members.length + 1 <= ROOM_MAX);
  }
  const people = [lead, ...members];
  const names = people.map(person => person.name.trim());
  const duplicate = names.find((value, i) => value && (names.indexOf(value) !== i || agents.some(agent => agent.name === value)));
  const ready = name.trim() && lead.name.trim() && members.length && members.every(member => member.name.trim()) && !duplicate;
  async function create() {
    setBusy(true); setError("");
    try {
      const clean = (person: TeamPerson) => ({ name: person.name.trim(), title: person.title?.trim() || "", persona: person.persona?.trim() || "" });
      onCreated(await api.createTeam({ name: name.trim(), lead: clean(lead), members: members.map(clean), reports_to: reportsTo, group_chat: chat && people.length <= ROOM_MAX,
                                       daily_report: daily, lead_primary: primary, lead_manage: manage }));
    } catch (err) { setError(message(err)); setBusy(false); }
  }
  return (
    <Modal label="建立團隊" onClose={() => { if (!busy) onClose(); }} className="team-builder">
      <ModalHeader title={picked ? `建立團隊：${picked.name || "自訂"}` : "建立團隊"} onClose={onClose} disabled={busy} />
      {!picked ? (
        <>
          <p className="dialog-lead">一次建立主管和成員：成員都向主管報告，主管向你（或你指定的 Bot）報告，放進同名的側欄分區。</p>
          <div className="team-templates">
            {[...templates, BLANK].map(template => (
              <button key={template.key} type="button" onClick={() => choose(template)}>
                <strong>{template.name || "自訂團隊"}</strong>
                <small>{template.about}</small>
                {template.members.length && template.lead.name ? <span>{[template.lead.name, ...template.members.map(m => m.name)].join("、")}</span> : null}
              </button>
            ))}
          </div>
        </>
      ) : (
        <form className="team-form" onSubmit={e => { e.preventDefault(); if (ready && !busy) void create(); }}>
          <label>團隊名稱<input value={name} maxLength={40} onChange={e => setName(e.target.value)} required /></label>
          <fieldset><legend>主管</legend><PersonRow person={lead} onChange={setLead} label="主管" /></fieldset>
          <fieldset>
            <legend>成員 <small>{members.length} 位，最多 9 位</small></legend>
            {members.map((member, i) => <PersonRow key={i} person={member} label="成員" onChange={next => setMembers(list => list.map((item, j) => j === i ? next : item))} onRemove={members.length > 1 ? () => setMembers(list => list.filter((_, j) => j !== i)) : undefined} />)}
            {members.length < 9 ? <button type="button" className="link-btn" onClick={() => setMembers(list => [...list, { name: "", title: "" }])}><Plus />加一位成員</button> : null}
          </fieldset>
          <label>主管向誰報告<select value={reportsTo} onChange={e => setReportsTo(e.target.value)}>
            <option value="">你（董事長）</option>
            {agents.map(agent => <option key={agent.id} value={agent.id}>{agent.name}{agent.title ? `（${agent.title}）` : ""}</option>)}
          </select></label>
          <fieldset className="team-options">
            <label><input type="checkbox" checked={chat && people.length <= ROOM_MAX} disabled={people.length > ROOM_MAX} onChange={e => setChat(e.target.checked)} />開一個團隊群組對話</label>
            <label><input type="checkbox" checked={daily} onChange={e => setDaily(e.target.checked)} />主管每個工作日 18:00 向你回報團隊進度</label>
            <label><input type="checkbox" checked={manage} onChange={e => setManage(e.target.checked)} />給主管團隊管理權限（可以替你建團隊、調整 Bot）</label>
            <label><input type="checkbox" checked={primary} onChange={e => setPrimary(e.target.checked)} />把主管設為主要 Bot</label>
          </fieldset>
          {duplicate ? <p className="pane-error">已經有叫「{duplicate}」的 Bot，請換個名字。</p> : null}
          {error ? <p className="pane-error" role="alert">{error}</p> : null}
          <small className="team-note">會建立 {people.length} 個新 Bot，每個都有自己的電腦，第一次收到工作時才開機。</small>
          <div className="dialog-actions">
            <button type="button" disabled={busy} onClick={() => setPicked(null)}>換範本</button>
            <button type="submit" className="primary" disabled={!ready || busy}>{busy ? "建立中…" : `建立 ${people.length} 個 Bot`}</button>
          </div>
        </form>
      )}
    </Modal>
  );
}

function ApprovalCard({ row, agents, onDone }: { row: Approval; agents: AgentRow[]; onDone: (row: Approval) => void }) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState<"" | "approve" | "reject">("");
  async function decide(decision: "approve" | "reject") {
    setBusy(decision);
    try { onDone(await api.decideApproval(row.id, decision, note)); toast(decision === "approve" ? `已核准：${row.subject}` : `已退回：${row.subject}`, "ok"); }
    catch (err) { toast(message(err), "error"); setBusy(""); }
  }
  return (
    <article className="approval-card">
      <header><Face agents={agents} id={row.agent_id} size={26} /><span><strong>{row.agent_name}</strong><small>{ago(row.created_at)}{row.from_name ? ` · ${row.from_name} 交辦的工作` : ""}</small></span></header>
      <h3>{row.subject}</h3>
      {row.details ? <div className="approval-details"><ChatMessageBody content={row.details} agentId={row.agent_id} knownPaths={[]} onOpen={() => {}} /></div> : null}
      <textarea rows={2} value={note} maxLength={2000} placeholder="給 Bot 的備註（選填），例如：上限 3500 元" aria-label="核定備註" onChange={e => setNote(e.target.value)} />
      <div className="dialog-actions">
        <button type="button" disabled={!!busy} onClick={() => void decide("reject")}>{busy === "reject" ? "退回中…" : "退回"}</button>
        <button type="button" className="primary" disabled={!!busy} onClick={() => void decide("approve")}><Check />{busy === "approve" ? "核准中…" : "核准"}</button>
      </div>
    </article>
  );
}

/** Decisions waiting for the user. They never expire; the answer goes back to the bot as the user's own message. */
export function ApprovalsDialog({ agents, onClose, onChanged }: { agents: AgentRow[]; onClose: () => void; onChanged: () => void }) {
  const [rows, setRows] = useState<Approval[] | null>(null);
  const [history, setHistory] = useState<Approval[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    api.approvals("pending").then(data => setRows(data.approvals)).catch(err => setError(message(err)));
    api.approvals("all").then(data => setHistory(data.approvals.filter(row => row.state !== "pending").slice(0, 10))).catch(() => {});
  }, []);
  return (
    <Modal label="待你核定" onClose={onClose} className="approvals-dialog">
      <ModalHeader title={<span className="org-title"><Stamp aria-hidden="true" />待你核定</span>} onClose={onClose} />
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      {!rows ? <p className="muted">讀取中…</p> : !rows.length ? <p className="muted">目前沒有需要你核定的事。</p> : (
        <div className="approval-list">
          {rows.map(row => <ApprovalCard key={row.id} row={row} agents={agents} onDone={done => { setRows(list => list?.filter(item => item.id !== row.id) || null); setHistory(list => [done, ...list]); onChanged(); }} />)}
        </div>
      )}
      {history.length ? (
        <details className="approval-history">
          <summary>最近的決定</summary>
          <ul>{history.map(row => <li key={row.id} data-state={row.state}><b>{row.state === "approved" ? "核准" : row.state === "rejected" ? "退回" : "已撤回"}</b><span>{row.subject}</span><small>{row.agent_name} · {ago(row.decided_at || row.created_at)}</small></li>)}</ul>
        </details>
      ) : null}
    </Modal>
  );
}
