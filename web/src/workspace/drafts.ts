import type { Upload } from "../api";

export type Draft = { text: string; files: Upload[] };
const drafts = new Map<string, Draft>();
const EVENT = "lazyhermes:draft";

/** The main thread keeps the bare bot id so drafts written before threads still apply. */
export function draftKey(agentId: string, threadId?: string | null): string {
  return threadId && threadId !== agentId ? `${agentId}:${threadId}` : agentId;
}

export function readDraft(agentId: string): Draft {
  return drafts.get(agentId) || { text: "", files: [] };
}

export function writeDraft(agentId: string, change: Partial<Draft>, announce = false) {
  drafts.set(agentId, { ...readDraft(agentId), ...change });
  if (announce) window.dispatchEvent(new CustomEvent(EVENT, { detail: agentId }));
}

export function appendDraft(agentId: string, text: string) {
  const current = readDraft(agentId).text;
  writeDraft(agentId, { text: [current.trim(), text].filter(Boolean).join("\n\n") }, true);
}

export function onDraftChange(listener: (agentId: string) => void) {
  const handler = (event: Event) => listener(String((event as CustomEvent).detail));
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
