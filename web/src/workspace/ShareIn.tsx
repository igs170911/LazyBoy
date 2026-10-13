import { useRef } from "react";
import type { AgentRow, RoomRow } from "../api";
import { Avatar } from "../avatar";
import { useDialogFocus } from "../dialog-focus";
import { X } from "../ui/icons";
import { avatarProps } from "./Sidebar";
import { RoomAvatar } from "./Team";

/** What another app shared into LazyBoy (Android's share sheet), waiting for a conversation to go to. */
export function sharedText(search: string) {
  const params = new URLSearchParams(search);
  const parts = [params.get("share_title"), params.get("share_text"), params.get("share_url")].map(value => (value || "").trim()).filter(Boolean);
  // Many apps put the link in the text as well; keep it once.
  const unique = parts.filter((part, index) => !parts.slice(0, index).some(earlier => earlier.includes(part)));
  return unique.join("\n");
}

/** Grok's share sheet: pick the chat, and the shared words land in its composer to add a note before sending. */
export function ShareInDialog({ text, agents, rooms, onPickBot, onPickRoom, onClose }: {
  text: string; agents: AgentRow[]; rooms: RoomRow[];
  onPickBot: (id: string) => void; onPickRoom: (id: string) => void; onClose: () => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  useDialogFocus(root, onClose);
  return (
    <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <div ref={root} role="dialog" aria-modal="true" aria-label="分享到 LazyBoy" className="dialog share-in-dialog">
        <div className="dialog-title"><h2>分享到哪個對話？</h2><button type="button" className="icon-btn" aria-label="關閉" onClick={onClose}><X /></button></div>
        <blockquote className="share-in-preview">{text.length > 300 ? `${text.slice(0, 300)}…` : text}</blockquote>
        <ul className="share-in-list">
          {agents.filter(a => !a.hidden).map(agent => (
            <li key={agent.id}><button type="button" onClick={() => onPickBot(agent.id)}><Avatar {...avatarProps(agent)} size={28} gaze={false} /><span>{agent.name}</span>{agent.title ? <small>{agent.title}</small> : null}</button></li>
          ))}
          {rooms.map(room => (
            <li key={room.id}><button type="button" onClick={() => onPickRoom(room.id)}><RoomAvatar members={room.members} size={28} /><span>{room.name}</span><small>群組</small></button></li>
          ))}
        </ul>
      </div>
    </div>
  );
}
