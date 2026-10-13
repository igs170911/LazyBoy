import { useEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { ChevronDown, Search, X } from "../ui/icons";

const MAX_MATCHES = 800;
// Labels and controls are not conversation text.
const SKIP = "button, time, .msg-time, .find-bar, .quote-btn, script, style";

type HighlightRegistryLike = { set(name: string, value: unknown): void; delete(name: string): void };
const registry = (): HighlightRegistryLike | null => (typeof CSS !== "undefined" && "highlights" in CSS ? (CSS as unknown as { highlights: HighlightRegistryLike }).highlights : null);
const makeHighlight = (ranges: Range[]) => new (window as unknown as { Highlight: new (...r: Range[]) => unknown }).Highlight(...ranges);

function collect(root: Element, needle: string) {
  const found: Range[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: node => node.parentElement?.closest(SKIP) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  for (let node = walker.nextNode(); node && found.length < MAX_MATCHES; node = walker.nextNode()) {
    const text = (node.nodeValue || "").toLowerCase();
    for (let at = text.indexOf(needle); at >= 0 && found.length < MAX_MATCHES; at = text.indexOf(needle, at + needle.length)) {
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + needle.length);
      found.push(range);
    }
  }
  return found;
}

/**
 * Find in this chat (Grok's Ctrl/⌘+F): every match is highlighted, Enter and Shift+Enter step through them, Esc
 * closes. Uses the CSS Custom Highlight API, so the conversation's markup is never touched.
 */
export function FindBar({ scroller, version, pinned, onClose }: {
  scroller: RefObject<HTMLElement | null>;
  version: unknown;
  pinned?: MutableRefObject<boolean>;
  onClose: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const ranges = useRef<Range[]>([]);
  const [query, setQuery] = useState("");
  const [count, setCount] = useState(0);
  const [index, setIndex] = useState(0);

  useEffect(() => { input.current?.focus(); input.current?.select(); }, []);
  useEffect(() => {
    const focusAgain = () => { input.current?.focus(); input.current?.select(); };
    window.addEventListener("lazyboy-find-again", focusAgain);
    return () => window.removeEventListener("lazyboy-find-again", focusAgain);
  }, []);

  useEffect(() => {
    const root = scroller.current?.querySelector(".chat-content") || scroller.current;
    const needle = query.trim().toLowerCase();
    const found = root && needle ? collect(root, needle) : [];
    ranges.current = found;
    setCount(found.length);
    setIndex(i => Math.min(i, Math.max(0, found.length - 1)));
  }, [query, version, scroller]);

  useEffect(() => {
    const highlights = registry();
    const current = ranges.current[index];
    if (highlights) {
      if (ranges.current.length) highlights.set("chat-find", makeHighlight(ranges.current)); else highlights.delete("chat-find");
      if (current) highlights.set("chat-find-current", makeHighlight([current])); else highlights.delete("chat-find-current");
    }
    const box = scroller.current;
    if (current && box) {
      const rect = current.getBoundingClientRect();
      const frame = box.getBoundingClientRect();
      if (rect.height && (rect.top < frame.top + 48 || rect.bottom > frame.bottom - 48)) {
        if (pinned) pinned.current = false;
        box.scrollBy({ top: rect.top - frame.top - frame.height / 2, behavior: "smooth" });
      }
    }
  }, [index, count, query, version, scroller, pinned]);

  useEffect(() => () => { registry()?.delete("chat-find"); registry()?.delete("chat-find-current"); }, []);

  const step = (delta: number) => { if (count) setIndex(i => (i + delta + count) % count); };
  return (
    <div className="find-bar" role="search">
      <Search aria-hidden="true" />
      <input ref={input} value={query} placeholder="在這個對話中尋找" aria-label="在這個對話中尋找"
        onChange={event => { setQuery(event.target.value); setIndex(0); }}
        onKeyDown={event => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Enter") { event.preventDefault(); step(event.shiftKey ? -1 : 1); }
          else if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
        }} />
      <small aria-live="polite">{query.trim() ? (count ? `${index + 1}/${count}${count >= MAX_MATCHES ? "+" : ""}` : "沒有結果") : ""}</small>
      <button type="button" className="icon-btn small" aria-label="上一個" title="上一個（Shift+Enter）" disabled={!count} onClick={() => step(-1)}><ChevronDown className="up" /></button>
      <button type="button" className="icon-btn small" aria-label="下一個" title="下一個（Enter）" disabled={!count} onClick={() => step(1)}><ChevronDown /></button>
      <button type="button" className="icon-btn small" aria-label="關閉尋找" title="關閉（Esc）" onClick={onClose}><X /></button>
    </div>
  );
}
