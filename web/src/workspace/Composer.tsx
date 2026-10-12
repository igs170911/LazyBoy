import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Upload } from "../api";
import { AttachmentPicker } from "../attachment-picker";
import { ArrowUp, Square } from "../ui/icons";
import { toast } from "../ui/overlay";
import { onDraftChange, readDraft, writeDraft } from "./drafts";

export function Composer({ agentId, draft, agentName, busy, elsewhere, stopping, blocked, answering, onSend, onStop, onTeach, autoFocus }: {
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
  const [uploading, setUploading] = useState(false);
  const [sending, setSending] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => onDraftChange(id => { if (id === draft) { setText(readDraft(id).text); area.current?.focus(); } }), [draft]);
  useEffect(() => { if (autoFocus && window.matchMedia("(pointer: fine)").matches) area.current?.focus({ preventScroll: true }); }, [draft, autoFocus]);
  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    // CSS max-height caps the growth; past it the field scrolls.
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  const update = (value: string) => { setText(value); writeDraft(draft, { text: value }); };
  const updateFiles = (value: Upload[]) => { setFiles(value); writeDraft(draft, { files: value }); };
  const disabled = !!blocked || stopping;
  const payload = [text.trim(), ...files.map(file => `[${file.name}](${file.url})`)].filter(Boolean).join("\n\n");

  async function submit() {
    if (!payload || sending || uploading || disabled) return;
    const sentText = text, sentFiles = files;
    setSending(true);
    update(""); updateFiles([]);
    try { await onSend(payload); }
    catch (err) {
      const restored = readDraft(draft).text;
      writeDraft(draft, { text: restored ? `${sentText}\n\n${restored}` : sentText, files: sentFiles });
      setText(readDraft(draft).text); setFiles(sentFiles);
      toast(err instanceof Error ? err.message : String(err), "error");
    } finally { setSending(false); }
  }

  const placeholder = blocked || (answering ? "直接輸入回答，或在上方卡片選擇…" : busy ? `補充說明給 ${agentName}，會送進目前的工作…` : elsewhere ? `${agentName} 正在其他對話工作，這則訊息會排在後面…` : `交代 ${agentName} 要做的事…`);
  return (
    <div className="composer-dock">
      <form className={`composer${disabled ? " is-disabled" : ""}`} onSubmit={event => { event.preventDefault(); void submit(); }}>
        <textarea ref={area} rows={1} value={text} aria-label="訊息" placeholder={placeholder} disabled={disabled} maxLength={64000}
          onChange={event => update(event.target.value)}
          onKeyDown={event => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) {
              event.preventDefault();
              void submit();
            }
          }} />
        <div className="composer-bar">
          <AttachmentPicker key={agentId} agentId={agentId} files={files} busy={uploading} disabled={disabled || sending}
            onChange={updateFiles} onBusy={setUploading} onTeach={onTeach} />
          <span className="composer-hint">{busy ? "Enter 送出補充 · Shift+Enter 換行" : "Enter 送出 · Shift+Enter 換行"}</span>
          {busy ? (
            <button type="button" className="composer-stop" aria-label="停止工作" title="停止工作" disabled={stopping} onClick={() => void onStop().catch(err => toast(err instanceof Error ? err.message : String(err), "error"))}>
              <Square />
            </button>
          ) : null}
          <button type="submit" className="composer-send" aria-label={busy ? "送出補充" : "送出"} title={busy ? "送出補充" : "送出"} disabled={!payload || sending || uploading || disabled}>
            <ArrowUp />
          </button>
        </div>
      </form>
    </div>
  );
}
