import {useEffect, useId, useRef, useState} from "react";
import {api, type MediaCapabilities, type Upload, type UploadLimits} from "./api";
import { bytes } from "./workspace/format";
import { Plus, X, Paperclip, GraduationCap } from "./ui/icons";

const MB = 1024 * 1024;
/** Used until the server says otherwise (see UPLOAD_LIMITS in server/main.py). */
const DEFAULT_LIMITS: UploadLimits = { files: 10, file_bytes: 50 * MB, image_bytes: 20 * MB, total_bytes: 200 * MB };
const isImage = (name: string) => /\.(png|jpe?g|webp|gif|heic)$/i.test(name);

export function AttachmentPicker({agentId, disabled, files, busy, onChange, onBusy, onTeach}: {
  agentId: string; disabled: boolean; files: Upload[]; busy: boolean;
  onChange: (files: Upload[]) => void; onBusy: (busy: boolean) => void;
  onTeach: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [caps, setCaps] = useState<MediaCapabilities | null>(null);
  const [limits, setLimits] = useState<UploadLimits>(DEFAULT_LIMITS);
  const [error, setError] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const inputId = useId();
  useEffect(() => {
    if(!menuOpen)return;
    menu.current?.querySelector<HTMLElement>('.composer-add-menu :is(label[tabindex], button:not(:disabled))')?.focus();
    const outside = (event: PointerEvent) => {if(!menu.current?.contains(event.target as Node))setMenuOpen(false);};
    document.addEventListener('pointerdown',outside);
    return ()=>document.removeEventListener('pointerdown',outside);
  },[menuOpen]);
  const [capsFailed, setCapsFailed] = useState(false);
  const inFlight = useRef(false);
  const takeRef = useRef<(files: File[]) => Promise<void>>(async () => {});
  async function refresh() {
    try { const result = await api.media(agentId); setCaps(result.capabilities || null); if (result.limits) setLimits(result.limits); setCapsFailed(!result.capabilities); }
    catch { setCapsFailed(true); }
  }
  useEffect(() => { void refresh(); window.addEventListener("focus", refresh); window.addEventListener("lazyboy:model-changed", refresh); return () => { window.removeEventListener("focus", refresh); window.removeEventListener("lazyboy:model-changed", refresh); }; }, [agentId]);
  useEffect(() => {
    const form = input.current?.closest(".composer-dock")?.querySelector("form.composer");
    if (!form) return;
    const paste = (event: Event) => {
      const e = event as ClipboardEvent;
      const items = Array.from(e.clipboardData?.files || []);
      if (!items.length || disabled) return;
      e.preventDefault(); void takeRef.current(items);
    };
    const drag = (e: Event) => { if (!disabled && (e as DragEvent).dataTransfer?.types.includes("Files")) e.preventDefault(); };
    const drop = (e: Event) => { if (disabled) return; const items = Array.from((e as DragEvent).dataTransfer?.files || []); if (items.length) { e.preventDefault(); void takeRef.current(items); } };
    form.addEventListener("paste", paste); form.addEventListener("dragover", drag); form.addEventListener("drop", drop);
    return () => { form.removeEventListener("paste", paste); form.removeEventListener("dragover", drag); form.removeEventListener("drop", drop); };
  }, [disabled]);
  takeRef.current = async (selected: File[]) => {
    if (busy || inFlight.current || disabled || !agentId) return;
    if (files.length + selected.length > limits.files) { setError(`每則訊息最多 ${limits.files} 個附件`); return; }
    const big = selected.find(file => file.size > (isImage(file.name) ? limits.image_bytes : limits.file_bytes));
    if (big) { setError(`${big.name} 太大：圖片上限 ${bytes(limits.image_bytes)}，其他檔案 ${bytes(limits.file_bytes)}`); return; }
    if ((files.reduce((sum, file) => sum + file.bytes, 0) + selected.reduce((sum, file) => sum + file.size, 0)) > limits.total_bytes) { setError(`附件合計上限 ${bytes(limits.total_bytes)}`); return; }
    const id = agentId;
    inFlight.current = true; onBusy(true); setError("");
    const uploaded = [...files];
    try {
      for (const file of selected) {
        const result = await api.upload(id, file);
        // The parent callback retains the original conversation even after navigation.
        uploaded.push(result); onChange([...uploaded]);
      }
    } catch (err) { setError(err instanceof Error ? err.message : "上傳失敗"); }
    finally { inFlight.current = false; onBusy(false); }
  };
  // Everything here is read by the bot's tools in /workspace; only pictures need a model that can see.
  const accept = [".txt,.md,.csv,.tsv,.json,.log,.yaml,.yml,.xml,.html,.htm,.docx,.xlsx,.xls,.pptx,.zip,.py,.js,.ts,.sql", caps?.images ? ".png,.jpg,.jpeg,.webp,.gif" : "", caps?.pdf ? ".pdf" : ""].filter(Boolean).join(",");
  return <div className="attachment-picker">
    <div className="composer-add-wrap" ref={menu} onBlur={event=>{if(event.relatedTarget&&!event.currentTarget.contains(event.relatedTarget as Node))setMenuOpen(false);}} onKeyDown={event=>{
      if(event.key==='Escape'){event.stopPropagation();setMenuOpen(false);trigger.current?.focus();}
    }}>
      <button type="button" ref={trigger} className="composer-plus" disabled={disabled || !agentId} aria-label="加入檔案或教它一件事" aria-expanded={menuOpen} onClick={()=>setMenuOpen(value=>!value)}>{menuOpen?<X/>:<Plus/>}</button>
      {menuOpen ? <div className="composer-add-menu">
        {/* A label opens the native file picker itself; iOS ignores scripted clicks on a hidden file input. */}
        {disabled || busy || !caps
          ? <button type="button" disabled><Paperclip/>加入檔案</button>
          : <label htmlFor={inputId} role="button" tabIndex={0} onClick={()=>window.setTimeout(()=>setMenuOpen(false))}
              onKeyDown={event=>{if(event.key==="Enter"||event.key===" "){event.preventDefault();input.current?.click();setMenuOpen(false);trigger.current?.focus();}}}><Paperclip/>加入檔案</label>}
        <button type="button" disabled={disabled} onClick={()=>{setMenuOpen(false);onTeach();}}><GraduationCap aria-hidden="true"/>教它一件事</button>
        {caps ? <small>{`${caps.images ? `圖片（${bytes(limits.image_bytes)}）、` : ""}${caps.pdf ? "PDF、" : ""}文件、試算表、文字檔（${bytes(limits.file_bytes)}）\n最多 ${limits.files} 個，共 ${bytes(limits.total_bytes)}`}</small> : capsFailed ? <button type="button" onClick={()=>void refresh()}>重試附件支援</button> : <small role="status">檢查附件支援中…</small>}
      </div> : null}
    </div>
    {busy ? <div className="composer-attachment-status" role="status">附件上傳中…</div> : null}
    <input ref={input} id={inputId} className="file-input" type="file" multiple accept={accept} tabIndex={-1} aria-hidden="true" onChange={e => { const selected = Array.from(e.target.files || []); e.target.value = ""; void takeRef.current(selected); }} />
    {files.length ? <div className="attachment-list">{files.map(file => <span key={file.id}><a href={file.url} target="_blank" rel="noreferrer">{file.name}</a><button type="button" disabled={disabled || busy} aria-label={`移除 ${file.name}`} onClick={() => onChange(files.filter(item => item.id !== file.id))}><X/></button></span>)}</div> : null}
    {error ? <div role="alert" className="pane-error">{error}</div> : null}
  </div>;
}
