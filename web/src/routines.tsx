import { useEffect, useRef, useState, type RefObject } from "react";
import { api, type Routine, type RoutineRun, type RoutineSchedule } from "./api";
import { X } from "./ui/icons";
import { ChatMessageBody, type PreviewFile } from "./markdown";

const frequencies = {interval:"每隔一段時間",once:"單次",daily:"每天",weekdays:"週一至週五",weekly:"每週",monthly:"每月",trigger:"只由網址觸發"};
function every(minutes?: number | null) {
  const value = minutes || 60;
  return value % 1440 === 0 ? `${value / 1440} 天` : value % 60 === 0 ? `${value / 60} 小時` : `${value} 分鐘`;
}
const days = ["一","二","三","四","五","六","日"];
function when(at: string | null | undefined, timezone: string) {
  if (!at) return "尚未安排";
  const date = new Date(at), zone = timezone || "Asia/Taipei";
  const part = (options: Intl.DateTimeFormatOptions) => {
    try { return new Intl.DateTimeFormat("zh-TW", { timeZone: zone, ...options }).format(date); }
    catch { return new Intl.DateTimeFormat("zh-TW", options).format(date); }
  };
  const sameYear = part({ year: "numeric" }) === new Intl.DateTimeFormat("zh-TW", { timeZone: zone, year: "numeric" }).format(new Date());
  return `${part(sameYear ? { month: "numeric", day: "numeric" } : { year: "numeric", month: "numeric", day: "numeric" })}（${part({ weekday: "short" })}）${part({ hour: "2-digit", minute: "2-digit", hourCycle: "h23" })}`;
}
function scheduleText(schedule: RoutineSchedule) {
  if (schedule.frequency === "trigger") return "只由網址觸發（其他服務呼叫時執行）";
  if (schedule.frequency === "interval") return `每 ${every(schedule.every_minutes)}（從 ${schedule.time || "00:00"} 起算）` + (schedule.timezone && schedule.timezone !== "Asia/Taipei" ? ` · ${schedule.timezone}` : "");
  const day = schedule.frequency === "weekly" ? (schedule.weekdays || []).map(day => `週${days[day-1]}`).join("、")
    : schedule.frequency === "monthly" ? (schedule.day_of_month ? `${schedule.day_of_month} 日` : "") : schedule.frequency === "once" ? schedule.date || "" : "";
  return [frequencies[schedule.frequency] || "每天", day, schedule.time || "09:00"].filter(Boolean).join(" ") + (schedule.timezone && schedule.timezone !== "Asia/Taipei" ? ` · ${schedule.timezone}` : "");
}
const RESULT: Record<string, string> = { done: "已完成", answer: "已完成", failed: "失敗", cancelled: "已取消", blocked: "需要協助" };
function runLabel(run: { state: string; verdict?: string | null }) {
  return run.state === "terminal" ? RESULT[run.verdict || ""] || "已結束" : run.state === "queued" ? "排隊中" : run.state.startsWith("waiting") ? "等待回覆" : "執行中";
}
function finishedOnce(item: Routine) { return !item.enabled && item.schedule.frequency === "once" && !!item.last_run_at; }
type Edit = { id?: string; title: string; instructions: string; schedule: RoutineSchedule; enabled: boolean; hook?: boolean };

/** A routine's trigger address, for Slack, GitHub or any service that can send a POST. */
function HookAddress({ path, busy, onRotate }: { path: string; busy: boolean; onRotate: () => void }) {
  const url = `${window.location.origin}${path}`;
  const [copied, setCopied] = useState(false);
  return <div className="routine-hook">
    <span>觸發網址</span>
    <code title={url}>{url}</code>
    <button type="button" disabled={busy} onClick={() => { void navigator.clipboard?.writeText(url).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1500); }); }}>{copied ? "已複製" : "複製"}</button>
    <button type="button" disabled={busy} onClick={onRotate} title="舊網址會立刻失效">換新網址</button>
    <small>用 POST 呼叫這個網址就會執行，送來的內容會當成資料交給 Bot；同一個任務每分鐘最多觸發一次。外部服務要能連到這台主機（同一個網路，或透過通道轉發）。</small>
  </div>;
}

