import { zhTW } from "./locales/zh-TW";

export const supportedLocales = ["zh-TW"] as const;
export type Locale = (typeof supportedLocales)[number];

const messages = { "zh-TW": zhTW } as const;

export const locale: Locale = "zh-TW";
export type MessageKey = keyof typeof zhTW;
export type MessageParams = Record<string, string | number>;

export function t(key: MessageKey, params?: MessageParams): string {
  const message: string = messages[locale][key];
  if (!params) return message;
  return message.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match,
  );
}
