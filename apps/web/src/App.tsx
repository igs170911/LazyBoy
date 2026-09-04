import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BotIcon, Brain, ChevronDown, ChevronsRight, CircleHelp, ClipboardPaste, Computer, Ellipsis, Info, LogOut, Megaphone, Pin, Plug, Plus, RefreshCw, Settings, Smartphone, Square, Users, X } from "./animated-icons";
import UseAnimations from "react-useanimations";
import loading from "react-useanimations/lib/loading";
import loading2 from "react-useanimations/lib/loading2";
import arrowUp from "react-useanimations/lib/arrowUp";
import bookmark from "react-useanimations/lib/bookmark";
import copy from "react-useanimations/lib/copy";
import folder from "react-useanimations/lib/folder";
import mail from "react-useanimations/lib/mail";
import menu from "react-useanimations/lib/menu";
import plusToX from "react-useanimations/lib/plusToX";
import settings from "react-useanimations/lib/settings";
import trash2 from "react-useanimations/lib/trash2";
import visibility from "react-useanimations/lib/visibility";
import visibility2 from "react-useanimations/lib/visibility2";
import searchToX from "react-useanimations/lib/searchToX";
import { api, ApiError } from "./api";
import { t } from "./i18n";
import blobshape from "blobshape";
import type { AvatarShape, Bot, ComputerMode, ComputerStatus, McpServer, McpTransport, MemoryItem, Message, Room, RoomMember, Session } from "./types";

const blankComputer:ComputerStatus={botId:"",mode:"team",state:"stopped",controlHolder:"none",takeoverRequested:false,busyBotName:null,busySessionId:null,busyRunId:null,display:null,profileMode:"per-bot",screenAvailable:false};
const SESSION_STORE="lazyboy.sessionByBot";
const PANE_STORE="lazyboy.rightPane";
const WORKSPACE_STORE="lazyboy.workspace";
type RightPart="computer"|"memory"|"settings"|"plugins";
type AccountDialog="phone"|"settings"|"about"|"help"|"feedback"|null;
function readSessionStore():Record<string,string>{try{const raw=localStorage.getItem(SESSION_STORE);return raw?JSON.parse(raw) as Record<string,string>:{}}catch{return {}}}
function writeSessionStore(botId:string,sessionId:string){const store=readSessionStore();store[botId]=sessionId;localStorage.setItem(SESSION_STORE,JSON.stringify(store))}
function readPaneStore():{collapsed:boolean;part:RightPart}{try{const raw=localStorage.getItem(PANE_STORE);if(!raw)return{collapsed:false,part:"computer"};const value=JSON.parse(raw) as {collapsed?:boolean;part?:string};return{collapsed:Boolean(value.collapsed),part:value.part==="memory"||value.part==="settings"||value.part==="plugins"?value.part:"computer"}}catch{return{collapsed:false,part:"computer"}}}
type WorkspacePrefs={name:string;showHidden:boolean};
function readWorkspace():WorkspacePrefs{try{const raw=localStorage.getItem(WORKSPACE_STORE);if(!raw)return{name:t("localWorkspace"),showHidden:false};const value=JSON.parse(raw) as {name?:string;showHidden?:boolean};const name=value.name?.trim();return{name:name&&name!=="Local workspace"?name:t("localWorkspace"),showHidden:Boolean(value.showHidden)}}catch{return{name:t("localWorkspace"),showHidden:false}}}

const botColors=["#3ec5a8","#f5a03c","#6a6bf5","#9b5cf6","#3b82f6","#d9508a"];
function Avatar({name,color,shape="blob",active=false,thinking=false,online=false,size=32}:{name:string;color?:string;shape?:AvatarShape;active?:boolean;thinking?:boolean;online?:boolean;size?:number}){const hash=[...`${name}-${shape}`].reduce((n,c)=>(n*31+c.charCodeAt(0))>>>0,7);const resolved=color||botColors[hash%botColors.length];const generated=shape==="blob"||shape.startsWith("organic-");const edges=shape==="blob"?7:Number(shape.slice(8));const generatedPath=generated?blobshape({size:100,growth:shape==="blob"?8:6,edges,seed:hash||1}).path:null;const specialPath=shape==="cloud"?"M23 80C9 80 2 70 7 57C10 48 18 44 27 45C29 30 40 21 53 24C63 25 70 32 72 43C87 42 96 51 95 64C94 75 86 81 73 80C67 88 56 90 48 84C39 91 28 89 23 80Z":shape==="drop"?"M52 5C44 20 18 44 18 65C18 83 32 95 50 95C69 95 83 82 82 64C81 43 60 21 52 5Z":shape==="cat"?"M22 42C16 36 14 20 17 8C28 14 38 22 42 28C47 26.5 53 26.5 58 28C62 22 72 14 83 8C86 20 84 36 78 42C86 50 90 58 90 64C90 82 73 94 50 94C27 94 10 82 10 64C10 58 14 50 22 42Z":shape==="bunny"?"M38 34C32 30 28 18 30 6C40 8 46 20 47 32C49 31 51 31 53 32C54 20 60 8 70 6C72 18 68 30 62 34C76 42 84 54 84 66C84 84 69 94 50 94C31 94 16 84 16 66C16 54 24 42 38 34Z":shape==="star"?"M50 8L62 38L94 40L69 60L77 91L50 74L23 91L31 60L6 40L38 38Z":shape==="heart"?"M50 90C30 74 10 58 10 38C10 22 22 12 35 12C43 12 48 16 50 22C52 16 57 12 65 12C78 12 90 22 90 38C90 58 70 74 50 90Z":shape==="egg"?"M50 6C32 6 16 34 16 58C16 80 31 94 50 94C69 94 84 80 84 58C84 34 68 6 50 6Z":shape==="ghost"?"M50 6C29 6 15 24 15 46L15 82C15 86 18 89 22 89C26 89 28 85 32 85C36 85 38 89 42 89C46 89 48 85 52 85C56 85 58 89 62 89C66 89 68 85 72 85C76 85 78 89 82 89C86 89 89 86 89 82L89 46C89 24 75 6 50 6Z":shape==="sprout"?"M50 36C48 20 38 10 24 10C26 26 36 34 48 36C30 38 18 52 18 66C18 84 32 94 50 94C68 94 82 84 82 66C82 52 70 38 52 36C64 34 74 26 76 10C62 10 52 20 50 36Z":shape==="cactus"?"M50 10C38 10 28 18 28 30L28 58L20 58C15 58 12 61 12 66C12 71 15 74 20 74L28 74L28 78C28 89 38 96 50 96C62 96 72 89 72 78L72 64L80 64C85 64 88 61 88 56C88 51 85 48 80 48L72 48L72 30C72 18 62 10 50 10Z":shape==="mushroom"?"M50 12C28 12 12 28 12 46C12 51 15 54 21 54L38 54C37 59 38 64 38 68L38 82C38 90 42 94 50 94C58 94 62 90 62 82L62 68C62 64 63 59 62 54L79 54C85 54 88 51 88 46C88 28 72 12 50 12Z":shape==="paw"?"M8 40A9 9 0 1 0 26 40A9 9 0 1 0 8 40Z M24.5 27A10.5 10.5 0 1 0 45.5 27A10.5 10.5 0 1 0 24.5 27Z M54.5 27A10.5 10.5 0 1 0 75.5 27A10.5 10.5 0 1 0 54.5 27Z M74 40A9 9 0 1 0 92 40A9 9 0 1 0 74 40Z M20 66A29 22 0 1 0 78 66A29 22 0 1 0 20 66Z":null;const path=generatedPath||specialPath;const svgShape=Boolean(path);return <span className={`avatar robot avatar-${shape} ${svgShape?"avatar-organic":""} ${active?"online":""} ${thinking?"thinking":""}`} style={{"--bot-color":resolved,"--avatar-size":`${size}px`} as React.CSSProperties}>{path&&<svg className="avatar-shape" viewBox="0 0 100 100" aria-hidden="true"><path d={path}/></svg>}<span className="robot-eyes"><i/><i/></span>{(online||active)&&<i className="presence" aria-hidden="true"/>}</span>}
function AvatarStack({members,size=38,online=false,thinkingIds}:{members:RoomMember[];size?:number;online?:boolean;thinkingIds?:string[]}){
  if(members.length===1){const member=members[0];return <Avatar name={member.name} color={member.avatarColor} shape={member.avatarShape} size={size} thinking={Boolean(thinkingIds?.includes(member.id))} online={online}/>}
  const pair=members.length===2;
  const miniSize=Math.round(size*(pair?.65:.54));
  const shown=members.slice(0,members.length>3?2:3);
  const positions=pair?[{left:0,top:0},{left:size-miniSize,top:size-miniSize}]:[{left:(size-miniSize)/2,top:0},{left:0,top:size-miniSize},{left:size-miniSize,top:size-miniSize}];
  return <span className="avatar-stack" style={{width:size,height:size}}>
    {shown.map((member,index)=><span className="stack-item" key={member.id} style={{...positions[index],zIndex:index+1}}><Avatar name={member.name} color={member.avatarColor} shape={member.avatarShape} size={miniSize} thinking={Boolean(thinkingIds?.includes(member.id))} online={Boolean(online&&index===shown.length-1)}/></span>)}
    {members.length>3&&<span className="stack-extra" style={{left:size-miniSize,top:size-miniSize,zIndex:3,width:miniSize,height:miniSize}}>+{members.length-2}</span>}
  </span>
}
function WorkspaceAvatar({name}:{name:string}){const parts=name.trim().split(/\s+/).filter(Boolean);const initials=(parts.length>1?parts.map(part=>part[0]).join(""):parts[0]?.slice(0,2)||"LB").slice(0,2).toUpperCase();return <span className="workspace-avatar" aria-hidden="true">{initials}</span>}
function modeLabel(mode:ComputerMode){return mode==="team"?t("sharedComputer"):t("privateComputer")}
function stateLabel(state:ComputerStatus["state"]){return ({stopped:t("stopped"),booting:t("booting"),running:t("running"),suspended:t("suspended"),error:t("error")})[state]}
function inboxTime(value:string|null){if(!value)return "";const date=new Date(value),now=new Date();if(date.toDateString()===now.toDateString())return new Intl.DateTimeFormat("zh-TW",{hour:"2-digit",minute:"2-digit",hour12:false}).format(date);const days=Math.floor((new Date(now.getFullYear(),now.getMonth(),now.getDate()).getTime()-new Date(date.getFullYear(),date.getMonth(),date.getDate()).getTime())/86400000);if(days<7)return new Intl.DateTimeFormat("zh-TW",{weekday:"long"}).format(date);return new Intl.DateTimeFormat("zh-TW",{month:"numeric",day:"numeric"}).format(date)}

