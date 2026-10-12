import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AgentGroup, type AgentRow } from "../api";

function sameRow(a: AgentRow, b: AgentRow) {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function useAgents() {
  const [agents, setAgents] = useState<AgentRow[]>([]);
  const [groups, setGroups] = useState<AgentGroup[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const version = useRef(0);

  const adopt = useCallback((next: AgentRow[]) => {
    setAgents(previous => {
      const byId = new Map(previous.map(row => [row.id, row]));
      const merged = next.map(row => { const old = byId.get(row.id); return old && sameRow(old, row) ? old : row; });
      return merged.length === previous.length && merged.every((row, i) => row === previous[i]) ? previous : merged;
    });
  }, []);

  const refresh = useCallback(async () => {
    const request = ++version.current;
    try {
      const data = await api.agents();
      if (request !== version.current) return;
      adopt(data.agents);
      setGroups(previous => JSON.stringify(previous) === JSON.stringify(data.groups) ? previous : data.groups);
      setError("");
    } catch (err) {
      if (request === version.current) setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (request === version.current) setLoaded(true);
    }
  }, [adopt]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, 5000);
    const visible = () => { if (!document.hidden) void refresh(); };
    document.addEventListener("visibilitychange", visible);
    return () => { window.clearInterval(timer); document.removeEventListener("visibilitychange", visible); };
  }, [refresh]);

  const upsert = useCallback((row: AgentRow) => {
    version.current++;
    setAgents(previous => previous.some(item => item.id === row.id) ? previous.map(item => item.id === row.id ? { ...item, ...row } : item) : [...previous, row]);
  }, []);

  const remove = useCallback((id: string) => {
    version.current++;
    setAgents(previous => previous.filter(item => item.id !== id));
  }, []);

  const applyTeam = useCallback((data: { groups?: AgentGroup[]; agents?: AgentRow[] }) => {
    version.current++;
    if (data.groups) setGroups([...data.groups].sort((a, b) => (a.position ?? 0) - (b.position ?? 0)));
    if (data.agents) adopt(data.agents);
  }, [adopt]);

  return { agents, groups, loaded, error, refresh, upsert, remove, applyTeam };
}
