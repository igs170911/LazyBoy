import { useEffect, useState } from "react";
import { api, type StorageUsage } from "./api";
import { toast } from "./ui/overlay";
import { bytes } from "./workspace/format";
import { tokens } from "./format-tokens";

const ROWS: { key: keyof Omit<StorageUsage, "total">; label: string; note?: string }[] = [
  { key: "workspace", label: "工作成果", note: "不會自動刪除" },
  { key: "uploads", label: "你傳的附件", note: "30 天後自動刪除" },
  { key: "caches", label: "快取與暫存", note: "每天自動清理" },
  { key: "browser", label: "瀏覽器登入資料" },
  { key: "tools", label: "Bot 安裝的工具" },
  { key: "other", label: "其他" },
];

/** Tokens the bot used in the last week, from what the model reported for each turn. */
function UsageLine({ agentId }: { agentId: string }) {
  const [usage, setUsage] = useState<Awaited<ReturnType<typeof api.usage>> | null>(null);
  useEffect(() => { api.usage(agentId, 7).then(setUsage).catch(() => setUsage(null)); }, [agentId]);
  if (!usage) return null;
  return (
    <div className="usage-line">
      <span className="avatar-section-label">最近 7 天用量</span>
      <strong>{usage.turns ? `${usage.turns} 次工作 · ${tokens(usage.total)} tokens` : "沒有用量"}</strong>
      {usage.turns ? <small>送出 {tokens(usage.prompt)} · 回覆 {tokens(usage.completion)}{usage.models.length > 1 ? ` · ${usage.models.map(m => `${m.model} ${tokens(m.total)}`).join("、")}` : ""}</small> : null}
    </div>
  );
}

/** How much disk the bot uses, by kind, with a manual cache clean. */
export function StoragePanel({ agentId }: { agentId: string }) {
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [error, setError] = useState("");
  const [cleaning, setCleaning] = useState(false);
  useEffect(() => {
    let alive = true;
    api.storage(agentId).then(value => { if (alive) setUsage(value); }).catch(err => { if (alive) setError(err instanceof Error ? err.message : String(err)); });
    return () => { alive = false; };
  }, [agentId]);
  const clean = async () => {
    setCleaning(true); setError("");
    try {
      const result = await api.cleanStorage(agentId);
      setUsage(result.usage);
      toast(result.freed ? `已清出 ${bytes(result.freed)}` : "目前沒有可以清理的快取", "ok");
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setCleaning(false); }
  };
  const total = usage?.total || 0;
  return (
    <section className="storage-panel" aria-label="用量與儲存空間">
      <UsageLine agentId={agentId} />
      <div className="storage-head">
        <span className="avatar-section-label">儲存空間</span>
        <strong>{usage ? bytes(total) : "計算中…"}</strong>
      </div>
      {usage ? (
        <>
          <div className="storage-bar" aria-hidden="true">
            {ROWS.map(row => usage[row.key] > 0 ? <i key={row.key} data-kind={row.key} style={{ flexGrow: usage[row.key] }} /> : null)}
          </div>
          <ul className="storage-rows">
            {ROWS.map(row => (
              <li key={row.key} data-kind={row.key}>
                <i aria-hidden="true" /><span>{row.label}{row.note ? <small>{row.note}</small> : null}</span><b>{bytes(usage[row.key])}</b>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      <div className="storage-actions">
        <button type="button" className="outline" disabled={cleaning || !usage} onClick={() => void clean()}>{cleaning ? "清理中…" : "立即清理快取"}</button>
        <small>工作成果與登入資料不會被清掉。</small>
      </div>
      {error ? <div className="pane-error" role="alert">{error}</div> : null}
    </section>
  );
}
