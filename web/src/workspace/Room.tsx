import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { api, type AgentRow, type RoomDetail, type RoomMember, type RoomMessage, type RoomRow, type Upload } from "../api";
import { Avatar } from "../avatar";
import { ChatMessageBody, type PreviewFile } from "../markdown";
import { ArrowUp, AtSign, ChevronDown, ChevronLeft, Ellipsis, Eraser, LoaderCircle, Paperclip, Pencil, Reply, Square, Trash2, Users, X } from "../ui/icons";
import { ConfirmDialog, MenuButton, toast, type MenuItem } from "../ui/overlay";
import { randomUUID } from "../uuid";
import { clock, dayLabel, sameDay } from "./format";
import { useMood } from "./mood";
import { avatarProps } from "./Sidebar";
import { RoomAvatar } from "./Team";
import { FindBar } from "./FindBar";
import { ReactButton, ReactionList, toggleReaction } from "./MessageActions";
import { useFocusMessage, type FocusTarget } from "./focus";
import { onDraftChange, readDraft, writeDraft } from "./drafts";

const EVERYONE = { id: "everyone", name: "everyone", label: "所有人" };

/** Bold the @mentions of members, so a hand-off reads at a glance. */
function highlight(text: string, members: RoomMember[]) {
  if (!members.length) return text;
  const names = members.map(m => m.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).sort((a, b) => b.length - a.length);
  return text.replace(new RegExp(`(^|[^\\w*])@(${[...names, "everyone"].join("|")})`, "g"), (_, lead, name) => `${lead}**@${name}**`);
}

function useRoom(roomId: string) {
  const [room, setRoom] = useState<RoomDetail | null>(null);
  const [error, setError] = useState("");
  const timer = useRef(0);
  const version = useRef(0);
  const load = useCallback(async () => {
    // Polls, events and sends overlap; an older answer arriving last must not roll the room back.
    const request = ++version.current;
    try { const data = await api.room(roomId); if (request === version.current) { setRoom(data); setError(""); } }
    catch (err) { if (request === version.current) setError(err instanceof Error ? err.message : String(err)); }
  }, [roomId]);
  const soon = useCallback(() => { window.clearTimeout(timer.current); timer.current = window.setTimeout(() => void load(), 150); }, [load]);
  useEffect(() => {
    setRoom(null); void load();
    let source: EventSource | null = null;
    const connect = () => {
      if (source || document.hidden) return;
      source = new EventSource(`/api/rooms/${encodeURIComponent(roomId)}/events`);
      source.onmessage = () => soon();
    };
    const visibility = () => { if (document.hidden) { source?.close(); source = null; } else { connect(); soon(); } };
    connect();
    document.addEventListener("visibilitychange", visibility);
    return () => { source?.close(); window.clearTimeout(timer.current); document.removeEventListener("visibilitychange", visibility); };
  }, [roomId, load, soon]);
  // A member moving from queued to working is not journaled in the room; poll while anyone has a turn.
  const active = !!room?.members.some(m => m.seat !== "idle");
  useEffect(() => {
    if (!active) return;
    const poll = window.setInterval(() => { if (!document.hidden) void load(); }, 2500);
    return () => window.clearInterval(poll);
  }, [active, load]);
  return { room, setRoom, error, load };
}

function MemberFace({ member, size = 28 }: { member: RoomMember; size?: number }) {
  const mood = useMood(member.seat === "working" ? "running" : member.seat === "waiting" ? "waiting_input" : undefined);
  return <Avatar {...avatarProps(member as AgentRow)} size={size} gaze={false} alive={member.seat !== "idle"} mood={mood} />;
}