export function App(){
  const [bots,setBots]=useState<Bot[]>([]); const [rooms,setRooms]=useState<Room[]>([]); const [activeId,setActiveId]=useState<string|null>(null); const [activeRoomId,setActiveRoomId]=useState<string|null>(null); const [busyMembers,setBusyMembers]=useState<RoomMember[]>([]);
  const [sessions,setSessions]=useState<Session[]>([]); const [activeSessionId,setActiveSessionId]=useState<string|null>(null);
  const [messages,setMessages]=useState<Message[]>([]); const [computer,setComputer]=useState<ComputerStatus>(blankComputer);
  const [screenUrl,setScreenUrl]=useState<string|null>(null); const [draft,setDraft]=useState(""); const [query,setQuery]=useState("");
  const [createOpen,setCreateOpen]=useState(false); const [createMenuOpen,setCreateMenuOpen]=useState(false); const [groupOpen,setGroupOpen]=useState(false); const [deleteOpen,setDeleteOpen]=useState(false); const [computerOpen,setComputerOpen]=useState(false);
  const paneStart=readPaneStore();
  const [rightCollapsed,setRightCollapsed]=useState(paneStart.collapsed); const [rightPart,setRightPart]=useState<RightPart>(paneStart.part);
  const [sessionMenuOpen,setSessionMenuOpen]=useState(false); const [clearOpen,setClearOpen]=useState(false); const [sessionToDelete,setSessionToDelete]=useState<string|null>(null); const [remembered,setRemembered]=useState<Record<string,boolean>>({});
  const [mobileNav,setMobileNav]=useState(false); const [error,setError]=useState<string|null>(null); const [busy,setBusy]=useState(false);
  const [desktopClipboard,setDesktopClipboard]=useState(""); const active=bots.find(b=>b.id===activeId)||null;
  const activeRoom=rooms.find(room=>room.id===activeRoomId)||null;
  const [clipboardOpen,setClipboardOpen]=useState(false);
  const [authRequired,setAuthRequired]=useState(false);
  const workspaceStart=readWorkspace();
  const [showHidden,setShowHidden]=useState(workspaceStart.showHidden);const[context,setContext]=useState<{bot:Bot;x:number;y:number}|null>(null);
  const [roomContext,setRoomContext]=useState<{room:Room;x:number;y:number}|null>(null);
  const [roomToDelete,setRoomToDelete]=useState<Room|null>(null); const [mcpServers,setMcpServers]=useState<McpServer[]>([]);
  const [accountOpen,setAccountOpen]=useState(false); const [accountDialog,setAccountDialog]=useState<AccountDialog>(null);
  const [workspaceName,setWorkspaceName]=useState(workspaceStart.name);
  const sendingRef=useRef(false);
  const roomsRef=useRef(rooms);
  roomsRef.current=rooms;
  const filtered=useMemo(()=>bots.filter(b=>(showHidden||!b.hidden)&&b.name.toLowerCase().includes(query.toLowerCase())),[bots,query,showHidden]);
  const filteredRooms=useMemo(()=>{const q=query.trim().toLowerCase();return rooms.filter(room=>!q||room.name.toLowerCase().includes(q)||room.members.some(member=>member.name.toLowerCase().includes(q)))},[rooms,query]);
  const sections=useMemo(()=>{const map=new Map<string,Bot[]>();for(const bot of filtered){const key=bot.pinned?t("pinned"):bot.groupName||t("agentGroup");map.set(key,[...(map.get(key)||[]),bot])}return [...map.entries()]},[filtered]);
  const paneBotId=busyMembers[0]?.id||activeRoom?.members[0]?.id||activeId;
  const paneBot=bots.find(bot=>bot.id===paneBotId)||active;
  const workingMembers=activeRoom?busyMembers:active&&computer.busySessionId===activeSessionId?[{id:active.id,name:active.name,avatarColor:active.avatarColor,avatarShape:active.avatarShape}]:[];

  const loadMcp=useCallback(async()=>{setMcpServers(await api<McpServer[]>("/api/mcp-servers").catch(()=>[] as McpServer[]))},[]);
  const loadBots=useCallback(async()=>{const [next,nextRooms]=await Promise.all([api<Bot[]>("/api/bots"),api<Room[]>("/api/rooms").catch(()=>[] as Room[])]);setBots(next);setRooms(nextRooms);setActiveRoomId(id=>id&&nextRooms.some(room=>room.id===id)?id:null);setActiveId(id=>id&&next.some(b=>b.id===id)?id:null);await loadMcp()},[loadMcp]);
  const sessionStoreKey=activeRoomId?`room:${activeRoomId}`:activeId;
  const sessionsPath=activeRoomId?`/api/rooms/${activeRoomId}/sessions`:activeId?`/api/bots/${activeId}/sessions`:null;
  const loadSessions=useCallback(async()=>{if(!sessionsPath){setSessions([]);setActiveSessionId(null);return}const next=await api<Session[]>(sessionsPath);setSessions(next);setActiveSessionId(id=>{if(id&&next.some(session=>session.id===id))return id;const stored=sessionStoreKey?readSessionStore()[sessionStoreKey]:undefined;if(stored&&next.some(session=>session.id===stored))return stored;return next[0]?.id||null})},[sessionsPath,sessionStoreKey]);
  const refresh=useCallback(async()=>{if(!activeSessionId)return;const messagesJob=api<Message[]>(`/api/sessions/${activeSessionId}/messages`).then(setMessages);
    let computerBot=activeId;
    if(activeRoomId){
      try{const status=await api<{busy:RoomMember[]}>(`/api/rooms/${activeRoomId}/status`);setBusyMembers(status.busy);computerBot=status.busy[0]?.id||roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||null}catch{setBusyMembers([]);computerBot=roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||null}
    }else setBusyMembers([]);
    if(!computerBot){await messagesJob;return}
    await Promise.all([messagesJob,api<ComputerStatus>(`/api/computer/${computerBot}/status`).then(setComputer),api<{url:string|null}>(`/api/computer/${computerBot}/screen`).then(screen=>setScreenUrl(screen.url)).catch(()=>setScreenUrl(null))])},[activeId,activeRoomId,activeSessionId]);
  useEffect(()=>{loadBots().catch(e=>{if(e instanceof ApiError&&e.status===401)setAuthRequired(true);else setError(e.message)})},[loadBots]);
  useEffect(()=>{if(activeId||activeRoomId||bots.length===0)return;setActiveId(bots[0].id)},[bots,activeId,activeRoomId]);
  useEffect(()=>{setMessages([]);loadSessions().catch(e=>setError(e.message))},[loadSessions]);
  useEffect(()=>{if(!activeSessionId||(!activeId&&!activeRoomId)){setMessages([]);if(!activeId&&!activeRoomId)setComputer(blankComputer);return}refresh().catch(e=>setError(e.message));const timer=setInterval(()=>{refresh().catch(()=>{});const beat=roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||activeId;if(beat)api(`/api/computer/${beat}/heartbeat`,{method:"POST",body:"{}"}).catch(()=>{})},2000);return()=>clearInterval(timer)},[activeId,activeRoomId,activeSessionId,refresh]);
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.origin!==location.origin||!event.data)return;if(event.data.type==="lazyboy-desktop-clipboard"){const text=String(event.data.text||"");setDesktopClipboard(text);navigator.clipboard.writeText(text).catch(()=>{})}};window.addEventListener("message",listener);return()=>window.removeEventListener("message",listener)});
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.origin!==location.origin||event.data?.type!=="lazyboy-request-control"||!paneBotId)return;void action(()=>api(`/api/computer/${paneBotId}/takeover`,{method:"POST",body:"{}"}))};window.addEventListener("message",listener);return()=>window.removeEventListener("message",listener)},[paneBotId]);
  useEffect(()=>{const close=()=>{setContext(null);setRoomContext(null);setSessionMenuOpen(false);setAccountOpen(false);setCreateMenuOpen(false)};window.addEventListener("click",close);return()=>window.removeEventListener("click",close)},[]);
  useEffect(()=>{const onKey=(event:KeyboardEvent)=>{if(event.key!=="Escape")return;setAccountOpen(false);setAccountDialog(null);setCreateMenuOpen(false);setSessionMenuOpen(false);setContext(null);setRoomContext(null)};window.addEventListener("keydown",onKey);return()=>window.removeEventListener("keydown",onKey)},[]);
  useEffect(()=>{localStorage.setItem(WORKSPACE_STORE,JSON.stringify({name:workspaceName,showHidden}))},[workspaceName,showHidden]);
  useEffect(()=>{if(!sessionStoreKey||!activeSessionId)return;if(!sessions.some(session=>session.id===activeSessionId))return;if(!activeRoomId&&!sessions.some(session=>session.id===activeSessionId&&session.botId===activeId))return;writeSessionStore(sessionStoreKey,activeSessionId)},[sessionStoreKey,activeId,activeRoomId,activeSessionId,sessions]);
  useEffect(()=>{localStorage.setItem(PANE_STORE,JSON.stringify({collapsed:rightCollapsed,part:rightPart}))},[rightCollapsed,rightPart]);
  function openPane(part:RightPart){setRightPart(part);setRightCollapsed(false)}

  const sessionBusy=workingMembers.length>0;
  const otherSessionBusy=Boolean(computer.busyBotName&&!sessionBusy);
  const chatName=activeRoom?.name||active?.name||"";

  async function action(work:()=>Promise<unknown>){setBusy(true);setError(null);try{await work();await refresh()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function stopChat(){if(activeSessionId)await api(`/api/sessions/${activeSessionId}/stop`,{method:"POST",body:"{}"});else if(active)await api(`/api/bots/${active.id}/stop`,{method:"POST",body:"{}"});}
  async function send(event:FormEvent){event.preventDefault();if(sendingRef.current||busy)return;const text=draft.trim();if((!active&&!activeRoom)||!activeSessionId||!text)return;sendingRef.current=true;setDraft("");try{await action(async()=>{await api(`/api/sessions/${activeSessionId}/messages`,{method:"POST",body:JSON.stringify({text,clientNonce:crypto.randomUUID()})});await loadSessions()})}finally{sendingRef.current=false}}
  function selectSession(id:string){setActiveSessionId(id);setSessionMenuOpen(false);if(sessionStoreKey)writeSessionStore(sessionStoreKey,id)}
  async function createSession(){if(!sessionsPath||!sessionStoreKey)return;setSessionMenuOpen(false);setBusy(true);setError(null);try{const session=await api<Session>(sessionsPath,{method:"POST",body:JSON.stringify({title:t("newConversation")})});writeSessionStore(sessionStoreKey,session.id);const next=await api<Session[]>(sessionsPath);setSessions(next);setActiveSessionId(session.id);setMessages([])}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function clearSession(){if(!activeSessionId)return;setClearOpen(false);setSessionMenuOpen(false);await action(async()=>{await api(`/api/sessions/${activeSessionId}/messages`,{method:"DELETE"});setMessages([]);await loadSessions()})}
  async function deleteSession(id:string){if(!sessionsPath||!sessionStoreKey)return;setSessionMenuOpen(false);setBusy(true);setError(null);try{await api(`/api/sessions/${id}`,{method:"DELETE"});const next=await api<Session[]>(sessionsPath);setSessions(next);const pick=id===activeSessionId||!next.some(session=>session.id===activeSessionId)?next[0]?.id||null:activeSessionId;setActiveSessionId(pick);if(pick)writeSessionStore(sessionStoreKey,pick)}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function rememberMessage(message:Message){const botId=message.speakerBotId||active?.id||activeRoom?.members[0]?.id;if(!botId||!message.body.trim())return;try{await api(`/api/bots/${botId}/memories`,{method:"POST",body:JSON.stringify({content:message.body,sessionId:activeSessionId})});setRemembered(current=>({...current,[message.id]:true}))}catch(e){setError(e instanceof Error?e.message:t("rememberFailed"))}}
  async function pasteClipboard(){try{const text=await navigator.clipboard.readText();document.querySelectorAll<HTMLIFrameElement>(".desktop-frame").forEach(frame=>frame.contentWindow?.postMessage({type:"lazyboy-host-clipboard",text},location.origin))}catch{setClipboardOpen(true)}}
  async function copyClipboard(){try{await navigator.clipboard.writeText(desktopClipboard)}catch{setError(t("clipboardWriteBlocked"))}}
  async function inbox(bot:Bot,actionName:string,groupName?:string|null){await api(`/api/bots/${bot.id}/inbox`,{method:"POST",body:JSON.stringify({action:actionName,groupName})});await loadBots()}
  function openBot(bot:Bot){setMobileNav(false);setSessionMenuOpen(false);if(bot.unreadCount>0)void inbox(bot,"read");if(bot.id===activeId&&!activeRoomId){if(!activeSessionId){const stored=readSessionStore()[bot.id];if(stored)setActiveSessionId(stored);else void loadSessions()}return}setMessages([]);setActiveSessionId(null);setActiveRoomId(null);setBusyMembers([]);setActiveId(bot.id)}
  function openRoom(room:Room){setMobileNav(false);setSessionMenuOpen(false);if(rightPart==="settings")setRightPart("computer");if(room.id===activeRoomId){if(!activeSessionId){const stored=readSessionStore()[`room:${room.id}`];if(stored)setActiveSessionId(stored);else void loadSessions()}return}setMessages([]);setActiveSessionId(null);setActiveId(null);setBusyMembers([]);setActiveRoomId(room.id)}
  async function deleteRoom(room:Room){setRoomToDelete(null);await action(async()=>{await api(`/api/rooms/${room.id}`,{method:"DELETE"});if(activeRoomId===room.id){setActiveRoomId(null);setActiveSessionId(null);setMessages([]);setBusyMembers([])}await loadBots()})}
  function openAccount(dialog:AccountDialog){setAccountOpen(false);setAccountDialog(dialog)}
  async function logout(){setAccountOpen(false);await api("/api/session",{method:"DELETE",body:"{}"}).catch(()=>{});setBots([]);setRooms([]);setMcpServers([]);setActiveId(null);setActiveRoomId(null);setAuthRequired(true)}
  const frame=screenUrl?<iframe className="desktop-frame" src={screenUrl} title={t("agentComputer")} allow="fullscreen; clipboard-read; clipboard-write"/>:<EmptyComputer state={computer.state}/>;
  const topTools=<nav className="top-tools" aria-label={t("workTools")}>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="computer"?"active":""}`} title={t("computer")} aria-label={t("computer")} onClick={()=>openPane("computer")}><Computer/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="memory"?"active":""}`} title={t("memory")} aria-label={t("memory")} onClick={()=>openPane("memory")} disabled={!paneBot}><Brain/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="plugins"?"active":""}`} title={t("plugins")} aria-label={t("plugins")} onClick={()=>openPane("plugins")}><Plug/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="settings"?"active":""}`} title={active?t("botSettings"):t("settings")} aria-label={active?t("botSettings"):t("settings")} onClick={()=>active?openPane("settings"):setAccountDialog("settings")}><Settings/></button>
  </nav>;

  if(authRequired)return <LoginScreen authenticated={async()=>{setAuthRequired(false);setError(null);try{await loadBots()}catch(e){if(e instanceof ApiError&&e.status===401)setAuthRequired(true);else setError(e instanceof Error?e.message:t("loginFailed"))}}}/>;

  return <div className={`app-shell ${rightCollapsed?"right-collapsed":"right-open"}`}>
    <aside className={`sidebar ${mobileNav?"open":""}`}>
      <div className="brand"><span>LazyBoy</span><div className="create-menu-wrap"><button className="icon-button" onClick={()=>setCreateMenuOpen(v=>!v)} aria-label={t("add")}><UseAnimations animation={plusToX} size={18} strokeColor="#dfdfe2"/></button>{createMenuOpen&&<div className="create-menu"><button onClick={()=>{setCreateMenuOpen(false);setCreateOpen(true)}}><BotIcon/>{t("addBot")}</button><button onClick={()=>{setCreateMenuOpen(false);setGroupOpen(true)}} disabled={bots.length===0}><Users/>{t("addGroup")}</button></div>}</div></div>
      <label className="search"><UseAnimations animation={searchToX} size={16} strokeColor="var(--muted)"/><input value={query} onChange={e=>setQuery(e.target.value)} placeholder={t("search")}/></label>
      <div className="bot-list">
        {filteredRooms.length>0&&<section className="bot-group"><div className="group-label">{t("groups")}</div>{filteredRooms.map(room=><button className={`bot-row room-row ${room.id===activeRoomId?"selected":""}`} key={room.id} onClick={()=>openRoom(room)} onContextMenu={e=>{e.preventDefault();setRoomContext({room,x:e.clientX,y:e.clientY})}}><span className="avatar-wrap"><AvatarStack members={room.members} online thinkingIds={room.id===activeRoomId?busyMembers.map(member=>member.id):[]}/>{room.unreadCount>0&&<i className="unread-dot" title={t("unreadMessages",{count:room.unreadCount})}/>}</span><span className="bot-copy"><strong>{room.name}</strong><small>{room.lastPreview||t("members",{count:room.members.length})}</small></span>{room.lastMessageAt&&<time className="row-time">{inboxTime(room.lastMessageAt)}</time>}</button>)}</section>}
        {sections.map(([label,items])=><section className="bot-group" key={label}><div className="group-label">{label}</div>{items.map(bot=><button className={`bot-row ${bot.id===activeId&&!activeRoomId?"selected":""}`} key={bot.id} onClick={()=>openBot(bot)} onContextMenu={e=>{e.preventDefault();setContext({bot,x:e.clientX,y:e.clientY})}}><span className="avatar-wrap"><Avatar name={bot.name} color={bot.avatarColor} shape={bot.avatarShape} active={bot.id===activeId&&!activeRoomId} online/>{bot.unreadCount>0&&<i className="unread-dot" title={t("unreadMessages",{count:bot.unreadCount})}/>}</span><span className="bot-copy"><strong>{bot.name}</strong><small>{modeLabel(bot.computerMode)}</small></span>{bot.tags?.[0]&&<span className="bot-tag side-tag">{bot.tags[0]}</span>}{bot.lastMessageAt&&<time className="row-time">{inboxTime(bot.lastMessageAt)}</time>}{bot.pinned&&<Pin className="row-pin"/>}</button>)}</section>)}
      </div>
      <button className="hidden-toggle" onClick={()=>setShowHidden(v=>!v)}>{showHidden?<UseAnimations animation={visibility2} size={14} strokeColor="var(--muted)"/>:<UseAnimations animation={visibility} size={14} strokeColor="var(--muted)"/>}{showHidden?t("hideHiddenItems"):t("showHiddenItems")}</button>
      <div className="sidebar-bottom">
        <button type="button" className={`plugin-row ${rightPart==="plugins"&&!rightCollapsed?"selected":""}`} onClick={()=>openPane("plugins")}>
          <span className="plugin-icon"><Plug/></span>
          <span className="bot-copy"><strong>{t("plugins")}</strong><small>{mcpServers.filter(server=>server.status==="connected").length>0?t("mcpConnected",{count:mcpServers.filter(server=>server.status==="connected").length}):t("connectMcpServer")}</small></span>
        </button>
        <div className="account-wrap" onClick={event=>event.stopPropagation()}>
          {accountOpen&&<div className="account-menu" role="menu">
            <button type="button" role="menuitem" onClick={()=>openAccount("phone")}><Smartphone/>{t("openOnPhone")}</button>
            <button type="button" role="menuitem" onClick={()=>openAccount("settings")}><Settings/>{t("settings")}</button>
            <button type="button" role="menuitem" disabled={!paneBot} onClick={()=>{setAccountOpen(false);openPane("memory")}}><Brain/>{t("memory")}</button>
            <button type="button" role="menuitem" onClick={()=>openAccount("about")}><Info/>{t("about")}</button>
            <button type="button" role="menuitem" onClick={()=>openAccount("help")}><CircleHelp/>{t("helpCenter")}</button>
            <button type="button" role="menuitem" onClick={()=>openAccount("feedback")}><Megaphone/>{t("sendFeedback")}</button>
            <hr/>
            <button type="button" role="menuitem" onClick={()=>void logout()}><LogOut/>{t("logout")}</button>
          </div>}
          <button type="button" className={`account ${accountOpen?"open":""}`} title={t("workspaceMenu")} aria-haspopup="menu" aria-expanded={accountOpen} onClick={()=>{setAccountOpen(open=>!open);setCreateMenuOpen(false)}}>
            <WorkspaceAvatar name={workspaceName}/><span>{workspaceName}</span><ChevronDown className="chevron"/>
          </button>
        </div>
      </div>
    </aside>

    <main className="chat-panel">
      <header className="topbar"><button className="icon-button mobile-menu" onClick={()=>setMobileNav(v=>!v)}><UseAnimations animation={menu} size={18} strokeColor="#dfdfe2"/></button>{activeRoom?<><AvatarStack members={activeRoom.members} online thinkingIds={busyMembers.map(member=>member.id)}/><strong>{activeRoom.name}</strong><SessionMenu sessions={sessions} activeSessionId={activeSessionId} open={sessionMenuOpen} setOpen={setSessionMenuOpen} busy={busy} onSelect={selectSession} onCreate={createSession} onDelete={id=>{setSessionMenuOpen(false);setSessionToDelete(id)}} onClear={()=>{setSessionMenuOpen(false);setClearOpen(true)}}/><span className="grow"/>{topTools}</>:active?<><Avatar name={active.name} color={active.avatarColor} shape={active.avatarShape} active online/><strong>{active.name}</strong><SessionMenu sessions={sessions} activeSessionId={activeSessionId} open={sessionMenuOpen} setOpen={setSessionMenuOpen} busy={busy} onSelect={selectSession} onCreate={createSession} onDelete={id=>{setSessionMenuOpen(false);setSessionToDelete(id)}} onClear={()=>{setSessionMenuOpen(false);setClearOpen(true)}}/><span className="grow"/>{topTools}</>:<><strong>{t("chooseBot")}</strong><span className="grow"/>{topTools}</>}</header>
      <div className="messages">{(activeRoom||active)&&messages.length===0?<div className="welcome">{activeRoom?<AvatarStack members={activeRoom.members} size={56} online/>:<Avatar name={active!.name} color={active!.avatarColor} shape={active!.avatarShape} active online size={64}/>}<h1>{activeRoom?t("startRoomDiscussion",{name:activeRoom.name}):t("startBotWork",{name:active!.name})}</h1><p>{activeRoom?t("roomWillReply",{names:activeRoom.members.map(member=>member.name).join("、")}):active!.description||t("botWelcome")}</p></div>:messages.map(message=>{const spoken=message.role!=="user"&&Boolean(activeRoom);const speakerName=message.speakerName||(spoken?paneBot?.name:undefined);const speakerShape=(message.speakerShape||paneBot?.avatarShape||"blob") as AvatarShape;return <div key={message.id} className={`message ${message.role} ${spoken?"spoken":""}`}>{spoken&&<span className="msg-avatar"><Avatar name={speakerName||"agent"} color={message.speakerColor||undefined} shape={speakerShape} size={22}/></span>}{spoken&&<b className="speaker" style={{color:message.speakerColor||undefined}}>{speakerName}</b>}<span className="message-body">{message.body}</span>{message.body.trim()&&<button type="button" className={`remember-msg ${remembered[message.id]?"saved":""}`} title={remembered[message.id]?t("remembered"):t("remember")} disabled={!!remembered[message.id]} onClick={()=>void rememberMessage(message)}><UseAnimations animation={bookmark} size={14} strokeColor="var(--muted)"/></button>}</div>})}{workingMembers.map(member=><div className="thinking-row" key={member.id}><Avatar name={member.name} color={member.avatarColor} shape={member.avatarShape} thinking online/><span className="working-label">{t("working",{name:member.name})}</span></div>)}</div>
      {error&&<div className="error-banner"><span>{error}</span><button onClick={()=>setError(null)}><X/></button></div>}
      {otherSessionBusy&&<div className="queue-hint">{t("anotherConversationQueued")}</div>}
      <form className="composer" onSubmit={send}><button type="button" className="composer-plus" disabled title={t("attachmentsUnavailable")} aria-label={t("attachmentsUnavailable")}><Plus/></button><textarea rows={1} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.nativeEvent.isComposing||e.key==="Process")return;if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();if(sendingRef.current||busy)return;e.currentTarget.form?.requestSubmit()}}} placeholder={activeSessionId&&chatName?t("messageTo",{name:chatName}):t("chooseConversationFirst")} disabled={!activeSessionId}/>{sessionBusy?<button type="button" className="send stop-send" title={t("stopConversation")} onClick={()=>void action(stopChat)}><Square/></button>:<button className="send" disabled={!activeSessionId||!draft.trim()||busy}><UseAnimations animation={arrowUp} size={20} strokeColor="#1b1b1c"/></button>}</form>
    </main>

    {!rightCollapsed&&<div className="side-card-backdrop" onClick={()=>setRightCollapsed(true)}/>}
    {!rightCollapsed&&<aside className="side-card">
      <>
        <header className="side-card-head">
          <span className="side-card-title">{rightPart==="computer"?t("computer"):rightPart==="memory"?t("memory"):rightPart==="plugins"?t("plugins"):t("settings")}</span>
          <button type="button" className="icon-button" title={t("collapseSidebar")} onClick={()=>setRightCollapsed(true)}><ChevronsRight/></button>
        </header>
        <div className="side-card-body">
          <div className={`side-part computer-part ${rightPart==="computer"?"":"hidden-part"}`}>
            <div className="computer-status-row">{paneBot?<span>{t("botComputer",{name:paneBot.name})}</span>:<span>{t("computer")}</span>}{computer.state==="booting"?<UseAnimations animation={loading} size={17} wrapperStyle={{display:"inline-block",verticalAlign:"middle"}}/>:<i className={`state-dot ${computer.state}`}/>}<small>{stateLabel(computer.state)}</small></div>
            <div className="preview">{computerOpen?<EmptyComputer state={computer.state}/>:frame}</div>
            {paneBot&&<><div className="computer-caption"><span>{t("dedicatedScreen")}</span><button className="outline" onClick={()=>setComputerOpen(true)}>{t("enlarge")}</button></div><ControlBar active={paneBot} computer={computer} busy={busy} action={action} paste={pasteClipboard} copy={copyClipboard} sessionId={activeSessionId}/></>}
          </div>
          {rightPart==="memory"&&paneBot&&<MemoryPane bot={paneBot} changed={loadBots}/>}
          {rightPart==="plugins"&&<McpPane servers={mcpServers} reload={loadMcp}/>}
          {rightPart==="settings"&&active&&<BotSettingsPane bot={active} saved={loadBots} onDelete={()=>setDeleteOpen(true)}/>}
        </div>
      </>
    </aside>}

    {computerOpen&&paneBot&&<div className="computer-overlay"><header><div><Avatar name={paneBot.name} color={paneBot.avatarColor} shape={paneBot.avatarShape} active online/><strong>{modeLabel(computer.mode)}</strong><span className="control-badge">{computer.controlHolder==="user"?t("userControlling"):computer.busyBotName?t("aiReadOnly"):t("readOnly")}</span></div><div><ControlButtons computer={computer} busy={busy} action={action} active={paneBot} sessionId={activeSessionId}/><button className="icon-button" onClick={pasteClipboard} disabled={computer.controlHolder!=="user"} title={t("pasteClipboard")}><ClipboardPaste/></button><button className="icon-button" onClick={copyClipboard} disabled={computer.controlHolder!=="user"||!desktopClipboard} title={t("copyDesktopClipboard")}><UseAnimations animation={copy} size={18} strokeColor="#dfdfe2"/></button><button className="icon-button" title={t("moreActions")}><Ellipsis/></button><button className="icon-button" onClick={()=>setComputerOpen(false)}><X/></button></div></header><div className="overlay-screen">{frame}</div>{error&&<div className="overlay-error">{error}</div>}</div>}
    {clipboardOpen&&<ClipboardDialog close={()=>setClipboardOpen(false)} paste={text=>{document.querySelectorAll<HTMLIFrameElement>(".desktop-frame").forEach(frame=>frame.contentWindow?.postMessage({type:"lazyboy-host-clipboard",text},location.origin));setClipboardOpen(false)}}/>}

    {createOpen&&<CreateDialog close={()=>setCreateOpen(false)} created={async bot=>{setCreateOpen(false);await loadBots();setActiveId(bot.id)}}/>}
    {groupOpen&&<CreateGroupDialog bots={bots} close={()=>setGroupOpen(false)} created={async room=>{setGroupOpen(false);setRooms(current=>[room,...current.filter(item=>item.id!==room.id)]);setActiveId(null);setActiveSessionId(null);setBusyMembers([]);setMessages([]);setRightPart("computer");setActiveRoomId(room.id);await loadBots()}}/>}
    {deleteOpen&&paneBot&&!activeRoom&&<ConfirmDelete bot={paneBot} close={()=>setDeleteOpen(false)} confirm={()=>action(async()=>{await api(`/api/bots/${paneBot.id}`,{method:"DELETE"});setDeleteOpen(false);setActiveId(null);await loadBots()})}/>}
    {clearOpen&&<ConfirmClear close={()=>setClearOpen(false)} confirm={()=>void clearSession()}/>}
    {sessionToDelete&&<ConfirmSessionDelete close={()=>setSessionToDelete(null)} confirm={()=>{const id=sessionToDelete;setSessionToDelete(null);void deleteSession(id)}}/>}
    {roomToDelete&&<ConfirmRoomDelete room={roomToDelete} close={()=>setRoomToDelete(null)} confirm={()=>void deleteRoom(roomToDelete)}/>}
    {context&&<BotContextMenu context={context} close={()=>setContext(null)} run={async(actionName,group)=>{setContext(null);if(actionName==="delete"){setActiveId(context.bot.id);setActiveRoomId(null);setDeleteOpen(true);return}await inbox(context.bot,actionName,group)}}/>}
    {roomContext&&<RoomContextMenu context={roomContext} close={()=>setRoomContext(null)} onDelete={()=>{setRoomToDelete(roomContext.room);setRoomContext(null)}}/>}
    {accountDialog==="phone"&&<PhoneAccessDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="settings"&&<WorkspaceSettingsDialog name={workspaceName} setName={setWorkspaceName} showHidden={showHidden} setShowHidden={setShowHidden} rightCollapsed={rightCollapsed} setRightCollapsed={setRightCollapsed} close={()=>setAccountDialog(null)}/>}
    {accountDialog==="about"&&<AboutDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="help"&&<HelpDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="feedback"&&<FeedbackDialog close={()=>setAccountDialog(null)}/>}
  </div>
}

