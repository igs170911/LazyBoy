export const supportedLocales = ["zh-TW"] as const;
export type Locale = (typeof supportedLocales)[number];

const messages = {
  "zh-TW": {
    search: "搜尋",
    sharedComputer: "共用電腦",
    privateComputer: "私人電腦",
    stopped: "已關閉",
    booting: "啟動中",
    running: "執行中",
    suspended: "休眠中",
    error: "發生錯誤",
    openComputer: "開啟電腦",
    stopTask: "停止任務",
    takeControl: "取得控制權",
    releaseControl: "交還控制",
    done: "完成",
    skip: "略過",
  },
} as const;

export const locale: Locale = "zh-TW";
export type MessageKey = keyof (typeof messages)["zh-TW"];
export function t(key: MessageKey): string { return messages[locale][key]; }
