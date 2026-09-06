import { useSyncExternalStore, type ReactNode } from "react";
import { en } from "./locales/en";
import { zhTW } from "./locales/zh-TW";

export const supportedLocales = ["zh-TW", "en"] as const;
export type Locale = (typeof supportedLocales)[number];

const messages = { "zh-TW": zhTW, en } as const;
const LOCALE_STORE = "lazyboy.locale";
const listeners = new Set<() => void>();

export type MessageKey = keyof typeof zhTW;
export type MessageParams = Record<string, string | number>;

function isLocale(value: string): value is Locale {
  return (supportedLocales as readonly string[]).includes(value);
}

function readStoredLocale(): Locale {
  try {
    const raw = localStorage.getItem(LOCALE_STORE);
    if (raw && isLocale(raw)) return raw;
  } catch {
    /* private mode */
  }
  return "zh-TW";
}

let current: Locale = "zh-TW";
try {
  current = readStoredLocale();
} catch {
  current = "zh-TW";
}

export function getLocale(): Locale {
  return current;
}

export function htmlLang(locale: Locale = current): string {
  return locale === "en" ? "en" : "zh-Hant";
}

export function dateLocale(locale: Locale = current): string {
  return locale === "en" ? "en" : "zh-TW";
}

export function listJoin(items: string[]): string {
  return items.join(current === "en" ? ", " : "、");
}

function applyDocumentLang(locale: Locale) {
  if (typeof document === "undefined") return;
  document.documentElement.lang = htmlLang(locale);
}

applyDocumentLang(current);

export function setLocale(next: Locale) {
  if (next === current) return;
  current = next;
  try {
    localStorage.setItem(LOCALE_STORE, next);
  } catch {
    /* private mode */
  }
  applyDocumentLang(next);
  listeners.forEach((fn) => fn());
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function t(key: MessageKey, params?: MessageParams): string {
  const table = messages[current];
  const message: string = table[key] ?? messages["zh-TW"][key];
  if (!params) return message;
  return message.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match,
  );
}

export function useLocale(): Locale {
  return useSyncExternalStore(subscribeLocale, getLocale, getLocale);
}

export function LocaleRoot({ children }: { children: ReactNode }) {
  useLocale();
  return children;
}
