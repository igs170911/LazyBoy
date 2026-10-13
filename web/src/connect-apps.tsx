import { useEffect, useMemo, useRef, useState } from "react";
import { api, type ConnectorApp } from "./api";
import { McpSettings } from "./mcp-settings";
import { ExternalLink, LoaderCircle, Search } from "./ui/icons";
import { Modal, ModalHeader, toast } from "./ui/overlay";
import { viewerUrl } from "./workspace/ComputerPane";
import { ago } from "./workspace/format";

const FEATURED = "推薦";
const ALL = "全部";
/** A remembered check older than this is redone quietly when the gallery opens. */
const STALE_MS = 6 * 3600_000;
const message = (err: unknown) => err instanceof Error ? err.message : String(err);

/** The app's own icon, or its first letter on a colour picked from its id when the icon cannot be fetched. */
export function AppIcon({ app, size = 36 }: { app: Pick<ConnectorApp, "id" | "title">; size?: number }) {
  const [failed, setFailed] = useState(false);
  const hue = [...app.id].reduce((sum, ch) => sum + ch.charCodeAt(0) * 7, 0) % 360;
  return failed
    ? <span className="app-icon app-icon-letter" style={{ width: size, height: size, fontSize: size * .46, background: `hsl(${hue} 55% 46%)` }} aria-hidden="true">{app.title.slice(0, 1).toUpperCase()}</span>
    : <img className="app-icon" src={`/api/connectors/${encodeURIComponent(app.id)}/icon`} width={size} height={size} alt="" loading="lazy" onLoad={event => { event.currentTarget.dataset.loaded = ""; }} onError={() => setFailed(true)} />;
}

function stateLine(app: ConnectorApp) {
  switch (app.state) {
    case "connected": return `可以使用 · ${app.tools.length} 個工具`;
    case "signing_in": return "等待登入…";
    case "needs_signin": return "需要登入";
    case "checking": return "已連接 · 尚未檢查";
    case "off": return "已停用";
    case "error": return "連線失敗";
    default: return "";
  }
}
const tone = (app: ConnectorApp) => app.state === "connected" ? "ok" : app.state === "error" ? "error" : app.state === "needs_signin" ? "warn" : "busy";

/** Live view of a bot's computer, where an app's sign-in page opens. */
export function LiveScreen({ agentId }: { agentId: string }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    let alive = true;
    api.computer(agentId, "start")
      .then(data => { if (alive) data.ready && data.viewer_url ? setUrl(viewerUrl(data.viewer_url, true)) : setError(data.error || "電腦還沒準備好"); })
      .catch(err => alive && setError(message(err)));
    return () => { alive = false; };
  }, [agentId]);
  useEffect(() => {
    if (!url) return;
    const post = () => frame.current?.contentWindow?.postMessage({ type: "lazyboy:viewer-active", active: !document.hidden }, window.location.origin);
    post();
    const timer = setInterval(post, 5000);
    return () => clearInterval(timer);
  }, [url]);
  return (
    <div className="connect-screen">
      {url ? <iframe ref={frame} src={url} title="Bot 的電腦畫面" allow="clipboard-read; clipboard-write" />
        : <p className="connect-screen-wait">{error || <><LoaderCircle className="spin" aria-hidden="true" />正在開啟 Bot 的電腦…</>}</p>}
    </div>
  );
}

/**
 * Signing in to an app: its page is already open on the bot's computer, shown live here. Signing in on this device
 * also works; the page the app returns to will not load, and pasting its address finishes the sign-in.
 */
export function SignInPanel({ title, signin, onPoll, onRelay, onCancel }: {
  title: string;
  signin: { agent: string; auth_url: string };
  onPoll: () => Promise<void>;
  onRelay: (address: string) => Promise<void>;
  onCancel: () => Promise<void>;
}) {
  const [own, setOwn] = useState(false);
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const poll = useRef(onPoll);
  poll.current = onPoll;
  useEffect(() => {
    let stop = false;
    const timer = setInterval(() => { if (!stop && !document.hidden) poll.current().catch(err => { stop = true; setError(message(err)); }); }, 2500);
    return () => { stop = true; clearInterval(timer); };
  }, []);
  async function relay() {
    setBusy(true); setError("");
    try { await onRelay(address); }
    catch (err) { setError(message(err)); }
    finally { setBusy(false); }
  }
  return (
    <div className="signin-panel">
      {own ? (
        <>
          <ol className="signin-steps">
            <li><a href={signin.auth_url} target="_blank" rel="noreferrer">開啟 {title} 登入頁 <ExternalLink aria-hidden="true" /></a>，登入並按允許。</li>
            <li>最後會停在一個打不開的頁面，這是正常的。複製網址列的整串網址。</li>
            <li>貼到下面，按「完成登入」。</li>
          </ol>
          <input value={address} onChange={e => setAddress(e.target.value)} placeholder="http://127.0.0.1:…/callback?code=…" inputMode="url" autoCapitalize="off" autoCorrect="off" spellCheck={false} aria-label="登入後的網址" />
          <div className="dialog-actions">
            <button type="button" onClick={() => setOwn(false)} disabled={busy}>改回 Bot 的電腦</button>
            <button type="button" className="primary" disabled={busy || !address.trim()} onClick={() => void relay()}>{busy ? "完成中…" : "完成登入"}</button>
          </div>
        </>
      ) : (
        <>
          <p className="signin-lead"><LoaderCircle className="spin" aria-hidden="true" />{title} 的登入頁已在 Bot 的電腦上開啟。在下面登入並按「允許」，完成後會自動繼續。</p>
          <LiveScreen agentId={signin.agent} />
          <div className="signin-alt">
            <button type="button" className="link-btn" onClick={() => setOwn(true)}>改在這台裝置登入</button>
            <button type="button" className="link-btn" onClick={() => void onCancel()}>取消登入</button>
          </div>
        </>
      )}
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
    </div>
  );
}

