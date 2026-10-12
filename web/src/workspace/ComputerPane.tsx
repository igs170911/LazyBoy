import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AgentRow, type HumanQuestion } from "../api";
import { useComputerRecovery } from "../computer-recovery";
import { Ellipsis, ExternalLink, Hand, LoaderCircle, Monitor, MousePointerClick, RotateCw, Sparkles } from "../ui/icons";
import { ConfirmDialog, MenuButton, toast } from "../ui/overlay";

type Action = "start" | "restart" | "update";
const BUSY_LABEL: Record<Action, string> = {
  start: "正在啟動電腦…",
  restart: "正在重新啟動電腦…",
  update: "正在更新電腦，可能需要幾分鐘…",
};

function viewerUrl(url: string, mobile: boolean) {
  const parsed = new URL(url, window.location.origin);
  parsed.searchParams.set("t", String(Date.now()));
  if (mobile) {
    parsed.searchParams.set("resize", "scale");
    parsed.searchParams.set("scaling", "local");
    if (!parsed.searchParams.get("autoconnect")) parsed.searchParams.set("autoconnect", "true");
  }
  return `${parsed.pathname}${parsed.search}${parsed.hash}`;
}

export function isHandover(question?: HumanQuestion | null) {
  return question?.kind === "handoff" || question?.kind === "box_help";
}

