import { project, survey } from "blobatar/gaze";

/**
 * What a bot's eyes do on their own, as in Grok Bot: roll around while it thinks, read along lines while it browses,
 * look down at the keys while it runs commands. Built on blobatar's own gaze projection, so the eyes turn around the
 * body (sliding toward the edge and foreshortening) exactly as when they follow the pointer, and the blink, breathing
 * and expressions keep running from blobatar's stylesheet.
 */
export type EyeMotion = "think" | "read" | "type" | "scan" | "wait" | "wander";

type Face = NonNullable<ReturnType<typeof survey>>;
type Aim = { x: number; y: number };
type Path = (seconds: number, aim: Aim, random: () => number) => Aim;

/** How far the face turns, in radians: large enough to read at 20px, short of the limb where eyes vanish. */
const REACH = 0.55;
/** Pursuit time constant: the eyes follow their path smoothly but a jump still reads as a quick glance. */
const SETTLE = 90;

/** A target that holds still, then jumps: glances rather than drifting. */
function glances(pick: (random: () => number, aim: Aim) => Aim, hold: [number, number]): Path {
  let until = 0;
  let target: Aim = { x: 0, y: 0 };
  return (seconds, aim, random) => {
    if (seconds >= until) {
      target = pick(random, aim);
      until = seconds + hold[0] + random() * (hold[1] - hold[0]);
    }
    return target;
  };
}

const PATHS: Record<EyeMotion, () => Path> = {
  // Eyes roll up and around, a little faster on the upswing, like turning a problem over.
  think: () => (s) => {
    const turn = s * Math.PI * 2 / 2.8;
    const angle = turn + 0.35 * Math.sin(turn);
    return { x: 0.1 + 0.5 * Math.cos(angle), y: -0.32 + 0.38 * Math.sin(angle) };
  },
  // Reading: left to right along a line, a quick return, the next line down.
  read: () => (s) => {
    const line = 1.5;
    const phase = (s % line) / line;
    const row = Math.floor(s / line) % 3;
    const x = phase < 0.82 ? -0.5 + phase / 0.82 : 0.5 - (phase - 0.82) / 0.18;
    return { x: x * 0.95, y: -0.12 + row * 0.2 };
  },
  // Looking down at the keys, small quick movements.
  type: () => (s) => ({ x: 0.16 * Math.sin(s * 6.3) + 0.08 * Math.sin(s * 14.1), y: 0.5 + 0.05 * Math.sin(s * 9.7) }),
  // Looking over a screen: quick glances anywhere.
  scan: () => glances(random => ({ x: (random() * 2 - 1) * 0.8, y: (random() * 2 - 1) * 0.6 }), [0.35, 0.9]),
  // Waiting on you: mostly down at the message box, now and then up at you.
  wait: () => glances((random, aim) => aim.y > 0.2 && random() < 0.6 ? { x: 0, y: -0.05 } : { x: (random() - 0.5) * 0.3, y: 0.55 }, [0.9, 2.6]),
  // Idle and alive: looks around, often back at you.
  wander: () => glances((random, aim) => Math.hypot(aim.x, aim.y) > 0.2 && random() < 0.5
    ? { x: 0, y: 0 }
    : { x: (random() * 2 - 1) * 0.75, y: (random() * 2 - 1) * 0.5 }, [1.1, 3.2]),
};

type Driver = {
  svg: SVGSVGElement; eyes: Element | null; face: Face | null; path: Path; aim: Aim; held: number; random: () => number; started: number;
  /** Stopped: easing back to the middle before handing the face back. */
  leaving: boolean;
};

const drivers = new Map<SVGSVGElement, Driver>();
let frame = 0;
let last = 0;
const reduced = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
const REST: Path = () => ({ x: 0, y: 0 });

function measure(driver: Driver) {
  driver.eyes = driver.svg.querySelector(".mo-eyes");
  driver.face = driver.eyes ? survey(driver.svg) : null;
}

function write(driver: Driver) {
  const { eyes, face, aim } = driver;
  if (!eyes || !face) return;
  const style = (eyes as SVGElement).style;
  face.marks.forEach((mark, index) => {
    const turn = project(mark, aim.x * REACH, aim.y * REACH);
    const n = index + 1;
    style.setProperty(`--mo-gz-dx${n}`, (turn.dx * face.rx).toFixed(3));
    style.setProperty(`--mo-gz-dy${n}`, (turn.dy * face.ry).toFixed(3));
    style.setProperty(`--mo-gz-sx${n}`, turn.sx.toFixed(4));
    style.setProperty(`--mo-gz-sy${n}`, turn.sy.toFixed(4));
    style.setProperty(`--mo-gz-t${n}`, turn.t.toFixed(3));
  });
}

function release(driver: Driver) {
  drivers.delete(driver.svg);
  driver.svg.style.removeProperty("--mo-track-hold");
  const style = (driver.eyes as SVGElement | null)?.style;
  if (!style || !driver.face) return;
  for (let n = 1; n <= driver.face.marks.length; n++) for (const key of ["dx", "dy", "sx", "sy", "t"]) style.removeProperty(`--mo-gz-${key}${n}`);
}

function tick(now: number) {
  frame = 0;
  const dt = last ? Math.min(now - last, 64) : 16;
  last = now;
  if (!document.hidden) {
    const follow = 1 - Math.exp(-dt / SETTLE);
    for (const driver of drivers.values()) {
      if (!driver.svg.isConnected) { release(driver); continue; }
      // An expression change can redraw the eyes; measure the new ones.
      if (!driver.eyes?.isConnected || !driver.face) measure(driver);
      if (!driver.face) continue;
      const target = driver.path((now - driver.started) / 1000, driver.aim, driver.random);
      driver.aim = { x: driver.aim.x + (target.x - driver.aim.x) * follow, y: driver.aim.y + (target.y - driver.aim.y) * follow };
      // Quiet blobatar's own idle glances while the eyes have somewhere to be, and bring them back after.
      const hold = driver.leaving ? Math.max(0, driver.held - dt / 300) : Math.min(1, driver.held + dt / 300);
      if (hold !== driver.held) driver.svg.style.setProperty("--mo-track-hold", (driver.held = hold).toFixed(3));
      write(driver);
      if (driver.leaving && Math.hypot(driver.aim.x, driver.aim.y) < 0.01) release(driver);
    }
  }
  if (drivers.size) frame = requestAnimationFrame(tick);
  else last = 0;
}

/**
 * Starts the eyes of one blobatar `svg` on a motion and returns the function that stops them. A new motion on the same
 * face carries on from where the eyes are, and stopping eases them back to the middle, so changes never jump.
 */
export function moveEyes(svg: SVGSVGElement, motion: EyeMotion): () => void {
  if (reduced?.matches) return () => undefined;
  let driver = drivers.get(svg);
  if (!driver) {
    let seed = Math.floor(Math.random() * 2 ** 31);
    const random = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
    driver = { svg, eyes: null, face: null, path: REST, aim: { x: 0, y: 0 }, held: 0, random, started: 0, leaving: false };
    measure(driver);
    drivers.set(svg, driver);
  }
  const mine = PATHS[motion]();
  Object.assign(driver, { path: mine, started: performance.now(), leaving: false });
  if (!frame) frame = requestAnimationFrame(tick);
  return () => {
    if (driver.path !== mine) return;
    Object.assign(driver, { path: REST, leaving: true });
    if (!frame) frame = requestAnimationFrame(tick);
  };
}
