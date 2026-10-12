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
  "zh-Hant": {
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
  },
  "zh-Hans": {
    title: "对话与工作分流", enabled: "自动选择模型", chat: "聊天模型", work: "工作模型",
    hint: "聊天先直接回答，开始工具工作后使用工作模型。未选择时沿用当前主模型。",
    protocolHint: "同一组聊天、工作与备用模型使用相同 API 接口。Bot 自己指定的模型优先于分流设置。",
    backendUnavailable: "服务尚未提供分流，请重新启动产品服务以载入新版。",
    inherit: "沿用当前主模型", effort: "推理强度", modelDefault: "沿用模型设置", providerDefault: "供应商默认",
    fallback: "备用模型", none: "不使用", fallbackHint: "限流或连接失败时依次切换；已开始输出的响应不会重播。",
    tools: "工具列表", auto: "自动预载", full: "全部提供", grouped: "按需加载",
    local: "本机意图判断", semantic: "Semantic Router", laya: "Laya 多语言模型", shadow: "仅记录分类结果",
    shadowHint: "开启时先观察分类，不影响模型与工具选择。分类不会禁止工具或批准操作。",
    deadline: "最长等待（毫秒）", confidence: "接受阈值", gateway: "模型网关", advanced: "高级设置", save: "保存分流设置",
  },
  en: {
    title: "Chat and work routing", enabled: "Choose models automatically", chat: "Chat model", work: "Work model",
    hint: "Answer chats directly, then use the work model when tool work starts. Unselected roles use the active model.",
    protocolHint: "Use the same API format for chat, work and fallback models. A Bot's own model takes priority.",
    backendUnavailable: "Routing is unavailable. Restart the product service to load the new version.",
    inherit: "Use active model", effort: "Reasoning effort", modelDefault: "Use model setting", providerDefault: "Provider default",
    fallback: "Fallback model", none: "None", fallbackHint: "Try in order on rate limits or connection failures. Responses already delivering output are never replayed.",
    tools: "Tool selection", auto: "Preload automatically", full: "Offer all tools", grouped: "Load on demand",
    local: "Local intent detection", semantic: "Semantic Router", laya: "Laya multilingual", shadow: "Record classifications only",
    shadowHint: "Observe classifications without changing models or tools. Classification never disables tools or authorizes actions.",
    deadline: "Maximum wait (ms)", confidence: "Acceptance threshold", gateway: "Model gateway", advanced: "Advanced settings", save: "Save routing settings",
  },
  ja: {
    title: "会話と作業の振り分け", enabled: "モデルを自動選択", chat: "会話モデル", work: "作業モデル",
    hint: "会話には直接回答し、ツールを使う作業では作業モデルを使用します。未選択の場合は現在のモデルを使います。",
    protocolHint: "会話・作業・予備モデルは同じ API 形式を使います。Bot に指定したモデルが優先されます。",
    backendUnavailable: "振り分けはまだ利用できません。製品サービスを再起動して最新版を読み込んでください。",
    inherit: "現在のモデルを使う", effort: "推論の強さ", modelDefault: "モデル設定を使う", providerDefault: "プロバイダーの既定値",
    fallback: "予備モデル", none: "使用しない", fallbackHint: "制限や接続失敗時に順に切り替えます。出力開始後の応答は再送しません。",
    tools: "ツール一覧", auto: "自動で準備", full: "全ツールを提供", grouped: "必要時に読み込む",
    local: "ローカル意図分類", semantic: "Semantic Router", laya: "Laya 多言語モデル", shadow: "分類結果のみ記録",
    shadowHint: "モデルとツールを変えずに分類を観察します。分類は操作を許可したりツールを禁止したりしません。",
    deadline: "最大待機時間（ms）", confidence: "採用しきい値", gateway: "モデルゲートウェイ", advanced: "詳細設定", save: "振り分け設定を保存",
  },
};