function RoomBubble({ message, room, showHead, onReply, onOpenBot, onOpen, onReact }: {
  message: RoomMessage; room: RoomDetail; showHead: boolean;
  onReply: (name: string) => void; onOpenBot: (id: string) => void; onOpen: (file: PreviewFile) => void; onReact: (message: RoomMessage, emoji: string) => void;
}) {
  const speaker = room.members.find(m => m.id === message.speaker_id);
  const text = useMemo(() => highlight(message.content, room.members), [message.content, room.members]);
  if (message.role === "user") {
    return (
      <div className={`msg user${message.pending ? " pending" : ""}`}>
        <div className="bubble"><ChatMessageBody content={text} agentId={room.member_ids[0] || ""} knownPaths={[]} onOpen={onOpen} /></div>
        <time className="msg-time">{message.pending ? "傳送中…" : clock(message.at)}</time>
      </div>
    );
  }
  const face = speaker || { id: message.speaker_id || "bot", name: message.speaker_name || "Bot", seat: "idle" as const };
  return (
    <div className={`msg assistant room-msg${message.failed ? " failed" : ""}`}>
      <div className="msg-avatar">{showHead ? <button type="button" className="room-speaker-face" aria-label={`打開 ${face.name}`} onClick={() => speaker && onOpenBot(speaker.id)}><Avatar {...avatarProps(face as AgentRow)} size={28} gaze={false} /></button> : null}</div>
      <div className="msg-body">
        {showHead ? <button type="button" className="room-speaker" onClick={() => speaker && onOpenBot(speaker.id)}>{face.name}{speaker?.title ? <small>{speaker.title}</small> : null}</button> : null}
        <ChatMessageBody content={text} agentId={face.id} knownPaths={[]} onOpen={onOpen} />
        <ReactionList reactions={message.reactions} onToggle={emoji => onReact(message, emoji)} />
        <div className="room-msg-foot">
          <time className="msg-time">{clock(message.at)}</time>
          {speaker ? <button type="button" className="room-reply" onClick={() => onReply(speaker.name)}><Reply aria-hidden="true" />回覆</button> : null}
          {!message.pending ? <ReactButton chosen={message.reactions} onToggle={emoji => onReact(message, emoji)} /> : null}
        </div>
      </div>
    </div>
  );
}

/** The bottom rows that say who is answering, queued, or waiting for the user in their own chat. */
function Seats({ members, onOpenBot }: { members: RoomMember[]; onOpenBot: (id: string) => void }) {
  const busy = members.filter(m => m.seat !== "idle");
  if (!busy.length) return null;
  return (
    <div className="room-seats" aria-live="polite">
      {busy.map(member => (
        <button type="button" key={member.id} className="room-seat" data-seat={member.seat} onClick={() => onOpenBot(member.id)}>
          <MemberFace member={member} size={22} />
          <span>{member.seat === "working" ? `${member.name} 正在回覆…` : member.seat === "waiting" ? `${member.name} 在自己的對話等你回覆` : `${member.name} 忙完手上的工作就回覆`}</span>
          {member.seat === "working" ? <i className="typing-dots" aria-hidden="true"><b /><b /><b /></i> : member.seat === "waiting" ? <strong>前往</strong> : null}
        </button>
      ))}
    </div>
  );
}

