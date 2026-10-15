import { useEffect, useState } from "react";
import { api, type ProviderUsage, type ProviderUsageSummary } from "./api";
import { tokens } from "./format-tokens";

const PERIODS = [7, 30, 90] as const;
const percent = (part: number, whole: number) => whole ? Math.round(part / whole * 100) : 0;

/** Each day's tokens in the period as thin bars; days without use stay empty. */
function DayBars({ days, since, period }: { days: ProviderUsage["days"]; since: string; period: number }) {
  const byDay = new Map(days.map(d => [d.day, d.tokens]));
  const start = new Date(`${since}T00:00:00`);
  const all = Array.from({ length: period }, (_, i) => {
    const date = new Date(start); date.setDate(start.getDate() + i);
    const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
    return { key, tokens: byDay.get(key) || 0 };
  });
  const max = Math.max(1, ...all.map(d => d.tokens));
  return (
    <div className="usage-bars" aria-hidden="true">
      {all.map(d => <i key={d.key} title={`${d.key}：${tokens(d.tokens)} tokens`} style={{ height: d.tokens ? `${Math.max(6, d.tokens / max * 100)}%` : 0 }} />)}
    </div>
  );
}

function ProviderCard({ item, since, period }: { item: ProviderUsage; since: string; period: number }) {
  const total = item.input + item.output;
  return (
    <details className="usage-card">
      <summary>
        <span className="usage-card-head">
          <strong>{item.label}</strong>
          {!item.configured ? <small>已移除的供應商</small> : null}
          <b>{tokens(total)} tokens</b>
        </span>
        <span className="usage-card-sub">
          {item.requests.toLocaleString()} 次請求
          {item.errors ? ` · ${item.errors} 次失敗` : ""}
          {` · 送出 ${tokens(item.input)}（快取命中 ${percent(item.cached, item.input)}%）· 回覆 ${tokens(item.output)}`}
          {item.reasoning ? `（含思考 ${tokens(item.reasoning)}）` : ""}
          {item.cost ? ` · 供應商回報費用 $${item.cost.toFixed(2)}` : ""}
        </span>
        <DayBars days={item.days} since={since} period={period} />
      </summary>
      <table className="usage-models">
        <thead><tr><th>模型</th><th>請求</th><th>送出</th><th>回覆</th></tr></thead>
        <tbody>
          {item.models.map(m => (
            <tr key={m.model}><td>{m.model}</td><td>{m.requests.toLocaleString()}</td><td>{tokens(m.input)}</td><td>{tokens(m.output)}</td></tr>
          ))}
        </tbody>
      </table>
    </details>
  );
}

/** Tokens each model provider served, for every bot and every kind of request. */
export function UsagePanel() {
  const [period, setPeriod] = useState<number>(30);
  const [data, setData] = useState<ProviderUsageSummary | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let alive = true;
    setError("");
    api.providerUsage(period).then(value => { if (alive) setData(value); }).catch(err => { if (alive) setError(err instanceof Error ? err.message : String(err)); });
    return () => { alive = false; };
  }, [period]);
  const providers = Array.isArray(data?.providers) ? data.providers : [];
  const sum = providers.reduce((acc, p) => ({ requests: acc.requests + p.requests, tokens: acc.tokens + p.input + p.output }), { requests: 0, tokens: 0 });
  return (
    <div className="lb-settings-group usage-panel">
      <p>所有 Bot 的模型請求都經過 LazyBoy 轉送，這裡依供應商與模型統計，包含背景小分隊、對話壓縮與自動命名。數字是供應商回報的 token 數；費用以供應商帳單為準。</p>
      <div className="usage-period" role="radiogroup" aria-label="統計期間">
        {PERIODS.map(days => (
          <button key={days} type="button" role="radio" aria-checked={period === days} className={period === days ? "picked" : ""} onClick={() => setPeriod(days)}>{days} 天</button>
        ))}
        {data ? <span>共 {sum.requests.toLocaleString()} 次請求 · {tokens(sum.tokens)} tokens</span> : null}
      </div>
      {error ? <p className="computer-settings-status" role="alert">{error}</p> : null}
      {data && !providers.length ? <p className="usage-empty">這段期間還沒有用量。從這個版本開始記錄，之前的用量不會出現。</p> : null}
      {data ? providers.map(item => <ProviderCard key={item.provider} item={item} since={data.since} period={data.days} />) : null}
    </div>
  );
}
