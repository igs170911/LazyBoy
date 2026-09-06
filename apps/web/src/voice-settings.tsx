import { useEffect, useState } from "react";
import { api } from "./api";
import { t } from "./i18n";
import { X } from "./animated-icons";
import type { VoiceProviderId, VoiceSettings } from "./types";

export type { VoiceProviderId, VoiceSettings };

export function VoiceSettingsDialog({ close }: { close: () => void }) {
  const [settings, setSettings] = useState<VoiceSettings | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [provider, setProvider] = useState<VoiceProviderId>("xai");
  const [modelId, setModelId] = useState("");
  const [voiceId, setVoiceId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api<VoiceSettings>("/api/voice/settings")
      .then((value) => {
        setSettings(value);
        setEnabled(value.enabled);
        setProvider(value.provider);
        setModelId(value.modelId);
        setVoiceId(value.voiceId);
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("loadFailed")));
  }, []);

  const current = settings?.providers.find((item) => item.id === provider);
  const models = (settings?.provider === provider ? settings.models : null)
    ?? (provider === "openai"
      ? [{ id: "gpt-realtime", name: "GPT Realtime" }]
      : [{ id: "grok-voice-latest", name: "Grok Voice Latest" }, { id: "grok-voice-think-fast-2.0", name: "Grok Voice Think Fast 2.0" }]);
  const voices = (settings?.provider === provider ? settings.voices : null)
    ?? (provider === "openai"
      ? [{ id: "marin", name: "Marin" }, { id: "alloy", name: "Alloy" }, { id: "verse", name: "Verse" }]
      : [{ id: "eve", name: "Eve" }, { id: "ara", name: "Ara" }, { id: "leo", name: "Leo" }, { id: "rex", name: "Rex" }, { id: "sal", name: "Sal" }]);

  function pickProvider(id: VoiceProviderId) {
    setProvider(id);
    setError("");
    if (id === "openai") {
      setModelId("gpt-realtime");
      setVoiceId("marin");
      return;
    }
    setModelId("grok-voice-latest");
    setVoiceId("eve");
  }

  return (
    <div className="modal-backdrop">
      <form
        className="dialog settings-dialog"
        onSubmit={async (event) => {
          event.preventDefault();
          setBusy(true);
          setError("");
          try {
            await api("/api/voice/settings", {
              method: "PATCH",
              body: JSON.stringify({
                enabled,
                provider,
                modelId: modelId.trim(),
                voiceId: voiceId.trim(),
                apiKey: clearKey ? "" : apiKey.trim() || null,
                clearApiKey: clearKey,
              }),
            });
            close();
          } catch (err) {
            setError(err instanceof Error ? err.message : t("settingsFailed"));
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="dialog-title">
          <h2>{t("voiceSettings")}</h2>
          <button type="button" onClick={close}><X /></button>
        </div>
        <p className="dialog-lead">{t("voiceSettingsHint")}</p>
        <label className="memory-toggle">
          <input type="checkbox" role="switch" checked={enabled} disabled={!settings || busy} onChange={(event) => setEnabled(event.target.checked)} />
          {t("voiceEnabled")}
        </label>
        <p className="dialog-lead">{t("voiceEnabledHint")}</p>
        {enabled ? <>
        <fieldset className="provider-fieldset">
          <legend>{t("voiceProvider")}</legend>
          <div className="provider-grid">
            {(settings?.providers || [
              { id: "xai" as const, name: t("providerXai"), envKeyName: "XAI_API_KEY" },
              { id: "openai" as const, name: t("providerOpenai"), envKeyName: "OPENAI_API_KEY" },
            ]).map((item) => (
              <button type="button" key={item.id} className={provider === item.id ? "picked" : ""} onClick={() => pickProvider(item.id)}>
                {item.id === "xai" ? t("providerXai") : item.id === "openai" ? t("providerOpenai") : item.name}
              </button>
            ))}
          </div>
          <p className="dialog-lead">{provider === "openai" ? t("providerOpenaiVoiceHint") : t("providerXaiVoiceHint")}</p>
        </fieldset>
        <label>
          {t("voiceModel")}
          {models.length ? (
            <select value={models.some((item) => item.id === modelId) ? modelId : modelId} onChange={(event) => setModelId(event.target.value)}>
              {models.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
              {!models.some((item) => item.id === modelId) && modelId ? <option value={modelId}>{modelId}</option> : null}
            </select>
          ) : (
            <input value={modelId} onChange={(event) => setModelId(event.target.value)} />
          )}
        </label>
        <label>
          {t("voiceName")}
          {voices.length ? (
            <select value={voiceId} onChange={(event) => setVoiceId(event.target.value)}>
              {voices.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
          ) : (
            <input value={voiceId} onChange={(event) => setVoiceId(event.target.value)} />
          )}
        </label>
        <label>
          {t("apiKey")}
          <input type="password" autoComplete="off" value={apiKey} onChange={(event) => { setApiKey(event.target.value); setClearKey(false); }} placeholder={settings?.apiKeySet ? t("apiKeyStored") : t("apiKeyPlaceholder")} />
        </label>
        {settings?.reusesTextKey && !apiKey && !clearKey ? <p className="dialog-lead">{t("voiceReusesTextKey")}</p> : null}
        {settings?.envKeySet && !apiKey && !clearKey ? <p className="dialog-lead">{t("usingEnvKey", { name: current?.envKeyName || settings.envKeyName })}</p> : null}
        {settings?.apiKeySet ? <label className="memory-toggle"><input type="checkbox" checked={clearKey} onChange={(event) => setClearKey(event.target.checked)} /> {t("clearApiKey")}</label> : null}
        </> : null}
        {error ? <div className="pane-error">{error}</div> : null}
        <div className="dialog-actions">
          <button type="button" className="outline" onClick={close}>{t("cancel")}</button>
          <button className="primary" disabled={busy || !settings || !modelId.trim() || !voiceId.trim()}>{busy ? t("saving") : t("save")}</button>
        </div>
      </form>
    </div>
  );
}
