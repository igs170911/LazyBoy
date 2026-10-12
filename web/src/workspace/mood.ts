import { useEffect, useRef, useState } from "react";
import type { Mood } from "../avatar";

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
