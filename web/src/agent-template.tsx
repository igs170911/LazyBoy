import {useEffect,useMemo,useRef,useState} from "react";
import {useDialogFocus} from "./dialog-focus";
import {api,type AgentGroup,type AgentRow,type AgentTemplate} from "./api";
import {McpSettings} from "./mcp-settings";
import {X} from "./ui/icons";

function uuid(){const b=crypto.getRandomValues(new Uint8Array(16));b[6]=(b[6]&15)|64;b[8]=(b[8]&63)|128;const h=[...b].map(v=>v.toString(16).padStart(2,'0')).join('');return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`;}
function validateTemplate(data:unknown): AgentTemplate {
  if (!data || typeof data!=='object') throw Error('請選取有效的 Bot 範本（.lazyboy.json）');
  const d=data as AgentTemplate;
  const string=(v:unknown)=>typeof v==='string';
  const map=(v:unknown)=>!!v&&typeof v==='object'&&!Array.isArray(v)&&Object.values(v).every(string);
  if(d.format!=='lazyboy.agent-template'||d.version!==1||!d.profile||typeof d.profile!=='object'||!string(d.profile.name)||['title','description','persona','avatar_color','avatar_shape'].some(k=>(d.profile as any)[k]!==undefined&&!string((d.profile as any)[k]))||(d.profile.tags!==undefined&&(!Array.isArray(d.profile.tags)||!d.profile.tags.every(string)))||!Array.isArray(d.skills)||d.skills.length>64||d.skills.some(s=>!s||!string(s.name)||!string(s.description)||!string(s.instructions))||!d.mcp?.mcpServers||typeof d.mcp.mcpServers!=='object'||Array.isArray(d.mcp.mcpServers)||Object.keys(d.mcp.mcpServers).length>64||Object.values(d.mcp.mcpServers).some(v=>!v||typeof v!=='object'||(v.args!==undefined&&(!Array.isArray(v.args)||!v.args.every(string)))||(v.url!==undefined&&!string(v.url))||(v.command!==undefined&&!string(v.command))||(v.headers!==undefined&&!map(v.headers))||(v.env!==undefined&&!map(v.env)))) throw Error('請選取有效的 Bot 範本（.lazyboy.json）');
  return d;
}

function Contents({template}:{template:AgentTemplate}) {
  const connectors=Object.entries(template.mcp.mcpServers);
  return <div className="template-contents">
    <p className="template-counts">{template.skills.length} 個技能 · {connectors.length} 個串接</p>
    <details><summary>查看人設</summary><p>{template.profile.persona || template.profile.description || "未設定人設"}</p></details>
    <div className="template-skills">{template.skills.map((skill,i)=><details key={i}><summary>{skill.name}{skill.enabled===false?" · 已停用":""}</summary><p>{skill.description}</p><pre>{skill.instructions}</pre></details>)}</div>
    {connectors.length?<details><summary>需要的串接 · {connectors.length}</summary>{connectors.map(([name,cfg])=><article key={name}><strong>{name}</strong><code>{cfg.url || [cfg.command,...(cfg.args || [])].join(" ")}</code><small>待補填：{[...Object.keys(cfg.headers || {}),...Object.keys(cfg.env || {})].join("、") || "請確認網址或啟動設定"}</small></article>)}</details>:null}
    <p className="template-note">分享人設、技能與串接設定；私人記憶、聊天及登入狀態不會帶走，串接憑證會清空。技能內引用的外部檔案需另外提供。</p>
  </div>;
}

export function AgentShareDialog({agent,onClose}:{agent:AgentRow;onClose:()=>void}) {
  const [template,setTemplate]=useState<AgentTemplate|null>(null),[error,setError]=useState(""),[loading,setLoading]=useState(true),[sharing,setSharing]=useState(false),[notice,setNotice]=useState("");
  const [attempt,setAttempt]=useState(0);
  const root=useRef<HTMLElement>(null);
  useDialogFocus(root,()=>{if(!sharing)onClose();});
  useEffect(()=>{
    let alive=true;setLoading(true);setError("");
    void api.exportTemplate(agent.id).then(data=>{if(alive)setTemplate(data);}).catch(e=>{if(alive)setError(e instanceof Error?e.message:"無法準備分享檔案");}).finally(()=>{if(alive)setLoading(false);});
    return()=>{alive=false;};
  },[agent.id,attempt]);
  const file=useMemo(()=>template?new File([JSON.stringify(template,null,2)],`${template.profile.name.replace(/[/\\\u0000-\u001f]/g,"_")}.lazyboy.json`,{type:"application/json"}):null,[template]);
  const canShare=!!file&&!!navigator.share&&!!navigator.canShare?.({files:[file]});
  function download(){if(!file)return;const url=URL.createObjectURL(file);const link=document.createElement("a");link.href=url;link.download=file.name;document.body.append(link);link.click();link.remove();window.setTimeout(()=>URL.revokeObjectURL(url),1000);setNotice("分享檔案已交給瀏覽器下載，可傳給對方匯入。");}
  async function share(){if(!file||sharing)return;setSharing(true);setError("");try{await navigator.share({files:[file],title:`分享 ${template?.profile.name}`});setNotice("分享檔案已送出。");}catch(e){if(!(e instanceof DOMException&&e.name==="AbortError"))setError("系統分享未完成，可以改用下載檔案。");}finally{setSharing(false);}}
  return <div className="modal-backdrop" onClick={()=>{if(!sharing)onClose();}}><section ref={root} className="dialog template-dialog" role="dialog" aria-modal="true" aria-label="分享 Bot" onClick={e=>e.stopPropagation()}>
    <div className="dialog-title"><h2>分享「{template?.profile.name || agent.name}」</h2><button type="button" aria-label="關閉" disabled={sharing} onClick={onClose}><X/></button></div>
    <p>對方匯入後會得到一個獨立 Bot，保留你教他的技能。</p>
    {loading?<p role="status">準備分享內容…</p>:template?<><Contents template={template}/>{!template.skills.length?<p className="template-note">目前還沒有儲存的技能。可先到 Bot 設定新增技能，或在對話中教他流程。</p>:null}<p className="template-note">請先展開技能，確認文字適合分享。已儲存的技能仍需依實際環境驗證。</p></>:null}
    {error?<p role="alert" className="pane-error">{error}{!template?<button type="button" onClick={()=>setAttempt(v=>v+1)}>重試</button>:null}</p>:null}
    {notice?<p role="status">{notice}</p>:null}
    <div className="dialog-actions"><button type="button" disabled={sharing} onClick={onClose}>返回</button>{canShare?<button type="button" disabled={loading||sharing} onClick={()=>void share()}>{sharing?"分享中…":"分享檔案"}</button>:null}<button type="button" className="primary" disabled={!file||loading||sharing} onClick={download}>下載分享檔案</button></div>
  </section></div>;
}

export function TemplateImportDialog({onClose,onImported,onCreated}:{onClose:()=>void;onImported:(row:AgentRow)=>void;onCreated?:()=>void}) {
  const input=useRef<HTMLInputElement>(null),id=useRef(uuid()),readVersion=useRef(0),inFlight=useRef(false),alive=useRef(true);
  const [template,setTemplate]=useState<AgentTemplate|null>(null),[name,setName]=useState(""),[group,setGroup]=useState(""),[groups,setGroups]=useState<AgentGroup[]>([]),[busy,setBusy]=useState(false),[error,setError]=useState(""),[created,setCreated]=useState<AgentRow|null>(null),[configure,setConfigure]=useState(false),[dragging,setDragging]=useState(false),[reading,setReading]=useState(false);
  const names=useRef(new Set<string>()),roster=useRef<Promise<void>|null>(null);
  useEffect(()=>{alive.current=true;roster.current=api.agents().then(v=>{if(alive.current){setGroups(v.groups || []);names.current=new Set(v.agents.map(a=>a.name));}}).catch(()=>{});return()=>{alive.current=false;readVersion.current++;};},[]);
  const close=()=>{if(!inFlight.current)onClose();};
  const root=useRef<HTMLElement>(null);
  useDialogFocus(root,close);
  async function read(file:File){
    if(inFlight.current||created)return;
    const version=++readVersion.current;setError("");setTemplate(null);setReading(true);
    try{
      if(file.size>12*1024*1024)throw Error("分享檔案上限 12 MB");
      const data=validateTemplate(JSON.parse(await file.text()));
      await roster.current;
      if(version!==readVersion.current||!alive.current)return;
      let chosen=data.profile.name.slice(0,80),n=1;
      while(names.current.has(chosen)){n++;const suffix=` (${n})`;chosen=data.profile.name.slice(0,80-suffix.length)+suffix;}
      id.current=uuid();setTemplate(data);setName(chosen);
    }catch(e){if(version===readVersion.current&&alive.current)setError(e instanceof SyntaxError?"檔案不是有效的 JSON 分享檔案":e instanceof Error?e.message:"無法讀取檔案");}
    finally{if(version===readVersion.current&&alive.current)setReading(false);}
  }
  async function save(){if(!template||inFlight.current)return;inFlight.current=true;setBusy(true);setError("");try{const row=await api.importTemplate(id.current,template,name.trim(),group);if(alive.current){setCreated(row);onCreated?.();}}catch(e){if(alive.current)setError(e instanceof Error?e.message:"匯入失敗");}finally{inFlight.current=false;if(alive.current)setBusy(false);}}
  const requirements=template?Object.keys(template.mcp.mcpServers):[];
  return <div className="modal-backdrop" onClick={close}><section ref={root} className="dialog template-dialog" role="dialog" aria-modal="true" aria-label="匯入 Bot" onClick={e=>e.stopPropagation()}>
    <div className="dialog-title"><h2>{created?"Bot 已匯入":"匯入 Bot"}</h2><button type="button" aria-label="關閉" disabled={busy} onClick={close}><X/></button></div>
    {created?<>
      <p role="status">已建立「{created.name}」，帶入 {template?.skills.length || 0} 個技能。</p>
      {requirements.length?<><p className="template-note">有 {requirements.length} 個串接需要設定：{requirements.join("、")}。依賴這些串接的技能，設定後才能完整使用。</p><button type="button" className="outline" aria-expanded={configure} onClick={()=>setConfigure(v=>!v)}>{configure?"收合串接設定":"設定需要的串接"}</button>{configure?<McpSettings agentId={created.id} requiredServers={requirements}/>:null}</>:<p>可以直接開始對話，技能會依使用情境載入。</p>}
      <div className="dialog-actions"><button type="button" onClick={onClose}>稍後使用</button><button type="button" className="primary" onClick={()=>onImported(created)}>開始對話</button></div>
    </>:<>
      <input ref={input} hidden type="file" accept=".json,application/json" onChange={e=>{const f=e.target.files?.[0];e.target.value="";if(f)void read(f);}}/>
      <div className={`template-dropzone ${dragging?"dragging":""}`} onDragOver={e=>{e.preventDefault();if(!busy)setDragging(true);}} onDragLeave={()=>setDragging(false)} onDrop={e=>{e.preventDefault();setDragging(false);const f=e.dataTransfer.files[0];if(f)void read(f);}}>
        <button type="button" className="outline" disabled={busy} onClick={()=>input.current?.click()}>{template?"更換分享檔案":"選擇分享檔案"}</button><small>也可以拖曳 .lazyboy.json 到這裡</small>
      </div>
      {reading?<p role="status">讀取分享檔案…</p>:template?<><Contents template={template}/><label>Bot 名稱<input value={name} maxLength={80} disabled={busy} onChange={e=>setName(e.target.value)}/></label><label>放入分組<select value={group} disabled={busy} onChange={e=>setGroup(e.target.value)}><option value="">未分組</option>{groups.map(g=><option value={g.id} key={g.id}>{g.name}</option>)}</select></label><p className="template-note">建立獨立 Bot。需要的串接會先停用，補填設定後再啟用。</p></>:<p>選擇對方分享的檔案，先確認人設與技能，再建立 Bot。</p>}
      {error?<p className="pane-error" role="alert">{error}</p>:null}
      <div className="dialog-actions"><button type="button" disabled={busy} onClick={onClose}>取消</button><button type="button" className="primary" disabled={!template||!name.trim()||busy||reading} onClick={()=>void save()}>{busy?"匯入中…":"建立 Bot"}</button></div>
    </>}
  </section></div>;
}