export function ComputerPane({ agent, visible, mobile, botWorking, question, onSend, onAnswered }: {
  agent: AgentRow;
  visible: boolean;
  mobile: boolean;
  botWorking: boolean;
  question: HumanQuestion | null;
  onSend: (text: string) => Promise<void>;
  onAnswered: () => void;
}) {
  const [op, setOp] = useState<Action | null>(null);
  const [url, setUrl] = useState("");
  const [status, setStatus] = useState("");
  const [generation, setGeneration] = useState(0);
  const [control, setControl] = useState(false);
  const [returning, setReturning] = useState(false);
  const [confirm, setConfirm] = useState<Action | null>(null);
  const running = useRef<Promise<unknown> | null>(null);
  const handover = isHandover(question);

  const run = useCallback(async (action: Action, reload = false) => {
    if (running.current && action === "start") { await running.current; return; }
    setOp(action);
    if (action !== "start") setUrl("");
    const request = api.computer(agent.id, action);
    running.current = request;
    try {
      const data = await request;
      if (data.ready && data.viewer_url) {
        setUrl(current => (current && !reload && action === "start" ? current : viewerUrl(data.viewer_url, mobile)));
        setGeneration(n => n + 1);
        setStatus(data.crowded ? "主機負載偏高，畫面可能較慢" : action === "update" && data.updated === false ? "電腦已是最新版本" : "");
      } else {
        setUrl("");
        setStatus(data.error || "電腦還沒準備好，請稍後重試");
      }
    } catch (err) {
      setUrl("");
      setStatus(err instanceof Error ? err.message : String(err));
    } finally {
      running.current = null;
      setOp(null);
    }
  }, [agent.id, mobile]);

  const ensure = useCallback(() => run("start"), [run]);
  const reconnect = useCallback(() => run("start", true), [run]);
  const { issue, retry, connected } = useComputerRecovery(agent.id, visible, !!op, ensure, reconnect, generation);

  useEffect(() => { if (handover) setControl(true); }, [handover, question?.question_id]);
  useEffect(() => { if (!botWorking && !handover) setControl(false); }, [botWorking, handover]);

  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    if (!url) return;
    const post = () => frame.current?.contentWindow?.postMessage({ type: "lazyboy:viewer-active", active: visible && !document.hidden }, window.location.origin);
    post();
    const timer = setInterval(post, 5000);
    document.addEventListener("visibilitychange", post);
    return () => { clearInterval(timer); document.removeEventListener("visibilitychange", post); };
  }, [url, visible]);

  useEffect(() => {
    function onMessage(event: MessageEvent) {
      if (event.origin !== window.location.origin) return;
      const frame = document.querySelector<HTMLIFrameElement>("iframe.desktop-frame");
      if (!frame || event.source !== frame.contentWindow) return;
      const data = event.data as { type?: string; text?: string } | null;
      if (!data || typeof data !== "object") return;
      const source = event.source as Window;
      if (data.type === "lazyboy-clipboard-write" && typeof data.text === "string") void navigator.clipboard?.writeText(data.text).catch(() => undefined);
      if (data.type === "lazyboy-clipboard-read") {
        void (navigator.clipboard?.readText() ?? Promise.reject(new Error("unavailable")))
          .then(text => source.postMessage({ type: "lazyboy-clipboard-text", text }, event.origin))
          .catch(() => source.postMessage({ type: "lazyboy-clipboard-text", text: "" }, event.origin));
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  async function handBack() {
    if (returning) return;
    setReturning(true);
    try {
      if (question?.question_id && (handover || question.method === "clarify")) {
        await api.handoverDone(agent.id, question.task_id || "", question.question_id);
        onAnswered();
      } else if (botWorking) {
        await onSend("我剛剛在電腦上操作過，請重新看一下目前的畫面再繼續。");
      }
      setControl(false);
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), "error");
    } finally { setReturning(false); }
  }

  const state = op ? "busy" : url && connected && !issue ? "ok" : url && !issue ? "busy" : "off";
  const label = op ? BUSY_LABEL[op] : issue || status || (url ? (connected ? (control ? "你正在操作" : botWorking ? `${agent.name} 正在使用` : "已連線") : "正在連線畫面…") : "電腦未啟動");
  const shield = !!url && botWorking && !control;

  return (
    <section className="computer-pane" hidden={!visible} aria-label="Bot 的電腦">
      <div className="pane-toolbar">
        <span className={`dot ${state}`} aria-hidden="true" />
        <span className="pane-status" role="status">{label}</span>
        {control && url ? (
          <button type="button" className="btn small primary" disabled={returning} onClick={() => void handBack()}>
            {returning ? <LoaderCircle className="spin" /> : <Sparkles />}{handover ? "完成，交還給 Bot" : "交還給 Bot"}
          </button>
        ) : null}
        {issue && !op ? <button type="button" className="btn small" onClick={() => void retry()}><RotateCw />重新連線</button> : null}
        <MenuButton label="電腦選項" icon={<Ellipsis />} items={[
          { label: "重新連線畫面", icon: <RotateCw />, onSelect: () => void run("start", true), disabled: !!op },
          { label: "在新分頁開啟", icon: <ExternalLink />, onSelect: () => { if (url) window.open(url, "_blank", "noopener"); }, disabled: !url },
          "separator",
          { label: "重新啟動電腦", icon: <Monitor />, onSelect: () => setConfirm("restart"), disabled: !!op },
          { label: "更新電腦環境", icon: <Sparkles />, onSelect: () => setConfirm("update"), disabled: !!op },
        ]} />
      </div>
      {handover && question ? (
        <div className="handover-banner" role="alert">
          <Hand aria-hidden="true" />
          <span><strong>{agent.name} 需要你協助</strong>{question.prompt || question.question || question.reason || "請在下方電腦完成操作，好了再交還。"}</span>
        </div>
      ) : null}
      <div className="desktop-stage">
        {url ? (
          <iframe key={url} ref={frame} className="desktop-frame" src={url} title={`${agent.name} 的電腦`} allow="clipboard-read; clipboard-write; fullscreen" />
        ) : (
          <div className="desktop-empty">
            {op ? <LoaderCircle className="spin" /> : <Monitor />}
            <p>{op ? BUSY_LABEL[op] : status || "每個 Bot 都有自己的電腦，打開後會自動啟動。"}</p>
            {!op ? <button type="button" className="btn primary" onClick={() => void run("start", true)}>啟動電腦</button> : null}
          </div>
        )}
        {shield ? (
          <div className="desktop-shield">
            <div>
              <MousePointerClick aria-hidden="true" />
              <p><strong>{agent.name} 正在使用電腦</strong>你可以看著它做，或直接接手。</p>
              <button type="button" className="btn" onClick={() => setControl(true)}><Hand />接手操作</button>
            </div>
          </div>
        ) : null}
      </div>
      {confirm ? (
        <ConfirmDialog
          title={confirm === "restart" ? "重新啟動電腦？" : "更新電腦環境？"}
          body={confirm === "restart" ? "電腦會重新開機，正在進行的工作可能中斷。工作區檔案會保留。" : "會下載最新環境並重建電腦，可能需要幾分鐘。工作區檔案會保留。"}
          confirmLabel={confirm === "restart" ? "重新啟動" : "更新"}
          danger={confirm === "restart"}
          onConfirm={() => { const action = confirm; setConfirm(null); void run(action); }}
          onClose={() => setConfirm(null)}
        />
      ) : null}
    </section>
  );
}
