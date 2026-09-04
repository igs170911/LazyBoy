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
import { createContext, useContext, type CSSProperties, type ReactNode } from "react";
import type { AvatarShape, RoomMember } from "./types";

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

export const BLOBATAR_BACKGROUNDS = ["none", "circle", "squircle", "square"] as const;
export type AvatarBackground = (typeof BLOBATAR_BACKGROUNDS)[number];

export interface AvatarLook {
  expression: AvatarExpression;
  background: AvatarBackground;
}

export const DEFAULT_LOOK: AvatarLook = { expression: "idle", background: "none" };

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
  blob: "organic",
  squircle: "boxy",
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
const LOOK_STORE = "lazyboy.avatarLook";
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
    const raw = localStorage.getItem(LOOK_STORE);
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

export function writeAvatarLook(id: string, look: AvatarLook) {
  const next = { ...readAvatarLooks(), [id]: look };
  localStorage.setItem(LOOK_STORE, JSON.stringify(next));
}

export function resolveBlobatarShape(shape: string | undefined | null): BlobatarShape {
  if (shape && (BLOBATAR_SHAPES as readonly string[]).includes(shape)) return shape as BlobatarShape;
  if (shape && SHAPE_ALIAS[shape]) return SHAPE_ALIAS[shape];
  if (shape?.startsWith("organic-")) return "organic";
  return "organic";
}

/** Persist using names the existing API already accepts. */
const STORE_SHAPE: Record<BlobatarShape, AvatarShape> = {
  round: "round",
  organic: "blob",
  boxy: "squircle",
  capsule: "capsule",
  nub: "paw",
  cloud: "cloud",
  droplet: "drop",
  hexagon: "hexagon",
  sun: "star",
  triangle: "triangle",
};

export function persistBlobatarShape(shape: string | undefined | null): AvatarShape {
  return STORE_SHAPE[resolveBlobatarShape(shape)];
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

function blobBackground(value: AvatarBackground | undefined): false | "circle" | "squircle" | "square" {
  return !value || value === "none" ? false : value;
}

export function Avatar({
  name,
  color,
  shape = "blob",
  active = false,
  thinking = false,
  online = false,
  size = 32,
  lookId,
  expression,
  background,
  gaze = true,
}: {
  name: string;
  color?: string;
  shape?: AvatarShape;
  active?: boolean;
  thinking?: boolean;
  online?: boolean;
  size?: number;
  lookId?: string;
  expression?: AvatarExpression;
  background?: AvatarBackground;
  gaze?: boolean;
}) {
  const stored = useContext(AvatarLookContext)[lookId || ""] || DEFAULT_LOOK;
  const resolved = color || fallbackColor(name);
  const silhouette = resolveBlobatarShape(shape);
  const poseName = thinking ? "thinking" : expression || stored.expression;
  const pose = poseName === "idle" ? undefined : EXPRESSION_POSE[poseName];
  const plate = blobBackground(background || stored.background);
  const travel = Math.max(1.8, Math.min(4, size * 0.045));
  const { ref } = useGaze({ travel, lookAt: gaze ? "pointer" : null });
  return (
    <span
      className={`avatar blobatar ${active ? "online" : ""} ${thinking ? "thinking" : ""}`}
      style={{ "--bot-color": resolved, "--avatar-size": `${size}px` } as CSSProperties}
    >
      <Blobatar
        ref={ref}
        name={name}
        animate="always"
        expression={pose}
        background={plate}
        traits={{ shape: SHAPE_TRAIT[silhouette] }}
        palette={{ head: resolved, eye: contrastEye(resolved) }}
        title={name}
      />
      {(online || active) && <i className="presence" aria-hidden="true" />}
    </span>
  );
}

export function AvatarStack({
  members,
  size = 38,
  online = false,
  thinkingIds,
}: {
  members: RoomMember[];
  size?: number;
  online?: boolean;
  thinkingIds?: string[];
}) {
  if (members.length === 1) {
    const member = members[0];
    return (
      <Avatar
        lookId={member.id}
        name={member.name}
        color={member.avatarColor}
        shape={member.avatarShape}
        size={size}
        thinking={Boolean(thinkingIds?.includes(member.id))}
        online={online}
      />
    );
  }
  const pair = members.length === 2;
  const miniSize = Math.round(size * (pair ? 0.65 : 0.54));
  const shown = members.slice(0, members.length > 3 ? 2 : 3);
  const positions = pair
    ? [{ left: 0, top: 0 }, { left: size - miniSize, top: size - miniSize }]
    : [
        { left: (size - miniSize) / 2, top: 0 },
        { left: 0, top: size - miniSize },
        { left: size - miniSize, top: size - miniSize },
      ];
  return (
    <span className="avatar-stack" style={{ width: size, height: size }}>
      {shown.map((member, index) => (
        <span className="stack-item" key={member.id} style={{ ...positions[index], zIndex: index + 1 }}>
          <Avatar
            lookId={member.id}
            name={member.name}
            color={member.avatarColor}
            shape={member.avatarShape}
            size={miniSize}
            thinking={Boolean(thinkingIds?.includes(member.id))}
            online={Boolean(online && index === shown.length - 1)}
          />
        </span>
      ))}
      {members.length > 3 && (
        <span
          className="stack-extra"
          style={{ left: size - miniSize, top: size - miniSize, zIndex: 3, width: miniSize, height: miniSize }}
        >
          +{members.length - 2}
        </span>
      )}
    </span>
  );
}
