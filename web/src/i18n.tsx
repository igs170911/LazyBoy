import { createContext, useContext, useEffect, type ReactNode } from "react";

const zhHant = {
  retry: '重試',
  modelSection: '模型',
  provider: '供應商',
  baseUrl: 'API 網址',
  apiKey: 'API 金鑰',
  apiKeySet: '已設定',
  apiKeyKeep: '已設定（留空則不變更）',
  modelId: '模型',
  currentModel: '目前使用',
  settingsSaved: '已儲存',
  settingsLoading: '載入設定中…',
  loadModels: '載入模型',
  reasoningEffort: '推理強度',
  reasoningDefault: '供應商預設',
  noModelsFound: '此供應商沒有回傳模型清單。',
  fileOpen: '開啟',
  fileDownload: '下載',
  fileLoading: '正在開啟檔案…',
  fileUnavailable: '這個檔案目前無法開啟。',
  dataTable: '資料表格',
  imageUnavailable: '圖片無法顯示',

  settings: "設定",
  about: "關於",
  general: "一般",
  appearance: "外觀",
  theme: "主題",
  themeSystem: "跟隨系統",
  themeLight: "淺色",
  themeDark: "深色",
  updates: "更新",
  copyVersionInfo: "複製版本資訊",
  copied: "已複製",
  copyright: "Copyright © 2026 Daniel Wang",
  settingsLabel: "LazyBoy 設定",
  workspaceDetail: "Bot 與對話都留在這台機器。",
  versionLine: "版本 {version} · 第 {build} Build",
  localBuild: "本機版本",
  close: "關閉",
  addAgent: "新增 Bot",
  creating: "建立中…",
  cancel: "取消",
  create: "建立",
  agentSettings: "Bot 設定",
  avatarAppearance: "頭像",
  avatarBot: "Bot",
  avatarUpload: "上傳",
  avatarDrop: "拖放或貼上圖片",
  avatarOr: "或",
  avatarBrowse: "選擇檔案",
  avatarSet: "設為頭像",
  avatarRestart: "重來",
  avatarReposition: "拖曳調整位置",
  avatarZoom: "縮放",
  avatarPasted: "貼上的圖片",
  color: "顏色",
  shape: "形狀",
  name: "名稱",
  tags: "標籤",
  tagsPlaceholder: "用頓號分開，會顯示在名字旁邊",
  shortTitle: "標題",
  shortTitlePlaceholder: "跟在名字後面，例如：行銷專家",
  persona: "人設",
  personaPlaceholder: "你是誰、怎麼說話、做事前要先確認什麼。",
  saveSettings: "儲存",
  saving: "儲存中…",
  settingsFailed: "設定儲存失敗",
  featureLoading: "正在載入…",
  featureLoadFailed: "功能載入失敗，請重試或關閉後繼續操作。",
  markdownLoadFailed: "文字格式暫時無法載入，仍可閱讀原文。",
  localWorkspace: "本機工作區",
} as const;

export type MessageKey = keyof typeof zhHant;
export type Messages = Record<MessageKey, string>;

export function formatMessage(template: string, vars?: Record<string, string>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) => vars[key] ?? `{${key}}`);
}

type I18nValue = {
  t: Messages;
  format: (key: MessageKey, vars?: Record<string, string>) => string;
};

const VALUE: I18nValue = {
  t: zhHant,
  format: (key, vars) => formatMessage(zhHant[key], vars),
};
const I18nContext = createContext<I18nValue>(VALUE);

/** The product ships in Traditional Chinese only; the provider keeps one stable copy table. */
export function I18nProvider({ children }: { children: ReactNode }) {
  useEffect(() => { document.documentElement.lang = "zh-Hant"; }, []);
  return <I18nContext.Provider value={VALUE}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}