function BotContextMenu({context,close,run}:{context:{bot:Bot;x:number;y:number};close:()=>void;run:(action:string,group?:string|null)=>void}){const bot=context.bot;return <div className="context-menu" style={{left:Math.min(context.x,window.innerWidth-220),top:Math.min(context.y,window.innerHeight-280)}} onClick={e=>e.stopPropagation()}><button onClick={()=>run(bot.pinned?"unpin":"pin")}><Pin/>{bot.pinned?t("unpin"):t("pin")}</button><button onClick={()=>run("unread")}><UseAnimations animation={mail} size={15} strokeColor="#dfdfe2"/>{t("markUnread")}</button><button onClick={()=>{const name=window.prompt(t("enterGroupName"),bot.groupName||"");if(name!==null)run("group",name)}}><UseAnimations animation={folder} size={15} strokeColor="#dfdfe2"/>{bot.groupName?t("changeGroup"):t("createOrMoveGroup")}</button>{bot.groupName&&<button onClick={()=>run("group",null)}><X/>{t("removeFromGroup")}</button>}<button onClick={()=>run(bot.hidden?"show":"hide")}>{bot.hidden?<UseAnimations animation={visibility} size={15} strokeColor="#dfdfe2"/>:<UseAnimations animation={visibility2} size={15} strokeColor="#dfdfe2"/>}{bot.hidden?t("unhide"):t("hide")}</button><hr/><button className="danger-item" onClick={()=>run("delete")}><UseAnimations animation={trash2} size={15} strokeColor="#ff7777"/>{t("delete")}</button><button className="context-close" onClick={close}><X/></button></div>}

