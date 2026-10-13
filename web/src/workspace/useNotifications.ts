import { useEffect, useRef } from "react";
import type { AgentRow } from "../api";
import { isMuted, notifyOn, showSystem } from "../notify";
import { toast } from "../ui/overlay";

const WORKING = new Set(["running", "stopping", "queued"]);
const WAITING = new Set(["waiting_input", "waiting_approval"]);

/**
 * "Get notified when this Bot finishes or is waiting for you" (Grok Bot). Watches every bot's state from the roster
 * poll: when the page is hidden, a system notification; when you are looking at another bot, a toast with a link.
 */
export function useNotifications(agents: AgentRow[], openId: string | null, onOpen: (id: string) => void) {
  const previous = useRef<Record<string, string>>({});
  const open = useRef(onOpen);
  open.current = onOpen;
  useEffect(() => {
    for (const agent of agents) {
      const state = agent.activity?.state || "idle";
      const before = previous.current[agent.id];
      previous.current[agent.id] = state;
      if (!before || before === state || isMuted(agent.id)) continue;
      const finished = WORKING.has(before) && state === "idle";
      const needsYou = WAITING.has(state) && !WAITING.has(before);
      if (!finished && !needsYou) continue;
      const text = needsYou ? "等你回覆" : "完成了";
      const body = needsYou ? (agent.activity?.question?.prompt || agent.activity?.question?.question || "需要你的回覆").slice(0, 140) : (agent.preview || "").slice(0, 140);
      if (document.hidden) { if (notifyOn()) void showSystem(`${agent.name} ${text}`, body, agent.id, agent.avatar_url || undefined); }
      else if (agent.id !== openId) toast(`${agent.name} ${text}`, needsYou ? "info" : "ok", { label: "查看", run: () => open.current(agent.id) }, 8000);
    }
  }, [agents, openId]);
  // Clicks on notifications, from the service worker or a page notification.
  useEffect(() => {
    const fromWorker = (event: MessageEvent) => { if (event.data?.type === "open-bot" && event.data.agentId) open.current(event.data.agentId); };
    const fromPage = (event: Event) => open.current(String((event as CustomEvent).detail));
    navigator.serviceWorker?.addEventListener("message", fromWorker);
    window.addEventListener("lazyhermes:open-bot", fromPage);
    const target = new URLSearchParams(location.search).get("bot");
    if (target) { open.current(target); history.replaceState(null, "", location.pathname); }
    return () => { navigator.serviceWorker?.removeEventListener("message", fromWorker); window.removeEventListener("lazyhermes:open-bot", fromPage); };
  }, []);
}
