import { useEffect, useState } from "react";
import { api, type AppSettings, type RoutingSettings, type RoutingTarget } from "./api";
import { useI18n } from "./i18n";

export const DEFAULT_ROUTING: RoutingSettings = {
  enabled: true, tool_groups: "auto", chat: null, work: null, fallbacks: [], chat_timeout_ms: 15000, work_timeout_ms: 180000,
  semantic: false, laya: false, classifier_shadow: true,
  classifier_url: "http://127.0.0.1:8778", classifier_timeout_ms: 250,
  min_confidence: 0.9, gateway_provider: null, gateway_chat_pool: [], gateway_work_pool: [],
};
const copy = {
  title: "對話與工作分流", enabled: "自動選擇模型", chat: "聊天模型", work: "工作模型",
  hint: "聊天先直接回答，開始工具工作後使用工作模型。未選擇時沿用目前主模型。",
  protocolHint: "同一組聊天、工作與備援模型使用相同 API 介面。Bot 自己指定的模型優先於分流設定。",
  backendUnavailable: "服務尚未提供分流，請重新啟動產品服務以載入新版。",
  inherit: "沿用目前主模型", effort: "推理強度", modelDefault: "沿用模型設定", providerDefault: "供應商預設",
  fallback: "備援模型", none: "不使用", fallbackHint: "限流或連線失敗時依序切換；已開始送出內容的回應不會重播。",
  tools: "工具清單", auto: "自動預載", full: "全部提供", grouped: "依需求載入",
  local: "本機意圖判斷", semantic: "Semantic Router", laya: "Laya 多語模型", shadow: "僅記錄分類結果",
  shadowHint: "開啟時先觀察分類，不影響模型與工具選擇。分類不會禁止工具或批准操作。",
  deadline: "最長等待（毫秒）", confidence: "接受門檻", gateway: "模型閘道", advanced: "進階設定", save: "儲存分流設定",
};

