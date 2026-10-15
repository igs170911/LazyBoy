import { useEffect, useState, type ReactNode } from "react";
import { dismissBoot } from "./boot";

export function AuthGate({children}: {children: ReactNode}) {
  const [status, setStatus] = useState<"loading" | "login" | "ready">("loading");
  const [token, setToken] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    // `make up` prints a login link ending in #token=…; the fragment never reaches the server or its logs.
    const linked = new URLSearchParams(location.hash.slice(1)).get("token");
    if (linked) history.replaceState(null, "", location.pathname + location.search);
    // An expired or rotated link must say so, not just drop to the plain form.
    let linkFailed = false;
    (linked ? fetch("/api/auth/login", {method:"POST", headers:{"content-type":"application/json"}, body:JSON.stringify({token: linked})}).then(r => {linkFailed = !r.ok;}) : Promise.resolve())
      .then(() => fetch("/api/auth/status")).then(r => r.json())
      .then(data => {if (!alive) return; setStatus(data.authenticated ? "ready" : "login"); if (!data.authenticated && linkFailed) setError("登入連結已失效，請重新取得。");}).catch(() => {if (alive) {setStatus("login"); setError("服務尚未連線，請確認後端已啟動。");}});
    return () => {alive = false;};
  }, []);
  useEffect(() => { if (status !== "loading") dismissBoot(); }, [status]);
  if (status === "ready") return children;
  if (status === "loading") return <main className="auth-gate">正在連線…</main>;
  return <main className="auth-gate"><form onSubmit={async e => {
    e.preventDefault(); setBusy(true); setError("");
    try {
      const r = await fetch("/api/auth/login", {method:"POST", headers:{"content-type":"application/json"}, body:JSON.stringify({token})});
      const data = await r.json(); if (!r.ok) throw new Error(data.error || "登入失敗");
      setToken(""); setStatus("ready");
    } catch (e) {setError(e instanceof Error ? e.message : String(e));} finally {setBusy(false);}
  }}><h1>LazyBoy</h1><p>輸入這臺工作主機的登入碼，連線你的 Bots。</p><label>登入碼<input type="password" autoComplete="current-password" value={token} onChange={e => setToken(e.target.value)} required /></label><button type="submit" disabled={busy}>{busy ? "連線中…" : "登入"}</button>{error ? <p role="alert">{error}</p> : null}</form></main>;
}
