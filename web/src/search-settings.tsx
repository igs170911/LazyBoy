import { useEffect, useState } from "react";
import { api, type AppSettings } from "./api";
import { useI18n } from "./i18n";

const LABELS: Record<string, string> = { exa: "Exa", tavily: "Tavily", "brave-free": "Brave Search", ddgs: "DuckDuckGo" };
const copy = {
  title: "網路搜尋", provider: "搜尋服務", key: "API key", keyHint: "Exa 與 Tavily 未填 key 時使用免費額度；Brave 需要 key。",
  keySaved: "已儲存，留空則保留", xai: "xAI API key（X 搜尋）", xaiHint: "填寫後 Bot 可以搜尋 X（Twitter）貼文。", save: "儲存搜尋設定",
  applies: "下一次工作生效。",
};

export function SearchPanel({ settings, onSaved, disabled = false }: { settings: AppSettings; onSaved: (s: AppSettings) => void; disabled?: boolean }) {
  const { t } = useI18n();
  const search = settings.search;
  const [provider, setProvider] = useState(search?.provider || "exa");
  const [key, setKey] = useState("");
  const [xai, setXai] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  useEffect(() => { setProvider(search?.provider || "exa"); }, [search?.provider]);
  if (!search) return null;
  const sameProvider = provider === search.provider;
  const save = async () => {
    setBusy(true); setError(""); setSaved(false);
    try {
      const body: Record<string, string> = { provider };
      if (key.trim()) body.api_key = key.trim();
      if (xai.trim()) body.xai_api_key = xai.trim();
      onSaved(await api.saveSettings({ search: body }));
      setKey(""); setXai(""); setSaved(true);
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  return <section className="lb-settings-group" aria-label={copy.title}>
    <h3>{copy.title}</h3>
    <label className="lb-settings-row">
      <span className="lb-settings-copy">{copy.provider}</span>
      <select className="lb-settings-select" aria-label={copy.provider} disabled={busy || disabled} value={provider}
        onChange={e => { setProvider(e.target.value); setSaved(false); }}>
        {search.providers.map(id => <option key={id} value={id}>{LABELS[id] || id}</option>)}
      </select>
    </label>
    {provider !== "ddgs" ? <label className="lb-settings-row">
      <span className="lb-settings-copy">{copy.key}</span>
      <input className="lb-settings-select" type="password" autoComplete="off" aria-label={copy.key} disabled={busy || disabled} value={key}
        placeholder={sameProvider && search.has_key ? copy.keySaved : ""} onChange={e => { setKey(e.target.value); setSaved(false); }} />
    </label> : null}
    <p className="computer-settings-status">{copy.keyHint}</p>
    <label className="lb-settings-row">
      <span className="lb-settings-copy">{copy.xai}</span>
      <input className="lb-settings-select" type="password" autoComplete="off" aria-label={copy.xai} disabled={busy || disabled} value={xai}
        placeholder={search.has_xai_key ? copy.keySaved : ""} onChange={e => { setXai(e.target.value); setSaved(false); }} />
    </label>
    <p className="computer-settings-status">{copy.xaiHint} {copy.applies}</p>
    {error ? <p role="alert" className="computer-settings-status">{error}</p> : null}
    {saved ? <p role="status" className="computer-settings-status">{t.settingsSaved}</p> : null}
    <div className="lb-settings-row"><span /><button className="outline" disabled={busy || disabled} onClick={() => void save()}>{busy ? t.saving : copy.save}</button></div>
  </section>;
}
