import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { api, type AgentRow, type AgentSkill, type ConnectorApp, type RoomRow } from "../api";
import { Avatar } from "../avatar";
import { Brain, Plug } from "../ui/icons";
import { avatarProps } from "./Sidebar";
import { RoomAvatar } from "./Team";

export type PickOption = { key: string; label: string; detail: string; icon: ReactNode; insert: string; group: string };
type Trigger = { char: "/" | "@"; query: string; start: number };

const CACHE_MS = 60_000;
const skillCache = new Map<string, { at: number; rows: AgentSkill[] }>();
let appCache: { at: number; rows: ConnectorApp[] } | null = null;

/** Where a `/` or `@` the user is typing starts, if the caret is right after one (start of text or after a space). */
function triggerAt(value: string, caret: number): Trigger | null {
  const match = /(^|\s)([/@])([^\s/@]{0,40})$/.exec(value.slice(0, caret));
  return match ? { char: match[2] as "/" | "@", query: match[3], start: caret - match[3].length - 1 } : null;
}

/**
 * Grok's composer references: `/` picks a saved skill, `@` a teammate bot, a group chat or a connected app. The
 * choice is written into the message as plain words the bot understands, e.g. 使用「deep-research」技能.
 */
export function useComposerPicker({ agentId, text, area, team, rooms, setText }: {
  agentId: string; text: string; area: RefObject<HTMLTextAreaElement | null>; team: AgentRow[]; rooms: RoomRow[]; setText: (value: string) => void;
}) {
  const [trigger, setTrigger] = useState<Trigger | null>(null);
  const [pick, setPick] = useState(0);
  const [skills, setSkills] = useState<AgentSkill[]>(() => skillCache.get(agentId)?.rows || []);
  const [apps, setApps] = useState<ConnectorApp[]>(() => appCache?.rows || []);

  useEffect(() => {
    if (trigger?.char !== "/") return;
    const cached = skillCache.get(agentId);
    if (cached && Date.now() - cached.at < CACHE_MS) { setSkills(cached.rows); return; }
    let alive = true;
    api.skills(agentId).then(data => { skillCache.set(agentId, { at: Date.now(), rows: data.skills || [] }); if (alive) setSkills(data.skills || []); }).catch(() => undefined);
    return () => { alive = false; };
  }, [trigger?.char, agentId]);
  useEffect(() => {
    if (trigger?.char !== "@") return;
    if (appCache && Date.now() - appCache.at < CACHE_MS) { setApps(appCache.rows); return; }
    let alive = true;
    api.connectors().then(data => { appCache = { at: Date.now(), rows: data.apps || [] }; if (alive) setApps(data.apps || []); }).catch(() => undefined);
    return () => { alive = false; };
  }, [trigger?.char]);

  const options = useMemo<PickOption[]>(() => {
    if (!trigger) return [];
    const needle = trigger.query.toLowerCase();
    const hit = (...values: (string | undefined)[]) => !needle || values.some(value => value?.toLowerCase().includes(needle));
    if (trigger.char === "/") {
      return skills.filter(skill => skill.enabled !== false && hit(skill.name, skill.description)).slice(0, 12).map(skill => ({
        key: `skill:${skill.id || skill.name}`, group: "技能", label: skill.name, detail: skill.description || "", icon: <Brain />, insert: `使用「${skill.name}」技能，`,
      }));
    }
    const bots = team.filter(bot => bot.id !== agentId && !bot.hidden && hit(bot.name, bot.title)).slice(0, 8).map(bot => ({
      key: `bot:${bot.id}`, group: "Bot", label: bot.name, detail: bot.title || "請它幫忙（Bot 之間傳訊息）", icon: <Avatar {...avatarProps(bot)} size={22} gaze={false} />, insert: `@${bot.name} `,
    }));
    const groups = rooms.filter(room => room.member_ids.includes(agentId) && hit(room.name)).slice(0, 4).map(room => ({
      key: `room:${room.id}`, group: "群組對話", label: room.name, detail: "在這個群組發言", icon: <RoomAvatar members={room.members} size={22} />, insert: `@${room.name} `,
    }));
    const connected = apps.filter(app => app.state === "connected" && hit(app.title, app.id, ...app.keywords)).slice(0, 6).map(app => ({
      key: `app:${app.id}`, group: "應用程式", label: app.title, detail: app.description, icon: <Plug />, insert: `@${app.title} `,
    }));
    return [...bots, ...groups, ...connected];
  }, [trigger, skills, apps, team, rooms, agentId]);

  const blurTimer = useRef(0);
  const track = useCallback((value: string, caret: number) => { window.clearTimeout(blurTimer.current); setTrigger(triggerAt(value, caret)); setPick(0); }, []);
  const close = useCallback(() => { window.clearTimeout(blurTimer.current); setTrigger(null); }, []);
  /** Leaving the field closes the list a moment later (a click on an option lands first); coming back cancels that. */
  const onBlur = useCallback(() => { window.clearTimeout(blurTimer.current); blurTimer.current = window.setTimeout(() => setTrigger(null), 150); }, []);
  const onFocus = useCallback(() => window.clearTimeout(blurTimer.current), []);
  useEffect(() => () => window.clearTimeout(blurTimer.current), []);
  const choose = useCallback((option: PickOption) => {
    const el = area.current;
    if (!el || !trigger) return;
    const caret = el.selectionStart ?? text.length;
    const before = text.slice(0, trigger.start) + option.insert;
    setText(before + text.slice(caret));
    setTrigger(null);
    requestAnimationFrame(() => { el.focus(); el.setSelectionRange(before.length, before.length); });
  }, [area, text, trigger, setText]);

  /** Arrow keys, Enter/Tab and Esc while the list is open; true when the key was used. */
  const onKey = useCallback((event: React.KeyboardEvent) => {
    if (!trigger || event.nativeEvent.isComposing) return false;
    if (event.key === "Escape") { event.preventDefault(); setTrigger(null); return true; }
    if (!options.length) return false;
    if (event.key === "ArrowDown") { event.preventDefault(); setPick(i => (i + 1) % options.length); return true; }
    if (event.key === "ArrowUp") { event.preventDefault(); setPick(i => (i - 1 + options.length) % options.length); return true; }
    if (event.key === "Enter" || event.key === "Tab") { event.preventDefault(); choose(options[Math.min(pick, options.length - 1)]); return true; }
    return false;
  }, [trigger, options, pick, choose]);

  const menu = trigger ? (
    <ul className="mention-menu composer-picker" role="listbox" aria-label={trigger.char === "/" ? "技能" : "提到"}>
      {options.length ? options.map((option, index) => (
        <li key={option.key}>
          {index === 0 || options[index - 1].group !== option.group ? <span className="picker-group">{option.group}</span> : null}
          <button type="button" role="option" aria-selected={index === pick} className={index === pick ? "picked" : ""}
            onMouseDown={event => event.preventDefault()} onMouseMove={() => setPick(index)} onClick={() => choose(option)}>
            <span className="picker-icon">{option.icon}</span>
            <strong>{trigger.char === "/" ? "/" : "@"}{option.label}</strong>
            {option.detail ? <small>{option.detail}</small> : null}
          </button>
        </li>
      )) : (
        <li className="picker-empty">{trigger.char === "/" ? (skills.length ? "沒有符合的技能" : "這個 Bot 還沒有技能，可以在 Bot 設定的「技能」新增，或用「教它一件事」") : "沒有符合的 Bot、群組或應用程式"}</li>
      )}
    </ul>
  ) : null;
  return { menu, track, close, onKey, onBlur, onFocus, open: !!trigger };
}
