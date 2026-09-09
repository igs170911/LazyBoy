/**
 * How a chat shows when something was said: the day is written once, as a
 * divider above the first message of that day, and each message carries only
 * its clock time. A day is named the way people say it — 今天, 昨天 — and
 * spelled out only when it is further away.
 */

export interface DayWords {
  today: string;
  yesterday: string;
}

/** Local calendar day, so two timestamps on the same date compare equal. */
export function dayKey(value: string | Date): string {
  const date = value instanceof Date ? value : new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

export function sameDay(a: string | Date, b: string | Date): boolean {
  const keyA = dayKey(a);
  return keyA !== "" && keyA === dayKey(b);
}

/** Whole local days between two moments; negative when `date` is in the future. */
function daysBefore(date: Date, now: Date): number {
  const start = (value: Date) => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();
  return Math.round((start(now) - start(date)) / 86_400_000);
}

/**
 * The divider text for a day: 今天 / 昨天, then a weekday for the past week
 * (people remember "Wednesday" before "the 3rd"), then a short date, and the
 * year only when it is not this one.
 */
export function dayLabel(value: string | Date, now: Date, locale: string, words: DayWords): string {
  const date = value instanceof Date ? value : new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  const ago = daysBefore(date, now);
  if (ago === 0) return words.today;
  if (ago === 1) return words.yesterday;
  if (ago > 1 && ago < 7) return date.toLocaleDateString(locale, { weekday: "long" });
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString(locale, { month: "numeric", day: "numeric", weekday: "short" });
  }
  return date.toLocaleDateString(locale, { year: "numeric", month: "numeric", day: "numeric" });
}

/** The clock time a message shows next to itself; the day lives in the divider. */
export function clockTime(value: string | Date, locale: string): string {
  const date = value instanceof Date ? value : new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  return date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false });
}
