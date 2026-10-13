import { readStorage, writeStorage } from "./storage";
export const THEME_PREFS = ["system", "light", "dark"] as const;
export type ThemePref = (typeof THEME_PREFS)[number];

const STORAGE_KEY = "lazyhermes.theme";

export function isThemePref(value: string | null | undefined): value is ThemePref {
  return value === "system" || value === "light" || value === "dark";
}

export function readTheme(): ThemePref {
  try {
    const saved = readStorage(STORAGE_KEY);
    if (isThemePref(saved)) return saved;
  } catch {
    /* ignore */
  }
  return "dark";
}

export function resolvedTheme(pref: ThemePref): "light" | "dark" {
  if (pref !== "system") return pref;
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

export function applyTheme(pref: ThemePref = readTheme()): void {
  if (typeof document === "undefined") return;
  const mode = resolvedTheme(pref);
  document.documentElement.dataset.theme = mode;
  document.documentElement.style.colorScheme = mode;
  // Match the browser and home-screen chrome to the page background so no band shows above or below the app.
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", mode === "light" ? "#eceef1" : "#0e0e11");
}

export function persistTheme(pref: ThemePref): void {
  try {
    writeStorage(STORAGE_KEY, pref);
  } catch {
    /* ignore */
  }
  applyTheme(pref);
}

export function watchSystemTheme(): () => void {
  if (typeof window === "undefined") return () => undefined;
  const media = window.matchMedia("(prefers-color-scheme: light)");
  const onChange = () => {
    if (readTheme() === "system") applyTheme("system");
  };
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

/** Accent color: Grok's black-and-white ("mono", the default) or LazyBoy's original violet. */
export const ACCENT_PREFS = ["mono", "violet"] as const;
export type AccentPref = (typeof ACCENT_PREFS)[number];
const ACCENT_KEY = "lazyhermes.accent";

export function readAccent(): AccentPref {
  try { const saved = readStorage(ACCENT_KEY); if (saved === "mono" || saved === "violet") return saved; } catch { /* ignore */ }
  return "mono";
}

export function applyAccent(pref: AccentPref = readAccent()): void {
  if (typeof document !== "undefined") document.documentElement.dataset.accent = pref;
}

export function persistAccent(pref: AccentPref): void {
  try { writeStorage(ACCENT_KEY, pref); } catch { /* ignore */ }
  applyAccent(pref);
}
