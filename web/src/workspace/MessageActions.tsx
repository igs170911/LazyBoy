import { useEffect, useRef, useState } from "react";
import { Reply, SmilePlus, Square, Volume2 } from "../ui/icons";
import { speak, speakable, speechSupported, stopSpeaking } from "./voice";

export const REACTIONS = ["👍", "❤️", "😂", "🎉", "🙏", "👀"] as const;

/** Markdown reduced to the words, for the one-line quote of a reply. */
export function plainLine(markdown: string) {
  return markdown
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^>.*$/gm, " ")
    .replace(/[#*_`~>|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function toggleReaction(current: string[] | undefined, emoji: string) {
  const list = current || [];
  return list.includes(emoji) ? list.filter(item => item !== emoji) : [...list, emoji];
}

/** Grok's lightweight acknowledgement: the chosen emoji under a message; tap one to take it back. */
export function ReactionList({ reactions, onToggle }: { reactions?: string[]; onToggle: (emoji: string) => void }) {
  if (!reactions?.length) return null;
  return (
    <div className="reaction-list">
      {reactions.map(emoji => <button type="button" key={emoji} className="reaction" aria-label={`移除 ${emoji}`} title="移除" onClick={() => onToggle(emoji)}>{emoji}</button>)}
    </div>
  );
}

export function ReactButton({ chosen, onToggle }: { chosen?: string[]; onToggle: (emoji: string) => void }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!box.current?.contains(event.target as Node)) setOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.stopPropagation(); setOpen(false); } };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    return () => { document.removeEventListener("pointerdown", outside, true); document.removeEventListener("keydown", escape, true); };
  }, [open]);
  return (
    <span ref={box} className="react-picker-wrap">
      <button type="button" className="msg-action" aria-label="表情回應" title="表情回應" aria-expanded={open} onClick={() => setOpen(value => !value)}><SmilePlus /></button>
      {open ? (
        <span className="react-picker" role="menu" aria-label="選擇表情">
          {REACTIONS.map(emoji => (
            <button type="button" role="menuitemcheckbox" aria-checked={!!chosen?.includes(emoji)} key={emoji} className={chosen?.includes(emoji) ? "chosen" : ""}
              onClick={() => { onToggle(emoji); setOpen(false); }}>{emoji}</button>
          ))}
        </span>
      ) : null}
    </span>
  );
}

export function ReplyButton({ onReply }: { onReply: () => void }) {
  return <button type="button" className="msg-action" aria-label="回覆這則訊息" title="回覆這則訊息" onClick={onReply}><Reply /></button>;
}

/** Only one reply reads aloud at a time; starting another resets the button of the one before. */
const speaking = new Set<(id: string | null) => void>();
let reading: string | null = null;
const announce = (id: string | null) => { reading = id; speaking.forEach(listener => listener(id)); };

/** Grok's "Play voice memo", for any reply: the browser reads it aloud. */
export function SpeakButton({ id, text }: { id: string; text: string }) {
  const [on, setOn] = useState(false);
  useEffect(() => {
    const listener = (current: string | null) => setOn(current === id);
    speaking.add(listener);
    return () => { speaking.delete(listener); };
  }, [id]);
  if (!speechSupported) return null;
  return (
    <button type="button" className="msg-action" aria-label={on ? "停止朗讀" : "朗讀"} title={on ? "停止朗讀" : "朗讀"} aria-pressed={on}
      onClick={() => {
        if (on) { stopSpeaking(); announce(null); return; }
        announce(id);
        // The reply read before this one ends (cancelled) after this starts; only this one's end resets the button.
        speak(speakable(text), () => { if (reading === id) announce(null); });
      }}>{on ? <Square /> : <Volume2 />}</button>
  );
}
