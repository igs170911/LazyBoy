import {useEffect,useMemo,useRef,useState} from "react";
import {api,type AgentSkill} from "./api";

const blank={id:"",name:"",description:"",instructions:"",enabled:true};
const SOURCE:Record<string,string>={builtin:"Hermes 內建",product:"LazyBoy 內建",learned:"從對話學到",shared:"由分享匯入"};
const PLACES:{key:NonNullable<AgentSkill["place"]>;title:string;note:string}[]=[
  {key:"list",title:"技能清單",note:"每次對話都會帶上名稱與用途，Bot 一看就知道有這些技能。"},
  {key:"library",title:"技能庫",note:"平常不佔用；Bot 需要時自己搜尋載入，用過會回到技能清單。30 天沒用的內建技能會自動放進來。"},
  {key:"off",title:"已停用",note:"Bot 不會使用。"},
];
// Roughly what one listed skill costs in every request: its name and summary (about 3 characters per token).
const listTokens=(skills:AgentSkill[])=>Math.round(skills.reduce((sum,s)=>sum+s.name.length+s.description.length+8,0)/3);
const COMMON=new Set(["use","when","the","and","with","for","via","from","into","your","this","that","how"]);
const words=(text:string)=>new Set(text.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(w=>w.length>1&&!COMMON.has(w)));
const overlap=(x:Set<string>,y:Set<string>)=>{const shared=[...x].filter(w=>y.has(w)).length;return {shared,ratio:shared/(x.size+y.size-shared||1)};};
/** Skills the bot wrote for the same job (mostly the same name words or the same summary): worth merging into one. */
function lookAlikes(skills:AgentSkill[]){
  const own=skills.filter(s=>s.source!=="builtin"&&s.source!=="product"&&s.place!=="off");
  const parent=own.map((_,i)=>i),root=(i:number):number=>parent[i]===i?i:(parent[i]=root(parent[i]));
  own.forEach((a,i)=>own.forEach((b,j)=>{
    if(j<=i)return;const name=overlap(words(a.name),words(b.name)),about=overlap(words(a.description),words(b.description));
    if(name.ratio>=.5||(about.shared>=3&&about.ratio>=.5))parent[root(j)]=root(i);
  }));
  const groups=new Map<number,AgentSkill[]>();own.forEach((s,i)=>groups.set(root(i),[...(groups.get(root(i))||[]),s]));
  return [...groups.values()].filter(g=>g.length>1);
}