function AppSheet({ app, agentId, onChange, onClose }: { app: ConnectorApp; agentId?: string | null; onChange: (app: ConnectorApp) => void; onClose: () => void }) {
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [token, setToken] = useState("");
  const [confirm, setConfirm] = useState(false);
  async function run(action: string, extra: Record<string, unknown> = {}) {
    setBusy(action); setError("");
    try {
      const next = await api.connector(app.id, { action, agent: agentId || undefined, ...extra });
      onChange(next);
      if (next.state === "connected" && app.state !== "connected") toast(`已連接 ${app.title}`, "ok");
      if (action === "disconnect") { toast(`已中斷 ${app.title}`, "ok"); onClose(); }
    } catch (err) { setError(message(err)); }
    finally { setBusy(""); setConfirm(false); }
  }
  const connected = app.state !== "available";
  const working = busy === "connect" || busy === "signin" || busy === "test";
  return (
    <Modal label={app.title} onClose={onClose} className={`app-sheet${app.signin ? " signing" : ""}`}>
      <ModalHeader title={<span className="app-sheet-title"><AppIcon app={app} size={32} />{app.title}</span>} onClose={onClose} />
      <p className="app-sheet-lead">{app.description}</p>
      {app.signin ? (
        <SignInPanel title={app.title} signin={app.signin}
          onPoll={async () => { const next = await api.connector(app.id, { action: "poll" }); onChange(next); if (next.state === "connected") toast(`已連接 ${app.title}`, "ok"); }}
          onRelay={async address => { const next = await api.connector(app.id, { action: "relay", address }); onChange(next); if (next.state === "connected") toast(`已連接 ${app.title}`, "ok"); }}
          onCancel={async () => { onChange(await api.connector(app.id, { action: "cancel" })); }} />
      ) : !connected ? (
        <div className="app-sheet-connect">
          {app.auth === "token" ? (
            <label>{app.token_hint}
              <input type="password" autoComplete="new-password" value={token} onChange={e => setToken(e.target.value)} placeholder="貼上 Token" />
              {app.token_url ? <a href={app.token_url} target="_blank" rel="noreferrer">建立 Token <ExternalLink aria-hidden="true" /></a> : null}
            </label>
          ) : null}
          <button type="button" className="primary" disabled={working || (app.auth === "token" && !token.trim())} onClick={() => void run("connect", app.auth === "token" ? { token } : {})}>
            {working ? (app.auth === "oauth" ? "準備登入…" : "連接中…") : app.auth === "oauth" ? "登入並連接" : "連接"}
          </button>
          <small>{app.auth === "oauth" ? `會在 Bot 的電腦開啟 ${app.title} 登入頁。登入一次，所有 Bot 都能使用。` : app.auth === "none" ? "不需要帳號。所有 Bot 都能使用。" : "所有 Bot 都能使用。Token 只存在這台主機。"}</small>
        </div>
      ) : (
        <div className="app-sheet-status" data-tone={tone(app)}>
          <strong><i aria-hidden="true" />{working && busy === "test" ? "檢查連線中…" : stateLine(app)}</strong>
          {app.checked_at && app.state !== "checking" ? <small>上次檢查 {ago(app.checked_at)}</small> : null}
          {app.state === "error" && app.error ? <p className="pane-error">{app.error}</p> : null}
          {app.tools.length ? (
            <details>
              <summary>Bot 可以使用的工具</summary>
              <ul>{app.tools.map(tool => <li key={tool.name}><code>{tool.name}</code>{tool.description ? <span>{tool.description}</span> : null}</li>)}</ul>
            </details>
          ) : null}
          {confirm ? (
            <div className="app-sheet-confirm">
              <span>中斷後所有 Bot 都不能再使用 {app.title}，{app.auth === "oauth" ? "登入資料也會刪除" : "設定會移除"}。</span>
              <div className="dialog-actions">
                <button type="button" onClick={() => setConfirm(false)} disabled={!!busy}>取消</button>
                <button type="button" className="danger solid" disabled={!!busy} onClick={() => void run("disconnect")}>{busy === "disconnect" ? "中斷中…" : "中斷連接"}</button>
              </div>
            </div>
          ) : (
            <div className="dialog-actions">
              <button type="button" className="outline" disabled={!!busy} onClick={() => setConfirm(true)}>中斷連接</button>
              {app.auth === "oauth" && app.state !== "connected" ? <button type="button" className="primary" disabled={!!busy} onClick={() => void run("signin")}>{busy === "signin" ? "準備登入…" : "登入"}</button> : null}
              <button type="button" className={app.state === "connected" || app.auth !== "oauth" ? "primary" : ""} disabled={!!busy} onClick={() => void run("test")}>{busy === "test" ? "檢查中…" : "重新檢查"}</button>
            </div>
          )}
        </div>
      )}
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
    </Modal>
  );
}

