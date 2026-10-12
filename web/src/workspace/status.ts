import type { AgentActivity } from "../api";

export type Tone = "idle" | "busy" | "attention" | "queued";

export function activityStatus(activity?: AgentActivity | null): { tone: Tone; label: string } {
  switch (activity?.state) {
    case "running": return { tone: "busy", label: "工作中" };
    case "stopping": return { tone: "busy", label: "正在停止" };
    case "queued": return { tone: "queued", label: "排隊中" };
    case "waiting_input": return { tone: "attention", label: "等你回覆" };
    case "waiting_approval": return { tone: "attention", label: "等你允許" };
    default:
      if (activity?.queued) return { tone: "queued", label: "排隊中" };
      // The turn ended but delegated children are still working: the bot is not idle.
      if (activity?.background) return { tone: "busy", label: `背景工作中（${activity.background}）` };
      return { tone: "idle", label: "待命" };
  }
}
