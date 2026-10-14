import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import qrcode from "qrcode-generator";
import { api, type ConnectInfo } from "./api";
import { installApp, installAvailable, onInstallChange, runningAsApp } from "./install-app";

/** A QR code as crisp SVG squares; `label` names it for screen readers. */
function QrCode({ text, label }: { text: string; label: string }) {
  const path = useMemo(() => {
    const qr = qrcode(0, "M"); qr.addData(text); qr.make();
    const size = qr.getModuleCount(); let d = "";
    for (let row = 0; row < size; row++) for (let col = 0; col < size; col++) if (qr.isDark(row, col)) d += `M${col + 4} ${row + 4}h1v1h-1z`;
    return { d, size: size + 8 };
  }, [text]);
  return <svg className="device-qr" viewBox={`0 0 ${path.size} ${path.size}`} role="img" aria-label={label} shapeRendering="crispEdges">
    <rect width={path.size} height={path.size} fill="#fff" /><path d={path.d} fill="#000" />
  </svg>;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => { if (!copied) return; const timer = window.setTimeout(() => setCopied(false), 1600); return () => window.clearTimeout(timer); }, [copied]);
  return <button type="button" className="outline" onClick={() => void navigator.clipboard?.writeText(text).then(() => setCopied(true))}>{copied ? "已複製" : "複製"}</button>;
}

type Platform = "ios" | "android" | "windows" | "mac" | "linux";
function currentPlatform(): Platform {
  const agent = navigator.userAgent;
  if (/iPhone|iPad|iPod/.test(agent) || (/Macintosh/.test(agent) && navigator.maxTouchPoints > 1)) return "ios";
  if (/Android/.test(agent)) return "android";
  if (/Windows/.test(agent)) return "windows";
  return /Macintosh/.test(agent) ? "mac" : "linux";
}

const TRUST_STEPS: { id: Platform; name: string; steps: string[] }[] = [
  { id: "ios", name: "iPhone、iPad", steps: [
    "用 Safari 掃描或開啟「下載憑證」，出現提示時選「允許」。",
    "打開「設定」，點最上方的「已下載描述檔」，再點「安裝」。",
    "到「設定 → 一般 → 關於本機 → 憑證信任設定」，開啟「LazyBoy Local CA」。"] },
  { id: "android", name: "Android", steps: [
    "下載憑證。",
    "到「設定 → 安全性與隱私權 → 更多安全性設定 → 加密與憑證 → 安裝憑證 → CA 憑證」，選擇剛下載的 LazyBoy-CA.crt（選單名稱依手機廠牌略有不同）。"] },
  { id: "windows", name: "Windows", steps: [
    "下載憑證後按兩下開啟，選「安裝憑證」。",
    "存放位置選「目前使用者」，再選「將所有憑證放入以下的存放區」→「受信任的根憑證授權單位」，完成後重新開啟瀏覽器。"] },
  { id: "mac", name: "macOS", steps: [
    "下載憑證後按兩下，加入「鑰匙圈存取」的「登入」鑰匙圈。",
    "在「鑰匙圈存取」打開「LazyBoy Local CA」，展開「信任」，把「使用此憑證時」設為「永遠信任」。"] },
  { id: "linux", name: "Linux", steps: [
    "Chrome、Edge：打開 chrome://certificate-manager，在「本機憑證 → 自訂」匯入憑證，並勾選信任網站。",
    "Firefox：到「設定 → 隱私權與安全性 → 憑證 → 檢視憑證 → 憑證機構」匯入，勾選「信任由此憑證機構來識別網站」。"] },
];

function InstallGroup() {
  const available = useSyncExternalStore(onInstallChange, installAvailable);
  const [installed, setInstalled] = useState(runningAsApp);
  const platform = currentPlatform();
  let body;
  if (installed) body = <span className="lb-settings-copy"><strong>已經以 App 開啟</strong><small>LazyBoy 有自己的視窗與圖示，可以接收通知。</small></span>;
  else if (available) body = <>
    <span className="lb-settings-copy"><strong>安裝 LazyBoy App</strong><small>在桌面或主畫面有自己的圖示與視窗，通知、分享與離線開啟也更順。</small></span>
    <button type="button" className="outline" onClick={() => void installApp().then(setInstalled)}>安裝</button>
  </>;
  else if (!window.isSecureContext) body = <span className="lb-settings-copy"><strong>這個網址不能安裝 App</strong>
    <small>瀏覽器只允許從 HTTPS 網址安裝 App、使用麥克風與通知。請改用下方「從其他裝置連線」的 HTTPS 網址。</small></span>;
  else if (platform === "ios") body = <span className="lb-settings-copy"><strong>加入主畫面</strong><small>在 Safari 點「分享」，再選「加入主畫面」。</small></span>;
  else body = <span className="lb-settings-copy"><strong>從瀏覽器安裝</strong>
    <small>Chrome 或 Edge：網址列右側的安裝圖示，或選單中的「安裝 LazyBoy」。Firefox 與部分瀏覽器不支援安裝 App。</small></span>;
  return <section className="lb-settings-group"><h3>App</h3><div className="lb-settings-row">{body}</div></section>;
}

