import { useEffect, useRef, useState } from "react";
import type { Mood } from "../avatar";
import type { EyeMotion } from "../eye-motion";
import { stepKind } from "./steps";

const WORKING = new Set(["running", "stopping"]);
const WAITING = new Set(["waiting_input", "waiting_approval"]);
const SLEEPY_AFTER = 6 * 3600_000;

function steady(state: string | undefined, lastAt?: string | null): Mood {
  if (state && WORKING.has(state)) return "working";
  if (state && WAITING.has(state)) return "waiting";
  const last = lastAt ? Date.parse(lastAt) : NaN;
  return Number.isFinite(last) && Date.now() - last > SLEEPY_AFTER ? "sleepy" : "idle";
}

/**
 * The bot's face follows its work: a start-of-task double take, thinking while it works, puzzled while it waits on
 * you, a short happy (or sad) beat when it finishes. `playful` adds an occasional wink while it is idle.
 */
export function useMood(state: string | undefined, { verdict, lastAt, playful = false }: { verdict?: string | null; lastAt?: string | null; playful?: boolean } = {}): Mood {
  const base = steady(state, lastAt);
  const [beat, setBeat] = useState<Mood | null>(null);
  const previous = useRef(base);
  const verdictRef = useRef(verdict);
  verdictRef.current = verdict;

  useEffect(() => {
    const before = previous.current;
    previous.current = base;
    if (before === base) return;
    let next: Mood | null = null;
    let ms = 0;
    if (base === "working" && before !== "waiting") { next = "ack"; ms = 700; }
    else if ((before === "working" || before === "waiting") && (base === "idle" || base === "sleepy")) {
      next = verdictRef.current === "failed" ? "failed" : "done"; ms = 1800;
    }
    setBeat(next);
    if (!next) return;
    const timer = window.setTimeout(() => setBeat(null), ms);
    return () => window.clearTimeout(timer);
  }, [base]);

  useEffect(() => {
    if (!playful || base !== "idle") return;
    let timer = 0;
    const schedule = () => {
      timer = window.setTimeout(() => {
        if (!document.hidden) setBeat(current => current || "wink");
        timer = window.setTimeout(() => { setBeat(current => current === "wink" ? null : current); schedule(); }, 900);
      }, 9000 + Math.random() * 14000);
    };
    schedule();
    return () => window.clearTimeout(timer);
  }, [playful, base]);

  return beat || base;
}

const READING = new Set(["browser", "search", "web", "read", "image", "skill"]);
const TYPING = new Set(["terminal", "write", "memory", "plan"]);

/**
 * Where the bot's eyes go while it works, from what it is doing right now: reading along lines while it browses or
 * searches, down at the keys while it runs commands or writes, around the screen on the desktop, rolling while it
 * just thinks. `steps` are the live task's tool calls, when known; the last one still running is what it does now.
 */
export function eyeMotion(state: string | undefined, steps: readonly { name: string; state: string }[] = []): EyeMotion | null {
  if (state && WAITING.has(state)) return "wait";
  if (!state || !WORKING.has(state)) return null;
  let step = "";
  for (let i = steps.length - 1; i >= 0 && !step; i--) if (steps[i].state === "running") step = steps[i].name;
  if (!step) return "think";
  const kind = stepKind(step);
  return READING.has(kind) ? "read" : TYPING.has(kind) ? "type" : kind === "desktop" ? "scan" : kind === "ask" ? "wait" : "think";
}
