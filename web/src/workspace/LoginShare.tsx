import { useEffect, useState } from "react";
import { api, type AgentRow } from "../api";
import { Avatar } from "../avatar";
import { LoaderCircle } from "../ui/icons";
import { Modal, ModalHeader, toast } from "../ui/overlay";
import { avatarProps } from "./Sidebar";

/**
 * Grok's bots share one computer, so a site signed in once works for all of them. LazyBoy's bots each have their own;
 * this copies the sign-ins of the sites you pick from one bot's browser to others', so they need not sign in again.
 */
export function LoginShareDialog({ agent, agents, onClose }: { agent: AgentRow; agents: AgentRow[]; onClose: () => void }) {
  const [sites, setSites] = useState<{ site: string; cookies: number }[] | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [targets, setTargets] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const others = agents.filter(row => row.id !== agent.id);
  useEffect(() => {
    let alive = true;
    api.logins(agent.id).then(data => { if (alive) setSites(Array.isArray(data?.sites) ? data.sites : []); })
      .catch(err => { if (alive) { setSites([]); setError(err instanceof Error ? err.message : String(err)); } });
    return () => { alive = false; };
  }, [agent.id]);
  const flip = (list: string[], value: string) => list.includes(value) ? list.filter(item => item !== value) : [...list, value];

  async function share() {
    setBusy(true); setError("");
    try {
      for (const target of targets) await api.copyLogins(agent.id, target, chosen);
      const names = others.filter(row => targets.includes(row.id)).map(row => row.name).join("、");
      toast(`已把 ${chosen.length} 個網站的登入分享給 ${names}`, "ok");
      onClose();
    } catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
  }

  return (
    <Modal label="分享登入給其他 Bot" onClose={() => { if (!busy) onClose(); }} className="login-share-dialog">
      <ModalHeader title={`分享 ${agent.name} 的登入`} onClose={onClose} disabled={busy} />
      <p className="dialog-lead">把 {agent.name} 瀏覽器裡已登入的網站分享給其他 Bot，對方就不用再登入一次。只會複製你勾選的網站，登入資料直接從一台電腦傳到另一台，不會出現在畫面上。</p>
      <section>
        <h3 className="login-share-head">要分享的網站</h3>
        {sites === null ? <p className="muted"><LoaderCircle className="spin" /> 讀取 {agent.name} 的瀏覽器…</p>
          : !sites.length ? <p className="muted">{agent.name} 的瀏覽器裡還沒有登入任何網站。</p>
          : <ul className="login-share-list">{sites.map(row => (
              <li key={row.site}><label><input type="checkbox" checked={chosen.includes(row.site)} onChange={() => setChosen(list => flip(list, row.site))} /><span>{row.site}</span><small>{row.cookies} 筆</small></label></li>
            ))}</ul>}
      </section>
      <section>
        <h3 className="login-share-head">分享給</h3>
        <ul className="login-share-list">{others.map(row => (
          <li key={row.id}><label><input type="checkbox" checked={targets.includes(row.id)} onChange={() => setTargets(list => flip(list, row.id))} /><Avatar {...avatarProps(row)} size={22} gaze={false} /><span>{row.name}</span></label></li>
        ))}</ul>
      </section>
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" onClick={onClose} disabled={busy}>取消</button>
        <button type="button" className="primary" disabled={busy || !chosen.length || !targets.length} onClick={() => void share()}>{busy ? "分享中…" : "分享登入"}</button>
      </div>
    </Modal>
  );
}