/**
 * Apps the bots can use, connected once for all of them: what is connected on top, a searchable gallery below, and
 * custom MCP servers for anything not in the gallery.
 */
export function ConnectApps({ agentId }: { agentId?: string | null }) {
  const [apps, setApps] = useState<ConnectorApp[] | null>(null);
  const [categories, setCategories] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState(FEATURED);
  const [open, setOpen] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);
  const update = (next: ConnectorApp) => setApps(current => current?.map(app => app.id === next.id ? next : app) || null);

  useEffect(() => {
    api.connectors().then(data => {
      if (!alive.current) return;
      setApps(data.apps); setCategories(data.categories);
      void recheck(data.apps);
    }).catch(err => setError(message(err)));
  }, []);

  /** Quietly re-check connected apps never checked or checked long ago, only on computers already running. */
  async function recheck(rows: ConnectorApp[]) {
    for (const app of rows) {
      if (!alive.current) return;
      if (app.state === "available" || app.state === "signing_in" || app.state === "off" || app.state === "needs_signin") continue;
      if (app.checked_at && Date.now() - Date.parse(app.checked_at) < STALE_MS) continue;
      try { const next = await api.connector(app.id, { action: "test", agent: agentId || undefined, auto: true }); if (alive.current) update(next); }
      catch { /* the card keeps its last state */ }
    }
  }

  const connected = useMemo(() => (apps || []).filter(app => app.state !== "available"), [apps]);
  const shown = useMemo(() => {
    const rows = apps || [];
    const words = query.trim().toLowerCase();
    if (words) return rows.filter(app => [app.title, app.id, app.description, ...app.keywords, ...app.hosts].some(text => text.toLowerCase().includes(words)));
    if (category === FEATURED) return rows.filter(app => app.featured);
    if (category === ALL) return rows;
    return rows.filter(app => app.category === category);
  }, [apps, query, category]);
  const current = apps?.find(app => app.id === open) || null;

  return (
    <div className="connect-apps lb-settings-stack">
      <p className="mcp-hint">連接常用的應用程式，Bot 就能直接讀寫裡面的資料。連接一次，所有 Bot 都能用。</p>
      {error ? <div role="alert" className="pane-error">{error}</div> : null}
      {connected.length ? (
        <section aria-label="已連接">
          <h3>已連接</h3>
          <ul className="connected-apps">
            {connected.map(app => (
              <li key={app.id}>
                <button type="button" onClick={() => setOpen(app.id)} data-tone={tone(app)}>
                  <AppIcon app={app} size={34} />
                  <span><strong>{app.title}</strong><small><i aria-hidden="true" />{stateLine(app)}</small></span>
                  {app.state === "needs_signin" ? <em>登入</em> : null}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      <section aria-label="應用程式">
        <h3>{connected.length ? "更多應用程式" : "應用程式"}</h3>
        <label className="app-search">
          <Search aria-hidden="true" />
          <input type="search" value={query} onChange={e => setQuery(e.target.value)} placeholder="搜尋應用程式" aria-label="搜尋應用程式" enterKeyHint="search" />
        </label>
        {!query.trim() ? (
          <div className="app-chips" role="tablist" aria-label="分類">
            {[FEATURED, ALL, ...categories].map(name => <button key={name} type="button" role="tab" aria-selected={category === name} onClick={() => setCategory(name)}>{name}</button>)}
          </div>
        ) : null}
        {!apps ? <p className="mcp-hint">讀取中…</p> : !shown.length ? <p className="mcp-hint">找不到「{query}」。可以在下方用網址自訂連接。</p> : (
          <ul className="app-grid">
            {shown.map(app => (
              <li key={app.id}>
                <button type="button" onClick={() => setOpen(app.id)} aria-label={`${app.title}：${app.description}`}>
                  <AppIcon app={app} />
                  <span><strong>{app.title}</strong><small>{app.description}</small></span>
                  {app.state !== "available" ? <i className="app-badge" data-tone={tone(app)} aria-hidden="true" /> : null}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <details className="custom-mcp">
        <summary>自訂連接（MCP）</summary>
        <p className="mcp-hint">不在上面的服務，只要有 MCP 網址或啟動指令，也可以自己加入。</p>
        <McpSettings everywhere custom agentId={agentId} />
      </details>
      {current ? <AppSheet app={current} agentId={agentId} onChange={update} onClose={() => setOpen(null)} /> : null}
    </div>
  );
}
