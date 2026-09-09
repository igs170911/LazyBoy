import type { RoomMember } from "./types";

/** One row of the `@` list. A null `member` is the whole-room escape hatch. */
export interface MentionChoice {
  name: string;
  member: RoomMember | null;
}

/**
 * The `@`-token under the caret, or null when the caret is not writing one.
 * A token ends at a space, so `@小美 幫我看` stops offering the list the moment
 * the name is finished, and `mail@example.com` never opens it.
 */
export function mentionToken(text: string, caret: number): string | null {
  const end = Math.max(0, Math.min(caret, text.length));
  const hit = /(?:^|\s)@([^\s@]*)$/.exec(text.slice(0, end));
  return hit ? hit[1] : null;
}

/**
 * Who the token could mean, in room order, capped so the list stays one glance.
 * `everyone` — the one name that reaches the whole room — leads the list while
 * the token is empty or could still become it, because typing `@` is where a
 * person discovers that escape hatch.
 */
export function mentionChoices(
  token: string,
  members: RoomMember[],
  everyone: string,
): MentionChoice[] {
  const needle = token.trim().toLowerCase();
  const choices: MentionChoice[] = [];
  if (everyone.toLowerCase().startsWith(needle)) {
    choices.push({ name: everyone, member: null });
  }
  for (const member of members) {
    if (member.name.toLowerCase().includes(needle)) choices.push({ name: member.name, member });
  }
  return choices.slice(0, 6);
}

/**
 * Replace the token being typed with a chosen name and one space, leaving the
 * rest of the message where it was. The new caret sits after that space, so the
 * next keystroke continues the sentence instead of reopening the list.
 */
export function acceptMention(
  text: string,
  caret: number,
  name: string,
): { text: string; caret: number } {
  const end = Math.max(0, Math.min(caret, text.length));
  const head = text.slice(0, end).replace(/@([^\s@]*)$/, "");
  const inserted = `${head}@${name} `;
  return { text: inserted + text.slice(end), caret: inserted.length };
}
