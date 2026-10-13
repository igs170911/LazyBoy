import type { Upload } from "../api";

/** The message a draft answers (Grok's "Reply"): shown above the composer and quoted when sent. */
export type ReplyTo = { id: string; author: string; text: string };
export type Draft = { text: string; files: Upload[]; reply?: ReplyTo | null };
const EVENT = "lazyhermes:draft";
const KEY = "lazyhermes.drafts";
const KEEP = 50;

/**
 * Unsent drafts survive a reload (including "update to the new version"), one per conversation, newest KEEP kept.
 * They live in sessionStorage, which the browser drops with the tab: chat text is not kept on the device for good.
 */
function load(): Map<string, Draft> {
  try {
    const saved = JSON.parse(window.sessionStorage.getItem(KEY) || "[]") as [string, Draft][];
    return new Map(Array.isArray(saved) ? saved.filter(([, d]) => d && typeof d.text === "string") : []);
  } catch { return new Map(); }
}
const drafts = load();
let timer = 0;
function persist() {
  window.clearTimeout(timer);
  timer = window.setTimeout(() => {
    const rows = [...drafts].filter(([, d]) => d.text.trim() || d.files.length || d.reply).slice(-KEEP);
    try { window.sessionStorage.setItem(KEY, JSON.stringify(rows)); } catch { /* storage blocked or full */ }
  }, 400);
}

/** The main thread keeps the bare bot id so drafts written before threads still apply. */
export function draftKey(agentId: string, threadId?: string | null): string {
  return threadId && threadId !== agentId ? `${agentId}:${threadId}` : agentId;
}

export function readDraft(agentId: string): Draft {
  return drafts.get(agentId) || { text: "", files: [] };
}

export function writeDraft(agentId: string, change: Partial<Draft>, announce = false) {
  const next = { ...readDraft(agentId), ...change };
  drafts.delete(agentId);
  drafts.set(agentId, next);
  persist();
  if (announce) window.dispatchEvent(new CustomEvent(EVENT, { detail: agentId }));
}

export function appendDraft(agentId: string, text: string) {
  const current = readDraft(agentId).text;
  writeDraft(agentId, { text: [current.trim(), text].filter(Boolean).join("\n\n") }, true);
}

export function setReply(agentId: string, reply: ReplyTo | null) {
  writeDraft(agentId, { reply }, true);
}

/** The quote that goes in front of a reply, one short line naming whose message it answers. */
export function replyQuote(reply: ReplyTo) {
  const line = reply.text.replace(/\s+/g, " ").trim();
  return `> 回覆 ${reply.author}：${line.length > 140 ? `${line.slice(0, 140)}…` : line}`;
}

export function onDraftChange(listener: (agentId: string) => void) {
  const handler = (event: Event) => listener(String((event as CustomEvent).detail));
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