function SessionMenu({sessions,activeSessionId,open,setOpen,busy,onSelect,onCreate,onDelete,onClear}:{sessions:Session[];activeSessionId:string|null;open:boolean;setOpen:(fn:(value:boolean)=>boolean)=>void;busy:boolean;onSelect:(id:string)=>void;onCreate:()=>void;onDelete:(id:string)=>void;onClear:()=>void}){
  const active=sessions.find(session=>session.id===activeSessionId);
  return <div className="session-picker" onClick={event=>event.stopPropagation()}>
    <button type="button" className="session-current" onClick={()=>setOpen(value=>!value)} aria-label={t("chooseConversation")} aria-expanded={open}>
      <span>{active?.title||t("chooseConversation")}</span><ChevronDown/>
    </button>
    <button type="button" className="icon-button" onClick={onCreate} title={t("newConversation")} disabled={busy}><Plus/></button>
    {open&&<div className="session-menu">
      <button type="button" onClick={onCreate} disabled={busy}><Plus/>{t("newConversation")}</button>
      <hr/>
      <div className="session-menu-list">{sessions.map(session=><div key={session.id} className={`session-item ${session.id===activeSessionId?"selected":""}`}>
        <button type="button" className="session-item-main" onClick={()=>onSelect(session.id)}>
          <strong>{session.title}</strong>
          <time>{inboxTime(session.updatedAt)}</time>
        </button>
        <button type="button" className="icon-button" title={t("deleteConversation")} disabled={busy} onClick={()=>onDelete(session.id)}><UseAnimations animation={trash2} size={18} strokeColor="#dfdfe2"/></button>
      </div>)}</div>
      <hr/>
      <button type="button" className="danger-item" disabled={busy||!activeSessionId} onClick={onClear}>{t("clearConversation")}</button>
    </div>}
  </div>
}

