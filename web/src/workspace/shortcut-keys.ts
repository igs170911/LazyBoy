const MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const MOD = MAC ? "⌘" : "Ctrl";
const ALT = MAC ? "⌥" : "Alt";

/** Display text for a shortcut written with "Mod" (⌘ on a Mac, Ctrl elsewhere) and "Alt". */
export function keys(combo: string) {
  return combo.replace(/Mod/g, MOD).replace(/Alt/g, ALT).replace(/Shift/g, MAC ? "⇧" : "Shift");
}
