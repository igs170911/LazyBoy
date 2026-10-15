import { readStorage } from "./storage";
import { Blobatar } from "@blobatar/react";
import { useGaze } from "@blobatar/react/gaze";
import {
  happy,
  idle,
  love,
  mad,
  sad,
  scared,
  shy,
  sick,
  sleepy,
  smug,
  surprised,
  thinking as thinkingPose,
  unsure,
  wink,
  type Expression,
} from "blobatar/expression";
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { EyeMotion } from "./eye-motion";
import type { AvatarShape } from "./types";

export const BLOBATAR_SHAPES = [
  "round",
  "organic",
  "boxy",
  "capsule",
  "nub",
  "cloud",
  "droplet",
  "hexagon",
  "sun",
  "triangle",
] as const;

export type BlobatarShape = (typeof BLOBATAR_SHAPES)[number];

/** grok-bot character roster, mapped onto Blobatar's silhouettes. */
export const CHARACTER_SHAPES: readonly { id: string; shape: BlobatarShape }[] = [
  { id: "blob", shape: "round" },
  { id: "pebble", shape: "organic" },
  { id: "squircle", shape: "boxy" },
  { id: "tablet", shape: "capsule" },
  { id: "wedge", shape: "triangle" },
  { id: "hex", shape: "hexagon" },
  { id: "cloud", shape: "cloud" },
  { id: "teardrop", shape: "droplet" },
  { id: "nub", shape: "nub" },
  { id: "sun", shape: "sun" },
];

export const EXPRESSION_LABELS: Record<string, string> = {
  idle: "平常", happy: "開心", wink: "眨眼", smug: "得意", surprised: "驚訝", love: "喜歡", shy: "害羞",
  unsure: "疑惑", sleepy: "想睡", sad: "難過", scared: "害怕", mad: "生氣", sick: "不舒服", thinking: "思考",
};

/** Face tuning a bot keeps on the server: blobatar trait positions, each 0–1. */
export const EYE_TRAITS = [
  { key: "eye.scale", label: "眼睛大小", low: "小", high: "大" },
  { key: "eye.ratio", label: "眼睛形狀", low: "圓", high: "長" },
  { key: "eye.gap", label: "眼距", low: "近", high: "遠" },
  { key: "eye.lean", label: "眼睛斜度", low: "／", high: "＼" },
  { key: "body.rot", label: "身體角度", low: "左", high: "右" },
] as const;
export type AvatarTraits = Partial<Record<(typeof EYE_TRAITS)[number]["key"], number>>;
/** Unset faces still vary by name, but only among the larger eye sizes. */
const DEFAULT_EYES = [0.6, 0.75, 0.9];

export function cleanTraits(value: unknown): AvatarTraits {
  const out: AvatarTraits = {};
  if (!value || typeof value !== "object") return out;
  for (const { key } of EYE_TRAITS) {
    const n = (value as Record<string, unknown>)[key];
    if (typeof n === "number" && Number.isFinite(n)) out[key] = Math.min(1, Math.max(0, n));
  }
  return out;
}

/** What the bot is doing, shown on its face instead of a status badge. */
export type Mood = "idle" | "ack" | "working" | "waiting" | "done" | "failed" | "sleepy" | "wink";
const MOOD_POSE: Record<Exclude<Mood, "idle">, AvatarExpression> = {
  ack: "surprised", working: "thinking", waiting: "unsure", done: "happy", failed: "sad", sleepy: "sleepy", wink: "wink",
};

export const AVATAR_COLORS = [
  { id: "white", value: "#F1F2F2" },
  { id: "brown", value: "#936439" },
  { id: "red", value: "#FF263C" },
  { id: "orange", value: "#FF6700" },
  { id: "yellow", value: "#FF9800" },
  { id: "green", value: "#00C972" },
  { id: "cyan", value: "#00BCA6" },
  { id: "blue", value: "#1084FE" },
  { id: "violet", value: "#9159FE" },
  { id: "magenta", value: "#FF309B" },
  { id: "gray", value: "#777777" },
] as const;

export const BLOBATAR_EXPRESSIONS = [
  "idle",
  "happy",
  "sad",
  "mad",
  "surprised",
  "wink",
  "sleepy",
  "smug",
  "unsure",
  "scared",
  "love",
  "shy",
  "sick",
  "thinking",
] as const;

export type AvatarExpression = (typeof BLOBATAR_EXPRESSIONS)[number];

