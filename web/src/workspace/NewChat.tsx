import { useEffect, useMemo, useRef, useState } from "react";
import { api, type AgentRow, type RoomRow, type SavedTemplate } from "../api";
import { randomUUID } from "../uuid";
import { Avatar } from "../avatar";
import { BOT_TEMPLATES, templatePreset, type BotTemplate } from "../bot-templates";
import { useDialogFocus } from "../dialog-focus";
import { Check, FolderPlus, Import, LoaderCircle, MessagesSquare, Network, Plus, Search, Trash2, X } from "../ui/icons";
import { avatarProps } from "./Sidebar";
import { ROOM_MAX } from "./Team";


/** A template's face before the bot exists. */
export function TemplateCard({ template, onPick }: { template: BotTemplate; onPick: (template: BotTemplate) => void }) {
  return (
    <button type="button" className="template-card" onClick={() => onPick(template)}>
      <Avatar lookId={`template-${template.id}`} name={template.name} color={template.avatar_color} shape={template.avatar_shape as never} size={40} gaze={false} />
      <span>
        <strong>{template.name}</strong>
        <small>{template.blurb}</small>
      </span>
    </button>
  );
}

/** The welcome screen's gallery: a few templates to start the first bot from. */
export function WelcomeTemplates({ onPick }: { onPick: (preset: Partial<AgentRow>) => void }) {
  return (
    <section className="welcome-templates">
      <h3>認識你未來的隊友</h3>
      <div className="template-grid">
        {BOT_TEMPLATES.slice(0, 6).map(template => <TemplateCard key={template.id} template={template} onPick={t => onPick(templatePreset(t))} />)}
      </div>
    </section>
  );
}

/**
 * Grok's "New chat": one place to create a bot (from a template or just a name), open a bot, or pick two to six
 * bots for a group chat.
 */