export function RoutinesDialog({agentId,onClose,onOpen,returnFocus}:{agentId:string;onClose:()=>void;onOpen:(file:PreviewFile)=>void;returnFocus:RefObject<HTMLButtonElement|null>}) {
  const root = useRef<HTMLElement>(null);
  const [items,setItems] = useState<Routine[]>([]);
  const [loading,setLoading] = useState(true);
  const [error,setError] = useState("");
  const [loadError,setLoadError] = useState("");
  const loadVersion=useRef(0);
  const [busy,setBusy] = useState(false);
  const [notice,setNotice] = useState("");
  const [edit,setEdit] = useState<Edit | null>(null);
  const [history,setHistory] = useState<{id:string;runs:RoutineRun[]} | null>(null);
  const [deleting,setDeleting] = useState<string | null>(null);
  const mounted = useRef(true);
  const load = async () => {
    const version=++loadVersion.current;
    try { const result = await api.routines(agentId); if(mounted.current && loadVersion.current===version) { setItems(result.routines || []); setLoadError(""); } }
    catch(err) { if(mounted.current && loadVersion.current===version)setLoadError(err instanceof Error ? err.message : "無法載入定時任務"); }
    finally { if(mounted.current && loadVersion.current===version)setLoading(false); }
  };
  useEffect(() => {
    mounted.current = true;
    void load();
    const refresh = window.setInterval(() => { if(!document.hidden)void load(); },10000);
    const previous = document.activeElement as HTMLElement | null;
    root.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if(document.querySelector('.file-sheet'))return;
      if(event.key === "Escape"){event.stopPropagation();onClose();}
      if(event.key === "Tab") {
        const controls=Array.from(root.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href]') || []);
        const first=controls[0],last=controls[controls.length-1];
        if(!root.current?.contains(document.activeElement)){event.preventDefault();first?.focus();}
        else if(event.shiftKey && document.activeElement===first){event.preventDefault();last?.focus();}
        else if(!event.shiftKey && document.activeElement===last){event.preventDefault();first?.focus();}
      }
    };
    document.addEventListener('keydown',onKey);
    return () => { mounted.current=false; window.clearInterval(refresh); document.removeEventListener('keydown',onKey); (returnFocus.current || previous)?.focus(); };
  },[agentId]);
  const action = async (body: Record<string,unknown>) => {
    setBusy(true);setError("");setNotice("");
    try {
      const response = await api.manageRoutine(agentId,body);
      if(!mounted.current)return;
      if(body.action === "history")setHistory({id:String(body.id),runs:response.runs || []});
      else {
        if(body.action === "create" || body.action === "update")setEdit(null);
        if(body.action === "delete"){setDeleting(null);setHistory(null);}
        setNotice(body.action === "run" ? "已開始執行，結果會回到主要對話。" : body.action === "delete" ? "已刪除" : body.action === "pause" ? "已暫停" : body.action === "resume" ? "已啟用" : body.action === "rotate_hook" ? "已換新網址，舊網址失效" : "已儲存");
        await load();
      }
    } catch(err) { if(mounted.current)setError(err instanceof Error ? err.message : "操作失敗，請重試"); }
    finally { if(mounted.current)setBusy(false); }
  };
  const changeSchedule = (patch: Partial<RoutineSchedule>) => setEdit(current => {
    if (!current) return null;
    const next = { ...current.schedule, ...patch };
    const today = new Date();
    // Show exactly what will be saved when switching to a frequency that needs a day.
    if (next.frequency === "weekly" && !next.weekdays?.length) next.weekdays = [today.getDay() || 7];
    if (next.frequency === "monthly" && !next.day_of_month) next.day_of_month = today.getDate();
    if (next.frequency === "once" && !next.date) next.date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
    if (next.frequency === "interval" && !next.every_minutes) next.every_minutes = 60;
    return { ...current, schedule: next };
  });
  return <div className="routines-backdrop" onClick={event => {if(event.target===event.currentTarget)onClose();}}>
    <section className="routines-dialog" ref={root} role="dialog" aria-modal="true" aria-label="定時任務">
      <header><h2>定時任務</h2><button type="button" aria-label="關閉定時任務" onClick={onClose}><X /></button></header>
      <p className="routine-hint">也可以直接說：「每天早上九點幫我整理新聞」。執行結果會出現在主要對話。</p>
      <p className="routine-hint">本機服務需保持運作；休眠或關機時無法執行，恢復後合併補跑一次。</p>
      {error || loadError ? <div role="alert" className="pane-error">{error || loadError}</div> : null}
      {notice ? <p role="status">{notice}</p> : null}
      {edit ? <form className="routine-form" onSubmit={event => {event.preventDefault();void action({...edit,action:edit.id?"update":"create"});}}>
        <label>任務名稱<input required maxLength={80} value={edit.title} onChange={event=>setEdit({...edit,title:event.target.value})} /></label>
        <label>要做什麼<textarea required maxLength={16000} rows={3} value={edit.instructions} placeholder="整理今天的 AI 新聞，附上來源，保留最重要的三則。" onChange={event=>setEdit({...edit,instructions:event.target.value})} /></label>
        <div className="routine-time-grid">
          <label>重複<select aria-label="重複" value={edit.schedule.frequency} onChange={event=>changeSchedule({frequency:event.target.value as RoutineSchedule["frequency"]})}>{Object.entries(frequencies).map(([value,label])=><option value={value} key={value}>{label}</option>)}</select></label>
          {edit.schedule.frequency === "trigger" ? null : <label>{edit.schedule.frequency === "interval" ? "起算時間" : "時間"}<input required type="time" value={edit.schedule.time} onChange={event=>changeSchedule({time:event.target.value})} /></label>}
        </div>
        {edit.schedule.frequency === "interval" ? (() => {
          const minutes = edit.schedule.every_minutes || 60;
          const unit = minutes % 60 === 0 ? 60 : 1;
          return <div className="routine-time-grid">
            <label>每隔<input required type="number" min={unit === 60 ? 1 : 5} max={unit === 60 ? 168 : 10080} value={minutes / unit} onChange={event=>changeSchedule({every_minutes:Math.max(5, Math.round(Number(event.target.value) || 0) * unit)})} /></label>
            <label>單位<select aria-label="單位" value={unit} onChange={event=>{const next = Number(event.target.value); changeSchedule({every_minutes: next === 60 ? Math.max(60, Math.round(minutes / 60) * 60) : minutes});}}><option value={1}>分鐘</option><option value={60}>小時</option></select></label>
            <small className="routine-hint">最短 5 分鐘。每次都會用到模型，間隔越短花費越多。</small>
          </div>;
        })() : null}
        {edit.schedule.frequency === "once" ? <label>日期<input required type="date" value={edit.schedule.date || ""} onChange={event=>changeSchedule({date:event.target.value})} /></label> : null}
        {edit.schedule.frequency === "weekly" ? <fieldset><legend>星期</legend><div className="routine-weekdays">{days.map((day,index)=><label key={day}><input type="checkbox" checked={edit.schedule.weekdays?.includes(index+1)||false} onChange={event=>changeSchedule({weekdays:event.target.checked?[...(edit.schedule.weekdays||[]),index+1].sort():(edit.schedule.weekdays||[]).filter(value=>value!==index+1)})} />{day}</label>)}</div></fieldset> : null}
        {edit.schedule.frequency === "monthly" ? <label>每月幾號<input required type="number" min={1} max={31} value={edit.schedule.day_of_month || 1} onChange={event=>changeSchedule({day_of_month:Number(event.target.value)})} /><small>沒有該日期的月份會跳過。</small></label> : null}
        {edit.schedule.frequency === "trigger" ? <p className="routine-hint">儲存後會產生一個網址，其他服務（Slack、GitHub、表單…）呼叫它時執行。</p>
          : <label className="routine-check"><input type="checkbox" checked={!!edit.hook} onChange={event=>setEdit({...edit,hook:event.target.checked})} />也可以用網址觸發（給 Slack、GitHub 等服務呼叫）</label>}
        <label>時區<input required value={edit.schedule.timezone} list="routine-timezones" onChange={event=>changeSchedule({timezone:event.target.value})} /><datalist id="routine-timezones"><option value="Asia/Taipei"/><option value="Asia/Tokyo"/><option value="America/New_York"/><option value="Europe/London"/><option value="UTC"/></datalist></label>
        <div className="routine-actions"><button type="submit" className="routine-primary" disabled={busy}>{busy?"儲存中…":"儲存任務"}</button><button type="button" disabled={busy} onClick={()=>setEdit(null)}>取消編輯</button></div>
      </form> : <>
        <button type="button" className="routine-new" disabled={busy} onClick={()=>{setError("");setNotice("");setEdit({title:"",instructions:"",enabled:true,schedule:{frequency:"daily",time:"09:00",timezone:"Asia/Taipei",weekdays:[1],day_of_month:1}});}}>＋ 新增定時任務</button>
        {loading ? <p role="status">載入中…</p> : items.length===0 ? <p className="routine-empty">還沒有定時任務。先安排一件你經常需要的事。</p> : null}
        {items.map(item=><article className="routine-item" key={item.id}>
          <div className="routine-item-head"><strong>{item.title}</strong><span>{item.enabled?"已啟用":finishedOnce(item)?"已執行":"已暫停"}</span></div>
          <p>{scheduleText(item.schedule)}</p>
          {item.enabled && item.schedule.frequency !== "trigger" ? <p className="routine-hint">下次：{when(item.next_run_at,item.schedule.timezone)}</p> : null}
          {item.hook_path ? <HookAddress path={item.hook_path} busy={busy} onRotate={()=>void action({action:"rotate_hook",id:item.id})} /> : null}
          {item.last_run ? <p className="routine-hint">上次：{when(item.last_run.at,item.schedule.timezone)} · {runLabel(item.last_run)}</p> : null}
          {item.last_error ? <p role="alert">無法開始執行：{item.last_error}{item.retry_at ? `，將於 ${when(item.retry_at,item.schedule.timezone)} 重試` : ""}</p> : null}
          <div className="routine-actions">
            {finishedOnce(item) ? null : <button type="button" disabled={busy} onClick={()=>void action({action:item.enabled?"pause":"resume",id:item.id})}>{item.enabled?"暫停":"啟用"}</button>}
            <button type="button" disabled={busy} onClick={()=>{setError("");setEdit({id:item.id,title:item.title,instructions:item.instructions,schedule:{...item.schedule,timezone:item.schedule.timezone||"Asia/Taipei",time:item.schedule.time||"09:00"},enabled:finishedOnce(item)||item.enabled,hook:!!item.hook_path});}}>編輯</button>
            <button type="button" disabled={busy} onClick={()=>void action({action:"run",id:item.id})}>立即執行</button>
            <button type="button" disabled={busy} aria-expanded={history?.id===item.id} onClick={()=>history?.id===item.id?setHistory(null):void action({action:"history",id:item.id})}>執行紀錄</button>
            <button type="button" disabled={busy} onClick={()=>setDeleting(item.id)}>刪除</button>
          </div>
          {deleting===item.id ? <div className="routine-delete"><p>刪除此排程？已開始的工作會繼續執行。</p><button type="button" disabled={busy} onClick={()=>void action({action:"delete",id:item.id})}>確定刪除</button><button type="button" onClick={()=>setDeleting(null)}>保留</button></div> : null}
          {history?.id===item.id ? <div className="routine-history">{history.runs.length===0?<p>尚無執行紀錄</p>:history.runs.map(run=><div key={run.task_id}><strong>{when(run.started_at,item.schedule.timezone)} · {runLabel(run)}</strong>{run.result?.summary?<ChatMessageBody content={run.result.summary} agentId={agentId} knownPaths={[]} onOpen={onOpen}/>:null}</div>)}</div> : null}
        </article>)}
      </>}
    </section>
  </div>;
}
