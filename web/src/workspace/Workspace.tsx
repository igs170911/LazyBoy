import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { api, type AgentGroup, type AgentRow } from "../api";
import { Avatar, AvatarLookProvider, readAvatarLooks } from "../avatar";
import { deferDialog } from "../deferred-dialog";
import type { PreviewFile } from "../markdown";
import type { SettingsSectionId } from "../settings";
import { readStorage, writeStorage } from "../storage";
import { Bell, BellOff, CircleDot, Copy, Eye, EyeOff, Pin, Brain, CalendarClock, ChevronLeft, Eraser, FolderOpen, Monitor, PanelRightClose, Pencil, Plus, Share2, Trash2, Users, GraduationCap, Import } from "../ui/icons";
import { ConfirmDialog, toast, Toaster, type MenuItem } from "../ui/overlay";
import { ComputerPane } from "./ComputerPane";
import { Conversation, type PanelTab } from "./Conversation";
import { appendDraft, draftKey } from "./drafts";
import { FilesPane } from "./FilesPane";
import { Sidebar } from "./Sidebar";
import { GroupEditor, MoveDialog } from "./TeamDialogs";
import { TeachStartDialog } from "./Teaching";
import { useSwipeBack } from "./swipe-back";
import { useAgents } from "./useAgents";
import { useRooms } from "./useRooms";
import { useSeen } from "./useSeen";
import { useNotifications } from "./useNotifications";
import { isMuted, setMuted } from "../notify";
import { RoomView } from "./Room";
import { PeerDialog, RoomDialog } from "./Team";
import type { RoomRow } from "../api";
import { BUSY_STATES, useSession } from "./useSession";

const RoutinesDialog = deferDialog<Parameters<typeof import("../routines").RoutinesDialog>[0]>(() => import("../routines").then(m => ({ default: m.RoutinesDialog })), "RoutinesDialog");
const AgentShareDialog = deferDialog<Parameters<typeof import("../agent-template").AgentShareDialog>[0]>(() => import("../agent-template").then(m => ({ default: m.AgentShareDialog })), "AgentShareDialog");
const TemplateImportDialog = deferDialog<Parameters<typeof import("../agent-template").TemplateImportDialog>[0]>(() => import("../agent-template").then(m => ({ default: m.TemplateImportDialog })), "TemplateImportDialog");
const AgentSettingsDialog = deferDialog<Parameters<typeof import("../agent-settings").AgentSettingsDialog>[0]>(() => import("../agent-settings").then(m => ({ default: m.AgentSettingsDialog })), "AgentSettingsDialog");
const SettingsDialog = deferDialog<Parameters<typeof import("../settings").SettingsDialog>[0]>(() => import("../settings").then(m => ({ default: m.SettingsDialog })), "SettingsDialog");
const AboutDialog = deferDialog<Parameters<typeof import("../settings").AboutDialog>[0]>(() => import("../settings").then(m => ({ default: m.AboutDialog })), "AboutDialog");

const ACTIVE_KEY = "lazyhermes.active";
const ROOM_KEY = "lazyhermes.room";
const PANEL_KEY = "lazyhermes.panel";
const MOBILE_QUERY = "(max-width: 860px)";

type Panel = { open: boolean; tab: PanelTab; width: number };
type Dialog =
  | { kind: "settings"; section?: SettingsSectionId }
  | { kind: "about" }
  | { kind: "create" }
  | { kind: "agent"; id: string; section: "profile" | "skills" | "memory" }
  | { kind: "routines"; id: string }
  | { kind: "share"; agent: AgentRow }
  | { kind: "import" }
  | { kind: "teach"; agent: AgentRow }
  | { kind: "group"; group?: AgentGroup }
  | { kind: "move"; agent: AgentRow }
  | { kind: "room"; room?: RoomRow }
  | { kind: "peer"; id: string; peer: string }
  | { kind: "confirm"; title: string; body: string; label: string; danger?: boolean; run: () => Promise<void> };

function readPanel(): Panel {
  try {
    const value = JSON.parse(readStorage(PANEL_KEY) || "{}") as Partial<Panel>;
    return { open: value.open === true, tab: value.tab === "files" ? "files" : "computer", width: typeof value.width === "number" ? value.width : 560 };
  } catch { return { open: false, tab: "computer", width: 560 }; }
}