export function RoutingPanel({ settings, onSaved, disabled: locked = false, onWriting }: { settings: AppSettings; onSaved: (s: AppSettings) => void; disabled?: boolean; onWriting?: (writing: boolean) => void }) {
  const { t } = useI18n();
  const hermes = settings.engine === "hermes";
  const [value, setValue] = useState<RoutingSettings>({ ...DEFAULT_ROUTING, ...settings.routing });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [health, setHealth] = useState<string>("");
  const [supported, setSupported] = useState<boolean | null>(null);
  const disabled = locked || (hermes && supported !== true);
  useEffect(() => { let alive = true; void api.routingHealth().then(h => {
    if (alive) {setSupported(h.supported === true);setHealth(`Semantic Router: ${h.engines?.semantic || "unavailable"} · Laya: ${h.engines?.laya || "unavailable"}`);}
  }).catch(() => {if (alive) setSupported(false);}); return () => { alive = false; }; }, [settings.routing]);
  useEffect(() => { setValue({ ...DEFAULT_ROUTING, ...settings.routing }); }, [settings.routing]);
  const edit = (patch: Partial<RoutingSettings>) => { setValue(v => ({ ...v, ...patch })); setSaved(false); };
  const inventory = settings.providers.flatMap(p => p.models.map(m => ({ p, m })));
  const targetKey = (target: RoutingTarget | null) => target ? JSON.stringify([target.provider, target.model]) : "";
  const choose = (key: string): RoutingTarget | null => {
    if (!key) return null;
    const [provider, model] = JSON.parse(key) as [string, string];
    return { provider, model, reasoning_effort: null };
  };
  const targetRow = (label: string, target: RoutingTarget | null, update: (t: RoutingTarget | null) => void, fallback = false) => {
    const selected = inventory.find(({ p, m }) => p.id === target?.provider && m.id === target.model);
    const key = targetKey(target);
    return <div key={label}>
      <label className="lb-settings-row">
        <span className="lb-settings-copy">{label}</span>
        <select className="lb-settings-select" aria-label={label} disabled={busy || disabled} value={key} onChange={e => update(choose(e.target.value))}>
          <option value="">{fallback ? copy.none : copy.inherit}</option>
          {inventory.map(({ p, m }) => <option key={JSON.stringify([p.id, m.id])} value={JSON.stringify([p.id, m.id])}>{p.label || p.id} · {m.label || m.id}</option>)}
          {target && !selected ? <option value={key}>{target.provider} · {target.model}</option> : null}
        </select>
      </label>
      {target ? <label className="lb-settings-row">
        <span className="lb-settings-copy">{label} · {copy.effort}</span>
        <select className="lb-settings-select" aria-label={`${label} · ${copy.effort}`} disabled={busy || disabled} value={target.reasoning_effort ?? "__inherit"}
          onChange={e => update({ ...target, reasoning_effort: e.target.value === "__inherit" ? null : e.target.value })}>
          <option value="__inherit">{copy.modelDefault}</option><option value="">{copy.providerDefault}</option>
          {(selected?.m.reasoning_levels || (hermes ? ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"] : [])).map(level => <option key={level} value={level}>{level}</option>)}
          {target.reasoning_effort && !(selected?.m.reasoning_levels || (hermes ? ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"] : [])).includes(target.reasoning_effort)
            ? <option value={target.reasoning_effort}>{target.reasoning_effort}</option> : null}
        </select>
      </label> : null}
    </div>;
  };
  const toggle = (label: string, field: "enabled" | "semantic" | "laya" | "classifier_shadow") => <label className="lb-settings-row">
    <span className="lb-settings-copy">{label}</span><input type="checkbox" disabled={busy || disabled} checked={value[field]}
      onChange={e => edit({ [field]: e.target.checked })} />
  </label>;
  const save = async () => {
    setBusy(true); onWriting?.(true); setError(""); setSaved(false);
    try { onSaved(await api.saveSettings({ routing: value })); setSaved(true); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); onWriting?.(false); }
  };
  return <section className="lb-settings-group" aria-label={copy.title}>
    <h3>{copy.title}</h3><p className="computer-settings-status">{copy.hint}</p>
    {hermes ? <p className="computer-settings-status">{copy.protocolHint}</p> : null}
    {hermes && supported === false ? <p className="computer-settings-status" role="status">{copy.backendUnavailable}</p> : null}
    {toggle(copy.enabled, "enabled")}
    {targetRow(copy.chat, value.chat, chat => edit({ chat }))}
    {targetRow(copy.work, value.work, work => edit({ work }))}
    {!hermes ? <label className="lb-settings-row"><span className="lb-settings-copy">{copy.tools}</span>
      <select className="lb-settings-select" disabled={busy || disabled} value={value.tool_groups}
        onChange={e => edit({ tool_groups: e.target.value as RoutingSettings["tool_groups"] })}>
        <option value="auto">{copy.auto}</option><option value="0">{copy.full}</option><option value="1">{copy.grouped}</option>
      </select>
    </label> : null}
    <details><summary>{copy.advanced}</summary>
      {(["chat", "work"] as const).map(role => {
        const field = role === "chat" ? "chat_timeout_ms" : "work_timeout_ms";
        return <label className="lb-settings-row" key={field}><span className="lb-settings-copy">{role === "chat" ? copy.chat : copy.work} · {copy.deadline}</span>
          <input className="lb-settings-select" type="number" min={role === "chat" ? 100 : 1000} max={role === "chat" ? 60000 : 300000}
            disabled={busy || disabled} value={value[field]} onChange={e => edit({ [field]: Number(e.target.value) })} />
        </label>;
      })}
      <p className="computer-settings-status">{copy.fallbackHint}</p>
      {[0, 1, 2].map(i => targetRow(`${copy.fallback} ${i + 1}`, value.fallbacks[i] || null, target => {
        const list = [...value.fallbacks]; if (target) list[i] = target; else list.splice(i, 1);
        edit({ fallbacks: list.filter(Boolean) });
      }, true))}
      <label className="lb-settings-row"><span className="lb-settings-copy">{copy.gateway}</span>
        <select className="lb-settings-select" disabled={busy || disabled} value={value.gateway_provider || ""}
          onChange={e => edit({ gateway_provider: e.target.value || null })}>
          <option value="">{copy.none}</option>
          {settings.providers.filter(p => ["lazyboy-chat", "lazyboy-work"].every(id => p.models.some(m => m.id === id)))
            .map(p => <option key={p.id} value={p.id}>{p.label || p.id}</option>)}
        </select>
      </label>
      {!hermes ? (["chat", "work"] as const).map(role => [0, 1, 2].map(i => {
        const field = role === "chat" ? "gateway_chat_pool" : "gateway_work_pool";
        return targetRow(`${copy.gateway} · ${role === "chat" ? copy.chat : copy.work} ${i + 2}`, value[field][i] || null, target => {
          const list = [...value[field]]; if (target) list[i] = target; else list.splice(i, 1);
          edit({ [field]: list.filter(Boolean) });
        }, true);
      })) : null}
      <h4>{copy.local}</h4><p className="computer-settings-status" role="status">{health}</p>{toggle(copy.semantic, "semantic")}{toggle(copy.laya, "laya")}
      <label className="lb-settings-row"><span className="lb-settings-copy">{copy.local} URL</span>
        <input className="lb-settings-select" type="url" aria-label={`${copy.local} URL`} disabled={busy || disabled} value={value.classifier_url}
          onChange={e => edit({ classifier_url:e.target.value })} />
      </label>
      {toggle(copy.shadow, "classifier_shadow")}<p className="computer-settings-status">{copy.shadowHint}</p>
      <label className="lb-settings-row"><span className="lb-settings-copy">{copy.deadline}</span>
        <input className="lb-settings-select" type="number" min={20} max={1000} disabled={busy || disabled} value={value.classifier_timeout_ms}
          onChange={e => edit({ classifier_timeout_ms: Number(e.target.value) })} />
      </label>
      <label className="lb-settings-row"><span className="lb-settings-copy">{copy.confidence}</span>
        <input className="lb-settings-select" type="number" min={0.5} max={1} step={0.01} disabled={busy || disabled} value={value.min_confidence}
          onChange={e => edit({ min_confidence: Number(e.target.value) })} />
      </label>
    </details>
    {error ? <p role="alert" className="computer-settings-status">{error}</p> : null}
    {saved ? <p role="status" className="computer-settings-status">{t.settingsSaved}</p> : null}
    <div className="lb-settings-row"><span /><button className="outline" disabled={busy || disabled} onClick={() => void save()}>{busy ? t.saving : copy.save}</button></div>
  </section>;
}
