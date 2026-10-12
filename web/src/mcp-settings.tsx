import {useEffect, useRef, useState} from "react";
import {api, type AgentRow, type McpServer} from "./api";
import { ago } from "./workspace/format";

const CATALOG = [
  {name:"GitHub", id:"github", description:"程式碼、議題與 Pull Request", url:"https://api.githubcopilot.com/mcp/", docs:"https://github.com/github/github-mcp-server", header:"Authorization", hint:"GitHub Personal Access Token"},
  {name:"Context7", id:"context7", description:"查詢套件的最新技術文件", url:"https://mcp.context7.com/mcp", docs:"https://github.com/upstash/context7", header:"CONTEXT7_API_KEY", hint:"Context7 API Key（選填）"},
];
const SHARED = "shared";
/** A remembered result older than this is re-checked in the background when the settings open. */
const STALE_MS = 6 * 3600_000;
const keyOf = (server: McpServer) => `${server.scope || SHARED}:${server.server}`;

type Form = McpServer & { scope: string };

/**
 * Connectors the bots can use. With `everywhere`, one list shows the shared connectors and every bot's own, each
 * marked with whom it belongs to — the same list whichever bot happens to be open. Without it, only `agentId`'s.
 */
export function McpSettings({agentId,requiredServers,everywhere = false}: {agentId?: string | null; requiredServers?: string[]; everywhere?: boolean}) {
  const [servers,setServers] = useState<McpServer[]>([]);
  const [agents,setAgents] = useState<AgentRow[]>([]);
  const [testing,setTesting] = useState<Record<string,boolean>>({});
  const [busy,setBusy] = useState("");
  const [error,setError] = useState("");
  const [notice,setNotice] = useState("");
  const [scope,setScope] = useState("");
  const [extraHeaders,setExtraHeaders] = useState<Record<string,string>>({});
  const [form,setForm] = useState<Form | null>(null);
  const [editing,setEditing] = useState(false);
  const [token,setToken] = useState("");
  const [header,setHeader] = useState("Authorization");
  const [args,setArgs] = useState("");
  const [env,setEnv] = useState("");
  const [remove,setRemove] = useState("");
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);
  const ownerOf = (scopeId: string) => scopeId === SHARED ? undefined : scopeId;
  const listBody = (scopeId?: string) => everywhere ? {agent: ownerOf(scopeId || SHARED), all: true} : {agent: agentId};

  async function load() {
    setBusy("load"); setError("");
    try {
      const data = await api.mcp({action:"list",...listBody()});
      if (!alive.current) return;
      setServers(data.servers || []); setScope(data.scope || SHARED);
      void autoTest(data.servers || []);
    } catch(e) { setError(String(e instanceof Error ? e.message : e)); }
    finally { setBusy(""); }
  }
  useEffect(() => { void load(); }, [agentId, everywhere]);
  useEffect(() => { if (everywhere) void api.agents().then(data => alive.current && setAgents(data.agents)).catch(() => {}); }, [everywhere]);

  /** Quietly re-check connectors never tested or tested long ago, only on computers that are already running. */
  async function autoTest(rows: McpServer[]) {
    for (const row of rows) {
      if (!alive.current) return;
      if (row.enabled === false || row.missing_fields?.length) continue;
      const at = row.last_test ? Date.parse(row.last_test.at) : 0;
      if (Date.now() - at < STALE_MS) continue;
      await test(row, true);
    }
  }

  async function test(row: McpServer, auto = false) {
    const key = keyOf(row);
    setTesting(current => ({...current,[key]:true}));
    if (!auto) { setError(""); setNotice(""); }
    try {
      const data = await api.mcp({action:"test",server:row.server,...listBody(row.scope),auto}, 120000);
      if (!alive.current || data.skipped) return;
      setServers(current => {
        const fresh = new Map((data.servers || []).map(item => [keyOf(item), item]));
        return everywhere ? data.servers : current.map(item => fresh.get(keyOf(item)) || item);
      });
    } catch(e) { if (!auto && alive.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (alive.current) setTesting(current => { const next = {...current}; delete next[key]; return next; }); }
  }

  function edit(server: McpServer) {
    const exists = servers.some(row => keyOf(row) === keyOf(server));
    setEditing(exists); setForm({...server, scope: server.scope || (everywhere ? SHARED : agentId || SHARED)}); setToken("");
    setEnv((server.missing_fields || []).filter(k=>k.startsWith("env:")).map(k=>`${k.slice(4)}=`).join("\n"));
    setExtraHeaders({}); setArgs((server.args || []).join("\n"));
    setHeader(CATALOG.find(item => item.id === server.server)?.header || server.header_names?.find(key=>key.toLowerCase()==="authorization") || server.header_names?.[0] || "Authorization");
    setError(""); setNotice("");
  }
  async function act(action: string, row: McpServer, extra: Record<string,unknown> = {}) {
    if (busy) return;
    setBusy(`${action}:${keyOf(row)}`); setError(""); setNotice("");
    try {
      const data=await api.mcp({action,server:row.server,...(everywhere ? listBody(row.scope) : {agent:agentId}),...extra});
      setServers(data.servers); setForm(null); setRemove(""); setToken(""); setEnv("");
      setNotice(action === "save" ? "已儲存，新任務可使用這個串接。" : "已移除串接。");
      if (action === "save") { const saved = data.servers.find(item => keyOf(item) === keyOf(row)); if (saved && saved.enabled !== false) void test(saved); }
    } catch(e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(""); }
  }
  function save() {
    if (!form) return;
    const row = {...form, server: form.server.trim()};
    if (!editing && servers.some(server => keyOf(server) === keyOf(row))) {
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
    void act("save",row,{enabled:form.enabled !== false,...(form.transport === "http" ? {url:form.url?.trim(),headers} : {command:form.command?.trim(),args:args.split("\n").filter(Boolean),env:variables})});
  }
  function status(server: McpServer) {
    if (server.enabled === false) return "已停用 · 編輯後啟用";
    if (testing[keyOf(server)]) return "測試中…（電腦剛開機時可能要一分鐘）";
    const last = server.last_test;
    if (!last) return "已設定 · 尚未測試";
    return last.ok ? `連線成功 · ${last.tools?.length || 0} 個工具 · ${ago(last.at)}` : `連線失敗 · ${ago(last.at)}`;
  }
  const shown = servers.filter(server=>!requiredServers||requiredServers.includes(server.server));
  const hint = everywhere ? "串接外部服務，讓 Bot 直接使用工具。「共用」的串接所有 Bot 都能用，也可以只給某個 Bot。" : scope === "agent" ? "這個 Bot 的獨立串接，修改只影響這個 Bot。" : "串接外部服務，讓 Bot 直接使用工具。";
  return <div className="mcp-settings lb-settings-stack">
    <p className="mcp-hint">{hint}</p>
    {error ? <div role="alert" className="pane-error">{error}</div> : null}
    {notice ? <p role="status">{notice}</p> : null}
    <section><div className="mcp-section-title"><h3>{requiredServers?"此 Bot 需要的串接":"已加入"}</h3><button type="button" disabled={!!busy} onClick={() => void load()}>重新整理</button></div>
      {!shown.length ? <p className="mcp-hint">{busy === "load" ? "讀取中…" : requiredServers?"尚未取得需要的串接，請重新整理。":"還沒有串接，從下方選一個開始。"}</p> : shown.map(server => {
        const key = keyOf(server), last = server.last_test, isTesting = !!testing[key];
        const tone = server.enabled === false ? "off" : isTesting ? "testing" : !last ? "unknown" : last.ok ? "ok" : "error";
        return <article className="mcp-card" key={key} data-tone={tone}>
          <div><strong>{server.server}{everywhere ? <span className="mcp-scope">{server.scope === SHARED ? "共用" : `只給 ${server.scope_name}`}</span> : null}</strong><small className="mcp-status"><i aria-hidden="true" />{status(server)}</small></div>
          <div className="mcp-actions"><button type="button" disabled={!!busy || isTesting || server.enabled === false} onClick={() => void test(server)}>{isTesting ? "測試中…" : "測試連線"}</button><button type="button" disabled={!!busy} onClick={() => edit(server)}>編輯</button><button type="button" disabled={!!busy} onClick={() => setRemove(key)}>移除</button></div>
          {server.missing_fields?.length ? <small className="mcp-missing">待補填：{server.missing_fields.join("、").replaceAll("header:","").replaceAll("env:","")}</small> : null}
          {!isTesting && last && !last.ok && last.error ? <p className="pane-error">{last.error}</p> : null}
          {last?.ok && last.tools?.length ? <details><summary>查看工具</summary><ul>{last.tools.map(tool => <li key={tool.name}><strong>{tool.name}</strong>{tool.description ? <span> · {tool.description}</span> : null}</li>)}</ul></details> : null}
          {remove === key ? <div className="mcp-actions"><span>確定移除此串接？</span><button type="button" disabled={!!busy} onClick={() => void act("remove",server)}>確定移除</button><button type="button" onClick={() => setRemove("")}>取消</button></div> : null}
        </article>;
      })}
    </section>
    {!requiredServers?<><section><h3>可串接的服務</h3><div className="mcp-catalog">{CATALOG.map(item => {
      const existing = servers.find(row => row.server === item.id);
      return <article className="mcp-card" key={item.id}><strong>{item.name}</strong><p>{item.description}</p><small>{item.hint}</small><div className="mcp-actions"><a href={item.docs} target="_blank" rel="noreferrer">串接說明 ↗</a><button type="button" disabled={!!busy} onClick={() => edit(existing || {server:item.id,transport:"http",url:item.url})}>{existing ? "編輯" : "加入"}</button></div></article>;
    })}</div></section>
    <button type="button" className="outline" disabled={!!busy} onClick={() => edit({server:"",transport:"http",url:""})}>＋ 自訂 MCP 服務</button></>:null}
    {form ? <section className="mcp-editor" aria-label="編輯 MCP 串接">
      <h3>設定串接</h3><small>更換網址或啟動程式時，需重新填寫驗證資料。</small>
      <label>名稱<input disabled={editing || !!busy} value={form.server} maxLength={100} onChange={e => setForm({...form,server:e.target.value})} /></label>
      {everywhere ? <label>給誰用<select disabled={editing || !!busy} value={form.scope} onChange={e => setForm({...form,scope:e.target.value})}><option value={SHARED}>所有 Bot 共用</option>{agents.map(agent => <option key={agent.id} value={agent.id}>只給 {agent.name}</option>)}</select></label> : null}
      <label>連線方式<select value={form.transport} onChange={e => setForm({...form,transport:e.target.value})}><option value="http">遠端網址（HTTP）</option><option value="stdio">本機程式（stdio）</option></select></label>
      {form.transport === "http" ? <><label>服務網址<input type="url" value={form.url || ""} placeholder="https://example.com/mcp" onChange={e => setForm({...form,url:e.target.value})} /></label><label>驗證欄位<input value={header} onChange={e => setHeader(e.target.value)} /></label><label>Token / API Key<input type="password" autoComplete="new-password" value={token} onChange={e => setToken(e.target.value)} placeholder={form.header_names?.length ? "留白保留原設定" : "不需要驗證可留白"} /></label>{(form.header_names || []).filter(key=>key !== header).map(key=><label key={key}>{key}<input type="password" autoComplete="new-password" value={extraHeaders[key] || ""} onChange={e=>setExtraHeaders(current=>({...current,[key]:e.target.value}))} placeholder={form.missing_fields?.includes(`header:${key}`) ? "請補填" : "留白保留原設定"}/></label>)}<small>目前支援 Token 驗證；需要 OAuth 登入的服務請使用供應商提供的 Token 端點。</small></> : <><label>啟動程式<input value={form.command || ""} placeholder="npx" onChange={e => setForm({...form,command:e.target.value})} /></label><label>參數（每行一個）<textarea value={args} rows={3} onChange={e => setArgs(e.target.value)} placeholder={'-y\n套件名稱'} /></label><label>環境變數（KEY=value）<textarea value={env} rows={2} onChange={e => setEnv(e.target.value)} placeholder={form.env_names?.length ? `已設定 ${form.env_names.join("、")}，留白保留` : "需要時填寫"} /></label><small>按「測試連線」或 Bot 使用工具時，會啟動此程式。</small></>}
      <label className="mcp-enable"><input type="checkbox" checked={form.enabled !== false} onChange={e=>setForm({...form,enabled:e.target.checked})}/>啟用串接</label>
      <div className="mcp-actions"><button type="button" className="primary" disabled={!!busy || !form.server.trim() || !(form.transport === "http" ? form.url?.trim() : form.command?.trim())} onClick={save}>{busy.startsWith("save:") ? "儲存中…" : "儲存串接"}</button><button type="button" disabled={!!busy} onClick={() => {setForm(null);setToken("");setEnv("");}}>取消</button></div>
    </section> : null}
  </div>;
}