export function RoutingPanel({ settings, onSaved, disabled: locked = false, onWriting }: { settings: AppSettings; onSaved: (s: AppSettings) => void; disabled?: boolean; onWriting?: (writing: boolean) => void }) {
  const { locale, t } = useI18n();
  const c = copy[locale];
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
          <option value="">{fallback ? c.none : c.inherit}</option>
          {inventory.map(({ p, m }) => <option key={JSON.stringify([p.id, m.id])} value={JSON.stringify([p.id, m.id])}>{p.label || p.id} · {m.label || m.id}</option>)}
          {target && !selected ? <option value={key}>{target.provider} · {target.model}</option> : null}
        </select>
      </label>
      {target ? <label className="lb-settings-row">
        <span className="lb-settings-copy">{label} · {c.effort}</span>
        <select className="lb-settings-select" aria-label={`${label} · ${c.effort}`} disabled={busy || disabled} value={target.reasoning_effort ?? "__inherit"}
          onChange={e => update({ ...target, reasoning_effort: e.target.value === "__inherit" ? null : e.target.value })}>
          <option value="__inherit">{c.modelDefault}</option><option value="">{c.providerDefault}</option>
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
  return <section className="lb-settings-group" aria-label={c.title}>
    <h3>{c.title}</h3><p className="computer-settings-status">{c.hint}</p>
    {hermes ? <p className="computer-settings-status">{c.protocolHint}</p> : null}
    {hermes && supported === false ? <p className="computer-settings-status" role="status">{c.backendUnavailable}</p> : null}
    {toggle(c.enabled, "enabled")}
    {targetRow(c.chat, value.chat, chat => edit({ chat }))}
    {targetRow(c.work, value.work, work => edit({ work }))}
    {!hermes ? <label className="lb-settings-row"><span className="lb-settings-copy">{c.tools}</span>
      <select className="lb-settings-select" disabled={busy || disabled} value={value.tool_groups}
        onChange={e => edit({ tool_groups: e.target.value as RoutingSettings["tool_groups"] })}>
        <option value="auto">{c.auto}</option><option value="0">{c.full}</option><option value="1">{c.grouped}</option>
      </select>
    </label> : null}
    <details><summary>{c.advanced}</summary>
      {(["chat", "work"] as const).map(role => {
        const field = role === "chat" ? "chat_timeout_ms" : "work_timeout_ms";
        return <label className="lb-settings-row" key={field}><span className="lb-settings-copy">{role === "chat" ? c.chat : c.work} · {c.deadline}</span>
          <input className="lb-settings-select" type="number" min={role === "chat" ? 100 : 1000} max={role === "chat" ? 60000 : 300000}
            disabled={busy || disabled} value={value[field]} onChange={e => edit({ [field]: Number(e.target.value) })} />
        </label>;
      })}
      <p className="computer-settings-status">{c.fallbackHint}</p>
      {[0, 1, 2].map(i => targetRow(`${c.fallback} ${i + 1}`, value.fallbacks[i] || null, target => {
        const list = [...value.fallbacks]; if (target) list[i] = target; else list.splice(i, 1);
        edit({ fallbacks: list.filter(Boolean) });
      }, true))}
      <label className="lb-settings-row"><span className="lb-settings-copy">{c.gateway}</span>
        <select className="lb-settings-select" disabled={busy || disabled} value={value.gateway_provider || ""}
          onChange={e => edit({ gateway_provider: e.target.value || null })}>
          <option value="">{c.none}</option>
          {settings.providers.filter(p => ["lazyboy-chat", "lazyboy-work"].every(id => p.models.some(m => m.id === id)))
            .map(p => <option key={p.id} value={p.id}>{p.label || p.id}</option>)}
        </select>
      </label>
      {!hermes ? (["chat", "work"] as const).map(role => [0, 1, 2].map(i => {
        const field = role === "chat" ? "gateway_chat_pool" : "gateway_work_pool";
        return targetRow(`${c.gateway} · ${role === "chat" ? c.chat : c.work} ${i + 2}`, value[field][i] || null, target => {
          const list = [...value[field]]; if (target) list[i] = target; else list.splice(i, 1);
          edit({ [field]: list.filter(Boolean) });
        }, true);
      })) : null}
      <h4>{c.local}</h4><p className="computer-settings-status" role="status">{health}</p>{toggle(c.semantic, "semantic")}{toggle(c.laya, "laya")}
      <label className="lb-settings-row"><span className="lb-settings-copy">{c.local} URL</span>
        <input className="lb-settings-select" type="url" aria-label={`${c.local} URL`} disabled={busy || disabled} value={value.classifier_url}
          onChange={e => edit({ classifier_url:e.target.value })} />
      </label>
      {toggle(c.shadow, "classifier_shadow")}<p className="computer-settings-status">{c.shadowHint}</p>
      <label className="lb-settings-row"><span className="lb-settings-copy">{c.deadline}</span>
        <input className="lb-settings-select" type="number" min={20} max={1000} disabled={busy || disabled} value={value.classifier_timeout_ms}
          onChange={e => edit({ classifier_timeout_ms: Number(e.target.value) })} />
      </label>
      <label className="lb-settings-row"><span className="lb-settings-copy">{c.confidence}</span>
        <input className="lb-settings-select" type="number" min={0.5} max={1} step={0.01} disabled={busy || disabled} value={value.min_confidence}
          onChange={e => edit({ min_confidence: Number(e.target.value) })} />
      </label>
    </details>
    {error ? <p role="alert" className="computer-settings-status">{error}</p> : null}
    {saved ? <p role="status" className="computer-settings-status">{t.settingsSaved}</p> : null}
    <div className="lb-settings-row"><span /><button className="outline" disabled={busy || disabled} onClick={() => void save()}>{busy ? t.saving : c.save}</button></div>
  </section>;
}