const BLOBATAR_BACKGROUNDS = ["none", "circle", "squircle", "square"] as const;
type AvatarBackground = (typeof BLOBATAR_BACKGROUNDS)[number];

/** Per-browser face overrides from an older build; still read so those faces don't change, never written now. */
export interface AvatarLook {
  expression: AvatarExpression;
  background: AvatarBackground;
}

const DEFAULT_LOOK: AvatarLook = { expression: "idle", background: "none" };

const EXPRESSION_POSE: Record<AvatarExpression, Expression> = {
  idle,
  happy,
  sad,
  mad,
  surprised,
  wink,
  sleepy,
  smug,
  unsure,
  scared,
  love,
  shy,
  sick,
  thinking: thinkingPose,
};

/** Midpoints of blobatar gen2 shape bands in styles/blob.ts. */
const SHAPE_TRAIT: Record<BlobatarShape, number> = {
  round: 0.11,
  organic: 0.35,
  boxy: 0.54,
  capsule: 0.65,
  nub: 0.745,
  cloud: 0.825,
  droplet: 0.887,
  hexagon: 0.932,
  sun: 0.965,
  triangle: 0.99,
};

const SHAPE_ALIAS: Record<string, BlobatarShape> = {
  blob: "round",
  pebble: "organic",
  squircle: "boxy",
  tablet: "capsule",
  wedge: "triangle",
  hex: "hexagon",
  teardrop: "droplet",
  diamond: "boxy",
  drop: "droplet",
  cat: "nub",
  bunny: "nub",
  star: "sun",
  heart: "organic",
  egg: "round",
  ghost: "cloud",
  sprout: "nub",
  cactus: "nub",
  mushroom: "nub",
  paw: "nub",
};

const FALLBACK_COLORS = ["#3ec5a8", "#f5a03c", "#6a6bf5", "#9b5cf6", "#3b82f6", "#d9508a"];
const LOOK_STORE = "lazyhermes.avatarLook";
const AvatarLookContext = createContext<Record<string, AvatarLook>>({});

export function AvatarLookProvider({
  value,
  children,
}: {
  value: Record<string, AvatarLook>;
  children: ReactNode;
}) {
  return <AvatarLookContext.Provider value={value}>{children}</AvatarLookContext.Provider>;
}

export function readAvatarLooks(): Record<string, AvatarLook> {
  try {
    const raw = readStorage(LOOK_STORE);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, Partial<AvatarLook>>;
    const out: Record<string, AvatarLook> = {};
    for (const [id, look] of Object.entries(parsed || {})) {
      out[id] = {
        expression: isExpression(look.expression) ? look.expression : "idle",
        background: isBackground(look.background) ? look.background : "none",
      };
    }
    return out;
  } catch {
    return {};
  }
}

export function resolveBlobatarShape(shape: string | undefined | null): BlobatarShape {
  if (shape && (BLOBATAR_SHAPES as readonly string[]).includes(shape)) return shape as BlobatarShape;
  if (shape && SHAPE_ALIAS[shape]) return SHAPE_ALIAS[shape];
  if (shape?.startsWith("organic-")) return "organic";
  return "organic";
}

function isExpression(value: unknown): value is AvatarExpression {
  return typeof value === "string" && (BLOBATAR_EXPRESSIONS as readonly string[]).includes(value);
}

function isBackground(value: unknown): value is AvatarBackground {
  return typeof value === "string" && (BLOBATAR_BACKGROUNDS as readonly string[]).includes(value);
}

function fallbackColor(name: string): string {
  const hash = [...name].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 7);
  return FALLBACK_COLORS[hash % FALLBACK_COLORS.length];
}

/** Dark eyes on light bodies, light eyes on dark ones — same polarity blobatar uses. */
export function contrastEye(hex: string): string {
  const value = /^#([0-9a-f]{6})$/i.test(hex) ? hex : "#8B5CF6";
  const n = parseInt(value.slice(1), 16);
  const channel = (shift: number) => {
    const s = ((n >> shift) & 255) / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const luminance = 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0);
  return luminance >= 0.5 ? "#1b1b22" : "#f3f3f6";
}

/** The bot's own color, the same one its avatar is drawn with. */
export function botColor(name: string, color?: string): string {
  return color && /^#([0-9a-f]{6})$/i.test(color) ? color : fallbackColor(name);
}

