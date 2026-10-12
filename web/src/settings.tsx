import { HermesSettings } from "./hermes-settings";
import { RoutingPanel } from "./routing-settings";
import { useDialogFocus } from "./dialog-focus";
import { McpSettings } from "./mcp-settings";
import { useEffect, useRef, useState } from "react";
import { api, type AppSettings, type ModelProtocol, type ProviderPreset, type SettingsModel } from "./api";
import { mergeProviderModels, providerModelProtocol } from "./model-settings";
import { useI18n } from "./i18n";
import { persistTheme, readTheme, THEME_PREFS, type ThemePref } from "./theme";

export const APP_NAME = "LazyBoy";
export const APP_VERSION = (import.meta.env.LAZYBOY_VERSION || "0.1.20").trim() || "0.1.20";
export const APP_BUILD = (import.meta.env.LAZYBOY_BUILD || "20").trim() || "20";

export type SettingsSectionId = "general" | "model" | "mcp" | "updates";

const SECTIONS: readonly { id: SettingsSectionId; icon: () => React.ReactElement }[] = [
  { id: "general", icon: GearIcon },
  { id: "model", icon: ModelIcon },
  { id: "mcp", icon: PlugIcon },
  { id: "updates", icon: DownloadIcon },
];
const CUSTOM_PROVIDER = "__custom__";

export type SettingsComputer = {
  pending: "start" | "restart" | "update" | null;
  working: boolean;
  upToDate: boolean;
  status?: string;
  onUpdate: () => void;
  onRestart: () => void;
};

function detectOs(): string {
  const ua = navigator.userAgent;
  if (/Mac/i.test(ua)) return "darwin";
  if (/Win/i.test(ua)) return "win32";
  if (/Linux/i.test(ua)) return "linux";
  return navigator.platform || "unknown";
}

function GearIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 17h8" />
      <path d="M12 7v9" />
      <path d="M16 12l-4 4-4-4" />
      <path d="M20 16.58A5 5 0 0 0 18 7h-1.26A8 8 0 1 0 4 15.25" />
    </svg>
  );
}

function ModelIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M10 10h4v4h-4zM9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" />
    </svg>
  );
}

function PlugIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 2v6M15 2v6M6 8h12v3a6 6 0 0 1-12 0zM12 17v5" />
    </svg>
  );
}

function InfoIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5M12 8h.01" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M18 6L6 18M6 6l12 12" />
    </svg>
  );
}

function SettingsGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="lb-settings-group">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function OverlayShell({
  label,
  className,
  onClose,
  children,
}: {
  label: string;
  className: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null);
  useDialogFocus(root, onClose);
  return (
    <div className="modal-backdrop lb-overlay" onClick={onClose}>
      <div
        ref={root}
        className={className}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}

function GeneralPanel() {
  const { t } = useI18n();
  const [theme, setTheme] = useState<ThemePref>(readTheme);

  return (
    <div className="lb-settings-stack">
      <SettingsGroup title={t.localWorkspace}>
        <div className="lb-settings-row">
          <span className="lb-settings-copy">
            <strong>{APP_NAME}</strong>
            <small>{t.workspaceDetail}</small>
          </span>
        </div>
      </SettingsGroup>
      <SettingsGroup title={t.appearance}>
        <label className="lb-settings-row">
          <span className="lb-settings-copy">{t.theme}</span>
          <select
            className="lb-settings-select"
            aria-label={t.theme}
            value={theme}
            onChange={(event) => {
              const next = event.target.value as ThemePref;
              if (!THEME_PREFS.includes(next)) return;
              persistTheme(next);
              setTheme(next);
            }}
          >
            <option value="system">{t.themeSystem}</option>
            <option value="light">{t.themeLight}</option>
            <option value="dark">{t.themeDark}</option>
          </select>
        </label>
      </SettingsGroup>
    </div>
  );
}

function modelChoices(models: SettingsModel[], current: string): string[] {
  const ids = models.map((item) => item.id).filter(Boolean);
  if (current && !ids.includes(current)) ids.unshift(current);
  return ids;
}

function ModelPanel() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [providerId, setProviderId] = useState("");
  const [providerLabel, setProviderLabel] = useState("");
  const [newProvider, setNewProvider] = useState(false);
  const [model, setModel] = useState("");
  const [manualModel, setManualModel] = useState<string | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [models, setModels] = useState<SettingsModel[]>([]);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [writing, setWriting] = useState(false);
  const [probing, setProbing] = useState(false);
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [editUrl, setEditUrl] = useState(false);
  const draftRef = useRef({ providerId: "", baseUrl: "", apiKey: "", models: [] as SettingsModel[] });
  draftRef.current = { providerId, baseUrl, apiKey, models };
  const loadedProvider = useRef("");
  const modelsRequest = useRef(0);

  const apply = (data: AppSettings) => {
    const providers = data?.providers || [];
    setSettings({ ...data, providers });
    setProviderId(data?.active || "");
    setModel(data?.model || "");
    const provider = providers.find((p) => p.id === data?.active);
    setProviderLabel(provider?.label || data?.active || "");
    setNewProvider(false);
    setManualModel(null);
    setBaseUrl(provider?.base_url || data?.base_url || "");
    setModels(provider?.models || []);
    setApiKey("");
    setEditUrl(false);
  };

  useEffect(() => {
    let alive = true;
    api
      .settings()
      .then((data) => { if (alive) apply(data); })
      .catch((e) => { if (alive) setError(e instanceof Error ? e.message : String(e)); });
    api.providerPresets()
      .then((data) => { if (alive) setPresets(Array.isArray(data?.presets) ? data.presets : []); })
      .catch(() => {});
    return () => { alive = false; };
  }, []);

  const selected = settings?.providers?.find((p) => p.id === providerId);
  const preset = presets.find((p) => p.id === providerId);
  const configuredIds = new Set((settings?.providers || []).map((p) => p.id));
  const availablePresets = presets.filter((p) => !configuredIds.has(p.id));

  const onProviderChange = (id: string) => {
    if (id === CUSTOM_PROVIDER) return addProvider();
    modelsRequest.current++;
    setBusy(false);
    setError("");
    setStatus("");
    setProviderId(id);
    setNewProvider(false);
    setManualModel(null);
    setApiKey("");
    setEditUrl(false);
    const provider = settings?.providers?.find((p) => p.id === id);
    const template = presets.find((p) => p.id === id);
    if (!provider && template) {
      const listed = template.models.map((model) => ({ id: model, protocol: providerModelProtocol(template.id, model, template.protocol) }));
      setProviderLabel(template.label);
      setBaseUrl(template.base_url);
      setModels(listed);
      setModel(listed[0]?.id || "");
      // Listing models needs the API key the user is about to enter.
      loadedProvider.current = id;
      return;
    }
    setProviderLabel(provider?.label || id);
    setBaseUrl(provider?.base_url || "");
    setModels(provider?.models || []);
    setModel(provider?.models[0]?.id || "");
    loadedProvider.current = "";
  };

  const addProvider = () => {
    modelsRequest.current++;
    const id = "custom-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 8);
    loadedProvider.current = id;
    setProviderId(id);
    setProviderLabel("");
    setNewProvider(true);
    setBaseUrl(""); setApiKey(""); setModels([]); setModel("");
    setManualModel(""); setBusy(false); setError(""); setStatus(""); setEditUrl(false);
  };

  const addModel = () => {
    const id = manualModel?.trim();
    if (!id) return;
    setModels(list => list.some(item => item.id === id) ? list : [...list, {id, protocol: preset?.protocol || "chat_completions"}]);
    setModel(id); setManualModel(null);
  };

  const loadModels = async (announce: boolean, isCurrent: () => boolean = () => true) => {
    const id = providerId.trim();
    if (!id) return;
    const draft = draftRef.current;
    const version = ++modelsRequest.current;
    const stillCurrent = () => isCurrent() && modelsRequest.current === version
      && draftRef.current.providerId === id
      && draftRef.current.baseUrl === draft.baseUrl && draftRef.current.apiKey === draft.apiKey;
    if (announce) {
      setBusy(true);
      setError("");
      setStatus("");
    }
    try {
      const data = await api.providerModels({ provider: id, base_url: draft.baseUrl.trim(), api_key: draft.apiKey });
      if (!stillCurrent()) return;
      const found = data.models || [];
      if (!found.length) {
        if (announce || draft.models.length === 0) setError(t.noModelsFound);
        return;
      }
      const next = mergeProviderModels(found, draft.models, id, presets.find((p) => p.id === id)?.protocol);
      setModels(next);
      setModel((current) => next.some((item) => item.id === current) ? current : next[0].id);
    } catch (e) {
      if (stillCurrent() && (announce || draft.models.length === 0)) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (announce && modelsRequest.current === version) setBusy(false);
    }
  };

  useEffect(() => {
    if (!settings || !providerId.trim() || loadedProvider.current === providerId) return;
    loadedProvider.current = providerId;
    let alive = true;
    void loadModels(false, () => alive);
    return () => { alive = false; };
  }, [settings, providerId]);

  const payload = () => ({
    active: providerId.trim(),
    model,
    provider: {
      id: providerId.trim(),
      label: providerLabel.trim() || providerId.trim(),
      base_url: baseUrl.trim(),
      api_key: apiKey,
      models: model && !models.some((item) => item.id === model)
        ? [{ id: model, protocol: preset?.protocol || "chat_completions" }, ...models]
        : models,
    },
  });

  const save = async () => {
    if (!providerId.trim()) return;
    modelsRequest.current++;
    setBusy(true);
    setWriting(true);
    setError("");
    setStatus("");
    try {
      apply(await api.saveSettings(payload()));
      setStatus(t.settingsSaved);
      window.dispatchEvent(new Event("lazyboy:model-changed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setWriting(false);
    }
  };

  const detectReasoning = async () => {
    if (!providerId.trim() || !model) return;
    modelsRequest.current++;
    setBusy(true);
    setWriting(true);
    setProbing(true);
    setError("");
    setStatus("");
    try {
      await api.saveSettings(payload());
      apply(await api.providerReasoning({ provider: providerId.trim(), model }));
      window.dispatchEvent(new Event("lazyboy:model-changed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setProbing(false);
      setBusy(false);
      setWriting(false);
    }
  };

  const current = models.find((item) => item.id === model);
  const hermes = settings?.engine === "hermes";
  const levels = hermes ? ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"] : current?.reasoning_levels;
  const setEffort = (effort: string) =>
    setModels((list) => list.map((item) => item.id === model ? { ...item, reasoning_effort: effort || null } : item));

  if (!settings) {
    return (
      <div className="lb-settings-stack">
        <p className="computer-settings-status" role={error ? "alert" : "status"}>
          {error || t.settingsLoading}
        </p>
      </div>
    );
  }

  return (
    <div className="lb-settings-stack">
      <SettingsGroup title="供應商與模型">
        <label className="lb-settings-row">
          <span className="lb-settings-copy">
            {t.provider}
            <small>{preset?.description || (newProvider ? "任何 OpenAI 相容的服務" : `可直接選內建的 ${presets.length || ""} 個供應商`)}</small>
          </span>
          <select className="lb-settings-select" aria-label={t.provider} value={providerId} disabled={writing} onChange={(e) => onProviderChange(e.target.value)}>
            <optgroup label="已設定">
              {(settings.providers || []).map((p) => <option key={p.id} value={p.id}>
                {p.label && p.label !== p.id ? p.label : presets.find((item) => item.id === p.id)?.label || p.id}
              </option>)}
              {providerId && !configuredIds.has(providerId) && !preset
                ? <option value={providerId}>{providerLabel || "自訂供應商"}</option>
                : null}
            </optgroup>
            {availablePresets.length ? <optgroup label="內建供應商">
              {availablePresets.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
            </optgroup> : null}
            {settings.engine === "hermes" ? <optgroup label="其他">
              <option value={CUSTOM_PROVIDER}>自訂 OpenAI 相容服務…</option>
            </optgroup> : null}
          </select>
        </label>
        {newProvider ? <label className="lb-settings-row">
          <span className="lb-settings-copy">供應商名稱</span>
          <input className="lb-settings-select" value={providerLabel} disabled={writing} onChange={e => setProviderLabel(e.target.value)} placeholder="例如：我的本機模型" />
        </label> : null}
        {preset && preset.base_url && !preset.local && !editUrl && baseUrl.trim() === preset.base_url ? <div className="lb-settings-row">
          <span className="lb-settings-copy">{t.baseUrl}<small className="lb-settings-url">{baseUrl}</small></span>
          <button type="button" className="outline" disabled={writing} onClick={() => setEditUrl(true)}>修改</button>
        </div> : <label className="lb-settings-row">
          <span className="lb-settings-copy">{t.baseUrl}</span>
          <input className="lb-settings-select" aria-label={t.baseUrl} value={baseUrl} disabled={writing} onChange={(e) => setBaseUrl(e.target.value)} placeholder={preset?.base_url || "https://api.example.com/v1"} />
        </label>}
        <label className="lb-settings-row">
          <span className="lb-settings-copy">
            {t.apiKey}
            {selected?.has_key ? <small>{t.apiKeySet}</small> : preset?.signup_url
              ? <small><a href={preset.signup_url} target="_blank" rel="noreferrer">取得 API 金鑰 ↗</a></small>
              : null}
          </span>
          <input
            className="lb-settings-select"
            type="password"
            aria-label={t.apiKey}
            disabled={writing}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            onBlur={() => { if (apiKey && preset && !selected) void loadModels(false); }}
            placeholder={selected?.has_key ? t.apiKeyKeep : preset?.local ? "本機服務可留空" : "貼上 API 金鑰"}
            autoComplete="off"
          />
        </label>
        <label className="lb-settings-row">
          <span className="lb-settings-copy">{t.modelId}</span>
          <span className="lb-settings-control">
            <select className="lb-settings-select" aria-label={t.modelId} value={model} onChange={(e) => setModel(e.target.value)} disabled={writing || !modelChoices(models, model).length}>
              {modelChoices(models, model).map((id) => <option key={id} value={id}>{id}</option>)}
            </select>
            <button type="button" className="outline" disabled={busy || writing || !providerId.trim()} onClick={() => void loadModels(true)}>
              {t.loadModels}
            </button>
            {hermes ? <button type="button" className="outline" disabled={writing} onClick={() => setManualModel("")}>手動加入模型</button> : null}
          </span>
        </label>
        {hermes && manualModel !== null ? <label className="lb-settings-row">
          <span className="lb-settings-copy">新模型 ID<small>使用供應商提供的模型名稱。</small></span>
          <span className="lb-settings-control">
            <input className="lb-settings-select" value={manualModel} disabled={writing} onChange={e => setManualModel(e.target.value)} />
            <button type="button" className="outline" disabled={writing || !manualModel.trim()} onClick={addModel}>加入模型</button>
          </span>
        </label> : null}
        {hermes ? <label className="lb-settings-row">
          <span className="lb-settings-copy">API 介面<small>依模型服務的文件選擇。</small></span>
          <select className="lb-settings-select" value={current?.protocol || "chat_completions"} disabled={writing || !current}
            onChange={e => setModels(list => list.map(item => item.id === model ? {...item, protocol:e.target.value as ModelProtocol} : item))}>
            <option value="chat_completions">Chat Completions</option>
            <option value="responses">Responses</option>
            <option value="messages">Anthropic Messages</option>
          </select>
        </label> : null}
        <label className="lb-settings-row">
          <span className="lb-settings-copy">
            {t.reasoningEffort}
            {hermes ? <small>模型支援時套用；未指定時使用 medium。</small> : !levels ? <small>{t.reasoningUnprobed}</small> : levels.length === 0 ? <small>{t.reasoningUnsupported}</small> : null}
          </span>
          <span className="lb-settings-control">
            <select
              className="lb-settings-select"
              aria-label={t.reasoningEffort}
              value={current?.reasoning_effort || ""}
              onChange={(e) => setEffort(e.target.value)}
              disabled={writing || !current || !levels?.length}
            >
              <option value="">{hermes ? "預設（medium）" : t.reasoningDefault}</option>
              {(levels || []).map((level) => <option key={level} value={level}>{level}</option>)}
            </select>
            {!hermes ? <button type="button" className="outline" disabled={busy || !providerId.trim() || !model} onClick={() => void detectReasoning()}>
              {probing ? t.reasoningDetecting : t.detectReasoning}
            </button> : null}
          </span>
        </label>
        {hermes ? <label className="lb-settings-row">
          <span className="lb-settings-copy">讀取圖片<small>請依選定模型的能力開啟，圖片會隨訊息送給模型。</small></span>
          <input type="checkbox" aria-label="讀取圖片" checked={!!current?.vision} disabled={writing || !current}
            onChange={e => setModels(list => list.map(item => item.id === model ? {...item, vision:e.target.checked} : item))}/>
        </label> : null}
        {hermes ? <label className="lb-settings-row">
          <span className="lb-settings-copy">產生圖片<small>供應商提供生圖介面時開啟，可用來製作 Bot 頭像。</small></span>
          <input type="checkbox" aria-label="產生圖片" checked={!!current?.image_generation} disabled={writing || !current}
            onChange={e => setModels(list => list.map(item => item.id === model ? {...item, image_generation:e.target.checked} : item))}/>
        </label> : null}
      </SettingsGroup>
      <SettingsGroup title={t.currentModel}>
        <div className="lb-settings-row">
          <span className="lb-settings-copy">
            <strong>{settings.active} · {settings.model}</strong>
            <small>{settings.protocol} · {settings.config_path}</small>
          </span>
        </div>
      </SettingsGroup>
      {error ? <p className="computer-settings-status" role="alert">{error}</p> : null}
      {status ? <p className="computer-settings-status" role="status">{status}</p> : null}
      <div className="lb-settings-row">
        <span />
        <button type="button" className="outline" disabled={busy || writing || !providerId.trim()} onClick={() => void save()}>
          {busy ? t.saving : t.saveSettings}
        </button>
      </div>
      <HermesSettings settings={settings} />
      <RoutingPanel settings={settings} onSaved={setSettings} disabled={busy || writing} onWriting={setWriting} />
    </div>
  );
}

function ComputerRow({
  label,
  description,
  extra,
  control,
}: {
  label: string;
  description: string;
  extra: string | null;
  control: React.ReactNode;
}) {
  return (
    <div className="lb-settings-row">
      <span className="lb-settings-copy">
        <strong>{label}</strong>
        <small>{description}</small>
        {extra ? <small>{extra}</small> : null}
      </span>
      <span className="lb-settings-control">{control}</span>
    </div>
  );
}

function UpdatesPanel({ computer }: { computer?: SettingsComputer }) {
  const { t } = useI18n();
  const pending = computer?.pending ?? null;
  const disabled = Boolean(pending);
  const busyCopy = computer?.working ? t.computerBusyCopy : null;
  return (
    <div className="lb-settings-stack">
      <SettingsGroup title="版本">
        <div className="lb-settings-row">
          <span className="lb-settings-copy">
            <strong>{APP_NAME} {APP_VERSION}</strong>
            <small>{t.localBuild}</small>
          </span>
        </div>
      </SettingsGroup>
      {computer?.status ? <p className="computer-settings-status" role="status">{computer.status}</p> : null}
      {computer ? (
        <SettingsGroup title={t.appComputer}>
          {computer.upToDate ? (
            <div className="lb-settings-banner" role="status">
              <strong>{t.computerUpToDate}</strong>
              <span>{t.computerUpdateCopy}</span>
              {busyCopy ? <span>{busyCopy}</span> : null}
              <button type="button" className="outline" disabled={disabled} onClick={computer.onUpdate}>
                {pending === "update" ? t.computerUpdating : t.update}
              </button>
            </div>
          ) : (
            <ComputerRow
              label={t.updateComputer}
              description={t.computerUpdateCopy}
              extra={busyCopy}
              control={
                <button type="button" className="outline" disabled={disabled} onClick={computer.onUpdate}>
                  {pending === "update" ? t.computerUpdating : t.update}
                </button>
              }
            />
          )}
          <ComputerRow
            label={t.restartComputer}
            description={t.restartComputerCopy}
            extra={busyCopy}
            control={
              <button type="button" className="danger" disabled={disabled} onClick={computer.onRestart}>
                {pending === "restart" ? t.computerRestarting : t.restart}
              </button>
            }
          />
        </SettingsGroup>
      ) : null}
    </div>
  );
}

function MobilePageHeader({ title, backLabel, onBack, close = false }: { title: string; backLabel: string; onBack: () => void; close?: boolean }) {
  return <header className="mobile-settings-header">
    <button type="button" aria-label={backLabel} onClick={onBack}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={close ? "m6 6 12 12M18 6 6 18" : "m14 6-6 6 6 6"} /></svg></button>
    <h2>{title}</h2><span aria-hidden="true" />
  </header>;
}

function MobileSettings({ onClose, onAbout, computer, agentId, initialSection = "general" }: { agentId?: string | null; onClose: () => void; onAbout?: () => void; computer?: SettingsComputer; initialSection?: SettingsSectionId }) {
  const { t } = useI18n();
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  useEffect(() => {
    if (initialSection === "mcp") document.getElementById("settings-mcp")?.scrollIntoView({ block: "start" });
  }, [initialSection]);
  return <OverlayShell label={t.settingsLabel} className="mobile-settings-page" onClose={onClose}>
    <header className="mobile-settings-header">
      <button type="button" aria-label={t.close} onClick={onClose}><CloseIcon /></button>
      <h2>{t.settings}</h2>
      <span aria-hidden="true" />
    </header>
    <div className="mobile-settings-content">
      <GeneralPanel />
      <section aria-label={t.modelSection}>
        <ModelPanel />
      </section>
      <section id="settings-mcp" className="lb-settings-group" aria-label="工具串接">
        <h3>工具串接</h3>
        <McpSettings agentId={agentId} />
      </section>
      <UpdatesPanel computer={computer} />
      {onAbout ? <button type="button" className="shell-action settings-about" onClick={onAbout}>{t.about}</button> : null}
    </div>
  </OverlayShell>;
}

export function SettingsDialog({ onClose, computer, mobile = false, onAbout, agentId, initialSection = "general" }: { agentId?: string | null; onClose: () => void; computer?: SettingsComputer; mobile?: boolean; onAbout?: () => void; initialSection?: SettingsSectionId }) {
  const { t } = useI18n();
  const [section, setSection] = useState<SettingsSectionId>(initialSection);
  const headingId = `lb-settings-heading-${section}`;
  const panelId = `lb-settings-panel-${section}`;
  const sectionLabel = section === "general" ? t.general : section === "model" ? t.modelSection : section === "mcp" ? "工具串接" : t.updates;
  if (mobile) return <MobileSettings agentId={agentId} onClose={onClose} onAbout={onAbout} computer={computer} initialSection={initialSection} />;

  return (
    <OverlayShell label={t.settingsLabel} className="lb-settings-dialog" onClose={onClose}>
      <div className="lb-settings-layout">
        <nav aria-label={t.settings} className="lb-settings-nav">
          {SECTIONS.map((item) => {
            const selected = item.id === section;
            return (
              <button
                key={item.id}
                type="button"
                className="lb-settings-nav__item"
                data-active={selected || undefined}
                aria-current={selected ? "page" : undefined}
                aria-controls={selected ? panelId : undefined}
                onClick={() => setSection(item.id)}
              >
                <item.icon />
                <span>{item.id === "general" ? t.general : item.id === "model" ? t.modelSection : item.id === "mcp" ? "工具串接" : t.updates}</span>
              </button>
            );
          })}
          {onAbout ? (
            <button type="button" className="lb-settings-nav__item" onClick={onAbout}>
              <InfoIcon />
              <span>{t.about}</span>
            </button>
          ) : null}
        </nav>
        <section className="lb-settings-panel" id={panelId} aria-labelledby={headingId}>
          <button type="button" className="lb-settings-close" aria-label={t.close} onClick={onClose}>
            <CloseIcon />
          </button>
          <h2 id={headingId}>{sectionLabel}</h2>
          <div className="lb-settings-body">
            {section === "general" ? <GeneralPanel /> : section === "model" ? <ModelPanel /> : section === "mcp" ? <McpSettings agentId={agentId} /> : <UpdatesPanel computer={computer} />}
          </div>
        </section>
      </div>
    </OverlayShell>
  );
}

export function AboutDialog({ onClose, mobile = false }: { onClose: () => void; mobile?: boolean }) {
  const { t, format } = useI18n();
  const [copied, setCopied] = useState(false);
  const [copyGeneration, setCopyGeneration] = useState(0);

  useEffect(() => {
    if (!copied) return;
    const timeout = window.setTimeout(() => setCopied(false), 1200);
    return () => window.clearTimeout(timeout);
  }, [copied, copyGeneration]);

  const copyVersion = async () => {
    try {
      await navigator.clipboard.writeText([
        `Version: ${APP_VERSION}`,
        `Build: ${APP_BUILD}`,
        `OS: ${detectOs()}`,
      ].join("\n"));
      setCopied(true);
      setCopyGeneration((n) => n + 1);
    } catch {
      /* leave the action idle, matching grok-bot */
    }
  };

  return (
    <OverlayShell label={t.about} className={`lb-about-dialog ${mobile ? "mobile-about-page" : ""}`} onClose={onClose}>
      {mobile ? <MobilePageHeader title={t.about} backLabel={t.settings} onBack={onClose} /> : null}
      <button type="button" className="lb-about-close" aria-label={t.close} onClick={onClose}>
        <CloseIcon />
      </button>
      <div className="lb-about-brand">
        <span className="lb-about-icon">
          <img src="/lazyboy-round.svg" width={64} height={64} alt="" draggable={false} decoding="async" />
        </span>
        <div className="lb-about-heading">
          <h2>{APP_NAME}</h2>
          <p>{format("versionLine", { version: APP_VERSION, build: APP_BUILD })}</p>
        </div>
        <small>{t.copyright}</small>
      </div>
      <footer>
        <button type="button" onClick={() => void copyVersion()}>
          {copied ? t.copied : t.copyVersionInfo}
        </button>
      </footer>
    </OverlayShell>
  );
}