export function SkillsPanel({agentId,onEditingChange}:{agentId:string;onEditingChange?:(editing:boolean)=>void}) {
  const [skills,setSkills]=useState<AgentSkill[]>([]),[draft,setDraft]=useState<typeof blank|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState(""),[loading,setLoading]=useState(true),[remove,setRemove]=useState(""),[notice,setNotice]=useState("");
  useEffect(()=>{onEditingChange?.(!!draft);},[draft,onEditingChange]);
  const input=useRef<HTMLInputElement>(null),version=useRef(0),alive=useRef(true),acting=useRef(false);
  async function load(){const request=++version.current;setLoading(true);try{const v=await api.skills(agentId);if(alive.current&&request===version.current){setSkills(v.skills || []);setError("");}}catch(e){if(alive.current&&request===version.current)setError(e instanceof Error?e.message:"技能載入失敗");}finally{if(alive.current&&request===version.current)setLoading(false);}}
  useEffect(()=>{alive.current=true;setDraft(null);setNotice("");void load();const refresh=(e:Event)=>{const detail=(e as CustomEvent<{agent:string;saved?:boolean;changed?:boolean;at?:string}>).detail;if(detail?.agent===agentId&&!acting.current&&(detail.saved||detail.changed||detail.at))void load();};window.addEventListener("lazyboy-skills-changed",refresh);return()=>{alive.current=false;version.current++;window.removeEventListener("lazyboy-skills-changed",refresh);};},[agentId]);
  async function act(body:Record<string,unknown>,done:string){
    if(acting.current)return;acting.current=true;version.current++;setBusy(true);setError("");setNotice("");
    try{const v=await api.skills(agentId,body);if(!alive.current)return;setSkills(v.skills);setDraft(null);setRemove("");setNotice(done);}
    catch(e){if(alive.current)setError(e instanceof Error?e.message:"技能儲存失敗");}
    finally{acting.current=false;if(alive.current){setBusy(false);setLoading(false);}}
  }
  async function merge(group:AgentSkill[]){
    if(acting.current)return;acting.current=true;setBusy(true);setError("");setNotice("");
    const names=group.map(s=>s.name).join("、");
    try{await api.send(agentId,`技能清單裡這幾個技能在做同一件事：${names}。請合併成一個：保留最新、實際驗證過的做法，刪掉過時或互相矛盾的步驟，名稱用最清楚的那個；合併好之後把其他幾個刪掉。這次只整理技能，不要做任何對外的動作。`);if(alive.current)setNotice("已請 Bot 合併，可以在對話裡看進度。");}
    catch(e){if(alive.current)setError(e instanceof Error?e.message:"無法交給 Bot");}
    finally{acting.current=false;if(alive.current)setBusy(false);}
  }
  const placed=(key:string)=>skills.filter(s=>(s.place||(s.enabled?"list":"off"))===key);
  const similar=useMemo(()=>lookAlikes(skills),[skills]);
  const row=(skill:AgentSkill)=>{
    const place=skill.place||(skill.enabled?"list":"off");
    return <article className="skill-row" key={skill.id}>
      <div><strong>{skill.name}</strong><small>{SOURCE[skill.source||""]||"自訂技能"}</small><p>{skill.description}</p><details><summary>查看操作步驟</summary><pre>{skill.instructions}</pre></details></div>
      <div className="skill-actions">
        {place==="list"?<button type="button" disabled={busy} onClick={()=>void act({action:"place",id:skill.id,place:"library"},"已放進技能庫，需要時 Bot 會自己載入。")}>放進技能庫</button>:null}
        {place==="library"?<button type="button" disabled={busy} onClick={()=>void act({action:"place",id:skill.id,place:"list"},"已放回技能清單。")}>放回技能清單</button>:null}
        <button type="button" aria-label={`${place==="off"?"啟用":"停用"} ${skill.name}`} disabled={busy} onClick={()=>void act({action:"toggle",id:skill.id,enabled:place==="off"},place==="off"?"技能已啟用。":"技能已停用，Bot 不會使用。")}>{place==="off"?"啟用":"停用"}</button>
        <button type="button" disabled={busy} onClick={()=>{setDraft(skill);setNotice("");}}>編輯</button>
        <button type="button" disabled={busy} onClick={()=>setRemove(skill.id)}>移除</button>
      </div>
      {remove===skill.id?<div className="skill-actions"><span>移除此技能？移除後不再自動使用。</span><button type="button" disabled={busy} onClick={()=>void act({action:"delete",id:skill.id},"技能已移除。")}>確定移除</button><button type="button" disabled={busy} onClick={()=>setRemove("")}>取消</button></div>:null}
    </article>;
  };
  return <section className="skills-panel" aria-label="技能">
    <div className="skills-heading"><strong>技能 · {skills.length}</strong><button type="button" disabled={busy} onClick={()=>{setDraft({...blank});setNotice("");}}>新增技能</button><button type="button" disabled={busy} onClick={()=>input.current?.click()}>匯入 SKILL.md</button></div>
    <p className="template-note">教他流程後，儲存成功會顯示技能提示，可在此確認與修訂。分享保留技能，私人記憶不會帶走。</p>
    {skills.length?<p className="skills-cost" role="note">技能清單 {placed("list").length} 個，每次對話約佔 {listTokens(placed("list")).toLocaleString()} token；技能庫 {placed("library").length} 個，需要時才載入，平常不佔用。</p>:null}
    <input ref={input} hidden type="file" accept=".md,.txt,text/markdown,text/plain" onChange={async e=>{
      const file=e.target.files?.[0];e.target.value="";if(!file)return;
      if(file.size>64000){setError("技能檔案最多 64 KB");return;}
      try{const text=await file.text();if(!alive.current)return;const name=text.match(/^name:\s*["']?([^\n"']+)/m)?.[1] || file.name.replace(/\.(md|txt)$/i,"");const description=text.match(/^description:\s*["']?([^\n"']+)/m)?.[1] || "";setDraft({...blank,name,description,instructions:text});setError("");}catch{if(alive.current)setError("無法讀取檔案");}
    }}/>
    {loading&&!skills.length?<p role="status">載入技能…</p>:!skills.length&&!draft?<small>新增技能，或在對話裡教他一套流程。</small>:null}
    {!draft?similar.map(group=><div className="skill-similar" role="note" key={group.map(s=>s.id).join()}>
      <span>這幾個技能在做同一件事：{group.map(s=>s.name).join("、")}。合併成一個可以少佔 token，也避免做法互相矛盾。</span>
      <button type="button" disabled={busy} onClick={()=>void merge(group)}>請 Bot 合併</button>
    </div>):null}
    {!draft?PLACES.map(({key,title,note})=>{
      const items=placed(key);if(!items.length)return null;
      const body=<><p className="template-note">{note}</p>{items.map(row)}</>;
      return key==="list"?<section className="skill-place" key={key} aria-label={title}><h4>{title} · {items.length}</h4>{body}</section>
        :<details className="skill-place" key={key}><summary><h4>{title} · {items.length}</h4></summary>{body}</details>;
    }):null}
    {draft?<div className="skill-editor">
      <label>技能名稱<input value={draft.name} maxLength={50} disabled={busy} onChange={e=>setDraft({...draft,name:e.target.value})}/></label>
      <label>什麼時候使用<textarea rows={2} value={draft.description} maxLength={650} disabled={busy} onChange={e=>setDraft({...draft,description:e.target.value})}/></label>
      <label>操作步驟<textarea rows={8} value={draft.instructions} maxLength={20000} disabled={busy} onChange={e=>setDraft({...draft,instructions:e.target.value})} placeholder="使用條件、操作步驟、完成後怎麼檢查，以及仍需確認的地方。"/></label>
      <div className="skill-actions"><button type="button" disabled={busy||!draft.name.trim()||!draft.description.trim()||!draft.instructions.trim()} onClick={()=>void act({action:"save",...draft},"技能已儲存，可以分享；實際使用仍需驗證。")}>{busy?"儲存中…":"儲存技能"}</button><button type="button" disabled={busy} onClick={()=>setDraft(null)}>取消編輯</button></div>
    </div>:null}
    {notice?<p role="status">{notice}</p>:null}
    {error?<p className="pane-error" role="alert">{error}<button type="button" disabled={busy} onClick={()=>void load()}>重新載入</button></p>:null}
  </section>;
}
