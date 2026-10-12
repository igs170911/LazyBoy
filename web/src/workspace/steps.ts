export type StepKind = "browser" | "search" | "web" | "terminal" | "write" | "read" | "desktop" | "skill" | "memory" | "plan" | "ask" | "image" | "tool";

export function stepKind(name: string): StepKind {
  const tool = name.trim().toLowerCase();
  if (tool.startsWith("browser")) return "browser";
  if (tool.includes("search_files") || tool.includes("list_files") || tool.includes("read_file")) return "read";
  if (tool.includes("search")) return "search";
  if (tool.includes("extract") || tool.includes("fetch") || tool.includes("crawl") || tool.startsWith("web")) return "web";
  if (tool.includes("terminal") || tool.includes("shell") || tool.includes("bash") || tool.includes("execute") || tool.includes("process")) return "terminal";
  if (tool.includes("write") || tool.includes("patch") || tool.includes("edit")) return "write";
  if (tool.includes("read") || tool.includes("list_files") || tool.includes("search_files")) return "read";
  if (tool.includes("desktop") || tool.includes("computer") || tool.includes("screenshot")) return "desktop";
  if (tool.includes("skill")) return "skill";
  if (tool.includes("memory")) return "memory";
  if (tool === "todo") return "plan";
  if (tool === "clarify") return "ask";
  if (tool.includes("image") || tool.includes("vision")) return "image";
  return "tool";
}

const LABELS: Record<StepKind, string> = {
  browser: "操作瀏覽器",
  search: "搜尋資料",
  web: "讀取網頁",
  terminal: "執行指令",
  write: "寫入檔案",
  read: "讀取檔案",
  desktop: "操作桌面",
  skill: "使用技能",
  memory: "更新記憶",
  plan: "整理步驟",
  ask: "詢問你",
  image: "檢視圖片",
  tool: "使用工具",
};

export function stepLabel(name: string) {
  return LABELS[stepKind(name)];
}

export function stepDetail(detail: string) {
  const text = detail.replace(/\s+/g, " ").trim();
  return text.length > 140 ? text.slice(0, 137) + "…" : text;
}
