import { useEffect, useState, type ReactNode } from "react";
import { api, type AgentRow, type BotMessage, type PeerFields, type RoomMember, type RoomRow } from "../api";
import { Avatar } from "../avatar";
import { ChatMessageBody, type PreviewFile } from "../markdown";
import { ArrowLeftRight, ChevronDown, ChevronRight, Clock, MessagesSquare, Network, Pencil, RotateCw, Stamp } from "../ui/icons";
import { openApprovals } from "./useNotifications";
import { Modal, ModalHeader } from "../ui/overlay";
import { clock, dayLabel } from "./format";
import { avatarProps } from "./Sidebar";

export const INTENT_LABEL: Record<string, string> = { request: "請求", question: "提問", result: "回覆", fyi: "通知" };
export const ROOM_MIN = 2, ROOM_MAX = 10;

type Face = Pick<AgentRow, "id" | "name"> & Partial<Pick<AgentRow, "avatar_color" | "avatar_shape" | "avatar_url" | "avatar_traits" | "avatar_expression">>;

function faceOf(team: Face[], id?: string, name?: string): Face {
  return team.find(bot => bot.id === id) || { id: id || name || "bot", name: name || "Bot" };
}

/**
 * A hand-off between bots, shown in a bot's own chat as a small centred line (Grok's "Messaged …" / "Message from …").
 * It opens to the message itself, and to the whole exchange between the two bots.
 */
