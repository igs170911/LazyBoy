import { useDialogFocus } from "./dialog-focus";
import { FormEvent, useEffect, useRef, useState } from "react";
import { SkillsPanel } from "./skills-panel";
import { api, type AgentGroup, type AgentProfile, type AgentRow, type SettingsProvider } from "./api";
import { X } from "./ui/icons";
import {
  Avatar,
  AVATAR_COLORS,
  BLOBATAR_EXPRESSIONS,
  CHARACTER_SHAPES,
  cleanTraits,
  EXPRESSION_LABELS,
  EYE_TRAITS,
  type AvatarTraits,
  type Mood,
  persistBlobatarShape,
  resolveBlobatarShape,
  type BlobatarShape,
} from "./avatar";
import {
  AvatarUploadPanel,
  clampAvatarCrop,
  encodeAvatarPng,
  initialAvatarCrop,
  normalizeDataUrl,
  panAvatarCrop,
  readImageFile,
  transformAvatar,
  type AvatarCrop,
  type AvatarImage,
} from "./avatar-upload";
import { useI18n } from "./i18n";

function parseTags(text: string): string[] {
  return text.split(/[、,，]/).map((tag) => tag.trim()).filter(Boolean).slice(0, 6);
}

function joinTags(tags: string[] | undefined): string {
  return (tags || []).join("、");
}

function defaultColor(value?: string) {
  const found = AVATAR_COLORS.find((item) => item.value.toLowerCase() === (value || "").toLowerCase());
  return found?.value || AVATAR_COLORS[6].value;
}

function randomLook(previousColor?: string, previousShape?: string) {
  const choices = CHARACTER_SHAPES.flatMap(item => AVATAR_COLORS.map(color => ({shape:item.shape, color:color.value})))
    .filter(look => look.color !== previousColor || look.shape !== previousShape);
  return choices[crypto.getRandomValues(new Uint32Array(1))[0] % choices.length];
}

