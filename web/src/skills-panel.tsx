import {useEffect,useRef,useState} from "react";
import {api,type AgentSkill} from "./api";

const blank={id:"",name:"",description:"",instructions:"",enabled:true};
export function SkillsPanel({agentId,onEditingChange}:{agentId:string;onEditingChange?:(editing:boolean)=>void}) {
  const [skills,setSkills]=useState<AgentSkill[]>([]),[draft,setDraft]=useState<typeof blank|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState(""),[loading,setLoading]=useState(true),[remove,setRemove]=useState(""),[notice,setNotice]=useState("");
  useEffect(()=>{onEditingChange?.(!!draft);},[draft,onEditingChange]);
  const input=useRef<HTMLInputElement>(null),version=useRef(0),alive=useRef(true),acting=useRef(false);
  async function load(){const request=++version.current;setLoading(true);try{const v=await api.skills(agentId);if(alive.current&&request===version.current){setSkills(v.skills || []);setError("");}}catch(e){if(alive.current&&request===version.current)setError(e instanceof Error?e.message:"技能載入失敗");}finally{if(alive.current&&request===version.current)setLoading(false);}}
  useEffect(()=>{alive.current=true;setDraft(null);setNotice("");void load();const refresh=(e:Event)=>{const detail=(e as CustomEvent<{agent:string;saved?:boolean;changed?:boolean;at?:string}>).detail;if(detail?.agent===agentId&&!acting.current&&(detail.saved||detail.changed||detail.at))void load();};window.addEventListener("lazyboy-skills-changed",refresh);return()=>{alive.current=false;version.current++;window.removeEventListener("lazyboy-skills-changed",refresh);};},[agentId]);
  async function act(body:Record<string,unknown>){
    if(acting.current)return;acting.current=true;version.current++;setBusy(true);setError("");setNotice("");
    try{const v=await api.skills(agentId,body);if(!alive.current)return;setSkills(v.skills);setDraft(null);setRemove("");setNotice(body.action==="save"?"技能已儲存，可以分享；實際使用仍需驗證。":body.action==="delete"?"技能已移除。":body.enabled?"技能已啟用。":"技能已停用，Bot 不會自動使用。");}
    catch(e){if(alive.current)setError(e instanceof Error?e.message:"技能儲存失敗");}
    finally{acting.current=false;if(alive.current){setBusy(false);setLoading(false);}}
  }
  return <section className="skills-panel" aria-label="技能">
    <div className="skills-heading"><strong>技能 · {skills.length}</strong><button type="button" disabled={busy} onClick={()=>{setDraft({...blank});setNotice("");}}>新增技能</button><button type="button" disabled={busy} onClick={()=>input.current?.click()}>匯入 SKILL.md</button></div>
    <p className="template-note">教他流程後，儲存成功會顯示技能提示，可在此確認與修訂。分享保留技能，私人記憶不會帶走。</p>
    <input ref={input} hidden type="file" accept=".md,.txt,text/markdown,text/plain" onChange={async e=>{
      const file=e.target.files?.[0];e.target.value="";if(!file)return;
      if(file.size>64000){setError("技能檔案最多 64 KB");return;}
      try{const text=await file.text();if(!alive.current)return;const name=text.match(/^name:\s*["']?([^\n"']+)/m)?.[1] || file.name.replace(/\.(md|txt)$/i,"");const description=text.match(/^description:\s*["']?([^\n"']+)/m)?.[1] || "";setDraft({...blank,name,description,instructions:text});setError("");}catch{if(alive.current)setError("無法讀取檔案");}
    }}/>
    {loading?<p role="status">載入技能…</p>:!skills.length&&!draft?<small>新增技能，或在對話裡教他一套流程。</small>:null}
    {!draft?skills.map(skill=><article className="skill-row" key={skill.id}>
      <div><strong>{skill.name}</strong><small>{skill.source==="learned"?"從對話學到":skill.source==="shared"?"由分享匯入":"自訂技能"} · {skill.enabled?"已啟用":"已停用"}</small><p>{skill.description}</p><details><summary>查看操作步驟</summary><pre>{skill.instructions}</pre></details></div>
      <div className="skill-actions"><button type="button" aria-label={`${skill.enabled?"停用":"啟用"} ${skill.name}`} disabled={busy} onClick={()=>void act({action:"toggle",id:skill.id,enabled:!skill.enabled})}>{skill.enabled?"停用":"啟用"}</button><button type="button" disabled={busy} onClick={()=>{setDraft(skill);setNotice("");}}>編輯</button><button type="button" disabled={busy} onClick={()=>setRemove(skill.id)}>移除</button></div>
      {remove===skill.id?<div className="skill-actions"><span>移除此技能？移除後不再自動使用。</span><button type="button" disabled={busy} onClick={()=>void act({action:"delete",id:skill.id})}>確定移除</button><button type="button" disabled={busy} onClick={()=>setRemove("")}>取消</button></div>:null}
    </article>):null}
    {draft?<div className="skill-editor">
      <label>技能名稱<input value={draft.name} maxLength={50} disabled={busy} onChange={e=>setDraft({...draft,name:e.target.value})}/></label>
      <label>什麼時候使用<textarea rows={2} value={draft.description} maxLength={650} disabled={busy} onChange={e=>setDraft({...draft,description:e.target.value})}/></label>
      <label>操作步驟<textarea rows={8} value={draft.instructions} maxLength={20000} disabled={busy} onChange={e=>setDraft({...draft,instructions:e.target.value})} placeholder="使用條件、操作步驟、完成後怎麼檢查，以及仍需確認的地方。"/></label>
      <div className="skill-actions"><button type="button" disabled={busy||!draft.name.trim()||!draft.description.trim()||!draft.instructions.trim()} onClick={()=>void act({action:"save",...draft})}>{busy?"儲存中…":"儲存技能"}</button><button type="button" disabled={busy} onClick={()=>setDraft(null)}>取消編輯</button></div>
    </div>:null}
    {notice?<p role="status">{notice}</p>:null}
    {error?<p className="pane-error" role="alert">{error}<button type="button" disabled={busy} onClick={()=>void load()}>重新載入</button></p>:null}
  </section>;
}
