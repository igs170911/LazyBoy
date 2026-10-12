import {useEffect, useRef, useState} from "react";
import {api, type MediaCapabilities, type Upload} from "./api";
import { Plus, X, Paperclip, Video } from "./ui/icons";

export function AttachmentPicker({agentId, disabled, files, busy, onChange, onBusy, onRecord}: {
  agentId: string; disabled: boolean; files: Upload[]; busy: boolean;
  onChange: (files: Upload[]) => void; onBusy: (busy: boolean) => void;
  onRecord: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [caps, setCaps] = useState<MediaCapabilities | null>(null);
  const [error, setError] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if(!menuOpen)return;
    menu.current?.querySelector<HTMLButtonElement>('.composer-add-menu button:not(:disabled)')?.focus();
    const outside = (event: PointerEvent) => {if(!menu.current?.contains(event.target as Node))setMenuOpen(false);};
    document.addEventListener('pointerdown',outside);
    return ()=>document.removeEventListener('pointerdown',outside);
  },[menuOpen]);
  const [capsFailed, setCapsFailed] = useState(false);
  const inFlight = useRef(false);
  const takeRef = useRef<(files: File[]) => Promise<void>>(async () => {});
  async function refresh() {
    try { const result = await api.media(agentId); setCaps(result.capabilities || null); setCapsFailed(!result.capabilities); }
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
    if (files.length + selected.length > 4) { setError("每則訊息最多 4 個附件"); return; }
    if (selected.some(file => file.size > 5 * 1024 * 1024)) { setError("每個附件上限 5 MB"); return; }
    if ((files.reduce((sum, file) => sum + file.bytes, 0) + selected.reduce((sum, file) => sum + file.size, 0)) > 10 * 1024 * 1024) { setError("附件合計上限 10 MB"); return; }
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
  const accept = [".txt,.md,.csv,.tsv,.json,.log,.yaml,.yml,.xml", caps?.images ? ".png,.jpg,.jpeg,.webp" : "", caps?.pdf ? ".pdf" : ""].filter(Boolean).join(",");
  return <div className="attachment-picker">
    <div className="composer-add-wrap" ref={menu} onBlur={event=>{if(!event.currentTarget.contains(event.relatedTarget as Node | null))setMenuOpen(false);}} onKeyDown={event=>{
      if(event.key==='Escape'){event.stopPropagation();setMenuOpen(false);trigger.current?.focus();}
    }}>
      <button type="button" ref={trigger} className="composer-plus" disabled={disabled || !agentId} aria-label="加入檔案或錄製示範" aria-expanded={menuOpen} onClick={()=>setMenuOpen(value=>!value)}>{menuOpen?<X/>:<Plus/>}</button>
      {menuOpen ? <div className="composer-add-menu">
        <button type="button" disabled={disabled || busy || !caps} onClick={()=>{setMenuOpen(false);trigger.current?.focus();input.current?.click();}}><Paperclip/>加入檔案</button>
        <button type="button" disabled={disabled} onClick={()=>{setMenuOpen(false);onRecord();}}><Video aria-hidden="true"/>錄製示範</button>
        {caps ? <small>{`${caps.images ? "圖片、" : ""}${caps.pdf ? "PDF、" : ""}文字檔 · 每檔 5 MB\n最多 4 個，共 10 MB`}</small> : capsFailed ? <button type="button" onClick={()=>void refresh()}>重試附件支援</button> : <small role="status">檢查附件支援中…</small>}
      </div> : null}
    </div>
    {busy ? <div className="composer-attachment-status" role="status">附件上傳中…</div> : null}
    <input ref={input} type="file" multiple accept={accept} hidden onChange={e => { const selected = Array.from(e.target.files || []); e.target.value = ""; void takeRef.current(selected); }} />
    {files.length ? <div className="attachment-list">{files.map(file => <span key={file.id}><a href={file.url} target="_blank" rel="noreferrer">{file.name}</a><button type="button" disabled={disabled || busy} aria-label={`移除 ${file.name}`} onClick={() => onChange(files.filter(item => item.id !== file.id))}><X/></button></span>)}</div> : null}
    {error ? <div role="alert" className="pane-error">{error}</div> : null}
  </div>;
}
