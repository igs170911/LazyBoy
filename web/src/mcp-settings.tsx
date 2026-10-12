import {useEffect, useState} from "react";
import {api, type McpServer} from "./api";

const CATALOG = [
  {name:"GitHub", id:"github", description:"程式碼、議題與 Pull Request", url:"https://api.githubcopilot.com/mcp/", docs:"https://github.com/github/github-mcp-server", header:"Authorization", hint:"GitHub Personal Access Token"},
  {name:"Context7", id:"context7", description:"查詢套件的最新技術文件", url:"https://mcp.context7.com/mcp", docs:"https://github.com/upstash/context7", header:"CONTEXT7_API_KEY", hint:"Context7 API Key（選填）"},
];
export function McpSettings({agentId,requiredServers}: {agentId?: string | null; requiredServers?: string[]}) {
  const [servers,setServers] = useState<McpServer[]>([]);
  const [checks,setChecks] = useState<Record<string,McpServer>>({});
  const [busy,setBusy] = useState("");
  const [error,setError] = useState("");
  const [notice,setNotice] = useState("");
  const [scope,setScope] = useState("");
  const [extraHeaders,setExtraHeaders] = useState<Record<string,string>>({});
  const [form,setForm] = useState<McpServer | null>(null);
  const [editing,setEditing] = useState(false);
  const [token,setToken] = useState("");
  const [header,setHeader] = useState("Authorization");
  const [args,setArgs] = useState("");
  const [env,setEnv] = useState("");
  const [remove,setRemove] = useState("");
  async function load() { setBusy("load"); setError(""); try { const data=await api.mcp({action:"list",agent:agentId}); setServers(data.servers || []); setScope(data.scope || "shared"); } catch(e) { setError(String(e instanceof Error ? e.message : e)); } finally { setBusy(""); } }
  useEffect(() => { void load(); }, [agentId]);
  function edit(server: McpServer) { setEditing(servers.some(row => row.server === server.server)); setForm(server); setToken(""); setEnv((server.missing_fields || []).filter(k=>k.startsWith("env:")).map(k=>`${k.slice(4)}=`).join("\n")); setExtraHeaders({}); setArgs((server.args || []).join("\n")); setHeader(CATALOG.find(item => item.id === server.server)?.header || server.header_names?.find(key=>key.toLowerCase()==="authorization") || server.header_names?.[0] || "Authorization"); setError(""); setNotice(""); }
  async function act(action: string, server: string, extra: Record<string,unknown> = {}) {
    if (busy) return;
    setBusy(`${action}:${server}`); setError(""); setNotice("");
    try {
      const data=await api.mcp({action,server,agent:agentId,...extra});
      if(action === "test") setChecks(current => ({...current,[server]:data.servers[0]}));
      else { setServers(data.servers); setForm(null); setRemove(""); setToken(""); setEnv(""); setChecks(current => {const next={...current}; delete next[server]; return next;}); setNotice(action === "save" ? "已儲存，新任務可使用這個串接。" : "已移除串接。"); }
    } catch(e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(""); }
  }
  function save() {
    if (!form) return;
    if (!editing && servers.some(server => server.server === form.server.trim())) {
      setError("這個名稱已存在，請使用其他名稱或編輯已加入的服務。");
      return;
    }
    const variables: Record<string,string> = {};
    for (const line of env.split("\n").filter(line => line.trim())) {
      const eq=line.indexOf("="); if(eq < 1) {setError("環境變數請一行填寫 KEY=value");return;}
      variables[line.slice(0,eq).trim()]=line.slice(eq+1);
    }
    const headers:Record<string,string> = token.trim() ? {[header]:header.toLowerCase()==="authorization" && !token.startsWith("Bearer ") ? `Bearer ${token.trim()}` : token.trim()} : {};
    for (const [key,value] of Object.entries(extraHeaders)) if(value.trim())headers[key]=value.trim();
    void act("save",form.server.trim(),{enabled:form.enabled !== false,...(form.transport === "http" ? {url:form.url?.trim(),headers} : {command:form.command?.trim(),args:args.split("\n").filter(Boolean),env:variables})});
  }
  return <div className="mcp-settings lb-settings-stack">
    <p className="mcp-hint">{scope === "agent" ? "這個 Bot 的獨立串接，修改只影響這個 Bot。" : "串接外部服務，讓 Bot 直接使用工具。新增的服務預設共用；已有專案設定則保留原範圍。"}</p>
    {error ? <div role="alert" className="pane-error">{error}</div> : null}
    {notice ? <p role="status">{notice}</p> : null}
    <section><div className="mcp-section-title"><h3>{requiredServers?"此 Bot 需要的串接":"已加入"}</h3><button type="button" disabled={!!busy} onClick={() => void load()}>重新整理</button></div>
      {!servers.length ? <p className="mcp-hint">{busy === "load" ? "讀取中…" : requiredServers?"尚未取得需要的串接，請重新整理。":"還沒有串接，從下方選一個開始。"}</p> : servers.filter(server=>!requiredServers||requiredServers.includes(server.server)).map(server => {
        const check=checks[server.server];
        return <article className="mcp-card" key={server.server}>
          <div><strong>{server.server}</strong><small>{server.enabled === false ? "已停用 · 編輯後啟用" : check?.serverStatus === "connected" ? `連線成功 · ${check.tools?.length || 0} 個工具` : check?.serverStatus === "error" ? "連線失敗" : "已設定 · 尚未測試"}</small></div>
          <div className="mcp-actions"><button type="button" disabled={!!busy || server.enabled === false} onClick={() => void act("test",server.server)}>{busy === `test:${server.server}` ? "測試中…" : "測試連線"}</button><button type="button" disabled={!!busy} onClick={() => edit(server)}>編輯</button><button type="button" disabled={!!busy} onClick={() => setRemove(server.server)}>移除</button></div>
          {server.missing_fields?.length ? <small className="mcp-missing">待補填：{server.missing_fields.join("、").replaceAll("header:","").replaceAll("env:","")}</small> : null}
          {check?.serverError ? <p className="pane-error">{check.serverError}</p> : null}
          {check?.tools?.length ? <details><summary>查看工具</summary><ul>{check.tools.map(tool => <li key={tool.name}><strong>{tool.name}</strong>{tool.description ? <span> · {tool.description}</span> : null}</li>)}</ul></details> : null}
          {remove === server.server ? <div className="mcp-actions"><span>確定移除此串接？</span><button type="button" disabled={!!busy} onClick={() => void act("remove",server.server)}>確定移除</button><button type="button" onClick={() => setRemove("")}>取消</button></div> : null}
        </article>;
      })}
    </section>
    {!requiredServers?<><section><h3>可串接的服務</h3><div className="mcp-catalog">{CATALOG.map(item => <article className="mcp-card" key={item.id}><strong>{item.name}</strong><p>{item.description}</p><small>{item.hint}</small><div className="mcp-actions"><a href={item.docs} target="_blank" rel="noreferrer">串接說明 ↗</a><button type="button" disabled={!!busy} onClick={() => edit(servers.find(row => row.server === item.id) || {server:item.id,transport:"http",url:item.url})}>{servers.some(row => row.server === item.id) ? "編輯" : "加入"}</button></div></article>)}</div></section>
    <button type="button" className="outline" disabled={!!busy} onClick={() => edit({server:"",transport:"http",url:""})}>＋ 自訂 MCP 服務</button></>:null}
    {form ? <section className="mcp-editor" aria-label="編輯 MCP 串接">
      <h3>設定串接</h3><small>更換網址或啟動程式時，需重新填寫驗證資料。</small>
      <label>名稱<input disabled={editing || !!busy} value={form.server} maxLength={100} onChange={e => setForm({...form,server:e.target.value})} /></label>
      <label>連線方式<select value={form.transport} onChange={e => setForm({...form,transport:e.target.value})}><option value="http">遠端網址（HTTP）</option><option value="stdio">本機程式（stdio）</option></select></label>
      {form.transport === "http" ? <><label>服務網址<input type="url" value={form.url || ""} placeholder="https://example.com/mcp" onChange={e => setForm({...form,url:e.target.value})} /></label><label>驗證欄位<input value={header} onChange={e => setHeader(e.target.value)} /></label><label>Token / API Key<input type="password" autoComplete="new-password" value={token} onChange={e => setToken(e.target.value)} placeholder={form.header_names?.length ? "留白保留原設定" : "不需要驗證可留白"} /></label>{(form.header_names || []).filter(key=>key !== header).map(key=><label key={key}>{key}<input type="password" autoComplete="new-password" value={extraHeaders[key] || ""} onChange={e=>setExtraHeaders(current=>({...current,[key]:e.target.value}))} placeholder={form.missing_fields?.includes(`header:${key}`) ? "請補填" : "留白保留原設定"}/></label>)}<small>目前支援 Token 驗證；需要 OAuth 登入的服務請使用供應商提供的 Token 端點。</small></> : <><label>啟動程式<input value={form.command || ""} placeholder="npx" onChange={e => setForm({...form,command:e.target.value})} /></label><label>參數（每行一個）<textarea value={args} rows={3} onChange={e => setArgs(e.target.value)} placeholder={'-y\n套件名稱'} /></label><label>環境變數（KEY=value）<textarea value={env} rows={2} onChange={e => setEnv(e.target.value)} placeholder={form.env_names?.length ? `已設定 ${form.env_names.join("、")}，留白保留` : "需要時填寫"} /></label><small>按「測試連線」或 Bot 使用工具時，會啟動此程式。</small></>}
      <label className="mcp-enable"><input type="checkbox" checked={form.enabled !== false} onChange={e=>setForm({...form,enabled:e.target.checked})}/>啟用串接</label>
      <div className="mcp-actions"><button type="button" className="primary" disabled={!!busy || !form.server.trim() || !(form.transport === "http" ? form.url?.trim() : form.command?.trim())} onClick={save}>{busy.startsWith("save:") ? "儲存中…" : "儲存串接"}</button><button type="button" disabled={!!busy} onClick={() => {setForm(null);setToken("");setEnv("");}}>取消</button></div>
    </section> : null}
  </div>;
}