export function NewChatDialog({ agents, onClose, onCreate, onOpenBot, onRoomCreated, onImport, onCreateGroup, onCreated, startAt, onCreateTeam }: {
  /** A lead and members in one go (Org.tsx). */
  onCreateTeam?: () => void;
  /** Open on the template gallery (the sidebar's "Bot 範本"). */
  startAt?: "templates";
  /** A bot made from one of your saved templates. */
  onCreated: (row: AgentRow) => void;
  agents: AgentRow[];
  onClose: () => void;
  onCreate: (preset: Partial<AgentRow> | null) => void;
  onOpenBot: (id: string) => void;
  onRoomCreated: (room: RoomRow) => void;
  onImport: () => void;
  onCreateGroup: () => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState<SavedTemplate[]>([]);
  const [making, setMaking] = useState<string | null>(null);
  useDialogFocus(root, () => { if (!busy) onClose(); });
  useEffect(() => { api.savedTemplates().then(data => setSaved(Array.isArray(data?.templates) ? data.templates : [])).catch(() => undefined); }, []);
  const gallery = useRef<HTMLElement>(null);
  useEffect(() => { if (startAt === "templates") gallery.current?.scrollIntoView({ block: "start" }); }, [startAt]);
  /** Your own templates make a ready bot at once, skills included. */
  async function fromSaved(template: SavedTemplate) {
    setMaking(template.id); setError("");
    try {
      const full = await api.savedTemplate(template.id);
      const row = await api.importTemplate(randomUUID(), full, template.name, "");
      onClose(); onCreated(row);
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setMaking(null); }
  }
  async function forget(template: SavedTemplate) {
    try { await api.deleteSavedTemplate(template.id); setSaved(current => current.filter(item => item.id !== template.id)); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); }
  }
  const needle = query.trim().toLowerCase();
  const bots = useMemo(() => agents.filter(a => !needle || [a.name, a.title].some(v => v?.toLowerCase().includes(needle)))
    .sort((a, b) => (Date.parse(b.last_at || "") || 0) - (Date.parse(a.last_at || "") || 0)), [agents, needle]);
  const templates = BOT_TEMPLATES.filter(t => !needle || [t.name, t.title, t.blurb, ...t.tags].some(v => v.toLowerCase().includes(needle)));
  const exact = agents.some(a => a.name.trim().toLowerCase() === needle);
  const toggle = (id: string) => setPicked(current => current.includes(id) ? current.filter(item => item !== id) : current.length >= ROOM_MAX ? current : [...current, id]);
  const done = (run: () => void) => { onClose(); run(); };

  async function startRoom() {
    if (picked.length < 2 || busy) return;
    setBusy(true); setError("");
    try { const room = await api.createRoom(picked); onClose(); onRoomCreated(room); }
    catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
  }

  return (
    <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget && !busy) onClose(); }}>
      <div ref={root} role="dialog" aria-modal="true" aria-label="新增" className="dialog new-chat-dialog">
        <div className="dialog-title">
          <h2>新增</h2>
          <button type="button" className="icon-btn" aria-label="關閉" onClick={onClose}><X /></button>
        </div>
        <div className="new-chat-search">
          <Search aria-hidden="true" />
          <input autoFocus value={query} onChange={event => setQuery(event.target.value)} placeholder="輸入名字建立新 Bot，或搜尋要一起開群組的 Bot"
            onKeyDown={event => {
              if (event.key !== "Enter" || event.nativeEvent.isComposing || !query.trim()) return;
              event.preventDefault();
              if (bots.length === 1 && exact) done(() => onOpenBot(bots[0].id)); else done(() => onCreate({ name: query.trim() }));
            }} />
        </div>
        <div className="new-chat-body">
          <div className="new-chat-actions">
            {query.trim() && !exact ? (
              <button type="button" className="new-chat-action primary-action" onClick={() => done(() => onCreate({ name: query.trim() }))}>
                <span className="new-chat-icon"><Plus /></span><span>建立「{query.trim()}」Bot</span>
              </button>
            ) : (
              <button type="button" className="new-chat-action" onClick={() => done(() => onCreate(null))}>
                <span className="new-chat-icon"><Plus /></span><span>建立新的 Bot</span>
              </button>
            )}
            <button type="button" className="new-chat-action" onClick={() => done(onImport)}><span className="new-chat-icon"><Import /></span><span>匯入 Bot</span></button>
            {onCreateTeam ? <button type="button" className="new-chat-action" onClick={() => done(onCreateTeam)}><span className="new-chat-icon"><Network /></span><span>建立團隊</span></button> : null}
            <button type="button" className="new-chat-action" disabled={!agents.length} onClick={() => done(onCreateGroup)}><span className="new-chat-icon"><FolderPlus /></span><span>新增側欄分區</span></button>
          </div>

          {bots.length ? (
            <section className="new-chat-section">
              <h3>{agents.length >= 2 ? `選 2 到 ${ROOM_MAX} 個 Bot 開群組對話，或點名字打開` : "你的 Bot"}</h3>
              <ul className="new-chat-bots">
                {bots.slice(0, 30).map(agent => {
                  const on = picked.includes(agent.id);
                  return (
                    <li key={agent.id} className={on ? "picked" : ""}>
                      <button type="button" className="new-chat-bot" onClick={() => done(() => onOpenBot(agent.id))}>
                        <Avatar {...avatarProps(agent)} size={28} gaze={false} />
                        <span><strong>{agent.name}</strong>{agent.title ? <small>{agent.title}</small> : null}</span>
                      </button>
                      {agents.length >= 2 ? (
                        <button type="button" className="new-chat-check" role="checkbox" aria-checked={on} aria-label={`把 ${agent.name} 加入群組`}
                          disabled={!on && picked.length >= ROOM_MAX} onClick={() => toggle(agent.id)}>{on ? <Check /> : null}</button>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            </section>
          ) : null}

          {saved.filter(t => !needle || [t.name, t.title, t.blurb].some(v => v?.toLowerCase().includes(needle))).length ? (
            <section className="new-chat-section" ref={startAt === "templates" ? gallery : undefined}>
              <h3>你的範本</h3>
              <div className="template-grid">
                {saved.filter(t => !needle || [t.name, t.title, t.blurb].some(v => v?.toLowerCase().includes(needle))).map(template => (
                  <div key={template.id} className="saved-template">
                    <button type="button" className="template-card" disabled={!!making} onClick={() => void fromSaved(template)}>
                      {making === template.id ? <LoaderCircle className="spin" /> : <Avatar lookId={`saved-${template.id}`} name={template.name} color={template.avatar_color} shape={template.avatar_shape as never} size={40} gaze={false} />}
                      <span><strong>{template.name}</strong><small>{[template.title, template.skill_count ? `${template.skill_count} 個技能` : ""].filter(Boolean).join(" · ") || template.blurb}</small></span>
                    </button>
                    <button type="button" className="icon-btn small saved-template-remove" aria-label={`刪除範本 ${template.name}`} title="刪除這個範本" onClick={() => void forget(template)}><Trash2 /></button>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          {templates.length ? (
            <section className="new-chat-section" ref={startAt === "templates" && !saved.length ? gallery : undefined}>
              <h3>認識你未來的隊友</h3>
              <div className="template-grid">
                {templates.map(template => <TemplateCard key={template.id} template={template} onPick={t => done(() => onCreate(templatePreset(t)))} />)}
              </div>
            </section>
          ) : null}
        </div>
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        {picked.length ? (
          <div className="new-chat-foot">
            <span>已選 {picked.length} 個 Bot{picked.length < 2 ? "，再選一個" : ""}</span>
            <button type="button" className="btn ghost" onClick={() => setPicked([])}>清除</button>
            <button type="button" className="btn primary" disabled={picked.length < 2 || busy} onClick={() => void startRoom()}>
              <MessagesSquare />{busy ? "建立中…" : "開群組對話"}
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}
