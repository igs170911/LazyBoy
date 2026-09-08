// Everything that decides when the screen veil shows, and when the viewer may
// be thrown away. Both jobs used to be a fixed timer plus "null the url", which
// is exactly what made starting and handing over feel like a stall: the mascot
// popped, the desktop went black, and noVNC booted from scratch.

import type { ComputerState } from "./types";

/** The viewer url is the proxy path for a bot and never carries a nonce, so the
 * frame can mount while the desktop is still coming up and let noVNC dial into
 * it on retry instead of waiting for a status round trip. */
export function viewerPath(botId: string): string {
  return `/view/${encodeURIComponent(botId)}/vnc.html`;
}

/** Only a computer that is really gone may drop the pixels. While it boots,
 * wakes, or a poll blinks, the mounted frame keeps its VNC session; remounting
 * costs a page load, a handshake, and a black flash for no information. */
export function keepScreenUrl(current: string | null, next: string | null, state: ComputerState): string | null {
  if (next) return next;
  if (state === "stopped" || state === "error") return null;
  return current;
}

/** A handoff should read as one deliberate beat, not as a wait. This is the
 * ceiling, used only when the server never confirms; agreement on the new
 * holder usually releases the veil far sooner. */
export const HANDOFF_MS = 900;
/** The floor. Fast as the reply may land, the swap is worth one visible beat,
 * otherwise the mascot strobes instead of gesturing. */
export const HANDOFF_MIN_MS = 320;
/** Kept in step with the veil's CSS fade so it never vanishes mid-animation. */
export const VEIL_FADE_MS = 220;

/** How much longer the veil has to stay after the server agreed on a holder:
 * zero once the beat has been served, and never more than the ceiling timer,
 * which is what a lost reply falls back on. */
export function handoffRemaining(startedAt: number, now: number): number {
  const held = now - startedAt;
  if (held >= HANDOFF_MS) return 0;
  return Math.max(HANDOFF_MIN_MS - held, 0);
}

export interface Veil { label: string | null; leaving: boolean }

/** Fade in with a label, fade out once the label clears, and stay out when
 * there was never anything to show. Pure, so a status blip can be tested. */
export function nextVeil(label: string | null, current: Veil): Veil {
  if (label) return current.label === label && !current.leaving ? current : { label, leaving: false };
  if (!current.label) return current;
  return current.leaving ? current : { label: current.label, leaving: true };
}

/** The lease is advisory inside the container, so the browser gate is what
 * actually moves the mouse: whoever holds control may send input right now. */
export function viewOnlyFor(holder: "none" | "bot" | "user"): boolean {
  return holder !== "user";
}