function ConnectGroups({ info }: { info: ConnectInfo }) {
  const hosts = info.hosts.length ? info.hosts : [location.hostname];
  const [host, setHost] = useState(() => hosts.includes(location.hostname) ? location.hostname : hosts[0]);
  const [step, setStep] = useState<"trust" | "open">("trust");
  const [reveal, setReveal] = useState(false);
  const secure = info.https_port ? `https://${host.includes(":") ? `[${host}]` : host}:${info.https_port}/` : "";
  const plain = `http://${host.includes(":") ? `[${host}]` : host}:${info.port}/`;
  const caUrl = `${plain}lazyboy-ca.crt`;
  const signIn = `${secure || plain}#token=${encodeURIComponent(info.token)}`;
  const platform = currentPlatform();
  return <>
    <section className="lb-settings-group" aria-label="從其他裝置連線">
      <h3>從其他裝置連線</h3>
      <p className="device-note">手機或其他電腦要和這台在同一個網路。在外面也想連線時，可以用 Tailscale 這類 VPN 連回來。</p>
      {hosts.length > 1 ? <label className="lb-settings-row"><span className="lb-settings-copy">連線位址</span>
        <select className="lb-settings-select" value={host} onChange={e => setHost(e.target.value)}>{hosts.map(h => <option key={h}>{h}</option>)}</select></label> : null}
      {secure ? <div className="lb-settings-row"><span className="lb-settings-copy"><strong>HTTPS（建議）</strong><small className="device-url">{secure}</small></span><CopyButton text={secure} /></div> : null}
      <div className="lb-settings-row"><span className="lb-settings-copy"><strong>HTTP</strong><small className="device-url">{plain}</small></span><CopyButton text={plain} /></div>
      <div className="device-qr-card">
        {secure ? <div className="device-steps" role="tablist" aria-label="手機連線步驟">
          <button type="button" role="tab" aria-selected={step === "trust"} onClick={() => setStep("trust")}>1 安裝憑證</button>
          <button type="button" role="tab" aria-selected={step === "open"} onClick={() => setStep("open")}>2 開啟並登入</button>
        </div> : null}
        {secure && step === "trust" ? <>
          <QrCode text={caUrl} label="下載憑證的 QR code" />
          <p>用手機相機掃描，下載並信任 LazyBoy 的憑證（每台裝置只需要一次），再到步驟 2。</p>
        </> : reveal ? <>
          <QrCode text={signIn} label="開啟並登入 LazyBoy 的 QR code" />
          <p>掃描後會直接登入。這個 QR code 含登入碼，不要給別人看。</p>
          <button type="button" className="outline" onClick={() => setReveal(false)}>隱藏</button>
        </> : <>
          <p>產生一個掃描後就直接登入的 QR code。它含有登入碼，請確認旁邊沒有其他人。</p>
          <button type="button" className="outline" onClick={() => setReveal(true)}>顯示登入 QR code</button>
        </>}
      </div>
    </section>
    {secure ? <section className="lb-settings-group" aria-label="HTTPS 憑證">
      <h3>HTTPS 憑證</h3>
      <p className="device-note">LazyBoy 用自己的憑證提供 HTTPS。每台裝置第一次連線前，要先安裝並信任它；之後換網路位址或更新憑證都不必再做。這個憑證只能用在私人網路位址與本機名稱，無法冒充其他網站。</p>
      <div className="lb-settings-row"><span className="lb-settings-copy"><strong>LazyBoy Local CA（{info.hostname}）</strong>
        <small className="device-url">SHA-256 {info.fingerprint}</small></span><a className="btn outline" href="/lazyboy-ca.crt" download="LazyBoy-CA.crt">下載憑證</a></div>
      {TRUST_STEPS.map(item => <details key={item.id} className="device-trust" open={item.id === platform}>
        <summary>{item.name}</summary><ol>{item.steps.map(text => <li key={text}>{text}</li>)}</ol>
      </details>)}
    </section> : <p className="device-note">HTTPS 沒有啟用（LAZYHERMES_HTTPS_PORT=0）。手機只能用 HTTP 連線，無法安裝 App、使用麥克風與通知。</p>}
  </>;
}

export function DevicesPanel() {
  const [info, setInfo] = useState<ConnectInfo | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    api.connect().then(data => Array.isArray(data?.hosts) ? setInfo(data) : setError("無法讀取連線資訊，請重新整理。"))
      .catch(e => setError(e instanceof Error ? e.message : String(e)));
  }, []);
  return <div className="devices-panel">
    <InstallGroup />
    {info ? <ConnectGroups info={info} /> : error ? <p role="alert">{error}</p> : <p className="device-note">正在讀取連線資訊…</p>}
  </div>;
}