function ConfirmClear({close,confirm}:{close:()=>void;confirm:()=>void}){
  return <div className="modal-backdrop"><div className="dialog compact"><h2>{t("clearConversationTitle")}</h2><p>{t("clearConversationDescription")}</p><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="danger" onClick={confirm}>{t("clearMessages")}</button></div></div></div>
}

function ConfirmSessionDelete({close,confirm}:{close:()=>void;confirm:()=>void}){
  return <div className="modal-backdrop"><div className="dialog compact"><h2>{t("deleteConversationTitle")}</h2><p>{t("deleteConversationDescription")}</p><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="danger" onClick={confirm}>{t("deleteConversation")}</button></div></div></div>
}

function BotSettingsPane({bot,saved,onDelete}:{bot:Bot;saved:()=>Promise<void>|void;onDelete:()=>void}){
  const[name,setName]=useState(bot.name);const[title,setTitle]=useState(bot.title);const[description,setDescription]=useState(bot.description);const[color,setColor]=useState(bot.avatarColor||"#8B5CF6");const[shape,setShape]=useState<AvatarShape>(bot.avatarShape||"blob");const[tagText,setTagText]=useState((bot.tags||[]).join("、"));const[busy,setBusy]=useState(false);
  useEffect(()=>{setName(bot.name);setTitle(bot.title);setDescription(bot.description);setColor(bot.avatarColor||"#8B5CF6");setShape(bot.avatarShape||"blob");setTagText((bot.tags||[]).join("、"))},[bot.id,bot.name,bot.title,bot.description,bot.avatarColor,bot.avatarShape,bot.tags]);
  const colors=["#08A99D","#F1F2F2","#956A43","#DD263B","#F36C05","#F39A00","#00B873","#1985E6","#7140D9","#DC2781","#A7A7A7"];
  const shapes:AvatarShape[]=["round","blob","squircle","capsule","triangle","hexagon","cloud","drop","cat","bunny","star","heart","egg","ghost","sprout","cactus","mushroom","paw"];
  return <form className="pane-form settings-pane" onSubmit={async e=>{e.preventDefault();if(!name.trim())return;setBusy(true);try{await api(`/api/bots/${bot.id}`,{method:"PATCH",body:JSON.stringify({name:name.trim(),title,description,avatarColor:color,avatarShape:shape,tags:tagText.split(/[、,，]/).map(v=>v.trim()).filter(Boolean)})});await saved()}finally{setBusy(false)}}}>
    <div className="avatar-editor"><Avatar name={name||bot.name} color={color} shape={shape} size={78}/><strong>{t("avatarAppearance")}</strong><small>{t("avatarAppearanceHint")}</small></div>
    <fieldset><legend>{t("color")}</legend><div className="color-grid">{colors.map(value=><button type="button" key={value} className={color===value?"selected":""} style={{background:value}} onClick={()=>setColor(value)} aria-label={t("chooseColor",{value})}/>)}<label className="custom-color" title={t("customColor")}><input type="color" value={color} onChange={e=>setColor(e.target.value.toUpperCase())}/><span>＋</span></label></div></fieldset>
    <fieldset><legend>{t("shape")}</legend><div className="shape-grid">{shapes.map(value=><button type="button" className={shape===value?"selected":""} onClick={()=>setShape(value)} key={value}><Avatar name={name||bot.name} color={color} shape={value}/></button>)}</div></fieldset>
    <label>{t("name")}<input value={name} maxLength={80} onChange={e=>setName(e.target.value)}/></label>
    <label>{t("tags")}<input value={tagText} maxLength={120} onChange={e=>setTagText(e.target.value)} placeholder={t("tagsPlaceholder")}/><small>{t("tagsLimit")}</small></label>
    <label>{t("shortTitle")}<input value={title} maxLength={100} onChange={e=>setTitle(e.target.value)} placeholder={t("shortTitlePlaceholder")}/></label>
    <label>{t("description")}<textarea value={description} maxLength={1000} rows={4} onChange={e=>setDescription(e.target.value)} placeholder={t("descriptionPlaceholder")}/></label>
    <div className="pane-actions"><button className="primary" disabled={busy||!name.trim()}>{busy?t("saving"):t("saveSettings")}</button><button type="button" className="danger" onClick={onDelete}>{t("deleteBot")}</button></div>
  </form>
}