function RoomComposer({ room, draft, setDraft, onSend, autoFocus, area, files, setFiles }: {
  room: RoomDetail; draft: string; setDraft: (value: string) => void; onSend: () => void; autoFocus: boolean;
  area: RefObject<HTMLTextAreaElement | null>;
  files: Upload[]; setFiles: (files: Upload[]) => void;
}) {
  const [uploading, setUploading] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  /** Files go to every member's computer (Grok: you can attach files in groups). */
  const attach = async (chosen: File[]) => {
    if (!chosen.length) return;
    setUploading(true);
    const added = [...files];
    try { for (const file of chosen) { added.push(await api.roomUpload(room.id, file)); setFiles([...added]); } }
    catch (err) { toast(err instanceof Error ? err.message : String(err), "error"); }
    finally { setUploading(false); }
  };
  const [query, setQuery] = useState<string | null>(null);
  const [pick, setPick] = useState(0);
  useLayoutEffect(() => {
    const el = area.current; if (!el) return;
    el.style.height = "auto"; el.style.height = `${el.scrollHeight}px`;
  }, [draft, area]);
  useEffect(() => { if (autoFocus && window.matchMedia("(pointer: fine)").matches) area.current?.focus({ preventScroll: true }); }, [autoFocus, area, room.id]);
  const options = useMemo(() => {
    if (query === null) return [];
    const needle = query.toLowerCase();
    return [...room.members.map(m => ({ id: m.id, name: m.name, label: m.title || "" })), EVERYONE].filter(o => o.name.toLowerCase().includes(needle) || o.label.includes(query));
  }, [query, room.members]);
  const track = (value: string, caret: number) => {
    const match = /(^|\s)@([^\s@]*)$/.exec(value.slice(0, caret));
    setQuery(match ? match[2] : null); setPick(0);
  };
  const insert = (name: string) => {
    const el = area.current; if (!el) return;
    const caret = el.selectionStart ?? draft.length;
    const before = draft.slice(0, caret).replace(/@([^\s@]*)$/, `@${name} `);
    const next = before + draft.slice(caret);
    setDraft(next); setQuery(null);
    requestAnimationFrame(() => { el.focus(); el.setSelectionRange(before.length, before.length); });
  };
  return (
    <div className="composer-dock">
      {options.length ? (
        <ul className="mention-menu" role="listbox" aria-label="點名成員">
          {options.map((option, index) => {
            const member = room.members.find(m => m.id === option.id);
            return (
              <li key={option.id}>
                <button type="button" role="option" aria-selected={index === pick} className={index === pick ? "picked" : ""} onMouseDown={event => event.preventDefault()} onClick={() => insert(option.name)}>
                  {member ? <Avatar {...avatarProps(member as AgentRow)} size={22} gaze={false} /> : <span className="mention-all"><Users /></span>}
                  <strong>@{option.name}</strong><small>{option.id === EVERYONE.id ? "所有成員" : option.label}</small>
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
      {files.length ? (
        <ul className="room-attachments" aria-label="附件">
          {files.map(file => <li key={file.id}><Paperclip aria-hidden="true" /><span>{file.name}</span><button type="button" className="icon-btn small" aria-label={`移除 ${file.name}`} onClick={() => setFiles(files.filter(item => item.id !== file.id))}><X /></button></li>)}
        </ul>
      ) : null}
      <form className="composer" onSubmit={event => { event.preventDefault(); if (!uploading) onSend(); }}
        onDragOver={event => { if (event.dataTransfer.types.includes("Files")) event.preventDefault(); }}
        onDrop={event => { if (!event.dataTransfer.files.length) return; event.preventDefault(); void attach([...event.dataTransfer.files]); }}>
        <textarea ref={area} rows={1} value={draft} aria-label="群組訊息" maxLength={12000}
          onPaste={event => { const pasted = [...event.clipboardData.files]; if (pasted.length) { event.preventDefault(); void attach(pasted); } }}
          placeholder={`傳訊息到「${room.name}」，用 @ 指定負責的 Bot…`}
          onChange={event => { setDraft(event.target.value); track(event.target.value, event.target.selectionStart ?? event.target.value.length); }}
          onKeyDown={event => {
            if (options.length) {
              if (event.key === "ArrowDown") { event.preventDefault(); setPick(i => (i + 1) % options.length); return; }
              if (event.key === "ArrowUp") { event.preventDefault(); setPick(i => (i - 1 + options.length) % options.length); return; }
              if ((event.key === "Enter" || event.key === "Tab") && !event.nativeEvent.isComposing) { event.preventDefault(); insert(options[pick].name); return; }
              if (event.key === "Escape") { setQuery(null); return; }
            }
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); onSend(); }
          }} />
        <div className="composer-bar">
          <button type="button" className="composer-plus" aria-label="點名成員" title="點名成員" onClick={() => { const el = area.current; setDraft(draft + (draft && !draft.endsWith(" ") ? " @" : "@")); setQuery(""); requestAnimationFrame(() => el?.focus()); }}><AtSign /></button>
          <button type="button" className="composer-plus" aria-label="加入檔案" title="加入檔案（每位成員的電腦都會收到）" disabled={uploading} onClick={() => picker.current?.click()}>{uploading ? <LoaderCircle className="spin" /> : <Paperclip />}</button>
          <input ref={picker} type="file" multiple hidden onChange={event => { void attach([...(event.target.files || [])]); event.target.value = ""; }} />
          <span className="composer-hint">沒有點名時，由成員自己判斷要不要回答 · Enter 送出</span>
          <button type="submit" className="composer-send" aria-label="送出" disabled={(!draft.trim() && !files.length) || uploading}><ArrowUp /></button>
        </div>
      </form>
    </div>
  );
}

export function RoomView({ roomId, mobile, onBack, onEdit, onDeleted, onOpenBot, onOpenFile, onChanged, finding = false, onCloseFind, focusMessage, onFocused }: {
  finding?: boolean;
  onCloseFind?: () => void;
  focusMessage?: FocusTarget | null;
  onFocused?: () => void;
  roomId: string;
  mobile: boolean;
  onBack: () => void;
  onEdit: (room: RoomRow) => void;
  onDeleted: () => void;
  onOpenBot: (id: string) => void;
  onOpenFile: (file: PreviewFile) => void;
  onChanged: () => void;
}) {
  const { room, setRoom, error, load } = useRoom(roomId);
  // Like a bot chat, an unsent group message waits here while you look at other conversations.
  const draftId = `room:${roomId}`;
  const [draft, setDraftState] = useState(() => readDraft(draftId).text);
  const setDraft = useCallback((value: string | ((current: string) => string)) => setDraftState(current => {
    const next = typeof value === "function" ? value(current) : value;
    writeDraft(draftId, { text: next });
    return next;
  }), [draftId]);
  // Text shared in from another app lands in the draft of the room that may already be open.
  useEffect(() => onDraftChange(id => { if (id === draftId) { setDraftState(readDraft(id).text); area.current?.focus(); } }), [draftId]);
  const [confirm, setConfirm] = useState<"clear" | "delete" | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const pinned = useRef(true);
  const [showJump, setShowJump] = useState(false);
  const count = room?.transcript.length || 0;
  const toBottom = useCallback((smooth = false) => { const el = scroller.current; if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" }); }, []);
  useLayoutEffect(() => { pinned.current = true; toBottom(); }, [roomId, toBottom]);
  useLayoutEffect(() => { if (pinned.current) toBottom(); }, [count, room?.working.length, toBottom]);
  useFocusMessage(scroller, focusMessage, room?.transcript, pinned, onFocused);

  const [files, setFiles] = useState<Upload[]>([]);
  const send = async () => {
    const text = [draft.trim(), ...files.map(file => `[${file.name}](${file.url})`)].filter(Boolean).join("\n\n");
    if (!text || !room) return;
    const sentText = draft, sentFiles = files;
    setFiles([]);
    const local: RoomMessage = { id: `local:${randomUUID()}`, role: "user", content: text, at: new Date().toISOString(), pending: true };
    setRoom({ ...room, transcript: [...room.transcript, local] }); setDraft(""); pinned.current = true;
    try { await api.postRoom(room.id, text, local.id); await load(); onChanged(); }
    catch (err) {
      // Anything typed while it was sending stays, after the message that failed.
      setDraft(current => current ? `${sentText}\n\n${current}` : sentText); setFiles(current => [...sentFiles, ...current]);
      setRoom(current => current && { ...current, transcript: current.transcript.filter(m => m.id !== local.id) }); toast(err instanceof Error ? err.message : String(err), "error");
    }
  };
  const reply = (name: string) => {
    setDraft(current => current.startsWith(`@${name} `) ? current : `@${name} ${current}`);
    requestAnimationFrame(() => area.current?.focus());
  };
  const openFile = (file: PreviewFile) => onOpenFile(file);
  const react = (message: RoomMessage, emoji: string) => {
    setRoom(current => current && { ...current, transcript: current.transcript.map(m => m.id === message.id ? { ...m, reactions: toggleReaction(m.reactions, emoji) } : m) });
    void api.react(roomId, message.id, emoji).catch(err => { toast(err instanceof Error ? err.message : String(err), "error"); void load(); });
  };

  if (!room) {
    return (
      <main className="chat room-chat" aria-label="群組對話">
        <header className="chat-header">{mobile ? <button type="button" className="icon-btn" aria-label="返回" onClick={onBack}><ChevronLeft /></button> : null}</header>
        <div className="chat-scroll"><div className="chat-content">{error ? <div className="pane-error" role="alert">{error}<button type="button" className="btn small" onClick={() => void load()}>重試</button></div> : <div className="chat-loading"><LoaderCircle className="spin" />載入群組對話…</div>}</div></div>
      </main>
    );
  }
  const working = room.members.filter(m => m.seat === "working" || m.seat === "queued");
  const subtitle = working.length ? `${working.map(m => m.name).join("、")} 正在回覆…` : `${room.members.length} 位成員 · ${room.members.map(m => m.name).join("、")}`;
  const menu: MenuItem[] = [
    { label: "名稱與成員", icon: <Pencil />, onSelect: () => onEdit(room) },
    { label: "清除群組訊息", icon: <Eraser />, onSelect: () => setConfirm("clear"), disabled: !room.transcript.length },
    "separator",
    { label: "刪除群組對話", icon: <Trash2 />, danger: true, onSelect: () => setConfirm("delete") },
  ];
  const [a, b, c] = room.members;
  const kickoff = [
    a && b && c ? `@${a.name} 蒐集需要的資料並附上來源。@${b.name} 把結果整理成草稿。@${c.name} 檢查草稿，只列出必須修改的地方。` : "",
    a && b ? `@${a.name} 先列出你的做法，@${b.name} 補充你那邊需要注意的地方` : "",
    "大家各自用一句話說明自己能幫上什麼",
  ].filter(Boolean);

  return (
    <main className="chat room-chat" aria-label={`群組對話「${room.name}」`}>
      <header className="chat-header">
        {mobile ? <button type="button" className="icon-btn" aria-label="返回清單" onClick={onBack}><ChevronLeft /></button> : null}
        <button type="button" className="chat-identity" onClick={() => onEdit(room)} title="名稱與成員">
          <RoomAvatar members={room.members} size={34} />
          <span>
            <strong className="chat-title">{room.name}</strong>
            <small className={`chat-status ${working.length ? "busy" : "idle"}`}><i aria-hidden="true" />{subtitle}</small>
          </span>
        </button>
        <div className="chat-actions">
          {working.length ? <button type="button" className="icon-btn" aria-label="停止所有成員" title="停止所有成員" onClick={() => void api.stopRoom(room.id).then(() => load()).catch(err => toast(err instanceof Error ? err.message : String(err), "error"))}><Square /></button> : null}
          <button type="button" className="icon-btn" aria-label="名稱與成員" title="名稱與成員" onClick={() => onEdit(room)}><Users /></button>
          <MenuButton label="更多" icon={<Ellipsis />} items={menu} />
        </div>
      </header>
      {finding && onCloseFind ? <FindBar scroller={scroller} version={room.transcript} pinned={pinned} onClose={onCloseFind} /> : null}
      <div className="chat-scroll" ref={scroller} onScroll={event => {
        const el = event.currentTarget;
        const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        pinned.current = near; setShowJump(!near);
        el.parentElement?.classList.toggle("scrolled", el.scrollTop > 4);
      }}>
        <div className="chat-content">
          {!room.transcript.length ? (
            <div className="chat-empty room-empty">
              <RoomAvatar members={room.members} size={72} />
              <h2>{room.name}</h2>
              <p className="chat-empty-lead">你和 {room.members.map(m => m.name).join("、")} 的群組。用 @名字 指定這一步誰負責；沒有點名時，成員會自己判斷要不要回答。成員也會互相點名，把下一步交給對的人。</p>
              <div className="suggestions">{kickoff.map(text => <button type="button" key={text} onClick={() => { setDraft(text); area.current?.focus(); }}>{text}</button>)}</div>
            </div>
          ) : null}
          {room.transcript.map((message, index) => {
            const previous = room.transcript[index - 1];
            const newDay = !previous || !sameDay(previous.at, message.at);
            const showHead = message.role === "assistant" && (newDay || previous?.role !== "assistant" || previous?.speaker_id !== message.speaker_id);
            return (
              <div key={message.id} className="msg-wrap" data-msg={message.id}>
                {newDay && message.at ? <div className="day-sep"><span>{dayLabel(message.at)}</span></div> : null}
                <RoomBubble message={message} room={room} showHead={showHead} onReply={reply} onOpenBot={onOpenBot} onOpen={openFile} onReact={react} />
              </div>
            );
          })}
          <Seats members={room.members} onOpenBot={onOpenBot} />
        </div>
      </div>
      <div className="chat-bottom">
        {showJump ? <button type="button" className="jump-latest" onClick={() => { pinned.current = true; toBottom(true); }}><ChevronDown />最新訊息</button> : null}
        <RoomComposer room={room} draft={draft} setDraft={setDraft} onSend={() => void send()} autoFocus={!mobile} area={area} files={files} setFiles={setFiles} />
      </div>
      {confirm === "clear" ? (
        <ConfirmDialog title="清除群組訊息" confirmLabel="清除" danger onClose={() => setConfirm(null)}
          body={<>會清除「{room.name}」的訊息並停止成員目前的回覆。成員各自的對話、記憶與檔案都會保留。</>}
          onConfirm={async () => { await api.clearRoom(room.id); await load(); onChanged(); toast("群組訊息已清除", "ok"); }} />
      ) : null}
      {confirm === "delete" ? (
        <ConfirmDialog title="刪除群組對話" confirmLabel="刪除" danger onClose={() => setConfirm(null)}
          body={<>要刪除「{room.name}」嗎？群組的訊息會一併刪除；成員 Bot 本身、它們的對話、記憶與檔案都不受影響。</>}
          onConfirm={async () => { await api.deleteRoom(room.id); onDeleted(); toast("群組對話已刪除", "ok"); }} />
      ) : null}
    </main>
  );
}