function luminance(rgb: number[]): number {
  const [r, g, b] = rgb.map(v => { const s = v / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * Fill and text colors for a bubble in the bot's color. Bright colors (orange, green, cyan, white) keep their fill
 * with dark text; the rest get white text, darkened just enough for 4.5:1 contrast.
 */
export function bubbleTone(hex: string): { fill: string; ink: string } {
  const n = parseInt(hex.slice(1), 16);
  let rgb = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  if (1.05 / (luminance(rgb) + 0.05) < 3.3) return { fill: hex, ink: "#16161a" };
  for (let i = 0; i < 12 && 1.05 / (luminance(rgb) + 0.05) < 4.5; i++) rgb = rgb.map(v => Math.round(v * 0.94));
  return { fill: `#${rgb.map(v => v.toString(16).padStart(2, "0")).join("")}`, ink: "#ffffff" };
}

function blobBackground(value: AvatarBackground | undefined): false | "circle" | "squircle" | "square" {
  return !value || value === "none" ? false : value;
}

export const Avatar = memo(function Avatar({
  name,
  color,
  shape = "blob",
  active = false,
  thinking = false,
  online = false,
  size = 32,
  lookId,
  expression,
  gaze = true,
  src,
  mood = "idle",
  alive = false,
  eyes,
  motion = null,
}: {
  name: string;
  color?: string;
  shape?: AvatarShape;
  active?: boolean;
  thinking?: boolean;
  online?: boolean;
  size?: number;
  lookId?: string;
  expression?: AvatarExpression | string;
  gaze?: boolean;
  src?: string | null;
  mood?: Mood;
  alive?: boolean;
  eyes?: AvatarTraits | null;
  /** Eyes that move on their own (rolling while thinking, reading, typing…); they take over from following the pointer. */
  motion?: EyeMotion | null;
}) {
  const stored = useContext(AvatarLookContext)[lookId || ""] || DEFAULT_LOOK;
  const resolved = color || fallbackColor(name);
  const silhouette = resolveBlobatarShape(shape);
  const poseName = mood !== "idle" ? MOOD_POSE[mood] : thinking ? "thinking" : isExpression(expression) ? expression : stored.expression;
  const pose = poseName === "idle" ? undefined : EXPRESSION_POSE[poseName];
  const plate = blobBackground(stored.background);
  const travel = Math.max(1.8, Math.min(4, size * 0.045));
  const eyeKey = JSON.stringify(eyes || {});
  const traits = useMemo(() => ({ "eye.scale": DEFAULT_EYES, ...cleanTraits(JSON.parse(eyeKey)), shape: SHAPE_TRAIT[silhouette] }), [silhouette, eyeKey]);
  const photo = src?.trim() || "";
  // A stale or broken photo URL should fall back to the generated avatar, not a broken image.
  const [photoFailed, setPhotoFailed] = useState(false);
  useEffect(() => {
    setPhotoFailed(false);
  }, [photo]);
  const showPhoto = photo && !photoFailed;
  const moving = showPhoto ? null : motion;
  const animate = alive || thinking || moving || mood !== "idle" ? "always" as const : gaze ? "hover" as const : undefined;
  const palette = useMemo(() => ({ head: resolved, eye: contrastEye(resolved) }), [resolved]);
  const { ref } = useGaze({ travel, lookAt: gaze && !moving ? "pointer" : null });
  const host = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const svg = host.current?.querySelector("svg");
    if (!moving || !svg) return;
    // Loaded the first time a face moves; most pages show none until a bot starts working.
    let stop: (() => void) | undefined;
    let cancelled = false;
    void import("./eye-motion").then(({ moveEyes }) => { if (!cancelled) stop = moveEyes(svg, moving); });
    return () => { cancelled = true; stop?.(); };
  }, [moving]);
  return (
    <span
      ref={host}
      data-motion={moving || undefined}
      className={`avatar blobatar ${showPhoto ? "photo" : ""} ${active ? "online" : ""} ${thinking ? "thinking" : ""}`}
      style={{ "--bot-color": resolved, "--avatar-size": `${size}px` } as CSSProperties}
    >
      {showPhoto ? (
        <img alt="" draggable={false} src={photo} onError={() => setPhotoFailed(true)} />
      ) : (
        <Blobatar
          {...(animate ? { ref: moving ? undefined : ref, animate } : { animate: undefined })}
          name={name}
          expression={pose}
          background={plate}
          traits={traits}
          palette={palette}
          title={name}
        />
      )}
      {(online || active) && <i className="presence" aria-hidden="true" />}
    </span>
  );
});