export function PeerEvent({ item, agentId, team, knownPaths, onOpen, onPeer }: {
  item: PeerFields & { content: string; at?: string };
  agentId: string;
  team: Face[];
  knownPaths: string[];
  onOpen: (file: PreviewFile) => void;
  onPeer: (peerId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const outgoing = item.event === "message_out";
  const peer = faceOf(team, outgoing ? item.peer_id : item.from_agent, outgoing ? item.peer_name : item.from_name);
  const kind = INTENT_LABEL[item.intent || "request"] || "訊息";
  const label = outgoing ? <>已傳{kind}給 <b>{peer.name}</b></> : <>來自 <b>{peer.name}</b> 的{kind}</>;
  return (
    <div className={`peer-event${open ? " open" : ""}`} data-direction={outgoing ? "out" : "in"}>
      <button type="button" className="peer-event-line" aria-expanded={open} onClick={() => setOpen(v => !v)}>
        <Avatar {...avatarProps(peer as AgentRow)} size={18} gaze={false} />
        <span>{label}</span>
        <time>{clock(item.at)}</time>
        {open ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
      </button>
      {open ? (
        <div className="peer-event-body">
          <ChatMessageBody content={item.content} agentId={outgoing ? agentId : peer.id} knownPaths={outgoing ? knownPaths : []} onOpen={onOpen} />
          <button type="button" className="link-btn" onClick={() => onPeer(peer.id)}><ArrowLeftRight aria-hidden="true" />查看與 {peer.name} 的所有往來</button>
        </div>
      ) : null}
    </div>
  );
}

const PEER_EVENTS = new Set(["message_in", "message_out"]);
export const isPeerEvent = (item: { role?: string; event?: string; from_agent?: string }) => !!item.event && PEER_EVENTS.has(item.event) || (item.role === "user" && !!item.from_agent);

/** Other things a bot did, shown as event lines too: opened or posted in a group, made a helper bot, changed its look. */
export function TeamEvent({ item, agentId, team, knownPaths, onOpen, onPeer, onOpenRoom, onOpenBot }: {
  item: PeerFields & { role?: string; content: string; at?: string };
  agentId: string;
  team: Face[];
  knownPaths: string[];
  onOpen: (file: PreviewFile) => void;
  onPeer: (peerId: string) => void;
  onOpenRoom?: (roomId: string) => void;
  onOpenBot?: (botId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  if (isPeerEvent(item)) return <PeerEvent item={item} agentId={agentId} team={team} knownPaths={knownPaths} onOpen={onOpen} onPeer={onPeer} />;
  const room = () => item.room_id && onOpenRoom?.(item.room_id);
  let icon = <MessagesSquare aria-hidden="true" />, label: ReactNode = item.content, action: (() => void) | undefined, expandable = false;
  if (item.event === "group_created") { label = <>建立了群組對話 <b>{item.peer_name}</b></>; action = room; }
  else if (item.event === "group_post") { label = <>在群組 <b>{item.peer_name}</b> 發言</>; expandable = true; }
  else if (item.event === "bot_created") {
    const face = faceOf(team, item.peer_id, item.peer_name);
    icon = <Avatar {...avatarProps(face as AgentRow)} size={18} gaze={false} />;
    label = <>建立了 Bot <b>{item.peer_name}</b></>; action = () => item.peer_id && onOpenBot?.(item.peer_id);
  }
  else if (item.event === "profile_updated") { icon = <Pencil aria-hidden="true" />; label = item.content; }
  else if (item.event === "question_expired") { icon = <Clock aria-hidden="true" />; label = item.content; }
  else if (item.event === "auto_continue") { icon = <RotateCw aria-hidden="true" />; label = item.content; }
  else if (item.event === "approval_request") { icon = <Stamp aria-hidden="true" />; label = <>請你核定：<b>{item.content}</b></>; action = openApprovals; }
  else if (item.event === "team_created") {
    icon = <Network aria-hidden="true" />; label = <>建立了團隊 <b>{item.peer_name}</b></>;
    action = item.room_id ? room : () => item.peer_id && onOpenBot?.(item.peer_id);
  }
  return (
    <div className={`peer-event${open ? " open" : ""}`} data-event={item.event}>
      <button type="button" className="peer-event-line" aria-expanded={expandable ? open : undefined} disabled={!action && !expandable}
        onClick={() => expandable ? setOpen(v => !v) : action?.()}>
        {icon}<span>{label}</span><time>{clock(item.at)}</time>
        {expandable ? (open ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />) : action ? <ChevronRight aria-hidden="true" /> : null}
      </button>
      {open ? (
        <div className="peer-event-body">
          <ChatMessageBody content={item.content} agentId={agentId} knownPaths={knownPaths} onOpen={onOpen} />
          {item.room_id ? <button type="button" className="link-btn" onClick={room}><MessagesSquare aria-hidden="true" />打開群組 {item.peer_name}</button> : null}
        </div>
      ) : null}
    </div>
  );
}

/** Several hand-offs in a row fold into one line: "與 [avatars] 2 個 Bot 的 3 則訊息", like Grok's "3 messages with 2 Bots". */
export function PeerGroup({ items, agentId, team, knownPaths, onOpen, onPeer }: {
  items: (PeerFields & { id?: string; role?: string; content: string; at?: string })[];
  agentId: string;
  team: Face[];
  knownPaths: string[];
  onOpen: (file: PreviewFile) => void;
  onPeer: (peerId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const peers = [...new Map(items.map(item => {
    const id = item.event === "message_out" ? item.peer_id : item.from_agent;
    const name = item.event === "message_out" ? item.peer_name : item.from_name;
    return [id || name, faceOf(team, id, name)] as const;
  })).values()];
  const who = peers.length === 1 ? peers[0].name : `${peers.length} 個 Bot`;
  return (
    <div className={`peer-event peer-group${open ? " open" : ""}`}>
      <button type="button" className="peer-event-line" aria-expanded={open} onClick={() => setOpen(v => !v)}>
        <span className="peer-faces">{peers.slice(0, 3).map(face => <Avatar key={face.id} {...avatarProps(face as AgentRow)} size={18} gaze={false} />)}</span>
        <span>與 <b>{who}</b> 的 {items.length} 則訊息</span>
        <time>{clock(items[items.length - 1].at)}</time>
        {open ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
      </button>
      {open ? <div className="peer-group-list">{items.map((item, index) => <PeerEvent key={item.id || index} item={item} agentId={agentId} team={team} knownPaths={knownPaths} onOpen={onOpen} onPeer={onPeer} />)}</div> : null}
    </div>
  );
}

/** "A ⇄ B": every message the two bots have sent each other, oldest first. */
export function PeerDialog({ agentId, peerId, team, onClose }: { agentId: string; peerId: string; team: Face[]; onClose: () => void }) {
  const [rows, setRows] = useState<BotMessage[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => { api.peerMessages(agentId, peerId).then(data => setRows(data.messages)).catch(err => setError(err instanceof Error ? err.message : String(err))); }, [agentId, peerId]);
  const a = faceOf(team, agentId), b = faceOf(team, peerId);
  return (
    <Modal label={`${a.name} 與 ${b.name}`} onClose={onClose} className="peer-dialog">
      <ModalHeader title={<span className="peer-title"><Avatar {...avatarProps(a as AgentRow)} size={24} gaze={false} />{a.name}<ArrowLeftRight aria-hidden="true" /><Avatar {...avatarProps(b as AgentRow)} size={24} gaze={false} />{b.name}</span>} onClose={onClose} />
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      {!rows ? <p className="muted">讀取中…</p> : !rows.length ? <p className="muted">{a.name} 和 {b.name} 還沒有往來。</p> : (
        <ol className="peer-thread">
          {rows.map((row, index) => {
            const from = faceOf(team, row.from, row.from_name);
            const newDay = index === 0 || dayLabel(rows[index - 1].at) !== dayLabel(row.at);
            return (
              <li key={row.id} data-side={row.from === agentId ? "a" : "b"}>
                {newDay ? <div className="day-sep"><span>{dayLabel(row.at)}</span></div> : null}
                <header><Avatar {...avatarProps(from as AgentRow)} size={20} gaze={false} /><strong>{from.name}</strong><small>{INTENT_LABEL[row.intent] || "訊息"} · {clock(row.at)}</small></header>
                <div className="peer-thread-text"><ChatMessageBody content={row.text} agentId={row.from} knownPaths={[]} onOpen={() => {}} /></div>
              </li>
            );
          })}
        </ol>
      )}
    </Modal>
  );
}

/** A group chat's face: its members' avatars in a 2×2 mosaic, like Grok's group rows. */
export function RoomAvatar({ members, size = 36 }: { members: RoomMember[]; size?: number }) {
  const shown = members.slice(0, 4);
  const cell = Math.round(size / 2);
  return (
    <span className="room-avatar" style={{ width: size, height: size }} aria-hidden="true">
      {shown.map(member => <Avatar key={member.id} {...avatarProps(member as AgentRow)} size={shown.length === 1 ? size : cell} gaze={false} />)}
    </span>
  );
}

/** Create a group chat with two to six bots, or change its name and members. */
export function RoomDialog({ room, agents, onClose, onSaved }: {
  room?: RoomRow;
  agents: AgentRow[];
  onClose: () => void;
  onSaved: (room: RoomRow) => void;
}) {
  const [name, setName] = useState(room && !room.auto_name ? room.name : "");
  const [members, setMembers] = useState<string[]>(room?.member_ids || []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const toggle = (id: string) => setMembers(current => current.includes(id) ? current.filter(item => item !== id) : current.length >= ROOM_MAX ? current : [...current, id]);
  const ready = members.length >= ROOM_MIN && members.length <= ROOM_MAX;
  async function save() {
    if (!ready || busy) return;
    setBusy(true); setError("");
    try {
      onSaved(room ? await api.updateRoom(room.id, { name: name.trim() || undefined, member_ids: members }) : await api.createRoom(members, name.trim()));
      onClose();
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
  }
  const title = room ? "編輯群組對話" : "新增群組對話";
  return (
    <Modal label={title} onClose={() => { if (!busy) onClose(); }} className="group-dialog room-dialog">
      <form onSubmit={event => { event.preventDefault(); void save(); }}>
        <ModalHeader title={title} onClose={onClose} disabled={busy} />
        <p className="dialog-lead">選 {ROOM_MIN} 到 {ROOM_MAX} 個 Bot。在群組裡用 @名字 指定誰負責；沒有點名時，由成員自己判斷要不要回答。</p>
        <label className="field">
          <span>名稱 <small>可以留白，用成員名字命名</small></span>
          <input value={name} maxLength={60} placeholder="例如：新品發表小組" onChange={event => setName(event.target.value)} />
        </label>
        <fieldset className="member-picker">
          <legend>成員 <small>{members.length} / {ROOM_MAX}</small></legend>
          {agents.length < ROOM_MIN ? <p className="muted">至少需要兩個 Bot 才能開群組對話。</p> : null}
          <ul>
            {agents.map(agent => {
              const picked = members.includes(agent.id);
              return (
                <li key={agent.id}>
                  <button type="button" className={picked ? "picked" : ""} aria-pressed={picked} disabled={!picked && members.length >= ROOM_MAX} onClick={() => toggle(agent.id)}>
                    <Avatar {...avatarProps(agent)} size={28} gaze={false} />
                    <span><strong>{agent.name}</strong>{agent.title ? <small>{agent.title}</small> : null}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </fieldset>
        {error ? <p className="pane-error" role="alert">{error}</p> : null}
        <div className="dialog-actions">
          <button type="button" className="outline" disabled={busy} onClick={onClose}>取消</button>
          <button type="submit" className="primary" disabled={busy || !ready}>{busy ? "儲存中…" : room ? "儲存" : "建立群組對話"}</button>
        </div>
      </form>
    </Modal>
  );
}
