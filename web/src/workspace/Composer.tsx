import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { AgentRow, RoomRow, Upload } from "../api";
import { AttachmentPicker } from "../attachment-picker";
import { ArrowUp, Headphones, Mic, Reply, Square, X } from "../ui/icons";
import { toast } from "../ui/overlay";
import { useComposerPicker } from "./ComposerPicker";
import { dictationSupported, useDictation } from "./dictation";
import { onDraftChange, readDraft, replyQuote, writeDraft, type ReplyTo } from "./drafts";

export function Composer({ agentId, draft, agentName, busy, elsewhere, stopping, blocked, answering, onSend, onStop, onTeach, autoFocus, team = [], rooms = [], onVoiceChat }: {
  /** Grok's "Start voice chat": offered while the composer is empty. */
  onVoiceChat?: () => void;
  /** Other bots and group chats `@` can refer to. */
  team?: AgentRow[];
  rooms?: RoomRow[];
  agentId: string;
  draft: string;
  agentName: string;
  busy: boolean;
  elsewhere: boolean;
  stopping: boolean;
  blocked: string;
  answering: boolean;
  onSend: (text: string) => Promise<void>;
  onStop: () => Promise<void>;
  onTeach: () => void;
  autoFocus: boolean;
}) {
  const [text, setText] = useState(() => readDraft(draft).text);
  const [files, setFiles] = useState<Upload[]>(() => readDraft(draft).files);
  const [reply, setReplyState] = useState<ReplyTo | null>(() => readDraft(draft).reply || null);
  const [uploading, setUploading] = useState(false);
  const [sending, setSending] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => onDraftChange(id => { if (id === draft) { const saved = readDraft(id); setText(saved.text); setReplyState(saved.reply || null); area.current?.focus(); } }), [draft]);
  useEffect(() => { if (autoFocus && window.matchMedia("(pointer: fine)").matches) area.current?.focus({ preventScroll: true }); }, [draft, autoFocus]);
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    // CSS max-height caps the growth; past it the field scrolls.
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  const update = (value: string) => { setText(value); writeDraft(draft, { text: value }); };
  const latest = useRef(text);
  latest.current = text;
  const picker = useComposerPicker({ agentId, text, area, team, rooms, setText: update });
  const dictation = useDictation(piece => {
    const current = latest.current;
    // Join Latin words with a space; Chinese runs on without one.
    const glue = current && /[A-Za-z0-9.,!?]$/.test(current) && /^[A-Za-z0-9]/.test(piece) ? " " : "";
    update(current + glue + piece.trim());
  });
  useEffect(() => {
    const dictate = () => { if (!document.querySelector(".chat .composer textarea:disabled")) dictation.toggle(); };
    window.addEventListener("lazyboy-dictate", dictate);
    return () => window.removeEventListener("lazyboy-dictate", dictate);
  }, [dictation.toggle]); // eslint-disable-line react-hooks/exhaustive-deps
  const updateFiles = (value: Upload[]) => { setFiles(value); writeDraft(draft, { files: value }); };
  const disabled = !!blocked || stopping;
  const body = [text.trim(), ...files.map(file => `[${file.name}](${file.url})`)].filter(Boolean).join("\n\n");
  const payload = body && reply ? `${replyQuote(reply)}\n\n${body}` : body;

  async function submit() {
    if (!payload || sending || uploading || disabled) return;
    const sentText = text, sentFiles = files, sentReply = reply;
    dictation.stop(); picker.close();
    setSending(true);
    update(""); updateFiles([]); setReplyState(null); writeDraft(draft, { reply: null });
    try { await onSend(payload); }
    catch (err) {
      const restored = readDraft(draft).text;
      writeDraft(draft, { text: restored ? `${sentText}\n\n${restored}` : sentText, files: sentFiles, reply: sentReply });
      setText(readDraft(draft).text); setFiles(sentFiles); setReplyState(sentReply);
      toast(err instanceof Error ? err.message : String(err), "error");
    } finally { setSending(false); }
  }

  const placeholder = blocked || (answering ? "直接輸入回答，或在上方卡片選擇…" : busy ? `補充說明給 ${agentName}，會送進目前的工作…` : elsewhere ? `${agentName} 正在其他對話工作，這則訊息會排在後面…` : `交代 ${agentName} 要做的事…`);
  return (
    <div className="composer-dock">
      {picker.menu}
      {reply ? (
        <div className="reply-chip">
          <Reply aria-hidden="true" />
          <span><strong>回覆 {reply.author}</strong>{reply.text.replace(/\s+/g, " ").slice(0, 120)}</span>
          <button type="button" className="icon-btn small" aria-label="取消回覆" onClick={() => { setReplyState(null); writeDraft(draft, { reply: null }); }}><X /></button>
        </div>
      ) : null}
      <form className={`composer${disabled ? " is-disabled" : ""}${dictation.listening ? " is-listening" : ""}`} onSubmit={event => { event.preventDefault(); void submit(); }}>
        <textarea ref={area} rows={1} value={text} aria-label="訊息" placeholder={dictation.listening ? "正在聽…說完再按一次麥克風" : placeholder} disabled={disabled} maxLength={64000}
          aria-keyshortcuts="Control+D Meta+D"
          onChange={event => { update(event.target.value); picker.track(event.target.value, event.target.selectionStart ?? event.target.value.length); }}
          onSelect={event => { const el = event.currentTarget; if (picker.open) picker.track(el.value, el.selectionStart ?? el.value.length); }}
          onBlur={picker.onBlur} onFocus={picker.onFocus}
          onKeyDown={event => {
            if (picker.onKey(event)) return;
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) {
              event.preventDefault();
              void submit();
            }
          }} />
        <div className="composer-bar">
          <AttachmentPicker key={agentId} agentId={agentId} files={files} busy={uploading} disabled={disabled || sending}
            onChange={updateFiles} onBusy={setUploading} onTeach={onTeach} />
          {dictationSupported ? (
            <button type="button" className={`composer-mic${dictation.listening ? " on" : ""}`} aria-pressed={dictation.listening}
              aria-label={dictation.listening ? "停止語音輸入" : "語音輸入"} title={dictation.listening ? "停止語音輸入（Ctrl D）" : "語音輸入（Ctrl D）"}
              disabled={disabled} onClick={dictation.toggle}><Mic /></button>
          ) : null}
          <span className="composer-hint">{dictation.listening ? (dictation.interim || "正在聽…") : busy ? "Enter 送出補充 · Shift+Enter 換行" : "Enter 送出 · / 技能 · @ 提到"}</span>
          {busy ? (
            <button type="button" className="composer-stop" aria-label="停止工作" title="停止工作" disabled={stopping} onClick={() => void onStop().catch(err => toast(err instanceof Error ? err.message : String(err), "error"))}>
              <Square />
            </button>
          ) : null}
          {onVoiceChat && !payload && !busy && !disabled ? (
            <button type="button" className="composer-send voice-start" aria-label="語音對話" title="語音對話：說話、聽它回答" onClick={onVoiceChat}><Headphones /></button>
          ) : (
            <button type="submit" className="composer-send" aria-label={busy ? "送出補充" : "送出"} title={busy ? "送出補充" : "送出"} disabled={!payload || sending || uploading || disabled}>
              <ArrowUp />
            </button>
          )}
        </div>
      </form>
    </div>
  );
}