function useMedia(query: string) {
  const [match, setMatch] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const list = window.matchMedia(query);
    const change = () => setMatch(list.matches);
    list.addEventListener("change", change);
    return () => list.removeEventListener("change", change);
  }, [query]);
  return match;
}

export function Workspace() {
  const { agents, groups, loaded, error, refresh, upsert, remove, applyTeam } = useAgents();
  const mobile = useMedia(MOBILE_QUERY);
  const [activeId, setActiveId] = useState<string | null>(() => readStorage(ACTIVE_KEY));
  const { rooms, refresh: refreshRooms, setRooms } = useRooms();
  // A group chat open in the main pane; selecting a bot closes it.
  const [roomId, setRoomId] = useState<string | null>(() => readStorage(ROOM_KEY) || null);
  const [view, setView] = useState<"list" | "chat">("list");
  const [panel, setPanelState] = useState<Panel>(readPanel);
  const [preview, setPreview] = useState<PreviewFile | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [looks, setLooks] = useState(readAvatarLooks);
  const routinesButton = useRef<HTMLButtonElement>(null);
  const active = agents.find(agent => agent.id === activeId) || null;
  const session = useSession(active?.id || null, () => void refresh());

  const setPanel = useCallback((change: Partial<Panel>) => setPanelState(current => {
    const next = { ...current, ...change };
    writeStorage(PANEL_KEY, JSON.stringify(next));
    return next;
  }), []);

  useEffect(() => {
    if (!loaded) return;
    if (!active && agents.length) setActiveId(agents.slice().sort((a, b) => (Date.parse(b.last_at || "") || 0) - (Date.parse(a.last_at || "") || 0))[0].id);
    if (!agents.length) setActiveId(null);
  }, [loaded, active, agents]);
  useEffect(() => { if (activeId) writeStorage(ACTIVE_KEY, activeId); }, [activeId]);
  useEffect(() => { document.title = active ? `${active.name} · LazyBoy` : "LazyBoy"; }, [active]);

  // Phone navigation is a stack: list -> chat -> computer/files sheet. The browser back button pops it.
  const depth = useRef(0);
  const state = useRef({ mobile, view, panelOpen: panel.open });
  state.current = { mobile, view, panelOpen: panel.open };
  const popLocal = useCallback(() => {
    const { panelOpen, view } = state.current;
    if (panelOpen) setPanel({ open: false });
    else if (view === "chat") setView("list");
  }, [setPanel]);
  useEffect(() => {
    const onPop = () => { depth.current = Math.max(0, depth.current - 1); popLocal(); };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [popLocal]);
  const pushMobile = () => { if (state.current.mobile) { window.history.pushState({ lazyhermes: true }, ""); depth.current++; } };
  const back = useCallback(() => { if (state.current.mobile && depth.current > 0) window.history.back(); else popLocal(); }, [popLocal]);
  useEffect(() => { if (mobile && panel.open && view === "list") setPanel({ open: false }); }, [mobile]); // eslint-disable-line react-hooks/exhaustive-deps

  const select = useCallback((id: string) => {
    setActiveId(id);
    setRoomId(null);
    setPreview(null);
    if (state.current.mobile && state.current.view === "list") { setView("chat"); window.history.pushState({ lazyhermes: true }, ""); depth.current++; }
  }, []);

  const selectRoom = useCallback((id: string) => {
    setRoomId(id);
    if (state.current.mobile && state.current.view === "list") { setView("chat"); window.history.pushState({ lazyhermes: true }, ""); depth.current++; }
  }, []);
  useEffect(() => { writeStorage(ROOM_KEY, roomId || ""); }, [roomId]);
  const room = rooms.find(item => item.id === roomId) || null;
  const seenItems = useMemo(() => [...agents.map(a => ({ id: a.id, last_at: a.last_at })), ...rooms.map(r => ({ id: r.id, last_at: r.last_at }))], [agents, rooms]);
  // Ctrl/⌘+K: jump to the sidebar search from anywhere (on a phone the list page has it on top).
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      const focus = () => { const input = document.querySelector<HTMLInputElement>(".sidebar-search input"); input?.focus(); input?.select(); };
      // Focus at once, so keys typed right after the shortcut land in the search, not in the composer.
      if (state.current.mobile && state.current.view === "chat") { back(); requestAnimationFrame(focus); } else focus();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [back]);
  const { isUnread: unread, markUnread } = useSeen(seenItems, mobile && view === "list" ? null : roomId || activeId);
  useNotifications(agents, mobile && view === "list" ? null : roomId ? null : activeId, id => select(id));
  useEffect(() => { if (roomId && rooms.length && !room) setRoomId(null); }, [roomId, rooms, room]);

  const togglePanel = useCallback((tab: PanelTab, force = false) => {
    const { open, tab: currentTab } = panel;
    if (open && currentTab === tab && !force) { if (mobile) back(); else setPanel({ open: false }); return; }
    if (!open) pushMobile();
    setPanel({ open: true, tab });
  }, [panel, mobile, back, setPanel]); // eslint-disable-line react-hooks/exhaustive-deps

  const openFile = useCallback((file: PreviewFile) => {
    setPreview(file);
    if (!panel.open) pushMobile();
    setPanel({ open: true, tab: "files" });
  }, [panel.open, setPanel]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const saved = (event: Event) => {
      const detail = (event as CustomEvent<{ agent: string; count: number }>).detail;
      const agent = agents.find(row => row.id === detail.agent);
      toast(`${agent?.name || "Bot"} 學會了 ${detail.count} 個新技能`, "ok", { label: "查看", run: () => setDialog({ kind: "agent", id: detail.agent, section: "skills" }) });
    };
    window.addEventListener("lazyboy-skills-saved", saved);
    return () => window.removeEventListener("lazyboy-skills-saved", saved);
  }, [agents]);

  const agentActions = useCallback((agent: AgentRow, inChat = false): MenuItem[] => [
    { label: "Bot 設定", icon: <Pencil />, onSelect: () => setDialog({ kind: "agent", id: agent.id, section: "profile" }) },
    { label: "技能", icon: <Brain />, onSelect: () => setDialog({ kind: "agent", id: agent.id, section: "skills" }) },
    ...(!inChat ? [{ label: "定時任務", icon: <CalendarClock />, onSelect: () => setDialog({ kind: "routines", id: agent.id }) }] : []),
    ...(!inChat ? [
      { label: agent.pinned ? "取消釘選" : "釘選到最上面", icon: <Pin />, onSelect: () => void api.sidebar(agent.id, { pinned: !agent.pinned }).then(upsert).catch(err => toast(String(err instanceof Error ? err.message : err), "error")) },
      { label: agent.hidden ? "顯示在側欄" : "從側欄隱藏", icon: agent.hidden ? <Eye /> : <EyeOff />, onSelect: () => void api.sidebar(agent.id, { hidden: !agent.hidden, ...(agent.hidden ? {} : { pinned: false }) }).then(upsert).catch(err => toast(String(err instanceof Error ? err.message : err), "error")) },
    ] : []),
    ...(!inChat ? [{ label: "標為未讀", icon: <CircleDot />, onSelect: () => markUnread(agent.id) }] : []),
    { label: "複製 Bot", icon: <Copy />, onSelect: () => void api.duplicate(agent.id).then(row => { upsert(row); toast(`已建立「${row.name}」`, "ok", { label: "打開", run: () => select(row.id) }); }).catch(err => toast(String(err instanceof Error ? err.message : err), "error")) },
    { label: isMuted(agent.id) ? "開啟通知" : "關閉通知", icon: isMuted(agent.id) ? <Bell /> : <BellOff />, onSelect: () => { setMuted(agent.id, !isMuted(agent.id)); toast(isMuted(agent.id) ? `已關閉 ${agent.name} 的通知` : `已開啟 ${agent.name} 的通知`, "ok"); } },
    { label: "分享 Bot", icon: <Share2 />, onSelect: () => setDialog({ kind: "share", agent }) },
    { label: "教它一件事", icon: <GraduationCap />, onSelect: () => setDialog({ kind: "teach", agent }) },
    ...(groups.length ? [{ label: "移到分區", icon: <Users />, onSelect: () => setDialog({ kind: "move", agent }) }] : []),
    "separator",
    { label: "清除對話", icon: <Eraser />, onSelect: () => {
      const current = inChat && agent.id === active?.id;
      const side = current && session.thread && !session.thread.main ? session.thread : null;
      setDialog({
        kind: "confirm", label: "清除對話",
        title: side ? `清除「${side.title}」的對話？` : `清除與「${agent.name}」的對話？`,
        body: side ? "只清除這段對話的紀錄，其他對話、Bot 的技能、檔案與電腦都會保留。" : "主要對話的紀錄會清空，其他對話、Bot 的技能、檔案與電腦都會保留。",
        run: async () => {
          if (current) await session.clear();
          else { await api.clearConversation(agent.id); if (agent.id === active?.id) session.refresh(); }
          void refresh(); toast("對話已清除", "ok");
        },
      });
    } },
    { label: "刪除 Bot", icon: <Trash2 />, danger: true, onSelect: () => setDialog({
      kind: "confirm", title: `刪除「${agent.name}」？`, body: "會永久刪除這個 Bot 的對話、技能、檔案與電腦，無法復原。", label: "永久刪除", danger: true,
      run: async () => {
        await api.deleteAgent(agent.id);
        remove(agent.id);
        if (agent.id === activeId) { setActiveId(null); if (state.current.mobile) back(); }
        toast(`已刪除 ${agent.name}`, "ok");
      },
    }) },
  ], [groups.length, active?.id, activeId, session, refresh, remove, back]);

  const groupActions = useCallback((group: AgentGroup): MenuItem[] => [
    { label: "編輯分區", icon: <Pencil />, onSelect: () => setDialog({ kind: "group", group }) },
    { label: "刪除分區", icon: <Trash2 />, danger: true, onSelect: () => setDialog({
      kind: "confirm", title: `刪除分區「${group.name}」？`, body: "分區內的 Bot 不會被刪除，會移回未分區。", label: "刪除分區", danger: true,
      run: async () => applyTeam(await api.manageTeam({ action: "delete_group", group_id: group.id })),
    }) },
  ], [applyTeam]);

  const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX, startWidth = panel.width;
    const handle = event.currentTarget;
    handle.setPointerCapture(event.pointerId);
    document.body.classList.add("is-resizing");
    const max = Math.max(420, window.innerWidth - 280 - 420);
    const move = (e: PointerEvent) => setPanelState(current => ({ ...current, width: Math.round(Math.min(max, Math.max(360, startWidth + startX - e.clientX))) }));
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      document.body.classList.remove("is-resizing");
      setPanelState(current => { writeStorage(PANEL_KEY, JSON.stringify(current)); return current; });
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };

  // On phones both pages stay mounted and slide, so going back and forth never reloads or flashes.
  const showSidebar = true;
  const showChat = true;
  const appRef = useRef<HTMLDivElement>(null);
  useSwipeBack(appRef, mobile && view === "chat" && !panel.open, back);
  useEffect(() => {
    const root = appRef.current;
    if (!root) return;
    const list = root.querySelector(":scope > .sidebar"), chat = root.querySelector(":scope > .chat");
    // The page slid off screen must not take focus or be read out.
    list?.toggleAttribute("inert", mobile && view === "chat");
    chat?.toggleAttribute("inert", mobile && view === "list");
  });
  const panelVisible = !!active && panel.open && (!mobile || view === "chat");
  const botWorking = BUSY_STATES.has(session.activity.state) && session.activity.state !== "waiting_input" && session.activity.state !== "waiting_approval";
  const dialogAgent = dialog && dialog.kind === "agent" ? agents.find(agent => agent.id === dialog.id) : undefined;
  const panelStyle = useMemo(() => (mobile ? undefined : { width: panel.width }), [mobile, panel.width]);

  return (
    <AvatarLookProvider value={looks}>
      <div ref={appRef} className={`app${mobile ? " is-mobile" : ""}${panelVisible ? " has-panel" : ""}`} data-view={view}>
        {showSidebar ? (
          <Sidebar agents={agents} groups={groups} activeId={mobile || roomId ? null : activeId} loaded={loaded} onSelect={select}
            rooms={rooms} activeRoomId={mobile ? null : roomId} onSelectRoom={selectRoom} onCreateRoom={() => setDialog({ kind: "room" })} unread={unread}
            onCreate={() => setDialog({ kind: "create" })} onImport={() => setDialog({ kind: "import" })}
            onCreateGroup={() => setDialog({ kind: "group" })} onSettings={() => setDialog({ kind: "settings" })}
            agentActions={agent => agentActions(agent)} groupActions={groupActions} />
        ) : null}

        {showChat ? (roomId && room ? (
          <RoomView key={room.id} roomId={room.id} mobile={mobile} onBack={back}
            onEdit={row => setDialog({ kind: "room", room: row })}
            onDeleted={() => { setRoomId(null); void refreshRooms(); if (state.current.mobile) back(); }}
            onOpenBot={select} onOpenFile={openFile} onChanged={() => void refreshRooms()} />
        ) : active ? (
          <Conversation agent={active} session={session} mobile={mobile} panel={panel} onBack={back}
            team={agents} rooms={rooms} onOpenRoom={selectRoom} onOpenBot={select} onPeer={peer => setDialog({ kind: "peer", id: active.id, peer })}
            onTogglePanel={togglePanel} onOpenFile={openFile}
            onRoutines={() => setDialog({ kind: "routines", id: active.id })}
            onTeach={() => setDialog({ kind: "teach", agent: active })}
            onPick={text => appendDraft(draftKey(active.id, session.threadId), text)}
            onOpenSettings={() => setDialog({ kind: "agent", id: active.id, section: "profile" })}
            menuItems={agentActions(active, true)} />
        ) : (
          <main className="chat chat-welcome">
            {!loaded ? null : error && !agents.length ? (
              <div className="pane-error" role="alert">無法連線到服務：{error}<button type="button" className="btn small" onClick={() => void refresh()}>重試</button></div>
            ) : (
              <div className="chat-empty">
                <img src="/lazyboy-round.svg" width={72} height={72} alt="" />
                <h2>歡迎使用 LazyBoy</h2>
                <p className="chat-empty-lead">每個 Bot 都有自己的電腦、技能與檔案。建立第一個 Bot，交代它工作吧。</p>
                <div className="welcome-actions">
                  <button type="button" className="btn primary" onClick={() => setDialog({ kind: "create" })}><Plus />新增 Bot</button>
                  <button type="button" className="btn" onClick={() => setDialog({ kind: "import" })}><Import />匯入 Bot</button>
                </div>
              </div>
            )}
          </main>
        )) : null}

        {panelVisible && active ? (
          <>
            {!mobile ? <div className="resizer" role="separator" aria-orientation="vertical" aria-label="調整面板寬度" onPointerDown={startResize} onDoubleClick={() => setPanel({ width: 560 })} /> : null}
            <aside className="side-panel" style={panelStyle} aria-label={panel.tab === "computer" ? "電腦" : "檔案"}>
              <header className="side-panel-head">
                {mobile ? <button type="button" className="icon-btn" aria-label="返回對話" onClick={back}><ChevronLeft /></button> : null}
                {mobile ? <Avatar {...{ lookId: active.id, name: active.name, color: active.avatar_color || undefined, src: active.avatar_url || undefined }} size={24} gaze={false} /> : null}
                <div className="segmented" role="tablist" aria-label="面板">
                  <button type="button" role="tab" aria-selected={panel.tab === "computer"} onClick={() => setPanel({ tab: "computer" })}><Monitor />電腦</button>
                  <button type="button" role="tab" aria-selected={panel.tab === "files"} onClick={() => setPanel({ tab: "files" })}><FolderOpen />檔案{session.files.length ? <small>{session.files.length}</small> : null}</button>
                </div>
                {!mobile ? <button type="button" className="icon-btn" aria-label="收起面板" title="收起面板" onClick={() => setPanel({ open: false })}><PanelRightClose /></button> : null}
              </header>
              <ComputerPane key={active.id} agent={active} visible={panel.tab === "computer"} mobile={mobile} botWorking={botWorking}
                question={session.activity.question || null} lesson={session.lesson} onTeach={async body => {
                  const result = await session.teach(body);
                  // On a phone the computer covers the chat; the bot's questions and report appear there.
                  if (mobile && (body.action === "finish" || body.action === "cancel")) back();
                  return result;
                }} onSend={session.send} onAnswered={() => void session.refreshActivity(active.id)} />
              {panel.tab === "files" ? (
                <FilesPane agentId={active.id} files={session.files} preview={preview} onPreview={setPreview} onReload={() => void session.reloadFiles(active.id)} />
              ) : null}
            </aside>
          </>
        ) : null}
      </div>

      {dialog?.kind === "settings" ? (
        <SettingsDialog agentId={activeId} mobile={mobile} initialSection={dialog.section} onAbout={() => setDialog({ kind: "about" })} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.kind === "about" ? <AboutDialog mobile={mobile} onClose={() => setDialog(mobile ? { kind: "settings" } : null)} /> : null}
      {dialog?.kind === "create" ? (
        <AgentSettingsDialog mode="create" onClose={() => setDialog(null)} onSaved={row => { setDialog(null); upsert(row); void refresh(); select(row.id); }} />
      ) : null}
      {dialog?.kind === "agent" && dialogAgent ? (
        <AgentSettingsDialog key={dialogAgent.id} agent={dialogAgent} initialSection={dialog.section}
          onShare={row => setDialog({ kind: "share", agent: row })} onClose={() => { setDialog(null); setLooks(readAvatarLooks()); }}
          onSaved={row => { upsert(row); setLooks(readAvatarLooks()); void refresh(); }} />
      ) : null}
      {dialog?.kind === "routines" ? <RoutinesDialog key={dialog.id} agentId={dialog.id} onClose={() => setDialog(null)} onOpen={file => { setDialog(null); if (dialog.id !== activeId) select(dialog.id); openFile(file); }} returnFocus={routinesButton} /> : null}
      {dialog?.kind === "share" ? <AgentShareDialog agent={dialog.agent} onClose={() => setDialog(null)} /> : null}
      {dialog?.kind === "import" ? (
        <TemplateImportDialog onCreated={() => void refresh()} onClose={() => setDialog(null)} onImported={row => { setDialog(null); upsert(row); void refresh(); select(row.id); }} />
      ) : null}
      {dialog?.kind === "teach" ? (
        <TeachStartDialog agent={dialog.agent} mobile={mobile} onClose={() => setDialog(null)} onStart={async (goal, trial) => {
          const agent = dialog.agent;
          // The demonstration happens on the bot's computer, in the conversation it was started from.
          const thread = agent.id === activeId ? session.threadId : agent.id;
          if (agent.id === activeId) await session.teach({ action: "start", goal, trial, thread });
          else { await api.teach(agent.id, { action: "start", goal, trial, thread }); select(agent.id); }
          togglePanel("computer", true);
        }} />
      ) : null}
      {dialog?.kind === "group" ? <GroupEditor group={dialog.group} agents={agents} onClose={() => setDialog(null)} onSaved={applyTeam} /> : null}
      {dialog?.kind === "move" ? <MoveDialog agent={dialog.agent} groups={groups} onClose={() => setDialog(null)} onSaved={applyTeam} /> : null}
      {dialog?.kind === "room" ? (
        <RoomDialog room={dialog.room} agents={agents} onClose={() => setDialog(null)}
          onSaved={row => { setRooms(current => [row, ...current.filter(item => item.id !== row.id)]); void refreshRooms(); if (!dialog.room) selectRoom(row.id); }} />
      ) : null}
      {dialog?.kind === "peer" ? <PeerDialog agentId={dialog.id} peerId={dialog.peer} team={agents} onClose={() => setDialog(null)} /> : null}
      {dialog?.kind === "confirm" ? (
        <ConfirmDialog title={dialog.title} body={dialog.body} confirmLabel={dialog.label} danger={dialog.danger} onConfirm={dialog.run} onClose={() => setDialog(null)} />
      ) : null}
      <Toaster />
    </AvatarLookProvider>
  );
}