function MemoryPane({bot,changed}:{bot:Bot;changed:()=>Promise<void>}){
  const[items,setItems]=useState<MemoryItem[]>([]);const[draft,setDraft]=useState("");const[query,setQuery]=useState("");
  const[enabled,setEnabled]=useState(bot.memoryEnabled);const[busy,setBusy]=useState(false);const[error,setError]=useState("");
  const[editingId,setEditingId]=useState<string|null>(null);const[editingText,setEditingText]=useState("");const[confirmClear,setConfirmClear]=useState(false);
  const load=useCallback(()=>api<MemoryItem[]>(`/api/bots/${bot.id}/memories`).then(setItems),[bot.id]);
  useEffect(()=>{load().catch(e=>setError(e instanceof Error?e.message:t("loadFailed")))},[load]);
  useEffect(()=>setEnabled(bot.memoryEnabled),[bot.id,bot.memoryEnabled]);
  const visible=items.filter(item=>!query.trim()||item.content.toLowerCase().includes(query.trim().toLowerCase()));
  async function run(work:()=>Promise<unknown>){setBusy(true);setError("");try{await work();await load()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function toggle(value:boolean){setEnabled(value);try{await api(`/api/bots/${bot.id}`,{method:"PATCH",body:JSON.stringify({name:bot.name,title:bot.title,description:bot.description,avatarColor:bot.avatarColor,avatarShape:bot.avatarShape,tags:bot.tags,memoryEnabled:value})});await changed()}catch(e){setEnabled(!value);setError(e instanceof Error?e.message:t("settingsFailed"))}}
  return <div className="memory-pane">
    <p className="memory-help">{t("memoryHelp")}</p>
    <label className="memory-toggle"><input type="checkbox" checked={enabled} onChange={e=>void toggle(e.target.checked)}/> {t("enableLongTermMemory")}</label>
    <form onSubmit={e=>{e.preventDefault();const content=draft.trim();if(!content)return;void run(async()=>{await api(`/api/bots/${bot.id}/memories`,{method:"POST",body:JSON.stringify({content,importance:.5})});setDraft("")})}}>
      <label>{t("addMemory")}<textarea value={draft} onChange={e=>setDraft(e.target.value)} rows={3} placeholder={t("memoryPlaceholder")}/></label>
      <button className="primary" disabled={busy||!draft.trim()}>{t("add")}</button>
    </form>
    {items.length>0&&<label className="memory-search">{t("memorySearch")}<input value={query} onChange={e=>setQuery(e.target.value)} placeholder={t("filterMemory")}/></label>}
    <div className="memory-list">{items.length===0?<p>{t("noMemory")}</p>:visible.length===0?<p>{t("noMatchingMemory")}</p>:visible.map(item=><div className="memory-row" key={item.id}>
      {editingId===item.id?<textarea value={editingText} onChange={e=>setEditingText(e.target.value)} rows={3}/>:<span>{item.content}</span>}
      <small>{inboxTime(item.updatedAt)}</small>
      <div>{editingId===item.id?<>
        <button className="primary" disabled={busy||!editingText.trim()} onClick={()=>void run(async()=>{await api(`/api/bots/${bot.id}/memories/${item.id}`,{method:"PATCH",body:JSON.stringify({content:editingText,importance:item.importance})});setEditingId(null)})}>{t("save")}</button>
        <button className="outline" disabled={busy} onClick={()=>setEditingId(null)}>{t("cancel")}</button>
      </>:<>
        <button className="outline" disabled={busy} onClick={()=>{setEditingId(item.id);setEditingText(item.content)}}>{t("edit")}</button>
        <button className="danger-ghost" disabled={busy} onClick={()=>void run(()=>api(`/api/bots/${bot.id}/memories/${item.id}`,{method:"DELETE"}))}>{t("delete")}</button>
      </>}</div>
    </div>)}</div>
    {error&&<div className="pane-error">{error}</div>}
    <div className="pane-actions">
      {confirmClear?<span className="memory-clear-confirm">{t("clearAllMemoryConfirm")}<button className="danger" disabled={busy} onClick={()=>void run(async()=>{await api(`/api/bots/${bot.id}/memories`,{method:"DELETE"});setConfirmClear(false)})}>{t("clearMemoryConfirm")}</button><button className="outline" onClick={()=>setConfirmClear(false)}>{t("cancel")}</button></span>:<button className="danger" disabled={busy||items.length===0} onClick={()=>setConfirmClear(true)}>{t("clearAll")}</button>}
    </div>
  </div>
}

function parsePairs(text:string){const out:Record<string,string>={};for(const line of text.split(/\n+/)){const trimmed=line.trim();if(!trimmed)continue;const cut=trimmed.indexOf("=");if(cut<=0)continue;out[trimmed.slice(0,cut).trim()]=trimmed.slice(cut+1)}return out}
function McpPane({servers,reload}:{servers:McpServer[];reload:()=>Promise<void>}){
  const[name,setName]=useState("");const[transport,setTransport]=useState<McpTransport>("http");
  const[command,setCommand]=useState("");const[args,setArgs]=useState("");const[url,setUrl]=useState("");
  const[envText,setEnvText]=useState("");const[headerText,setHeaderText]=useState("");
  const[busy,setBusy]=useState(false);const[error,setError]=useState("");const[openId,setOpenId]=useState<string|null>(null);
  async function run(work:()=>Promise<unknown>){setBusy(true);setError("");try{await work();await reload()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  return <div className="mcp-pane">
    <p className="memory-help">{t("mcpHelp")}</p>
    <form className="mcp-add" onSubmit={e=>{e.preventDefault();const trimmed=name.trim();if(!trimmed)return;void run(async()=>{await api("/api/mcp-servers",{method:"POST",body:JSON.stringify({name:trimmed,transport,command:command.trim()||null,args:args.trim()?args.trim().split(/\s+/):[],url:url.trim()||null,env:parsePairs(envText),headers:parsePairs(headerText),enabled:true})});setName("");setCommand("");setArgs("");setUrl("");setEnvText("");setHeaderText("")})}}>
      <label>{t("name")}<input value={name} maxLength={40} onChange={e=>setName(e.target.value)} placeholder={t("mcpNamePlaceholder")}/></label>
      <div className="mcp-transport">{(["http","sse","stdio"] as McpTransport[]).map(value=><button type="button" key={value} className={transport===value?"picked":""} onClick={()=>setTransport(value)}>{value==="http"?"HTTP":value==="sse"?"SSE":"stdio"}</button>)}</div>
      {transport==="stdio"?<>
        <label>{t("command")}<input value={command} onChange={e=>setCommand(e.target.value)} placeholder={t("commandPlaceholder")}/></label>
        <label>{t("arguments")}<input value={args} onChange={e=>setArgs(e.target.value)} placeholder="-y @modelcontextprotocol/server-github"/></label>
        <label>{t("environmentVariables")}<textarea rows={3} value={envText} onChange={e=>setEnvText(e.target.value)} placeholder="GITHUB_TOKEN=…"/></label>
      </>:<>
        <label>{t("url")}<input value={url} onChange={e=>setUrl(e.target.value)} placeholder="https://mcp.example.com/mcp"/></label>
        <label>{t("headers")}<textarea rows={3} value={headerText} onChange={e=>setHeaderText(e.target.value)} placeholder="Authorization=Bearer …"/></label>
      </>}
      <button className="primary" disabled={busy||!name.trim()||(transport==="stdio"?!command.trim():!url.trim())}>{busy?t("connecting"):t("connectMcp")}</button>
    </form>
    <div className="mcp-list">{servers.length===0?<p>{t("noMcp")}</p>:servers.map(server=><div className={`mcp-card ${server.status}`} key={server.id}>
      <button type="button" className="mcp-card-head" onClick={()=>setOpenId(id=>id===server.id?null:server.id)}>
        <i className={`mcp-dot ${server.status}`}/><strong>{server.name}</strong>
        <small>{server.status==="connected"?t("toolsCount",{count:server.tools.length}):server.status==="disabled"?t("disabled"):t("disconnected")}</small>
      </button>
      {server.error&&<div className="mcp-error">{server.error}</div>}
      {openId===server.id&&<>
        <div className="mcp-tools">{server.tools.length===0?<span>{t("noTools")}</span>:server.tools.map(tool=><div className="mcp-tool" key={tool.exposedName}><code>{tool.exposedName}</code><span>{tool.description||tool.name}</span></div>)}</div>
        <div className="mcp-card-actions">
          <button className="outline" disabled={busy} onClick={()=>void run(()=>api(`/api/mcp-servers/${server.id}/reconnect`,{method:"POST",body:"{}"}))}><RefreshCw/>{t("reconnect")}</button>
          <button className="outline" disabled={busy} onClick={()=>void run(()=>api(`/api/mcp-servers/${server.id}`,{method:"PATCH",body:JSON.stringify({enabled:!server.enabled})}))}>{server.enabled?t("disable"):t("enable")}</button>
          <button className="danger-ghost" disabled={busy} onClick={()=>void run(()=>api(`/api/mcp-servers/${server.id}`,{method:"DELETE"}))}>{t("delete")}</button>
        </div>
      </>}
    </div>)}</div>
    {error&&<div className="pane-error">{error}</div>}
  </div>
}

function EmptyComputer({state}:{state:ComputerStatus["state"]}){const loading=state==="booting";return <div className="empty-computer">{loading?<UseAnimations animation={loading2} size={38} wrapperStyle={{display:"block"}}/>:<Computer/>}<strong>{stateLabel(state)}</strong><span>{loading?t("preparingDesktop"):t("computerPreviewHint")}</span></div>}
function ControlButtons({computer,busy,action,active,sessionId}:{computer:ComputerStatus;busy:boolean;action:(w:()=>Promise<unknown>)=>Promise<void>;active:Bot;sessionId?:string|null}){const working=Boolean(computer.busyBotName);if(computer.state!=="running")return <button className="primary" disabled={busy||computer.state==="booting"} onClick={()=>action(()=>api(`/api/computer/${active.id}/boot`,{method:"POST",body:"{}"}))}>{(busy||computer.state==="booting")&&<UseAnimations animation={loading} size={17} wrapperStyle={{display:"inline-block",verticalAlign:"middle",marginRight:7}}/>}{computer.state==="booting"?t("bootingProgress"):t("openComputer")}</button>;if(working)return <button className="outline" disabled={busy} onClick={()=>action(async()=>{if(sessionId)await api(`/api/sessions/${sessionId}/stop`,{method:"POST",body:"{}"});else await api(`/api/bots/${active.id}/stop`,{method:"POST",body:"{}"});if(computer.takeoverRequested)await api(`/api/computer/${active.id}/takeover`,{method:"POST",body:"{}"})})}><Square/>{computer.takeoverRequested?t("stopAndTakeOver"):t("stopTask")}</button>;if(computer.controlHolder==="user")return <button className="outline" disabled={busy} onClick={()=>action(()=>api(`/api/computer/${active.id}/release`,{method:"POST",body:"{}"}))}>{t("releaseControl")}</button>;return <button className="primary" disabled={busy} onClick={()=>action(()=>api(`/api/computer/${active.id}/takeover`,{method:"POST",body:"{}"}))}>{t("takeControl")}</button>}
function ControlBar(props:{active:Bot;computer:ComputerStatus;busy:boolean;action:(w:()=>Promise<unknown>)=>Promise<void>;paste:()=>void;copy:()=>void;sessionId?:string|null}){const interactive=props.computer.controlHolder==="user";return <div className="control-bar"><ControlButtons {...props}/><button className="icon-button" disabled={!interactive} onClick={props.paste}><ClipboardPaste/></button><button className="icon-button" disabled={!interactive} onClick={props.copy}><UseAnimations animation={copy} size={18} strokeColor="#dfdfe2"/></button></div>}
function ClipboardDialog({close,paste}:{close:()=>void;paste:(text:string)=>void}){const[text,setText]=useState("");return <div className="modal-backdrop"><div className="dialog compact"><div className="dialog-title"><h2>{t("pasteToRemoteComputer")}</h2><button onClick={close}><X/></button></div><p>{t("pasteRemoteHelp")}</p><textarea className="clipboard-text" autoFocus value={text} onChange={e=>setText(e.target.value)} placeholder={t("pasteTextPlaceholder")}/><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={!text} onClick={()=>paste(text)}>{t("pasteIntoVnc")}</button></div></div></div>}
function CreateDialog({close,created}:{close:()=>void;created:(bot:Bot)=>void}){const[name,setName]=useState("");const[mode,setMode]=useState<ComputerMode>("team");const[busy,setBusy]=useState(false);return <div className="modal-backdrop"><form className="dialog" onSubmit={async e=>{e.preventDefault();if(!name.trim())return;setBusy(true);try{created(await api<Bot>("/api/bots",{method:"POST",body:JSON.stringify({name:name.trim(),computerMode:mode})}))}finally{setBusy(false)}}}><div className="dialog-title"><h2>{t("addBot")}</h2><button type="button" onClick={close}><X/></button></div><label>{t("name")}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder={t("botNamePlaceholder")}/></label><div className="mode-grid"><button type="button" className={mode==="team"?"picked":""} onClick={()=>setMode("team")}><BotIcon/><strong>{t("sharedComputer")}</strong><small>{t("sharedComputerHint")}</small></button><button type="button" className={mode==="dedicated"?"picked":""} onClick={()=>setMode("dedicated")}><Computer/><strong>{t("privateComputer")}</strong><small>{t("privateComputerHint")}</small></button></div><div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!name.trim()}>{t("create")}</button></div></form></div>}
function CreateGroupDialog({bots,close,created}:{bots:Bot[];close:()=>void;created:(room:Room)=>void}){const[name,setName]=useState("");const[selected,setSelected]=useState<string[]>([]);const[busy,setBusy]=useState(false);const visible=bots.filter(bot=>!bot.hidden);return <div className="modal-backdrop"><form className="dialog" onSubmit={async e=>{e.preventDefault();const groupName=name.trim();if(!groupName||selected.length<2)return;setBusy(true);try{created(await api<Room>("/api/rooms",{method:"POST",body:JSON.stringify({name:groupName,memberIds:selected})}))}finally{setBusy(false)}}}><div className="dialog-title"><h2>{t("addGroup")}</h2><button type="button" onClick={close}><X/></button></div><p className="dialog-lead">{t("groupDescription")}</p><label>{t("groupName")}<input autoFocus value={name} maxLength={30} onChange={e=>setName(e.target.value)} placeholder={t("groupNamePlaceholder")}/></label><fieldset className="group-picker"><legend>{t("chooseBots")}</legend>{visible.map(bot=><label key={bot.id}><input type="checkbox" checked={selected.includes(bot.id)} onChange={()=>setSelected(ids=>ids.includes(bot.id)?ids.filter(id=>id!==bot.id):[...ids,bot.id])}/><Avatar name={bot.name} color={bot.avatarColor} shape={bot.avatarShape} online/><span>{bot.name}</span></label>)}</fieldset><div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!name.trim()||selected.length<2}>{busy?t("creating"):t("createGroup")}</button></div></form></div>}
function RoomContextMenu({context,close,onDelete}:{context:{room:Room;x:number;y:number};close:()=>void;onDelete:()=>void}){return <div className="context-menu" style={{left:Math.min(context.x,window.innerWidth-220),top:Math.min(context.y,window.innerHeight-160)}} onClick={e=>e.stopPropagation()}><button className="danger-item" onClick={onDelete}><UseAnimations animation={trash2} size={15} strokeColor="#ff7777"/>{t("deleteGroup")}</button><button className="context-close" onClick={close}><X/></button></div>}
function ConfirmRoomDelete({room,close,confirm}:{room:Room;close:()=>void;confirm:()=>void}){return <div className="modal-backdrop"><div className="dialog compact"><h2>{t("deleteNamed",{name:room.name})}</h2><p>{t("deleteGroupDescription")}</p><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="danger" onClick={confirm}>{t("deleteGroup")}</button></div></div></div>}
function ConfirmDelete({bot,close,confirm}:{bot:Bot;close:()=>void;confirm:()=>void}){return <div className="modal-backdrop"><div className="dialog compact"><h2>{t("deleteNamed",{name:bot.name})}</h2><p>{bot.computerMode==="dedicated"?t("deleteDedicatedBotDescription"):t("deleteSharedBotDescription")}</p><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="danger" onClick={confirm}>{t("delete")}</button></div></div></div>}

function PhoneAccessDialog({close}:{close:()=>void}){
  const url=location.origin;
  const local=/^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/i.test(url);
  const[copied,setCopied]=useState(false);
  return <div className="modal-backdrop"><div className="dialog compact"><div className="dialog-title"><h2>{t("openOnPhone")}</h2><button type="button" onClick={close}><X/></button></div>
    <p>{t("phoneAccessDescription")}</p>
    <code className="share-url">{url}</code>
    {local&&<p className="dialog-lead">{t("localAddressWarning")}</p>}
    <div className="dialog-actions"><button className="outline" onClick={close}>{t("close")}</button><button className="primary" onClick={async()=>{try{await navigator.clipboard.writeText(url);setCopied(true)}catch{setCopied(false)}}}>{copied?t("copied"):t("copyUrl")}</button></div>
  </div></div>
}
function WorkspaceSettingsDialog({name,setName,showHidden,setShowHidden,rightCollapsed,setRightCollapsed,close}:{name:string;setName:(value:string)=>void;showHidden:boolean;setShowHidden:(value:boolean)=>void;rightCollapsed:boolean;setRightCollapsed:(value:boolean)=>void;close:()=>void}){
  const[draft,setDraft]=useState(name);
  return <div className="modal-backdrop"><form className="dialog compact" onSubmit={e=>{e.preventDefault();setName(draft.trim()||t("localWorkspace"));close()}}>
    <div className="dialog-title"><h2>{t("settings")}</h2><button type="button" onClick={close}><X/></button></div>
    <label>{t("workspaceName")}<input autoFocus value={draft} maxLength={40} onChange={e=>setDraft(e.target.value)}/></label>
    <label className="memory-toggle"><input type="checkbox" checked={showHidden} onChange={e=>setShowHidden(e.target.checked)}/> {t("showHiddenBots")}</label>
    <label className="memory-toggle"><input type="checkbox" checked={rightCollapsed} onChange={e=>setRightCollapsed(e.target.checked)}/> {t("collapseRightSidebar")}</label>
    <p className="dialog-lead">{t("workspaceSettingsHint")}</p>
    <div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary">{t("save")}</button></div>
  </form></div>
}
function AboutDialog({close}:{close:()=>void}){
  const[health,setHealth]=useState<"ok"|"bad"|"…">("…");
  useEffect(()=>{fetch("/api/health").then(r=>r.ok?setHealth("ok"):setHealth("bad")).catch(()=>setHealth("bad"))},[]);
  return <div className="modal-backdrop"><div className="dialog compact"><div className="dialog-title"><h2>{t("about")}</h2><button type="button" onClick={close}><X/></button></div>
    <div className="about-brand"><Avatar name="L" size={52} online/><strong>LazyBoy</strong><small>0.1.0</small></div>
    <p>{t("aboutDescription")}</p>
    <p className="dialog-lead">{t("apiStatus",{status:health==="ok"?t("statusNormal"):health==="bad"?t("statusUnavailable"):t("statusChecking")})}</p>
    <div className="dialog-actions"><button className="primary" onClick={close}>{t("close")}</button></div>
  </div></div>
}
function HelpDialog({close}:{close:()=>void}){
  return <div className="modal-backdrop"><div className="dialog help-dialog"><div className="dialog-title"><h2>{t("helpCenter")}</h2><button type="button" onClick={close}><X/></button></div>
    <div className="help-body">
      <section><h3>{t("helpBotsTitle")}</h3><p>{t("helpBots")}</p></section>
      <section><h3>{t("helpGroupsTitle")}</h3><p>{t("helpGroups")}</p></section>
      <section><h3>{t("helpComputerTitle")}</h3><p>{t("helpComputer")}</p></section>
      <section><h3>{t("helpMemoryTitle")}</h3><p>{t("helpMemory")}</p></section>
      <section><h3>{t("helpMcpTitle")}</h3><p>{t("helpMcp")}</p></section>
      <section><h3>{t("helpShortcutsTitle")}</h3><p>{t("helpShortcuts")}</p></section>
    </div>
    <div className="dialog-actions"><button className="primary" onClick={close}>{t("close")}</button></div>
  </div></div>
}
function FeedbackDialog({close}:{close:()=>void}){
  const[text,setText]=useState("");const[copied,setCopied]=useState(false);
  return <div className="modal-backdrop"><div className="dialog compact"><div className="dialog-title"><h2>{t("sendFeedback")}</h2><button type="button" onClick={close}><X/></button></div>
    <p>{t("feedbackDescription")}</p>
    <textarea className="clipboard-text" autoFocus rows={6} value={text} onChange={e=>setText(e.target.value)} placeholder={t("feedbackPlaceholder")}/>
    <div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={!text.trim()} onClick={async()=>{try{await navigator.clipboard.writeText(text.trim());setCopied(true)}catch{setCopied(false)}}}>{copied?t("copied"):t("copyContent")}</button></div>
  </div></div>
}

function LoginScreen({authenticated}:{authenticated:()=>void}){const[token,setToken]=useState("");const[busy,setBusy]=useState(false);const[error,setError]=useState("");return <main className="login-screen"><form className="dialog compact login-dialog" onSubmit={async e=>{e.preventDefault();if(!token)return;setBusy(true);setError("");try{await api("/api/session",{method:"POST",body:JSON.stringify({token})});authenticated()}catch(err){setError(err instanceof Error?err.message:t("loginFailed"))}finally{setBusy(false)}}}><Avatar name="L" size={58}/><h1>{t("loginTitle")}</h1><p>{t("loginDescription")}</p><label>{t("accessToken")}<input type="password" autoFocus autoComplete="current-password" value={token} onChange={e=>setToken(e.target.value)}/></label>{error&&<div className="login-error">{error}</div>}<button className="primary" disabled={busy||!token}>{busy?t("verifying"):t("login")}</button></form></main>}
