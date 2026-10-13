import { useEffect, useState } from "react";
import { api, type ContextUsage } from "../api";
import { LoaderCircle } from "../ui/icons";
import { Modal, ModalHeader, toast } from "../ui/overlay";

const tokens = (n: number) => n >= 10000 ? `${(n / 10000).toFixed(1)} 萬` : n.toLocaleString();

/**
 * What the model has to read on every request in this conversation, by part, and a way to shrink it: the older
 * turns can be summarized (Hermes also does this by itself when the window is about half full).
 */
export function ContextDialog({ agentId, agentName, threadId, busy, onClose, onNewThread }: {
  agentId: string; agentName: string; threadId: string; busy: boolean; onClose: () => void; onNewThread: () => void;
}) {
  const [data, setData] = useState<ContextUsage | null>(null);
  const [error, setError] = useState("");
  const [compressing, setCompressing] = useState(false);
  const load = () => api.context(agentId, threadId).then(setData).catch(err => setError(err instanceof Error ? err.message : String(err)));
  useEffect(() => { void load(); }, [agentId, threadId]); // eslint-disable-line react-hooks/exhaustive-deps

  async function compress() {
    setCompressing(true); setError("");
    try {
      const result = await api.compress(agentId, threadId);
      toast(result.status === "compressed" && result.before_tokens && result.after_tokens
        ? `已壓縮：約 ${tokens(result.before_tokens)} → ${tokens(result.after_tokens)} tokens` : "目前沒有需要壓縮的內容", "ok");
      await load();
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); }
    finally { setCompressing(false); }
  }

  const parts = data?.parts || [];
  const largest = Math.max(1, ...parts.map(p => p.tokens));
  return (
    <Modal label="Context 用量" onClose={onClose} className="context-dialog">
      <ModalHeader title={`${agentName} 的 Context 用量`} onClose={onClose} />
      {!data && !error ? <p className="muted"><LoaderCircle className="spin" /> 讀取中…</p> : null}
      {data && !data.available ? <p className="muted">{data.reason}</p> : null}
      {data?.available ? (
        <>
          <div className="context-meter" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={data.percent} aria-label="Context 已使用">
            <i style={{ width: `${Math.min(100, data.percent || 0)}%` }} data-level={(data.percent || 0) >= 50 ? "high" : (data.percent || 0) >= 30 ? "mid" : "low"} />
          </div>
          <p className="context-total"><strong>{data.percent}%</strong> · {tokens(data.used || 0)} / {tokens(data.max || 0)} tokens{data.estimated ? "（估計）" : ""}{data.model ? ` · ${data.model}` : ""}</p>
          <ul className="context-parts">
            {parts.map(part => (
              <li key={part.id}>
                <span>{part.label}</span>
                <i style={{ width: `${Math.max(2, part.tokens / largest * 100)}%` }} />
                <small>{tokens(part.tokens)}</small>
              </li>
            ))}
          </ul>
          <p className="muted context-hint">每次請求都會送出這些內容。人設、記憶與技能清單是固定的一小塊；對話紀錄會越來越長。用到一半左右時 Hermes 會自動壓縮舊對話；也可以現在壓縮，或開新對話從頭開始（記憶、技能與檔案都會保留）。</p>
        </>
      ) : null}
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" onClick={() => { onClose(); onNewThread(); }}>開新對話</button>
        <button type="button" className="primary" disabled={!data?.available || compressing || busy} title={busy ? "Bot 工作中，完成後再壓縮" : undefined}
          onClick={() => void compress()}>{compressing ? "壓縮中…" : "壓縮舊對話"}</button>
      </div>
    </Modal>
  );
}