export function AgentSettingsDialog({
  agent,
  mode = "edit",
  onClose,
  onSaved,
  onShare,
  initialSection = "profile",
}: {
  agent?: AgentRow | null;
  mode?: "create" | "edit";
  onClose: () => void;
  onSaved: (row: AgentRow) => void;
  onShare?: (row: AgentRow) => void;
  initialSection?: "profile" | "skills";
}) {
  const creating = mode === "create";
  const [section,setSection] = useState(initialSection);
  const [skillEditing,setSkillEditing] = useState(false);
  const { t } = useI18n();
  const pickerRef = useRef<HTMLDivElement>(null);
  const avatarHitRef = useRef<HTMLButtonElement>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [tab, setTab] = useState<"bot" | "upload" | "generate">("bot");
  const [initialLook] = useState(() => randomLook());
  const [imageModels, setImageModels] = useState<string[]>([]);
  const [model, setModel] = useState("");
  const [prompt, setPrompt] = useState("");
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [generating, setGenerating] = useState(false);
  const generation = useRef<AbortController | null>(null);
  const stagedUrl = useRef("");
  const createdAgent = useRef<AgentRow | null>(null);
  useEffect(() => () => { generation.current?.abort(); if (stagedUrl.current) URL.revokeObjectURL(stagedUrl.current); }, []);
  async function loadModels() {
    setModelsLoading(true); setModelsError("");
    try { const result = await api.avatarModels(); setImageModels(result.models); setModel(current => result.models.includes(current) ? current : result.models[0] || ""); }
    catch (err) { setModelsError(err instanceof Error ? err.message : "無法載入生圖模型"); }
    finally { setModelsLoading(false); }
  }
  useEffect(() => { if (pickerOpen) void loadModels(); }, [pickerOpen]);
  async function generate() {
    if (generating || !model || !prompt.trim()) return;
    const controller = new AbortController(); generation.current = controller;
    setGenerating(true); setError("");
    try {
      const result = await api.generateAvatar(model, prompt, controller.signal);
      if (controller.signal.aborted) return;
      const image = await normalizeDataUrl(result.data_url);
      if (controller.signal.aborted) return;
      setSource(image); setCrop(initialAvatarCrop(image.width, image.height)); setFileName("產生的大頭貼"); setTab("upload");
    } catch (err) { if (!controller.signal.aborted) setError(err instanceof Error ? err.message : "生圖失敗，請重試"); }
    finally { if (generation.current === controller) { setGenerating(false); generation.current = null; } }
  }
  const [name, setName] = useState(agent?.name || "");
  const [groupId, setGroupId] = useState(agent?.group_id || "");
  const [persona, setPersona] = useState(agent?.persona?.trim() ? agent.persona : agent?.description || "");
  const [groups,setGroups] = useState<AgentGroup[]>([]);
  const [providers, setProviders] = useState<SettingsProvider[]>([]);
  const [chatProvider, setChatProvider] = useState(agent?.provider || "");
  const [chatModel, setChatModel] = useState(agent?.model || "");
  const [replyStyle, setReplyStyle] = useState(agent?.reply_style || "");
  useEffect(()=>{ void api.agents().then(data=>setGroups(data.groups)).catch(()=>{}); },[]);
  useEffect(() => {
    void api.settings().then((data) => {
      // Only providers that can answer: a saved key, or a local service that needs none.
      // The provider this Bot already uses always stays listed, keyed or not.
      const keyed = data.providers.filter((provider) => provider.selectable && (provider.has_key || provider.id === agent?.provider || /^http:\/\/(127\.|localhost)/.test(provider.base_url)));
      setProviders(keyed);
    }).catch(() => {});
  }, []);
  const [modelChecking, setModelChecking] = useState(false);
  const chosenRow = providers.find((item) => item.id === chatProvider)?.models.find((item) => item.id === chatModel);
  // A model nobody has tried yet is checked when picked, so a broken choice shows up here, not mid-task.
  useEffect(() => {
    if (!chatProvider || !chatModel || chosenRow?.check || !chosenRow) { setModelChecking(false); return; }
    let alive = true;
    setModelChecking(true);
    void api.checkModel({ provider: chatProvider, model: chatModel }).then((data) => {
      if (!alive) return;
      setProviders((list) => list.map((provider) => provider.id !== chatProvider ? provider : { ...provider,
        models: provider.models.map((item) => item.id === chatModel ? { ...item, protocol: data.result.protocol || item.protocol, check: data.result } : item) }));
    }).catch(() => {}).finally(() => { if (alive) setModelChecking(false); });
    return () => { alive = false; setModelChecking(false); };
  }, [chatProvider, chatModel, !!chosenRow?.check, !!chosenRow]);
  const [title, setTitle] = useState(agent?.title || "");
  const [color, setColor] = useState(agent?.avatar_color ? defaultColor(agent.avatar_color) : initialLook.color);
  const [eyes, setEyes] = useState<AvatarTraits>(() => cleanTraits(agent?.avatar_traits));
  const [expression, setExpression] = useState(agent?.avatar_expression || "idle");
  const [previewMood, setPreviewMood] = useState<Mood>("idle");
  const [shape, setShape] = useState<BlobatarShape>(agent?.avatar_shape ? resolveBlobatarShape(agent.avatar_shape) : initialLook.shape);
  const [tagText, setTagText] = useState(joinTags(agent?.tags));
  const [photoUrl, setPhotoUrl] = useState(agent?.avatar_url || "");
  const [source, setSource] = useState<AvatarImage | null>(null);
  const [crop, setCrop] = useState<AvatarCrop | null>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [stagedPng, setStagedPng] = useState<Blob | null>(null);
  const [clearedPhoto, setClearedPhoto] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const dismissing = useRef(false);
  const ateOutside = useRef(false);
  const live = useRef({ creating, agent, name, title, groupId, persona, tagText, color, shape, photoUrl, source, crop, stagedPng, clearedPhoto, busy: busy || generating, pickerOpen, chatProvider, chatModel, replyStyle, eyes, expression });
  live.current = { creating, agent, name, title, groupId, persona, tagText, color, shape, photoUrl, source, crop, stagedPng, clearedPhoto, busy: busy || generating, pickerOpen, chatProvider, chatModel, replyStyle, eyes, expression };

  useEffect(() => {
    if (creating || !agent) return;
    setName(agent.name);
    setGroupId(agent.group_id || "");
    setTitle(agent.title || "");
    setPersona(agent.persona?.trim() ? agent.persona : agent.description || "");
    setColor(defaultColor(agent.avatar_color));
    setShape(resolveBlobatarShape(agent.avatar_shape));
    setEyes(cleanTraits(agent.avatar_traits));
    setExpression(agent.avatar_expression || "idle");
    setTagText(joinTags(agent.tags));
    setPhotoUrl(agent.avatar_url || "");
    setChatProvider(agent.provider || "");
    setChatModel(agent.model || "");
    setReplyStyle(agent.reply_style || "");
    setError("");
    // Roster polling returns fresh arrays every few seconds. Initialize a new
    // identity once; subsequent snapshots must not replace unsaved form fields.
  }, [creating, agent?.id]);

  const face = { name: name.trim() || agent?.name || "Bot", color, shape, src: photoUrl || undefined, eyes, expression };
  const ready = Boolean(name.trim() && (creating ? persona.trim() : true));

  function profileFrom(state = live.current): AgentProfile {
    return {
      name: state.name.trim() || state.agent?.name || "Bot",
      title: state.title.trim(),
      description: state.persona.trim().slice(0, 1000),
      tags: parseTags(state.tagText),
      group_id: state.groupId,
      persona: state.persona,
      avatar_color: state.color,
      avatar_shape: persistBlobatarShape(state.shape),
      provider: state.chatProvider,
      model: state.chatModel,
      reply_style: state.replyStyle,
      avatar_traits: state.eyes,
      avatar_expression: state.expression,
    };
  }

  async function persistAvatar() {
    if (dismissing.current || live.current.busy) return;
    dismissing.current = true;
    live.current.pickerOpen = false;
    setPickerOpen(false);
    const state = live.current;
    try {
      // Cropped/generated images are applied only with the explicit Use button.
      if (state.source && state.crop) return;
      if (state.creating || !state.agent) return;
      const lookChanged =
        persistBlobatarShape(state.shape) !== resolveBlobatarShape(state.agent.avatar_shape) ||
        state.color.toLowerCase() !== defaultColor(state.agent.avatar_color).toLowerCase() ||
        state.clearedPhoto ||
        JSON.stringify(state.eyes) !== JSON.stringify(cleanTraits(state.agent.avatar_traits)) ||
        state.expression !== (state.agent.avatar_expression || "idle");
      if (!lookChanged) return;
      setBusy(true);
      setError("");
      let row = await api.updateAgent(state.agent.id, profileFrom(state));
      if (state.clearedPhoto && state.agent.avatar_url) row = await api.clearAvatar(state.agent.id);
      onSaved(row);
    } catch (err) {
      setError(err instanceof Error ? err.message : t.settingsFailed);
    } finally {
      setBusy(false);
      dismissing.current = false;
    }
  }

  async function saveProfile(closeAfter: boolean) {
    const state = live.current;
    const nextName = state.name.trim();
    if (state.busy) return;
    if (skillEditing) {setError("請先儲存或取消正在編輯的技能。");return;}
    if (state.creating) {
      if (!nextName || !state.persona.trim()) {
        if (closeAfter) onClose();
        return;
      }
    } else if (!state.agent || !nextName) {
      if (closeAfter) onClose();
      return;
    }
    setBusy(true);
    setError("");
    try {
      if (state.creating) {
        // Creation and photo upload are separate requests. A failed upload
        // must retry the confirmed identity, including any newly edited fields.
        const created = createdAgent.current
          ? await api.updateAgent(createdAgent.current.id, profileFrom(state))
          : await api.createAgent(profileFrom(state));
        createdAgent.current = created;
        if (state.stagedPng) onSaved(await api.setAvatar(created.id, state.stagedPng));
        else onSaved(created);
      } else if (state.agent) {
        let row = await api.updateAgent(state.agent.id, profileFrom(state));
        if (state.clearedPhoto && state.agent.avatar_url) row = await api.clearAvatar(state.agent.id);
        onSaved(row);
        if (closeAfter) onClose();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : t.settingsFailed);
    } finally {
      setBusy(false);
    }
  }

  const persistAvatarRef = useRef(persistAvatar);
  persistAvatarRef.current = persistAvatar;

  useEffect(() => {
    if (!pickerOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (pickerRef.current?.contains(target) || avatarHitRef.current?.contains(target)) return;
      if (target instanceof Element && target.closest(".dialog-title button")) return;
      ateOutside.current = true;
      void persistAvatarRef.current();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      ateOutside.current = true;
      void persistAvatarRef.current();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [pickerOpen]);

  async function ingest(file: File) {
    setError("");
    try {
      const image = await normalizeDataUrl(await readImageFile(file));
      setSource(image);
      setCrop(initialAvatarCrop(image.width, image.height));
      setFileName(file.name || t.avatarPasted);
      setTab("upload");
      setPickerOpen(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : t.settingsFailed);
    }
  }

  async function commitPhoto() {
    const state = live.current;
    if (!state.source || !state.crop) return;
    setBusy(true);
    setError("");
    try {
      const png = await encodeAvatarPng(state.source, state.crop);
      if (state.creating || !state.agent) {
        setStagedPng(png);
        if (stagedUrl.current) URL.revokeObjectURL(stagedUrl.current);
        stagedUrl.current = URL.createObjectURL(png);
        setPhotoUrl(stagedUrl.current);
      } else {
        const row = await api.setAvatar(state.agent.id, png);
        setPhotoUrl(row.avatar_url || "");
        onSaved(row);
      }
      setSource(null);
      setCrop(null);
      setFileName(null);
      setClearedPhoto(false);
      setPickerOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : t.settingsFailed);
    } finally {
      setBusy(false);
    }
  }

  async function resetAvatar() {
    setSource(null);
    setCrop(null);
    setFileName(null);
    setStagedPng(null);
    setClearedPhoto(true);
    const next = randomLook(color, shape);
    setShape(next.shape);
    setColor(next.color);
    setTab("bot");
    setPhotoUrl("");

  }

  function onSubmit(event: FormEvent) {
    event.preventDefault();
    void saveProfile(!creating);
  }

  const dialogRoot = useRef<HTMLFormElement>(null);
  useDialogFocus(dialogRoot, () => {
    if (live.current.busy) return;
    if (creating) onClose();
    else void saveProfile(true);
  });

  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (ateOutside.current) {
          ateOutside.current = false;
          return;
        }
        void saveProfile(true);
      }}
    >
      <form
        ref={dialogRoot}
        role="dialog"
        aria-modal="true"
        aria-label={creating ? t.addAgent : t.agentSettings}
        className="dialog settings-dialog agent-settings"
        onClick={(event) => event.stopPropagation()}
        onSubmit={onSubmit}
        onPaste={(event) => {
          const item = Array.from(event.clipboardData.items).find((candidate) => candidate.kind === "file" && candidate.type.toLowerCase().startsWith("image/"));
          const file = item?.getAsFile();
          if (!file) return;
          event.preventDefault();
          void ingest(file);
        }}
      >
        <div className="dialog-title">
          <h2>{creating ? t.addAgent : t.agentSettings}</h2>
          <button type="button" aria-label={t.close} onClick={onClose}><X /></button>
        </div>
        {!creating&&!pickerOpen?<div className="agent-section-tabs" role="tablist" aria-label="Bot 設定內容" onKeyDown={e=>{if(["ArrowLeft","ArrowRight","Home","End"].includes(e.key)){e.preventDefault();const next=e.key==="Home"?"profile":e.key==="End"?"skills":section==="profile"?"skills":"profile";setSection(next);e.currentTarget.querySelector<HTMLButtonElement>(`#agent-${next}-tab`)?.focus();}}}><button type="button" role="tab" tabIndex={section==="profile"?0:-1} aria-selected={section==="profile"} aria-controls="agent-profile-panel" id="agent-profile-tab" onClick={()=>setSection("profile")}>基本設定</button><button type="button" role="tab" tabIndex={section==="skills"?0:-1} aria-selected={section==="skills"} aria-controls="agent-skills-panel" id="agent-skills-tab" onClick={()=>setSection("skills")}>技能 · {agent?.skill_count || 0}</button></div>:null}
        <button
          hidden={!creating&&section!=="profile"&&!pickerOpen}
          ref={avatarHitRef}
          type="button"
          className="avatar-hit"
          onClick={() => {
            if (pickerOpen) void persistAvatar();
            else setPickerOpen(true);
          }}
          aria-label={t.avatarAppearance}
        >
          <Avatar {...face} size={88} alive gaze mood={pickerOpen ? previewMood : "idle"} />
        </button>
        {pickerOpen ? (
          <div className="avatar-picker" ref={pickerRef}>
            {!photoUrl ? (
              <div className="mood-preview" role="group" aria-label="預覽狀態表情">
                {([["idle", "待命"], ["ack", "收到"], ["working", "工作中"], ["waiting", "等你"], ["done", "完成"], ["failed", "失敗"], ["sleepy", "想睡"]] as [Mood, string][]).map(([value, label]) => (
                  <button type="button" key={value} aria-pressed={previewMood === value} onClick={() => setPreviewMood(value)}>{label}</button>
                ))}
              </div>
            ) : null}
            <div className="avatar-picker-bar">
              <div className="avatar-tabs" role="tablist">
                <button type="button" role="tab" disabled={generating} aria-selected={tab === "bot"} className={tab === "bot" ? "selected" : ""} onClick={() => setTab("bot")}>{t.avatarBot}</button>
                <button type="button" role="tab" disabled={generating} aria-selected={tab === "upload"} className={tab === "upload" ? "selected" : ""} onClick={() => setTab("upload")}>{t.avatarUpload}</button>
                {imageModels.length > 0 ? <button type="button" role="tab" disabled={generating} aria-selected={tab === "generate"} className={tab === "generate" ? "selected" : ""} onClick={() => setTab("generate")}>AI 產生</button> : null}
              </div>
              <button type="button" className="avatar-reset" disabled={busy || generating} onClick={() => void resetAvatar()}>重新隨機</button>
            </div>
            {modelsLoading ? <small role="status">檢查生圖支援…</small> : modelsError ? <small className="pane-error">{modelsError} <button type="button" onClick={() => void loadModels()}>重試</button></small> : !imageModels.length ? <small>目前供應商未提供可用的生圖模型</small> : null}
            {tab === "generate" ? (
              <div className="avatar-generate">
                <label>生圖模型<select value={model} disabled={generating} onChange={event => setModel(event.target.value)}>{imageModels.map(id => <option key={id}>{id}</option>)}</select></label>
                <label>描述你想要的大頭貼<textarea value={prompt} maxLength={2000} rows={3} disabled={generating} onChange={event => setPrompt(event.target.value)} placeholder="例如：戴圓眼鏡的橘貓，簡單插畫，淡綠背景" /></label>
                <small>使用目前供應商的 API 額度，產生後可裁切再套用。</small>
                <div className="pane-actions"><button type="button" className="primary" disabled={generating || !model || !prompt.trim()} onClick={() => void generate()}>{generating ? "產生中…" : "產生大頭貼"}</button>{generating ? <button type="button" onClick={() => { generation.current?.abort(); setGenerating(false); }}>取消等待</button> : null}</div>
                {error ? <div role="alert" className="pane-error">{error}</div> : null}
              </div>
            ) : tab === "bot" ? (
              <>
                <div className="shape-grid" role="group" aria-label={t.shape}>
                  {CHARACTER_SHAPES.map((item) => (
                    <button
                      type="button"
                      className={shape === item.shape ? "selected" : ""}
                      onClick={() => { setShape(item.shape); setSource(null); setCrop(null); setStagedPng(null); setPhotoUrl(""); setClearedPhoto(true); }}
                      key={item.id}
                      title={item.id}
                      aria-label={item.id}
                    >
                      <Avatar name={face.name} color={color} shape={item.shape} gaze={false} size={44} />
                    </button>
                  ))}
                </div>
                <div className="color-grid" role="group" aria-label={t.color}>
                  {AVATAR_COLORS.map((item) => (
                    <button
                      type="button"
                      key={item.id}
                      className={color.toLowerCase() === item.value.toLowerCase() ? "selected" : ""}
                      style={{ background: item.value }}
                      onClick={() => setColor(item.value)}
                      aria-label={item.id}
                    />
                  ))}
                </div>
                <div className="avatar-section">
                  <span className="avatar-section-label">平常的表情</span>
                  <div className="expression-grid" role="group" aria-label="平常的表情">
                    {BLOBATAR_EXPRESSIONS.filter(item => item !== "thinking").map(item => (
                      <button type="button" key={item} className={expression === item ? "selected" : ""} aria-pressed={expression === item}
                        onClick={() => { setExpression(item); setPreviewMood("idle"); setSource(null); setCrop(null); setStagedPng(null); setPhotoUrl(""); setClearedPhoto(true); }}>
                        <Avatar name={face.name} color={color} shape={shape} eyes={eyes} expression={item} gaze={false} size={34} />
                        <small>{EXPRESSION_LABELS[item]}</small>
                      </button>
                    ))}
                  </div>
                </div>
                <div className="avatar-section">
                  <div className="avatar-section-head">
                    <span className="avatar-section-label">眼睛與身體</span>
                    <button type="button" className="link-btn" disabled={!Object.keys(eyes).length} onClick={() => setEyes({})}>恢復預設</button>
                  </div>
                  {EYE_TRAITS.map(item => (
                    <label className="trait-slider" key={item.key}>
                      <span>{item.label}</span>
                      <small aria-hidden="true">{item.low}</small>
                      <input type="range" min={0} max={1} step={0.01} value={eyes[item.key] ?? (item.key === "eye.scale" ? 0.75 : 0.5)}
                        onChange={event => { const value = Number(event.target.value); setEyes(current => ({ ...current, [item.key]: value })); setPreviewMood("idle"); }} />
                      <small aria-hidden="true">{item.high}</small>
                    </label>
                  ))}
                </div>
              </>
            ) : (
              <>
              {!source && photoUrl ? <button type="button" className="outline" onClick={async () => { try { const response = await fetch(photoUrl); if (!response.ok) throw new Error("無法讀取頭像"); await ingest(new File([await response.blob()], "目前頭像.png", {type:"image/png"})); } catch (err) { setError(err instanceof Error ? err.message : "無法讀取頭像"); } }}>編輯目前頭像</button> : null}
              <AvatarUploadPanel
                source={source}
                crop={crop}
                fileName={fileName}
                busy={busy}
                error={error}
                onFile={(file) => void ingest(file)}
                onZoom={(zoom) => source && crop && setCrop(clampAvatarCrop(source, { ...crop, zoom }))}
                onPan={(dx, dy) => source && crop && setCrop(panAvatarCrop(source, crop, dx, dy))}
                onReset={() => { setSource(null); setCrop(null); setFileName(null); }}
                onSave={() => void commitPhoto()}
                onCenter={() => source && setCrop(initialAvatarCrop(source.width, source.height))}
                onTransform={async action => { if (!source || busy) return; setBusy(true); try { const image = await transformAvatar(source, action); setSource(image); setCrop(initialAvatarCrop(image.width,image.height)); } catch (err) { setError(err instanceof Error ? err.message : "編輯失敗"); } finally { setBusy(false); } }}
              />
              </>
            )}
            {!(source && crop) ? <button type="button" className="avatar-editor-done outline" disabled={busy || generating} onClick={() => void persistAvatar()}>完成編輯</button> : null}
          </div>
        ) : (
          <>
            <div id="agent-profile-panel" role="tabpanel" aria-labelledby="agent-profile-tab" hidden={!creating&&section!=="profile"}>
            <label>
              {t.name}
              <input autoFocus={creating} required value={name} maxLength={80} onChange={(event) => setName(event.target.value)} />
            </label>
            <label>
              {t.shortTitle}
              <input value={title} maxLength={100} onChange={(event) => setTitle(event.target.value)} placeholder={t.shortTitlePlaceholder} />
            </label>
            <label>
              {t.tags}
              <input value={tagText} maxLength={120} onChange={(event) => setTagText(event.target.value)} placeholder={t.tagsPlaceholder} />
            </label>
            <label>分組<select value={groupId} onChange={e=>setGroupId(e.target.value)}><option value="">未分組</option>{groups.map(group=><option key={group.id} value={group.id}>{group.name}</option>)}</select></label>
            <label>{t.provider}<select value={chatProvider} onChange={(event) => {
              const next = event.target.value;
              setChatProvider(next);
              const provider = providers.find((item) => item.id === next);
              const model = provider?.models.some((item) => item.id === chatModel) ? chatModel : provider?.models[0]?.id || "";
              setChatModel(model);
            }}><option value="">使用全域設定</option>{providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.label || provider.id}</option>)}</select></label>
            {chatProvider ? <label>模型<select aria-label="模型" value={chatModel} onChange={(event) => setChatModel(event.target.value)}>{(providers.find((provider) => provider.id === chatProvider)?.models || []).map((item) => {
              const broken = ["unavailable", "auth", "protocol"].includes(item.check?.status || "");
              return <option key={item.id} value={item.id} disabled={broken && item.id !== chatModel}>{item.label || item.id}{broken ? "（無法使用）" : ""}</option>;
            })}</select>
              {modelChecking ? <small className="model-check">檢查中…</small>
                : chosenRow?.check ? <small className={`model-check ${chosenRow.check.status === "ok" ? "ok" : ["busy", "unknown"].includes(chosenRow.check.status) ? "warn" : "bad"}`} title={chosenRow.check.detail || ""}>{chosenRow.check.status === "ok" ? "✓ 可以使用" : `${["busy", "unknown"].includes(chosenRow.check.status) ? "⚠" : "✗"} ${chosenRow.check.message}`}</small> : null}
            </label> : null}
            <label>回覆長度<select value={replyStyle} onChange={e => setReplyStyle(e.target.value)}>
              <option value="brief">精簡：1 到 3 行</option>
              <option value="">標準：先結論，約 8 行內</option>
              <option value="detailed">詳細：完整說明</option>
            </select></label>
            <label>{t.persona}<textarea required={creating} value={persona} maxLength={8000} rows={4} onChange={e=>setPersona(e.target.value)} placeholder={t.personaPlaceholder} /></label>
            </div>
            {!creating && agent ? <div id="agent-skills-panel" role="tabpanel" aria-labelledby="agent-skills-tab" hidden={section!=="skills"}><SkillsPanel agentId={agent.id} onEditingChange={setSkillEditing}/></div> : <small>建立後即可加入技能與教學流程。</small>}
            {!creating && agent && onShare ? <button type="button" className="outline" disabled={busy || !ready || skillEditing} onClick={async()=>{if(live.current.busy)return;setBusy(true);setError("");try{let row=await api.updateAgent(agent.id,profileFrom());if(live.current.clearedPhoto&&agent.avatar_url)row=await api.clearAvatar(agent.id);onSaved(row);onShare(row);}catch(e){setError(e instanceof Error?e.message:t.settingsFailed);}finally{setBusy(false);}}}>儲存並分享 Bot</button> : null}
            {skillEditing?<p className="template-note">請先儲存或取消正在編輯的技能，再分享或儲存設定。</p>:null}
            {error ? <div className="pane-error" role="alert">{error}</div> : null}
            <div className="dialog-actions">
              <button type="button" className="outline" onClick={onClose}>{t.cancel}</button>
              <button className="primary" disabled={busy || !ready || skillEditing}>{busy ? (creating ? t.creating : t.saving) : creating ? t.create : t.saveSettings}</button>
            </div>
          </>
        )}
      </form>
    </div>
  );
}
