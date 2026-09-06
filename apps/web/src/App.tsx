import { FormEvent, KeyboardEvent as ReactKeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BotIcon, Brain, ChevronDown, ChevronsRight, CircleHelp, ClipboardPaste, Computer, Download, Ellipsis, Info, LogOut, Megaphone, Paperclip, Pencil, Pin, Plug, Plus, RefreshCw, Settings, Smartphone, Sparkle, Square, Upload, Users, X } from "./animated-icons";
import UseAnimations from "./use-animations";
import loading from "react-useanimations/lib/loading";
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
import { Avatar, AvatarLookProvider, AvatarStack, BLOBATAR_BACKGROUNDS, BLOBATAR_EXPRESSIONS, BLOBATAR_SHAPES, DEFAULT_LOOK, persistBlobatarShape, readAvatarLooks, resolveBlobatarShape, writeAvatarLook, type AvatarBackground, type AvatarExpression, type AvatarLook } from "./avatar";
import { t, type MessageKey } from "./i18n";
import type { AvatarShape, Bot, ComputerMode, ComputerStatus, FileSkill, McpCatalogEntry, McpServer, McpTransport, MemoryItem, Message, MessageFile, ModelProviderId, Playbook, PlaybookInput, PlaybookStep, Room, RoomMember, Session, TaughtSkill, VoiceSettings, WorkspaceSettings } from "./types";
import { ChatMarkdown, CopyMessageButton } from "./markdown";
import { ScheduleEditor, ScheduleList, cronFromPreset, defaultCronPreset, presetFromCron, type CronPreset, type ScheduleItem } from "./schedule";
import { CallOverlay, PhoneIcon } from "./call";
import { VoiceSettingsDialog } from "./voice-settings";

const blankComputer:ComputerStatus={botId:"",mode:"team",state:"stopped",controlHolder:"none",takeoverRequested:false,busyBotName:null,busySessionId:null,busyRunId:null,busyStep:null,usingComputer:false,waitingRunId:null,waitingSessionId:null,queuedRuns:0,display:null,profileMode:"per-bot",screenAvailable:false};
const SESSION_STORE="lazyboy.sessionByBot";
const PANE_STORE="lazyboy.rightPane";
const WORKSPACE_STORE="lazyboy.workspace";
type RightPart="computer"|"memory"|"settings"|"plugins"|"accounts";
type AccountDialog="phone"|"settings"|"model"|"voice"|"about"|"help"|"feedback"|null;
function readSessionStore():Record<string,string>{try{const raw=localStorage.getItem(SESSION_STORE);return raw?JSON.parse(raw) as Record<string,string>:{}}catch{return {}}}
function writeSessionStore(botId:string,sessionId:string){const store=readSessionStore();store[botId]=sessionId;localStorage.setItem(SESSION_STORE,JSON.stringify(store))}
function readPaneStore():{collapsed:boolean;part:RightPart}{try{const raw=localStorage.getItem(PANE_STORE);if(!raw)return{collapsed:false,part:"computer"};const value=JSON.parse(raw) as {collapsed?:boolean;part?:string};return{collapsed:Boolean(value.collapsed),part:value.part==="memory"||value.part==="settings"||value.part==="plugins"||value.part==="accounts"?value.part:"computer"}}catch{return{collapsed:false,part:"computer"}}}
type WorkspacePrefs={name:string;showHidden:boolean};
function readWorkspace():WorkspacePrefs{try{const raw=localStorage.getItem(WORKSPACE_STORE);if(!raw)return{name:t("localWorkspace"),showHidden:false};const value=JSON.parse(raw) as {name?:string;showHidden?:boolean};const name=value.name?.trim();return{name:name&&name!=="Local workspace"?name:t("localWorkspace"),showHidden:Boolean(value.showHidden)}}catch{return{name:t("localWorkspace"),showHidden:false}}}

function WorkspaceAvatar({name}:{name:string}){const parts=name.trim().split(/\s+/).filter(Boolean);const initials=(parts.length>1?parts.map(part=>part[0]).join(""):parts[0]?.slice(0,2)||"LB").slice(0,2).toUpperCase();return <span className="workspace-avatar" aria-hidden="true">{initials}</span>}
function modeLabel(mode:ComputerMode){return mode==="team"?t("sharedComputer"):t("privateComputer")}
function stateLabel(state:ComputerStatus["state"]){return ({stopped:t("stopped"),booting:t("booting"),running:t("running"),suspended:t("suspended"),error:t("error")})[state]}
function isTransitionStep(step?:string|null){return step==="電腦啟動中"||step==="喚醒中"||step==="換手中"}
function hudLabel(computer:ComputerStatus,connecting:boolean,handingOff:boolean){
  const step=computer.busyStep||"";
  if(computer.state==="booting"||step==="電腦啟動中")return t("hudBooting");
  if(computer.state==="suspended"||step==="喚醒中")return t("hudWaking");
  if(handingOff||step==="換手中")return t("hudHandoff");
  if(connecting)return t("hudConnecting");
  return null;
}
function inboxTime(value:string|null){if(!value)return "";const date=new Date(value),now=new Date();if(date.toDateString()===now.toDateString())return new Intl.DateTimeFormat("zh-TW",{hour:"2-digit",minute:"2-digit",hour12:false}).format(date);const days=Math.floor((new Date(now.getFullYear(),now.getMonth(),now.getDate()).getTime()-new Date(date.getFullYear(),date.getMonth(),date.getDate()).getTime())/86400000);if(days<7)return new Intl.DateTimeFormat("zh-TW",{weekday:"long"}).format(date);return new Intl.DateTimeFormat("zh-TW",{month:"numeric",day:"numeric"}).format(date)}
const ATTACH_MAX=4;
const ATTACH_MAX_BYTES=10*1024*1024;
const ATTACH_ACCEPT=".png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,.json,.html,.htm,.xml,.docx,.xlsx,.pptx,image/*,text/*,application/pdf";
type PendingFile={id:string;file:File;preview:string|null};
function attachAllowed(file:File){const mime=(file.type||"").toLowerCase();if(mime.startsWith("image/")||mime.startsWith("text/")||mime==="application/pdf"||mime==="application/json"||mime==="application/xml")return true;return /\.(png|jpe?g|gif|webp|pdf|txt|md|csv|json|html?|xml|docx|xlsx|pptx)$/i.test(file.name)}
function readAsBase64(file:File){return new Promise<string>((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>{const value=String(reader.result||"");const comma=value.indexOf(",");resolve(comma>=0?value.slice(comma+1):value)};reader.onerror=()=>reject(reader.error||new Error("read failed"));reader.readAsDataURL(file)})}
function formatBytes(size:number){if(size<1024)return `${size} B`;if(size<1024*1024)return `${Math.round(size/102.4)/10} KB`;return `${Math.round(size/104857.6)/10} MB`}
function fileExt(name:string){const dot=name.lastIndexOf(".");const ext=dot>=0?name.slice(dot+1).replace(/[^a-z0-9]/gi,""):"";return (ext||"FILE").slice(0,4).toUpperCase()}
function messageFiles(blocks:unknown):MessageFile[]{if(!Array.isArray(blocks))return [];return blocks.flatMap(block=>{if(!block||typeof block!=="object")return [];const value=block as {kind?:string;name?:string;mimeType?:string;size?:number};if(value.kind!=="file"&&value.kind!=="image")return [];return [{kind:value.kind,name:value.name||"file",mimeType:value.mimeType,size:value.size}]})}
function chipBlocks(blocks:unknown){if(!Array.isArray(blocks))return [] as {kind:string;site?:string;why?:string;name?:string;human?:string}[];return blocks.flatMap(block=>{if(!block||typeof block!=="object")return [];const value=block as {kind?:string;site?:string;why?:string;name?:string;human?:string};if(value.kind==="login"||value.kind==="schedule"||value.kind==="scheduleRun")return [value];return []})}
function isAutoAttachCaption(body:string,files:MessageFile[]){const text=body.trim();if(!files.length)return false;if(!text)return true;return files.some(file=>text===file.name||text===`附件 ${file.name}`||text===t("attachedFile",{name:file.name}))}
function FileCard({file,preview,onRemove}:{file:{name:string;size?:number};preview?:string|null;onRemove?:()=>void}){const ext=fileExt(file.name);return <div className={`file-card ${onRemove?"is-pending":""}`}>{preview?<img className="file-card-thumb" src={preview} alt=""/>:<div className="file-card-badge" aria-hidden="true">{ext}</div>}<div className="file-card-meta"><strong>{file.name}</strong><small>{typeof file.size==="number"?formatBytes(file.size):ext}</small></div>{onRemove&&<button type="button" className="file-card-remove" title={t("attachRemove",{name:file.name})} onClick={onRemove}><X/></button>}</div>}
function clientNonce(){
  const webCrypto=globalThis.crypto;
  if(webCrypto&&typeof webCrypto.randomUUID==="function")return webCrypto.randomUUID();
  const bytes=new Uint8Array(16);
  if(webCrypto&&typeof webCrypto.getRandomValues==="function")webCrypto.getRandomValues(bytes);
  else for(let i=0;i<bytes.length;i++)bytes[i]=Math.floor(Math.random()*256);
  bytes[6]=(bytes[6]&0x0f)|0x40;
  bytes[8]=(bytes[8]&0x3f)|0x80;
  const hex=[...bytes].map(b=>b.toString(16).padStart(2,"0")).join("");
  return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
}

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
  const [voiceSettings,setVoiceSettings]=useState<VoiceSettings|null>(null); const [callOpen,setCallOpen]=useState(false);
  const [skills,setSkills]=useState<TaughtSkill[]>([]); const [fileSkills,setFileSkills]=useState<FileSkill[]>([]); const [plusOpen,setPlusOpen]=useState(false); const [skillQuery,setSkillQuery]=useState(""); const [teachOpen,setTeachOpen]=useState(false); const [editingSkillId,setEditingSkillId]=useState<string|null>(null);
  const [schedules,setSchedules]=useState<ScheduleItem[]>([]); const [scheduleDraft,setScheduleDraft]=useState<{name:string;instructions:string;enabled:boolean;preset:CronPreset;id?:string;timezone?:string;threadId?:string|null}|null>(null);
  const [scheduleError,setScheduleError]=useState<string|null>(null); const [scheduleSaving,setScheduleSaving]=useState(false); const [runningScheduleId,setRunningScheduleId]=useState<string|null>(null);
  const [workspaceName,setWorkspaceName]=useState(workspaceStart.name);
  const [looks,setLooks]=useState(readAvatarLooks);
  const sendingRef=useRef(false); const refreshSeqRef=useRef(0); const importRef=useRef<HTMLInputElement>(null); const attachRef=useRef<HTMLInputElement>(null);
  const desktopFrameRef=useRef<HTMLIFrameElement>(null);
  const holderRef=useRef(computer.controlHolder); const paneBotRef=useRef<string|null>(null); const skipHandoffRef=useRef(true);
  const [desktopReady,setDesktopReady]=useState(false); const [handingOff,setHandingOff]=useState(false);
  const [pendingFiles,setPendingFiles]=useState<PendingFile[]>([]);
  const messageEndRef=useRef<HTMLDivElement|null>(null);
  const sentHistoryRef=useRef<string[]>([]); const historyIndexRef=useRef<number|null>(null); const historyDraftRef=useRef("");
  const roomsRef=useRef(rooms);
  roomsRef.current=rooms;
  const filtered=useMemo(()=>bots.filter(b=>(showHidden||!b.hidden)&&b.name.toLowerCase().includes(query.toLowerCase())),[bots,query,showHidden]);
  const filteredRooms=useMemo(()=>{const q=query.trim().toLowerCase();return rooms.filter(room=>!q||room.name.toLowerCase().includes(q)||room.members.some(member=>member.name.toLowerCase().includes(q)))},[rooms,query]);
  const sections=useMemo(()=>{const map=new Map<string,Bot[]>();for(const bot of filtered){const key=bot.pinned?t("pinned"):bot.groupName||t("agentGroup");map.set(key,[...(map.get(key)||[]),bot])}return [...map.entries()]},[filtered]);
  const paneBotId=busyMembers[0]?.id||activeRoom?.members[0]?.id||activeId;
  const paneBot=bots.find(bot=>bot.id===paneBotId)||active;
  const currentPaneRef=useRef(paneBotId);currentPaneRef.current=paneBotId;
  const pasteQueueRef=useRef<Promise<unknown>>(Promise.resolve());
  const [clipboardStatus,setClipboardStatus]=useState("");
  const workingMembers=activeRoom?busyMembers:active&&activeSessionId&&computer.busySessionId===activeSessionId?[{id:active.id,name:active.name,avatarColor:active.avatarColor,avatarShape:active.avatarShape}]:[];
  const lastMessageId=messages[messages.length-1]?.id||"";
  // The bot is parked in waiting_takeover: nothing moves (including queued
  // messages) until the human releases the screen, so say so loudly.
  const pausedForUser=computer.takeoverRequested&&workingMembers.length===0&&(!computer.waitingSessionId||computer.waitingSessionId===activeSessionId||Boolean(activeRoom));
  // Teaching by demonstration: one recording at a time per bot, then a draft
  // playbook the human names and saves before it becomes a real skill.
  const teaching=skills.find(skill=>skill.status==="recording")||null;
  const drafting=skills.find(skill=>skill.status==="drafting")||null;
  const skillDraft=teaching||drafting?null:skills.find(skill=>skill.status==="draft")||null;
  const savedSkills=skills.filter(skill=>skill.status==="saved");
  const skillNeedle=skillQuery.trim().toLowerCase();
  const listedSkills=skillNeedle?savedSkills.filter(skill=>skill.name.toLowerCase().includes(skillNeedle)||(skill.playbook.whenToUse||"").toLowerCase().includes(skillNeedle)||skill.goal.toLowerCase().includes(skillNeedle)):savedSkills;
  const [slashIndex,setSlashIndex]=useState(0);
  const [slashDismissed,setSlashDismissed]=useState(false);
  useEffect(()=>{setSlashIndex(0);setSlashDismissed(false)},[draft]);
  const slashToken=draft.trimStart().split(/\s/,1)[0].slice(1).toLowerCase();
  const slashSuggestions=!slashDismissed&&/^\/[^\s]*$/.test(draft.trimStart())
    ? [{name:"goal",description:t("goalCommandHint"),kind:"執行模式"},...fileSkills.map(skill=>({...skill,kind:"檔案技能"}))].filter(skill=>!slashToken||skill.name.startsWith(slashToken)).slice(0,8)
    : [];

  const loadMcp=useCallback(async()=>{setMcpServers(await api<McpServer[]>("/api/mcp-servers").catch(()=>[] as McpServer[]))},[]);
  const loadBots=useCallback(async()=>{const [next,nextRooms]=await Promise.all([api<Bot[]>("/api/bots"),api<Room[]>("/api/rooms").catch(()=>[] as Room[])]);setBots(next);setRooms(nextRooms);setActiveRoomId(id=>id&&nextRooms.some(room=>room.id===id)?id:null);setActiveId(id=>id&&next.some(b=>b.id===id)?id:null);await loadMcp()},[loadMcp]);
  const sessionStoreKey=activeRoomId?`room:${activeRoomId}`:activeId;
  const sessionsPath=activeRoomId?`/api/rooms/${activeRoomId}/sessions`:activeId?`/api/bots/${activeId}/sessions`:null;
  const loadSessions=useCallback(async()=>{if(!sessionsPath){setSessions([]);setActiveSessionId(null);return}const next=await api<Session[]>(sessionsPath);setSessions(next);setActiveSessionId(id=>{if(id&&next.some(session=>session.id===id))return id;const stored=sessionStoreKey?readSessionStore()[sessionStoreKey]:undefined;if(stored&&next.some(session=>session.id===stored))return stored;return next[0]?.id||null})},[sessionsPath,sessionStoreKey]);
  const refresh=useCallback(async()=>{if(!activeSessionId)return;const refreshSeq=++refreshSeqRef.current;let nextBusy:RoomMember[]=[];
    let computerBot=activeId;
    if(activeRoomId){
      try{const roomStatus=await api<{busy:RoomMember[]}>(`/api/rooms/${activeRoomId}/status`);if(refreshSeq!==refreshSeqRef.current)return;nextBusy=roomStatus.busy;computerBot=roomStatus.busy[0]?.id||roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||null}catch{if(refreshSeq!==refreshSeqRef.current)return;computerBot=roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||null}
    }
    const messagesJob=api<Message[]>(`/api/sessions/${activeSessionId}/messages`);
    if(!computerBot){const nextMessages=await messagesJob;if(refreshSeq===refreshSeqRef.current){setBusyMembers(nextBusy);setMessages(nextMessages);setComputer(blankComputer);setSkills([]);setScreenUrl(null)}return}
    const skillsJob=activeRoomId?Promise.resolve([] as TaughtSkill[]):api<TaughtSkill[]>(`/api/bots/${computerBot}/skills`).catch(()=>[] as TaughtSkill[]);
    const status=await api<ComputerStatus>(`/api/computer/${computerBot}/status`);if(refreshSeq!==refreshSeqRef.current)return;
    const [nextMessages,screen,nextSkills]=await Promise.all([messagesJob,status.state==="running"?api<{url:string|null}>(`/api/computer/${computerBot}/screen`).catch(()=>({url:null})):Promise.resolve({url:null}),skillsJob]);
    if(refreshSeq!==refreshSeqRef.current)return;
    setBusyMembers(nextBusy);setComputer(status);setMessages(nextMessages);setSkills(nextSkills);setScreenUrl(status.botId===computerBot?screen.url:null)
  },[activeId,activeRoomId,activeSessionId]);
  useEffect(()=>{loadBots().catch(e=>{if(e instanceof ApiError&&e.status===401)setAuthRequired(true);else setError(e.message)})},[loadBots]);
  useEffect(()=>{if(!voiceSettings?.enabled)setCallOpen(false)},[voiceSettings?.enabled]);
  useEffect(()=>{api<VoiceSettings>("/api/voice/settings").then(setVoiceSettings).catch(()=>setVoiceSettings(null))},[]);
  useEffect(()=>{if(authRequired)return;api<FileSkill[]>("/api/file-skills").then(setFileSkills).catch(()=>setFileSkills([]))},[authRequired]);
  useEffect(()=>{if(activeId||activeRoomId||bots.length===0)return;setActiveId(bots[0].id)},[bots,activeId,activeRoomId]);
  useEffect(()=>{setMessages([]);loadSessions().catch(e=>setError(e.message))},[loadSessions]);
  useEffect(()=>{historyIndexRef.current=null;historyDraftRef.current="";setPendingFiles(current=>{current.forEach(file=>file.preview&&URL.revokeObjectURL(file.preview));return []})},[activeSessionId]);
  useEffect(()=>{if(!plusOpen)setSkillQuery("")},[plusOpen]);
  useEffect(()=>{messageEndRef.current?.scrollIntoView({block:"end",behavior:"smooth"})},[activeSessionId,lastMessageId,workingMembers.length,pausedForUser]);
  useEffect(()=>{if(!activeSessionId||(!activeId&&!activeRoomId)){refreshSeqRef.current+=1;setMessages([]);setScreenUrl(null);if(!activeId&&!activeRoomId)setComputer(blankComputer);return}setScreenUrl(null);refresh().catch(e=>setError(e.message));const timer=setInterval(()=>{refresh().catch(()=>{});const beat=roomsRef.current.find(room=>room.id===activeRoomId)?.members[0]?.id||activeId;if(beat)api(`/api/computer/${beat}/heartbeat`,{method:"POST",body:"{}"}).catch(()=>{})},2000);return()=>{clearInterval(timer);refreshSeqRef.current+=1}},[activeId,activeRoomId,activeSessionId,refresh]);
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.origin!==location.origin||event.source!==desktopFrameRef.current?.contentWindow||!event.data)return;if(event.data.type==="lazyboy-desktop-clipboard"){const text=String(event.data.text||"");setDesktopClipboard(text);navigator.clipboard?.writeText(text).then(()=>setClipboardStatus("剪貼簿已同步")).catch(()=>setClipboardStatus("瀏覽器未允許同步，請按複製按鈕"))}if(event.data.type==="lazyboy-copy-request"&&computer.controlHolder==="user")void copySelection();if(event.data.type==="lazyboy-paste-text"&&typeof event.data.text==="string")pasteText(event.data.text);if(event.data.type==="lazyboy-paste-request"&&computer.controlHolder==="user")setClipboardOpen(true);if(event.data.type==="lazyboy-desktop-ready")setDesktopReady(true);if(event.data.type==="lazyboy-desktop-lost")setDesktopReady(false)};window.addEventListener("message",listener);return()=>window.removeEventListener("message",listener)});
  useEffect(()=>{setDesktopReady(false)},[screenUrl,paneBotId]);
  useEffect(()=>{if(skipHandoffRef.current){skipHandoffRef.current=false;holderRef.current=computer.controlHolder;paneBotRef.current=paneBotId;return}if((holderRef.current!==computer.controlHolder||paneBotRef.current!==paneBotId)&&computer.state==="running")setHandingOff(true);holderRef.current=computer.controlHolder;paneBotRef.current=paneBotId},[computer.controlHolder,paneBotId,computer.state]);
  useEffect(()=>{if(!handingOff)return;const timer=setTimeout(()=>setHandingOff(false),1600);return()=>clearTimeout(timer)},[handingOff]);
  useEffect(()=>{const frame=desktopFrameRef.current;if(!frame?.contentWindow||!screenUrl)return;frame.contentWindow.postMessage({type:"lazyboy-view-only",viewOnly:computer.controlHolder!=="user"},location.origin)},[computer.controlHolder,screenUrl,desktopReady]);
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.origin!==location.origin||event.source!==desktopFrameRef.current?.contentWindow||event.data?.type!=="lazyboy-request-control"||!paneBotId)return;void action(()=>api(`/api/computer/${paneBotId}/takeover`,{method:"POST",body:"{}"}))};window.addEventListener("message",listener);return()=>window.removeEventListener("message",listener)},[paneBotId]);
  useEffect(()=>{const close=(event:MouseEvent)=>{const target=event.target;if(target instanceof Element&&target.closest(".create-menu-wrap,.account-wrap,.session-picker,.context-menu,.plus-menu-wrap"))return;setContext(null);setRoomContext(null);setSessionMenuOpen(false);setAccountOpen(false);setCreateMenuOpen(false);setPlusOpen(false)};window.addEventListener("click",close);return()=>window.removeEventListener("click",close)},[]);
  useEffect(()=>{const onKey=(event:KeyboardEvent)=>{if(event.key!=="Escape")return;setAccountOpen(false);setAccountDialog(null);setCreateMenuOpen(false);setPlusOpen(false);setTeachOpen(false);setEditingSkillId(null);setSessionMenuOpen(false);setContext(null);setRoomContext(null)};window.addEventListener("keydown",onKey);return()=>window.removeEventListener("keydown",onKey)},[]);
  useEffect(()=>{localStorage.setItem(WORKSPACE_STORE,JSON.stringify({name:workspaceName,showHidden}))},[workspaceName,showHidden]);
  useEffect(()=>{if(!sessionStoreKey||!activeSessionId)return;if(!sessions.some(session=>session.id===activeSessionId))return;if(!activeRoomId&&!sessions.some(session=>session.id===activeSessionId&&session.botId===activeId))return;writeSessionStore(sessionStoreKey,activeSessionId)},[sessionStoreKey,activeId,activeRoomId,activeSessionId,sessions]);
  useEffect(()=>{localStorage.setItem(PANE_STORE,JSON.stringify({collapsed:rightCollapsed,part:rightPart}))},[rightCollapsed,rightPart]);
  useEffect(()=>{if(!paneBotId){setSchedules([]);return}api<ScheduleItem[]>(`/api/bots/${paneBotId}/schedules`).then(setSchedules).catch(()=>setSchedules([]))},[paneBotId]);
  useEffect(()=>{if(computer.takeoverRequested){setRightPart("computer");setRightCollapsed(false)}},[computer.takeoverRequested]);
  function openPane(part:RightPart){setRightPart(part);setRightCollapsed(false)}
  async function openLoginScreen(){
    const id=paneBot?.id||active?.id;if(!id)return;
    setRightPart("computer");setRightCollapsed(false);setComputerOpen(true);
    await action(async()=>{
      try{await api(`/api/computer/${id}/boot`,{method:"POST",body:"{}"})}catch{/* already up */}
      await api(`/api/computer/${id}/takeover`,{method:"POST",body:"{}"}).catch(()=>{});
    });
  }
  async function reloadSchedules(){if(!paneBotId)return;setSchedules(await api<ScheduleItem[]>(`/api/bots/${paneBotId}/schedules`))}
  async function saveScheduleDraft(){
    if(!paneBot||!scheduleDraft)return;
    setScheduleSaving(true);setScheduleError(null);
    try{
      const body={name:scheduleDraft.name.trim(),cron:cronFromPreset(scheduleDraft.preset),instructions:scheduleDraft.instructions.trim(),timezone:scheduleDraft.timezone||Intl.DateTimeFormat().resolvedOptions().timeZone||"Asia/Taipei",enabled:scheduleDraft.enabled,threadId:scheduleDraft.id?scheduleDraft.threadId:activeRoomId?undefined:activeSessionId};
      if(scheduleDraft.id)await api(`/api/schedules/${scheduleDraft.id}`,{method:"PATCH",body:JSON.stringify(body)});
      else await api(`/api/bots/${paneBot.id}/schedules`,{method:"POST",body:JSON.stringify(body)});
      setScheduleDraft(null);await reloadSchedules();
    }catch(e){setScheduleError(e instanceof Error?e.message:t("operationFailed"))}
    finally{setScheduleSaving(false)}
  }

  const sessionBusy=workingMembers.length>0;
  const otherSessionBusy=Boolean(computer.busyBotName&&!sessionBusy);
  const chatName=activeRoom?.name||active?.name||"";

  async function action(work:()=>Promise<unknown>){setBusy(true);setError(null);try{await work();await refresh()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function stopChat(){if(activeSessionId)await api(`/api/sessions/${activeSessionId}/stop`,{method:"POST",body:"{}"});else if(active)await api(`/api/bots/${active.id}/stop`,{method:"POST",body:"{}"});}
  async function startTeaching(goal:string){if(!active)return;setTeachOpen(false);await action(async()=>{await api(`/api/bots/${active.id}/skills/start`,{method:"POST",body:JSON.stringify({goal})});setComputerOpen(true)})}
  async function stopTeaching(){if(!active)return;await action(()=>api(`/api/bots/${active.id}/skills/stop`,{method:"POST",body:"{}"}));setComputerOpen(false)}
  async function cancelTeaching(){if(!active)return;await action(()=>api(`/api/bots/${active.id}/skills/cancel`,{method:"POST",body:"{}"}));setComputerOpen(false)}
  async function saveSkill(skill:TaughtSkill,name:string,playbook:Playbook){await action(()=>api(`/api/skills/${skill.id}`,{method:"PATCH",body:JSON.stringify({name,playbook,save:true})}))}
  async function testSkill(skill:TaughtSkill,name:string,playbook:Playbook){await action(async()=>{await api(`/api/skills/${skill.id}`,{method:"PATCH",body:JSON.stringify({name,playbook})});await api(`/api/skills/${skill.id}/test`,{method:"POST",body:"{}"})})}
  async function discardSkill(skill:TaughtSkill){await action(()=>api(`/api/skills/${skill.id}`,{method:"DELETE"}))}
  async function updateSkill(skill:TaughtSkill,name:string,playbook:Playbook){await action(()=>api(`/api/skills/${skill.id}`,{method:"PATCH",body:JSON.stringify({name,playbook,save:true})}));setEditingSkillId(null)}
  async function deleteSkill(skill:TaughtSkill){if(!window.confirm(t("deleteSkillConfirm",{name:skill.name||skill.goal})))return;await action(()=>api(`/api/skills/${skill.id}`,{method:"DELETE"}));setEditingSkillId(null)}
  const editingSkill=editingSkillId?skills.find(skill=>skill.id===editingSkillId)||null:null;
  function runSkill(skill:TaughtSkill){setPlusOpen(false);setDraft(current=>`${current.trim()?current.trimEnd()+"\n":""}執行「${skill.name}」`);document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus()}
  async function importSkillFile(file:File){if(!active)return;await action(async()=>{let payload:unknown;try{payload=JSON.parse(await file.text())}catch{throw new Error(t("skillImportInvalid"))}const skill=await api<TaughtSkill>(`/api/bots/${active.id}/skills/import`,{method:"POST",body:JSON.stringify(payload)});setEditingSkillId(skill.id)})}
  function addPendingFiles(list:FileList|File[]){const incoming=[...list];if(!incoming.length)return;setError(null);setPendingFiles(current=>{const next=[...current];for(const file of incoming){if(next.length>=ATTACH_MAX){setError(t("attachTooMany"));break}if(file.size>ATTACH_MAX_BYTES){setError(t("attachTooLarge"));continue}if(!attachAllowed(file)){setError(t("attachType"));continue}next.push({id:clientNonce(),file,preview:file.type.startsWith("image/")?URL.createObjectURL(file):null})}return next})}
  function removePendingFile(id:string){setPendingFiles(current=>current.filter(item=>{if(item.id===id&&item.preview)URL.revokeObjectURL(item.preview);return item.id!==id}))}
  async function send(event:FormEvent){event.preventDefault();if(sendingRef.current||busy)return;const text=draft.trim();const files=pendingFiles;if((!active&&!activeRoom)||!activeSessionId||(!text&&files.length===0))return;sendingRef.current=true;setDraft("");setPendingFiles([]);historyIndexRef.current=null;historyDraftRef.current="";try{await action(async()=>{try{const attachments=await Promise.all(files.map(async item=>({name:item.file.name,mimeType:item.file.type,content:await readAsBase64(item.file)})));await api(`/api/sessions/${activeSessionId}/messages`,{method:"POST",body:JSON.stringify({text,clientNonce:clientNonce(),attachments})});sentHistoryRef.current.push(text||files[0]?.file.name||"");if(sentHistoryRef.current.length>100)sentHistoryRef.current.shift();files.forEach(item=>item.preview&&URL.revokeObjectURL(item.preview));await loadSessions()}catch(error){setPendingFiles(files);throw error}})}finally{sendingRef.current=false}}
  function composerKeyDown(event:ReactKeyboardEvent<HTMLTextAreaElement>){if(event.nativeEvent.isComposing||event.key==="Process")return;if(slashSuggestions.length){if(event.key==="Escape"){event.preventDefault();setSlashDismissed(true);return}if(event.key==="ArrowDown"||event.key==="ArrowUp"){event.preventDefault();setSlashIndex(index=>(index+(event.key==="ArrowDown"?1:slashSuggestions.length-1))%slashSuggestions.length);return}if(event.key==="Tab"||(event.key==="Enter"&&!event.shiftKey)){event.preventDefault();setDraft(`/${slashSuggestions[slashIndex%slashSuggestions.length].name} `);return}}const history=sentHistoryRef.current;if(event.key==="ArrowUp"&&history.length>0&&(!event.currentTarget.value.includes("\n")||event.currentTarget.selectionStart===0)){event.preventDefault();if(historyIndexRef.current===null){historyDraftRef.current=draft;historyIndexRef.current=history.length-1}else historyIndexRef.current=Math.max(0,historyIndexRef.current-1);setDraft(history[historyIndexRef.current]);return}if(event.key==="ArrowDown"&&historyIndexRef.current!==null&&(!event.currentTarget.value.includes("\n")||event.currentTarget.selectionEnd===event.currentTarget.value.length)){event.preventDefault();if(historyIndexRef.current<history.length-1){historyIndexRef.current+=1;setDraft(history[historyIndexRef.current])}else{historyIndexRef.current=null;setDraft(historyDraftRef.current)}return}if(event.key==="Enter"&&!event.shiftKey){event.preventDefault();if(sendingRef.current||busy)return;event.currentTarget.form?.requestSubmit()}}
  function selectSession(id:string){setActiveSessionId(id);setSessionMenuOpen(false);if(sessionStoreKey)writeSessionStore(sessionStoreKey,id)}
  async function createSession(){if(!sessionsPath||!sessionStoreKey)return;setSessionMenuOpen(false);setBusy(true);setError(null);try{const session=await api<Session>(sessionsPath,{method:"POST",body:JSON.stringify({title:t("newConversation")})});writeSessionStore(sessionStoreKey,session.id);const next=await api<Session[]>(sessionsPath);setSessions(next);setActiveSessionId(session.id);setMessages([])}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function clearSession(){if(!activeSessionId)return;setClearOpen(false);setSessionMenuOpen(false);await action(async()=>{await api(`/api/sessions/${activeSessionId}/messages`,{method:"DELETE"});setMessages([]);await loadSessions()})}
  async function deleteSession(id:string){if(!sessionsPath||!sessionStoreKey)return;setSessionMenuOpen(false);setBusy(true);setError(null);try{await api(`/api/sessions/${id}`,{method:"DELETE"});const next=await api<Session[]>(sessionsPath);setSessions(next);const pick=id===activeSessionId||!next.some(session=>session.id===activeSessionId)?next[0]?.id||null:activeSessionId;setActiveSessionId(pick);if(pick)writeSessionStore(sessionStoreKey,pick)}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  async function rememberMessage(message:Message){const botId=message.speakerBotId||active?.id||activeRoom?.members[0]?.id;if(!botId||!message.body.trim())return;try{await api(`/api/bots/${botId}/memories`,{method:"POST",body:JSON.stringify({content:message.body,sessionId:activeSessionId})});setRemembered(current=>({...current,[message.id]:true}))}catch(e){setError(e instanceof Error?e.message:t("rememberFailed"))}}
  function pasteText(text:string){
    const botId=paneBotId;
    if(!botId||computer.controlHolder!=="user")return;
    pasteQueueRef.current=pasteQueueRef.current.catch(()=>{}).then(async()=>{
      if(currentPaneRef.current!==botId)return;
      await api(`/api/computer/${botId}/input`,{method:"POST",body:JSON.stringify({kind:"clipboard",text})});
      if(currentPaneRef.current===botId)setClipboardStatus("已貼上文字");
    }).catch(()=>setClipboardStatus("貼上失敗，請確認已接管電腦後重試"));
  }
  async function copySelection(){
    const botId=paneBotId;if(!botId)return;
    try{const result=await api<{text:string}>(`/api/computer/${botId}/input`,{method:"POST",body:JSON.stringify({kind:"copy"})});
      if(currentPaneRef.current!==botId)return;
      setDesktopClipboard(result.text);await navigator.clipboard.writeText(result.text);setClipboardStatus("已複製選取文字");
    }catch{setClipboardStatus("複製未同步；請按複製按鈕，或在遠端使用 Ctrl+Shift+C");}
  }
  async function pasteClipboard(){try{pasteText(await navigator.clipboard.readText())}catch{setClipboardOpen(true)}}
  async function copyClipboard(){try{await navigator.clipboard.writeText(desktopClipboard)}catch{setError(t("clipboardWriteBlocked"))}}
  async function inbox(bot:Bot,actionName:string,groupName?:string|null){await api(`/api/bots/${bot.id}/inbox`,{method:"POST",body:JSON.stringify({action:actionName,groupName})});await loadBots()}
  function openBot(bot:Bot){setCallOpen(false);setMobileNav(false);setSessionMenuOpen(false);if(bot.unreadCount>0)void inbox(bot,"read");if(bot.id===activeId&&!activeRoomId){if(!activeSessionId){const stored=readSessionStore()[bot.id];if(stored)setActiveSessionId(stored);else void loadSessions()}return}setMessages([]);setActiveSessionId(null);setActiveRoomId(null);setBusyMembers([]);setActiveId(bot.id)}
  function openRoom(room:Room){setCallOpen(false);setMobileNav(false);setSessionMenuOpen(false);if(rightPart==="settings")setRightPart("computer");if(room.id===activeRoomId){if(!activeSessionId){const stored=readSessionStore()[`room:${room.id}`];if(stored)setActiveSessionId(stored);else void loadSessions()}return}setMessages([]);setActiveSessionId(null);setActiveId(null);setBusyMembers([]);setActiveRoomId(room.id)}
  async function deleteRoom(room:Room){setRoomToDelete(null);await action(async()=>{await api(`/api/rooms/${room.id}`,{method:"DELETE"});if(activeRoomId===room.id){setActiveRoomId(null);setActiveSessionId(null);setMessages([]);setBusyMembers([])}await loadBots()})}
  function openAccount(dialog:AccountDialog){setAccountOpen(false);setAccountDialog(dialog)}
  async function logout(){setAccountOpen(false);await api("/api/session",{method:"DELETE",body:"{}"}).catch(()=>{});setBots([]);setRooms([]);setMcpServers([]);setActiveId(null);setActiveRoomId(null);setAuthRequired(true)}
  async function changeComputer(operation:"boot"|"restart"|"stop"){
    const botId=paneBotId;if(!botId)return;
    const previous=computer;setScreenUrl(null);setDesktopReady(false);
    setComputer(current=>({...current,state:operation==="stop"?current.state:operation==="boot"&&current.state==="suspended"?"suspended":"booting"}));
    try{const status=await api<ComputerStatus>(`/api/computer/${botId}/${operation}`,{method:"POST",body:"{}",signal:AbortSignal.timeout(120_000)});if(currentPaneRef.current===botId)setComputer(status)}
    catch(error){
      const status=await api<ComputerStatus>(`/api/computer/${botId}/status`,{signal:AbortSignal.timeout(5_000)}).catch(()=>previous);
      if(currentPaneRef.current===botId)setComputer(status);
      throw error;
    }
  }
  const startBoot=()=>changeComputer("boot");
  const stopComputer=()=>changeComputer("stop");
  const restartComputer=()=>changeComputer("restart");
  const connecting=computer.state==="running"&&Boolean(screenUrl)&&!desktopReady;
  const overlayLabel=hudLabel(computer,connecting,handingOff);
  const frame=screenUrl?<iframe ref={desktopFrameRef} key={screenUrl} className="desktop-frame" src={screenUrl} title={t("agentComputer")} allow="fullscreen; clipboard-read; clipboard-write"/>:<EmptyComputer state={computer.state}/>;
  const hud=paneBot&&overlayLabel?<ComputerHud bot={paneBot} label={overlayLabel}/>:null;
  const statusMembers=workingMembers;
  const topTools=<nav className="top-tools" aria-label={t("workTools")}>
    {active&&!activeRoomId?<span className="call-entry" tabIndex={!voiceSettings?.enabled?0:undefined} title={!voiceSettings?.enabled?t("voiceDisabledHint"):undefined} aria-label={!voiceSettings?.enabled?t("voiceDisabledHint"):undefined}><button type="button" className={`top-tool-button ${callOpen?"call-active":""}`} title={!voiceSettings?.enabled?t("voiceDisabledHint"):voiceSettings.ready?t("call"):t("setUpVoiceToCall")} aria-label={t("call")} disabled={!activeSessionId||!voiceSettings?.enabled} onClick={()=>{if(!voiceSettings?.enabled)return;if(!voiceSettings.ready){setAccountDialog("voice");return}setCallOpen(true)}}><PhoneIcon/></button></span>:null}
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="computer"?"active":""}`} title={t("computer")} aria-label={t("computer")} onClick={()=>openPane("computer")}><Computer/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="accounts"?"active":""}`} title={t("accounts")} aria-label={t("accounts")} onClick={()=>openPane("accounts")} disabled={!paneBot}><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="memory"?"active":""}`} title={t("memory")} aria-label={t("memory")} onClick={()=>openPane("memory")} disabled={!paneBot}><Brain/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="plugins"?"active":""}`} title={t("plugins")} aria-label={t("plugins")} onClick={()=>openPane("plugins")}><Plug/></button>
    <button type="button" className={`top-tool-button ${!rightCollapsed&&rightPart==="settings"?"active":""}`} title={active?t("botSettings"):t("settings")} aria-label={active?t("botSettings"):t("settings")} onClick={()=>active?openPane("settings"):setAccountDialog("settings")}><Settings/></button>
  </nav>;

  if(authRequired)return <LoginScreen authenticated={async()=>{setAuthRequired(false);setError(null);try{await loadBots()}catch(e){if(e instanceof ApiError&&e.status===401)setAuthRequired(true);else setError(e instanceof Error?e.message:t("loginFailed"))}}}/>;

  return <AvatarLookProvider value={looks}><div className={`app-shell ${rightCollapsed?"right-collapsed":"right-open"}`}>
    <aside className={`sidebar ${mobileNav?"open":""}`}>
      <div className="brand"><span>LazyBoy</span><div className="create-menu-wrap" onClick={event=>event.stopPropagation()}><button type="button" className="icon-button" onClick={event=>{event.stopPropagation();setCreateMenuOpen(v=>!v)}} aria-label={t("add")} aria-haspopup="menu" aria-expanded={createMenuOpen}><UseAnimations animation={plusToX} size={18} strokeColor="#dfdfe2" wrapperStyle={{pointerEvents:"none"}}/></button>{createMenuOpen&&<div className="create-menu" role="menu"><button type="button" role="menuitem" onClick={()=>{setCreateMenuOpen(false);setCreateOpen(true)}}><BotIcon/>{t("addBot")}</button><button type="button" role="menuitem" onClick={()=>{setCreateMenuOpen(false);setGroupOpen(true)}} disabled={bots.length===0}><Users/>{t("addGroup")}</button></div>}</div></div>
      <label className="search"><UseAnimations animation={searchToX} size={16} strokeColor="var(--muted)"/><input value={query} onChange={e=>setQuery(e.target.value)} placeholder={t("search")}/></label>
      <div className="bot-list">
        {filteredRooms.length>0&&<section className="bot-group"><div className="group-label">{t("groups")}</div>{filteredRooms.map(room=><button className={`bot-row room-row ${room.id===activeRoomId?"selected":""}`} key={room.id} onClick={()=>openRoom(room)} onContextMenu={e=>{e.preventDefault();setRoomContext({room,x:e.clientX,y:e.clientY})}}><span className="avatar-wrap"><AvatarStack members={room.members} online thinkingIds={room.id===activeRoomId?busyMembers.map(member=>member.id):[]}/>{room.unreadCount>0&&<i className="unread-dot" title={t("unreadMessages",{count:room.unreadCount})}/>}</span><span className="bot-copy"><strong>{room.name}</strong><small>{room.lastPreview||t("members",{count:room.members.length})}</small></span>{room.lastMessageAt&&<time className="row-time">{inboxTime(room.lastMessageAt)}</time>}</button>)}</section>}
        {sections.map(([label,items])=><section className="bot-group" key={label}><div className="group-label">{label}</div>{items.map(bot=><button className={`bot-row ${bot.id===activeId&&!activeRoomId?"selected":""}`} key={bot.id} onClick={()=>openBot(bot)} onContextMenu={e=>{e.preventDefault();setContext({bot,x:e.clientX,y:e.clientY})}}><span className="avatar-wrap"><Avatar lookId={bot.id} name={bot.name} color={bot.avatarColor} shape={bot.avatarShape} active={bot.id===activeId&&!activeRoomId} online/>{bot.unreadCount>0&&<i className="unread-dot" title={t("unreadMessages",{count:bot.unreadCount})}/>}</span><span className="bot-copy"><strong>{bot.name}</strong><small>{modeLabel(bot.computerMode)}</small></span>{bot.tags?.[0]&&<span className="bot-tag side-tag">{bot.tags[0]}</span>}{bot.lastMessageAt&&<time className="row-time">{inboxTime(bot.lastMessageAt)}</time>}{bot.pinned&&<Pin className="row-pin"/>}</button>)}</section>)}
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
            <button type="button" role="menuitem" onClick={()=>openAccount("model")}><BotIcon/>{t("modelSettings")}</button>
            <button type="button" role="menuitem" onClick={()=>openAccount("voice")}><PhoneIcon/>{t("voiceSettings")}</button>
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
      <header className="topbar"><button className="icon-button mobile-menu" onClick={()=>setMobileNav(v=>!v)}><UseAnimations animation={menu} size={18} strokeColor="#dfdfe2"/></button>{activeRoom?<><AvatarStack members={activeRoom.members} size={32} online thinkingIds={busyMembers.map(member=>member.id)}/><strong>{activeRoom.name}</strong><SessionMenu sessions={sessions} activeSessionId={activeSessionId} open={sessionMenuOpen} setOpen={setSessionMenuOpen} busy={busy} onSelect={selectSession} onCreate={createSession} onDelete={id=>{setSessionMenuOpen(false);setSessionToDelete(id)}} onClear={()=>{setSessionMenuOpen(false);setClearOpen(true)}}/><span className="grow"/>{topTools}</>:active?<><Avatar lookId={active.id} name={active.name} color={active.avatarColor} shape={active.avatarShape} active online/><strong>{active.name}</strong><SessionMenu sessions={sessions} activeSessionId={activeSessionId} open={sessionMenuOpen} setOpen={setSessionMenuOpen} busy={busy} onSelect={selectSession} onCreate={createSession} onDelete={id=>{setSessionMenuOpen(false);setSessionToDelete(id)}} onClear={()=>{setSessionMenuOpen(false);setClearOpen(true)}}/><span className="grow"/>{topTools}</>:<><strong>{t("chooseBot")}</strong><span className="grow"/>{topTools}</>}</header>
      {callOpen&&voiceSettings?.enabled&&active&&activeSessionId&&!activeRoomId?<CallOverlay bot={active} sessionId={activeSessionId} takeover={computer.takeoverRequested||computer.controlHolder==="user"} onHangUp={()=>setCallOpen(false)} onTakeOver={()=>{setRightPart("computer");setRightCollapsed(false);if(active)void action(()=>api(`/api/computer/${active.id}/takeover`,{method:"POST",body:"{}"}))}}/>:null}
      <div className="messages">{(activeRoom||active)&&messages.length===0?<div className="welcome">{activeRoom?<AvatarStack members={activeRoom.members} size={56} online/>:<Avatar lookId={active!.id} name={active!.name} color={active!.avatarColor} shape={active!.avatarShape} active online size={64}/>}<h1>{activeRoom?t("startRoomDiscussion",{name:activeRoom.name}):t("startBotWork",{name:active!.name})}</h1><p>{activeRoom?t("roomWillReply",{names:activeRoom.members.map(member=>member.name).join("、")}):active!.description||t("botWelcome")}</p></div>:messages.map(message=>{const spoken=message.role!=="user"&&Boolean(activeRoom);const speakerName=message.speakerName||(spoken?paneBot?.name:undefined);const speakerShape=(message.speakerShape||paneBot?.avatarShape||"blob") as AvatarShape;const files=messageFiles(message.blocks);const chips=chipBlocks(message.blocks);const hideBody=isAutoAttachCaption(message.body,files);return <div key={message.id} className={`message ${message.role} ${spoken?"spoken":""} ${files.length?"with-files":""}`}>{spoken&&<span className="msg-avatar"><Avatar lookId={message.speakerBotId||paneBot?.id||undefined} name={speakerName||"agent"} color={message.speakerColor||undefined} shape={speakerShape} size={22}/></span>}{spoken&&<b className="speaker" style={{color:message.speakerColor||undefined}}>{speakerName}</b>}{files.length>0&&<div className="msg-attachments">{files.map(file=><FileCard key={file.name} file={file}/>)}</div>}{chips.map((chip,index)=>chip.kind==="login"?<div className="login-chip" key={`${message.id}-login-${index}`}><div className="login-label">{t("loginNeedsYou")}</div><div className="login-site">{chip.site||message.body}</div>{chip.why?<div className="login-why">{t("loginWhy",{why:chip.why})}</div>:null}<button type="button" className="primary" onClick={()=>void openLoginScreen()}>{t("loginOpenScreen")}</button></div>:chip.kind==="schedule"?<div className="sched-chip" key={`${message.id}-sched-${index}`}><div className="sched-label">{t("scheduleChip")}</div><strong>{chip.name}</strong><small>{chip.human}</small></div>:<div className="sched-chip" key={`${message.id}-run-${index}`}><div className="sched-label">{t("scheduleRunChip")}</div><strong>{chip.name}</strong><small>{chip.human}</small></div>)}{!hideBody&&(message.role==="assistant"?<div className="message-stack"><div className="message-body md"><ChatMarkdown>{message.body}</ChatMarkdown></div>{message.body.trim()?<CopyMessageButton text={message.body}/>:null}</div>:<span className="message-body">{message.body}</span>)}{!hideBody&&message.body.trim()&&<button type="button" className={`remember-msg ${remembered[message.id]?"saved":""}`} title={remembered[message.id]?t("remembered"):t("remember")} disabled={!!remembered[message.id]} onClick={()=>void rememberMessage(message)}><UseAnimations animation={bookmark} size={14} strokeColor="var(--muted)"/></button>}</div>})}{pausedForUser&&paneBot&&<div className="pause-banner" role="status"><span>{computer.controlHolder==="user"?t("pausedUserControl",{name:paneBot.name}):t("pausedNeedsUser",{name:paneBot.name})}{(computer.queuedRuns||0)>0&&<> {t("queuedMessages",{count:computer.queuedRuns||0})}</>}</span>{computer.controlHolder==="user"?<button type="button" className="primary" disabled={busy} onClick={()=>void action(()=>api(`/api/computer/${paneBot.id}/release`,{method:"POST",body:"{}"}))}>{t("releaseAndContinue")}</button>:<button type="button" className="primary" disabled={busy} onClick={()=>void action(()=>api(`/api/computer/${paneBot.id}/takeover`,{method:"POST",body:"{}"}))}>{t("takeOverNow")}</button>}</div>}{teaching&&active&&<div className="teach-banner recording" role="status"><span><i className="record-dot live"/>{t("teachingLive",{goal:teaching.goal})}<small>{t("teachingHint")}{teaching.eventCount>0&&<> · {t("teachingCaptured",{count:teaching.eventCount})}</>}</small></span><button type="button" className="primary" disabled={busy} onClick={()=>void stopTeaching()}>{t("finishDemo")}</button><button type="button" className="outline" disabled={busy} onClick={()=>void cancelTeaching()}>{t("cancel")}</button></div>}{drafting&&<div className="teach-banner" role="status"><UseAnimations animation={loading} size={18} wrapperStyle={{display:"inline-block",verticalAlign:"middle"}}/><span>{t("distilling",{goal:drafting.goal})}</span></div>}{skillDraft&&active&&<SkillDraftCard key={skillDraft.id} skill={skillDraft} busy={busy} onSave={(name,playbook)=>void saveSkill(skillDraft,name,playbook)} onTest={(name,playbook)=>void testSkill(skillDraft,name,playbook)} onDiscard={()=>void discardSkill(skillDraft)} onEdit={()=>setEditingSkillId(skillDraft.id)} onExport={(name,playbook)=>downloadSkill(name,skillDraft.goal,playbook)}/>}<div ref={messageEndRef} aria-hidden="true"/></div>
      {error&&<div className="error-banner"><span>{error}</span><button onClick={()=>setError(null)}><X/></button></div>}
      {otherSessionBusy&&<div className="queue-hint">{t("anotherConversationQueued")}</div>}
      <div className={`composer-dock ${statusMembers.length?"has-status":""}`}>
      {statusMembers.map(member=>{const step=computer.busyStep&&member.id===computer.botId?computer.busyStep:null;const transition=isTransitionStep(step);const label=t("working",{name:member.name});return <div className="thinking-row" key={member.id}><Avatar lookId={member.id} name={member.name} color={member.avatarColor} shape={member.avatarShape} thinking online/><span className="working-copy"><span className="working-label">{label}</span>{!transition&&step?<span className="working-step">{step}</span>:null}</span></div>})}
      <form className={`composer ${pendingFiles.length?"has-files":""}`} onSubmit={send} onDragOver={event=>{event.preventDefault()}} onDrop={event=>{event.preventDefault();if(event.dataTransfer.files.length)addPendingFiles(event.dataTransfer.files)}}><div className="plus-menu-wrap" onClick={event=>event.stopPropagation()}><button type="button" className={`composer-plus ${plusOpen?"open":""}`} disabled={!activeSessionId} title={t("moreActions")} aria-label={t("moreActions")} aria-haspopup="menu" aria-expanded={plusOpen} onClick={()=>setPlusOpen(v=>!v)}><Plus/></button>{plusOpen&&<div className="plus-menu" role="menu"><button type="button" role="menuitem" disabled={!activeSessionId||Boolean(teaching)} title={t("attachFileHint")} onClick={()=>{setPlusOpen(false);attachRef.current?.click()}}><Paperclip/>{t("attachFile")}</button><button type="button" role="menuitem" disabled={!active||Boolean(teaching)||Boolean(drafting)} title={active?t("teachTaskHint"):t("teachNeedsBot")} onClick={()=>{setPlusOpen(false);setTeachOpen(true)}}><i className="record-dot"/>{t("teachTask")}</button><button type="button" role="menuitem" disabled={!active} title={t("importSkillHint")} onClick={()=>{setPlusOpen(false);importRef.current?.click()}}><Upload/>{t("importSkill")}</button>{savedSkills.length>0&&<><hr/><div className="plus-menu-skills"><small className="plus-menu-label">{t("taughtSkills")}{savedSkills.length>5?` · ${savedSkills.length}`:""}</small>{savedSkills.length>=6&&<input className="plus-menu-search" value={skillQuery} onChange={e=>setSkillQuery(e.target.value)} placeholder={t("searchSkills")} aria-label={t("searchSkills")} onClick={e=>e.stopPropagation()}/>}<div className="plus-menu-skill-list">{listedSkills.map(skill=><div className="plus-menu-skill" key={skill.id}><button type="button" role="menuitem" title={t("runSkillNamed",{name:skill.name})+(skill.playbook.whenToUse?`\n${skill.playbook.whenToUse}`:"")} onClick={()=>runSkill(skill)}><Sparkle/>{skill.name}</button><button type="button" className="skill-edit" title={t("exportSkillHint")} aria-label={t("exportSkill")} onClick={()=>downloadSkill(skill.name,skill.goal,skill.playbook)}><Download/></button><button type="button" className="skill-edit" title={t("editSkill")} aria-label={t("editSkill")} onClick={()=>{setPlusOpen(false);setEditingSkillId(skill.id)}}><Pencil/></button></div>)}{listedSkills.length===0&&<small className="plus-menu-empty">{t("noMatchingSkills")}</small>}</div></div></>}</div>}</div><input ref={importRef} className="skill-import-input" type="file" accept="application/json,.json" tabIndex={-1} aria-hidden="true" onChange={event=>{const file=event.target.files?.[0];event.currentTarget.value="";if(file)void importSkillFile(file)}}/><input ref={attachRef} className="skill-import-input attach-input" type="file" multiple accept={ATTACH_ACCEPT} tabIndex={-1} aria-hidden="true" onChange={event=>{const files=[...event.target.files||[]];event.currentTarget.value="";if(files.length)addPendingFiles(files)}}/>{pendingFiles.length>0&&<div className="composer-files">{pendingFiles.map(item=><FileCard key={item.id} file={{name:item.file.name,size:item.file.size}} preview={item.preview} onRemove={()=>removePendingFile(item.id)}/>)}</div>}{slashSuggestions.length>0&&<div className="slash-suggestions" role="listbox" aria-label="指令與技能">{slashSuggestions.map((skill,index)=><button type="button" role="option" aria-selected={index===slashIndex} key={skill.name} onClick={()=>{setDraft(`/${skill.name} `);requestAnimationFrame(()=>document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus())}}><strong>/{skill.name}</strong><small>{skill.kind}</small><span>{skill.description}</span></button>)}</div>}<textarea rows={1} value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={composerKeyDown} onPaste={event=>{const files=event.clipboardData?.files;if(files&&files.length){event.preventDefault();addPendingFiles(files)}}} placeholder={teaching?t("teachingComposerHint"):activeSessionId&&chatName?t("messageTo",{name:chatName}):t("chooseConversationFirst")} disabled={!activeSessionId||Boolean(teaching)}/>{sessionBusy?<button type="button" className="send stop-send" title={t("stopConversation")} onClick={()=>void action(stopChat)}><Square/></button>:<button className="send" disabled={!activeSessionId||(!draft.trim()&&pendingFiles.length===0)||busy}><UseAnimations animation={arrowUp} size={20} strokeColor="#1b1b1c"/></button>}</form>
      </div>
    </main>

    {!rightCollapsed&&<div className="side-card-backdrop" onClick={()=>setRightCollapsed(true)}/>}
    {!rightCollapsed&&<aside className="side-card">
      <>
        <header className="side-card-head">
          <span className="side-card-title">{rightPart==="computer"?t("computer"):rightPart==="memory"?t("memory"):rightPart==="plugins"?t("plugins"):rightPart==="accounts"?t("accounts"):t("settings")}</span>
          <button type="button" className="icon-button" title={t("collapseSidebar")} onClick={()=>setRightCollapsed(true)}><ChevronsRight/></button>
        </header>
        <div className="side-card-body">
          <div className={`side-part computer-part ${rightPart==="computer"?"":"hidden-part"}`}>
            <div className="computer-status-row">{paneBot?<span>{t("botComputer",{name:paneBot.name})}</span>:<span>{t("computer")}</span>}{computer.state==="booting"?<UseAnimations animation={loading} size={17} wrapperStyle={{display:"inline-block",verticalAlign:"middle"}}/>:<i className={`state-dot ${computer.state}`}/>}<small>{stateLabel(computer.state)}</small></div>
            <div className="preview">{computerOpen?<EmptyComputer state={computer.state}/>:frame}{!computerOpen&&hud}</div>
            {paneBot&&<><div className="computer-caption"><span>{t("dedicatedScreen")}</span><button className="outline" onClick={()=>setComputerOpen(true)}>{t("enlarge")}</button></div>{teaching?<div className="control-bar"><div className="computer-actions"><button className="primary" disabled={busy} onClick={()=>void stopTeaching()}>{t("finishDemo")}</button><button className="outline" disabled={busy} onClick={()=>void cancelTeaching()}>{t("cancel")}</button></div></div>:<ControlBar active={paneBot} computer={computer} busy={busy} action={action} paste={pasteClipboard} copy={copyClipboard} sessionId={activeSessionId} onBoot={startBoot} onRestart={restartComputer} onStop={stopComputer}/>}<p className="computer-login-hint">{t("computerLoginHint")}</p><p className="clipboard-status" role="status">{clipboardStatus}</p>{scheduleDraft?<ScheduleEditor draft={scheduleDraft} timezone={scheduleDraft.timezone||Intl.DateTimeFormat().resolvedOptions().timeZone||"Asia/Taipei"} saving={scheduleSaving} error={scheduleError} onChange={next=>setScheduleDraft(current=>current?{...current,...next}:next)} onBack={()=>setScheduleDraft(null)} onSave={()=>void saveScheduleDraft()} onDelete={scheduleDraft.id?()=>void action(async()=>{await api(`/api/schedules/${scheduleDraft.id}`,{method:"DELETE"});setScheduleDraft(null);await reloadSchedules()}):undefined}/>:<ScheduleList items={schedules} runningId={runningScheduleId} onCreate={()=>setScheduleDraft({name:"",instructions:"",enabled:true,preset:defaultCronPreset()})} onOpen={item=>setScheduleDraft({id:item.id,timezone:item.timezone,threadId:item.threadId,name:item.name,instructions:item.instructions,enabled:item.enabled,preset:presetFromCron(item.cron)})} onRun={item=>void action(async()=>{setRunningScheduleId(item.id);try{await api(`/api/schedules/${item.id}/run`,{method:"POST",body:"{}"});await loadSessions()}finally{setRunningScheduleId(null)}})}/>}</>}
          </div>
          {rightPart==="accounts"&&paneBot&&<VaultPane bot={paneBot}/>}
          {rightPart==="memory"&&paneBot&&<MemoryPane bot={paneBot} changed={loadBots}/>}
          {rightPart==="plugins"&&<McpPane servers={mcpServers} reload={loadMcp}/>}
          {rightPart==="settings"&&active&&<BotSettingsPane bot={active} look={looks[active.id]||DEFAULT_LOOK} onLook={look=>{writeAvatarLook(active.id,look);setLooks(readAvatarLooks())}} saved={loadBots} onDelete={()=>setDeleteOpen(true)}/>}
        </div>
      </>
    </aside>}

    {computerOpen&&paneBot&&<div className="computer-overlay"><header><div><Avatar lookId={paneBot.id} name={paneBot.name} color={paneBot.avatarColor} shape={paneBot.avatarShape} active online/><strong>{modeLabel(computer.mode)}</strong><span className={`control-badge ${teaching?"teaching":""}`}>{teaching?t("teachingBadge"):computer.controlHolder==="user"?t("userControlling"):computer.usingComputer?t("aiReadOnly"):t("readOnly")}</span></div><div>{teaching?<div className="computer-actions"><button className="primary" disabled={busy} onClick={()=>void stopTeaching()}>{t("finishDemo")}</button><button className="outline" disabled={busy} onClick={()=>void cancelTeaching()}>{t("cancel")}</button></div>:<ControlButtons computer={computer} busy={busy} action={action} active={paneBot} sessionId={activeSessionId} onBoot={startBoot} onRestart={restartComputer} onStop={stopComputer}/>}<button className="icon-button" onClick={pasteClipboard} disabled={computer.controlHolder!=="user"} title={t("pasteClipboard")}><ClipboardPaste/></button><button className="icon-button" onClick={copyClipboard} disabled={computer.controlHolder!=="user"||!desktopClipboard} title={t("copyDesktopClipboard")}><UseAnimations animation={copy} size={18} strokeColor="#dfdfe2"/></button><button className="icon-button" onClick={()=>setComputerOpen(false)}><X/></button></div></header><div className="overlay-screen"><div className="overlay-desktop">{frame}{hud}</div></div>{error&&<div className="overlay-error">{error}</div>}</div>}
    {teachOpen&&active&&<TeachDialog bot={active} busy={busy} close={()=>setTeachOpen(false)} start={goal=>void startTeaching(goal)}/>}
    {editingSkill&&<SkillEditDialog key={editingSkill.id} skill={editingSkill} busy={busy} close={()=>setEditingSkillId(null)} save={(name,playbook)=>void updateSkill(editingSkill,name,playbook)} test={(name,playbook)=>void testSkill(editingSkill,name,playbook)} remove={()=>void deleteSkill(editingSkill)} exportFile={(name,playbook)=>downloadSkill(name,editingSkill.goal,playbook)}/>}
    {clipboardOpen&&<ClipboardDialog close={()=>setClipboardOpen(false)} paste={text=>{pasteText(text);setClipboardOpen(false)}}/>}

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
    {accountDialog==="model"&&<ModelSettingsDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="voice"&&<VoiceSettingsDialog close={()=>{setAccountDialog(null);api<VoiceSettings>("/api/voice/settings").then(setVoiceSettings).catch(()=>setVoiceSettings(null))}}/>}
    {accountDialog==="about"&&<AboutDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="help"&&<HelpDialog close={()=>setAccountDialog(null)}/>}
    {accountDialog==="feedback"&&<FeedbackDialog close={()=>setAccountDialog(null)}/>}
  </div></AvatarLookProvider>
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

const EXPR_I18N = {idle:"exprIdle",happy:"exprHappy",sad:"exprSad",mad:"exprMad",surprised:"exprSurprised",wink:"exprWink",sleepy:"exprSleepy",smug:"exprSmug",unsure:"exprUnsure",scared:"exprScared",love:"exprLove",shy:"exprShy",sick:"exprSick",thinking:"exprThinking"} as const satisfies Record<AvatarExpression, MessageKey>;
const BG_I18N = {none:"bgNone",circle:"bgCircle",squircle:"bgSquircle",square:"bgSquare"} as const satisfies Record<AvatarBackground, MessageKey>;

function BotSettingsPane({bot,look,onLook,saved,onDelete}:{bot:Bot;look:AvatarLook;onLook:(look:AvatarLook)=>void;saved:()=>Promise<void>|void;onDelete:()=>void}){
  const[name,setName]=useState(bot.name);const[title,setTitle]=useState(bot.title);const[description,setDescription]=useState(bot.description);const[color,setColor]=useState(bot.avatarColor||"#8B5CF6");const[shape,setShape]=useState<AvatarShape>(resolveBlobatarShape(bot.avatarShape));const[expression,setExpression]=useState<AvatarExpression>(look.expression);const[background,setBackground]=useState<AvatarBackground>(look.background);const[tagText,setTagText]=useState((bot.tags||[]).join("、"));const[busy,setBusy]=useState(false);
  useEffect(()=>{setName(bot.name);setTitle(bot.title);setDescription(bot.description);setColor(bot.avatarColor||"#8B5CF6");setShape(resolveBlobatarShape(bot.avatarShape));setExpression(look.expression);setBackground(look.background);setTagText((bot.tags||[]).join("、"))},[bot.id,bot.name,bot.title,bot.description,bot.avatarColor,bot.avatarShape,bot.tags,look.expression,look.background]);
  const colors=["#08A99D","#F1F2F2","#956A43","#DD263B","#F36C05","#F39A00","#00B873","#1985E6","#7140D9","#DC2781","#A7A7A7"];
  const face={name:name||bot.name,color,shape,expression,background};
  return <form className="pane-form settings-pane" onSubmit={async e=>{e.preventDefault();if(!name.trim())return;setBusy(true);try{await api(`/api/bots/${bot.id}`,{method:"PATCH",body:JSON.stringify({name:name.trim(),title,description,avatarColor:color,avatarShape:persistBlobatarShape(shape),tags:tagText.split(/[、,，]/).map(v=>v.trim()).filter(Boolean)})});onLook({expression,background});await saved()}finally{setBusy(false)}}}>
    <div className="avatar-editor"><Avatar {...face} size={96}/><strong>{t("avatarAppearance")}</strong><small>{t("avatarAppearanceHint")}</small></div>
    <fieldset><legend>{t("color")}</legend><div className="color-grid">{colors.map(value=><button type="button" key={value} className={color===value?"selected":""} style={{background:value}} onClick={()=>setColor(value)} aria-label={t("chooseColor",{value})}/>)}<label className="custom-color" title={t("customColor")}><input type="color" value={color} onChange={e=>setColor(e.target.value.toUpperCase())}/><span>＋</span></label></div></fieldset>
    <fieldset><legend>{t("shape")}</legend><div className="shape-grid">{BLOBATAR_SHAPES.map(value=><button type="button" className={resolveBlobatarShape(shape)===value?"selected":""} onClick={()=>setShape(value)} key={value}><Avatar {...face} shape={value} gaze={false}/></button>)}</div></fieldset>
    <fieldset><legend>{t("expression")}</legend><div className="expr-grid">{BLOBATAR_EXPRESSIONS.map(value=><button type="button" className={expression===value?"selected":""} onClick={()=>setExpression(value)} key={value}><Avatar {...face} expression={value} gaze={false} size={36}/><span>{t(EXPR_I18N[value])}</span></button>)}</div></fieldset>
    <fieldset><legend>{t("background")}</legend><div className="bg-grid">{BLOBATAR_BACKGROUNDS.map(value=><button type="button" className={background===value?"selected":""} onClick={()=>setBackground(value)} key={value}><Avatar {...face} background={value} gaze={false} size={36}/><span>{t(BG_I18N[value])}</span></button>)}</div></fieldset>
    <label>{t("name")}<input value={name} maxLength={80} onChange={e=>setName(e.target.value)}/></label>
    <label>{t("tags")}<input value={tagText} maxLength={120} onChange={e=>setTagText(e.target.value)} placeholder={t("tagsPlaceholder")}/><small>{t("tagsLimit")}</small></label>
    <label>{t("shortTitle")}<input value={title} maxLength={100} onChange={e=>setTitle(e.target.value)} placeholder={t("shortTitlePlaceholder")}/></label>
    <label>{t("description")}<textarea value={description} maxLength={1000} rows={4} onChange={e=>setDescription(e.target.value)} placeholder={t("descriptionPlaceholder")}/></label>
    <div className="pane-actions"><button className="primary" disabled={busy||!name.trim()}>{busy?t("saving"):t("saveSettings")}</button><button type="button" className="danger" onClick={onDelete}>{t("deleteBot")}</button></div>
  </form>
}

function VaultPane({bot}:{bot:Bot}){
  type Account={id:string;site:string;host:string;username:string;notes:string};
  const[items,setItems]=useState<Account[]>([]);const[busy,setBusy]=useState(false);const[error,setError]=useState("");
  const[form,setForm]=useState({site:"",host:"",username:"",password:"",notes:""});
  const load=useCallback(()=>api<Account[]>(`/api/bots/${bot.id}/accounts`).then(setItems),[bot.id]);
  useEffect(()=>{load().catch(e=>setError(e instanceof Error?e.message:t("loadFailed")))},[load]);
  async function run(work:()=>Promise<unknown>){setBusy(true);setError("");try{await work();await load()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  function setField<K extends keyof typeof form>(key:K,value:string){setForm(current=>({...current,[key]:value}))}
  return <div className="vault-pane">
    <p className="memory-help">{t("accountsHelp")}</p>
    <form className="vault-form" onSubmit={e=>{e.preventDefault();if(!form.site.trim()||!form.username.trim()||!form.password)return;void run(async()=>{await api(`/api/bots/${bot.id}/accounts`,{method:"POST",body:JSON.stringify(form)});setForm({site:"",host:"",username:"",password:"",notes:""})})}}>
      <div className="vault-fields">
        <label>{t("accountSite")}<input value={form.site} onChange={e=>setField("site",e.target.value)} placeholder={t("accountSitePlaceholder")} autoComplete="off"/></label>
        <label>{t("accountHost")}<input value={form.host} onChange={e=>setField("host",e.target.value)} placeholder={t("accountHostPlaceholder")} autoComplete="off"/></label>
        <label>{t("accountUsername")}<input value={form.username} onChange={e=>setField("username",e.target.value)} autoComplete="username"/></label>
        <label>{t("accountPassword")}<input type="password" value={form.password} onChange={e=>setField("password",e.target.value)} autoComplete="new-password"/></label>
      </div>
      <label>{t("accountNotes")}<input value={form.notes} onChange={e=>setField("notes",e.target.value)} placeholder={t("accountNotesPlaceholder")} autoComplete="off"/></label>
      <button className="primary" disabled={busy||!form.site.trim()||!form.username.trim()||!form.password}>{t("addAccount")}</button>
    </form>
    <div className="account-list">{items.length===0?<p className="vault-empty">{t("noAccounts")}</p>:items.map(item=><div className="account-row" key={item.id}><div className="account-row-main"><strong>{item.site}</strong><small>{item.username}{item.host?` · ${item.host}`:""}</small>{item.notes?<small className="account-notes">{item.notes}</small>:null}</div><button type="button" className="icon-button" title={t("delete")} aria-label={t("delete")} disabled={busy} onClick={()=>void run(()=>api(`/api/bots/${bot.id}/accounts/${item.id}`,{method:"DELETE"}))}><UseAnimations animation={trash2} size={18} strokeColor="#dfdfe2"/></button></div>)}</div>
    {error&&<div className="pane-error">{error}</div>}
  </div>
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
  const[picker,setPicker]=useState(false);const[busy,setBusy]=useState(false);const[error,setError]=useState("");const[openId,setOpenId]=useState<string|null>(null);
  async function run(work:()=>Promise<unknown>){setBusy(true);setError("");try{await work();await reload()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}finally{setBusy(false)}}
  return <div className="mcp-pane">
    <p className="memory-help">{t("mcpHelp")}</p>
    <button type="button" className="primary mcp-choose" disabled={busy} onClick={()=>setPicker(true)}><Plug/>{t("chooseMcp")}</button>
    {picker&&<McpPickerDialog added={servers.map(server=>server.name)} close={()=>setPicker(false)} connected={async()=>{setPicker(false);await reload()}}/>}
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

function McpPickerDialog({added,close,connected}:{added:string[];close:()=>void;connected:()=>Promise<void>}){
  const[query,setQuery]=useState("");const[custom,setCustom]=useState(false);
  const[items,setItems]=useState<McpCatalogEntry[]>([]);const[loading,setLoading]=useState(true);
  const[picked,setPicked]=useState<McpCatalogEntry|null>(null);
  const[secrets,setSecrets]=useState<Record<string,string>>({});
  const[busy,setBusy]=useState(false);const[error,setError]=useState("");
  const addedSet=useMemo(()=>new Set(added.map(name=>name.toLowerCase())),[added]);
  useEffect(()=>{const timer=setTimeout(()=>{setLoading(true);api<{servers:McpCatalogEntry[]}>(`/api/mcp-catalog?q=${encodeURIComponent(query.trim())}`).then(result=>setItems(result.servers)).catch(e=>setError(e instanceof Error?e.message:t("loadFailed"))).finally(()=>setLoading(false))},query?280:0);return()=>clearTimeout(timer)},[query]);
  function needsSecrets(entry:McpCatalogEntry){return entry.envKeys.some(field=>field.required)||entry.headerKeys.some(field=>field.required)}
  async function connect(entry:McpCatalogEntry,values:Record<string,string>){
    const env:Record<string,string>={};const headers:Record<string,string>={};
    for(const field of entry.envKeys){const value=values[field.name]?.trim();if(value)env[field.name]=value;else if(field.required){setError(t("mcpKeyHint"));return}}
    for(const field of entry.headerKeys){const value=values[field.name]?.trim();if(value)headers[field.name]=value;else if(field.required){setError(t("mcpKeyHint"));return}}
    setBusy(true);setError("");
    try{
      const server=await api<McpServer>("/api/mcp-servers",{method:"POST",body:JSON.stringify({name:entry.title.slice(0,80),transport:entry.transport,command:entry.command,args:entry.args,url:entry.url,env,headers,enabled:true})});
      if(server.enabled&&server.status!=="connected"){setError(server.error||t("mcpConnectFailed"));return}
      await connected();
    }catch(e){setError(e instanceof Error?e.message:t("operationFailed"))}
    finally{setBusy(false)}
  }
  function choose(entry:McpCatalogEntry){
    if(addedSet.has(entry.title.toLowerCase())||addedSet.has(entry.id.toLowerCase()))return;
    if(needsSecrets(entry)){setPicked(entry);setSecrets({});return}
    void connect(entry,{});
  }
  return <div className="modal-backdrop" onClick={close}><div className="dialog mcp-picker" onClick={e=>e.stopPropagation()}>
    <div className="dialog-title"><h2>{t("mcpPickerTitle")}</h2><button type="button" onClick={close}><X/></button></div>
    {picked?<div className="mcp-secret-step">
      <button type="button" className="outline mcp-back" onClick={()=>setPicked(null)}>{t("mcpBackToList")}</button>
      <strong>{t("mcpConnectNamed",{name:picked.title})}</strong>
      <p className="dialog-lead">{t("mcpKeyHint")}</p>
      {[...picked.envKeys,...picked.headerKeys].map(field=><label key={field.name}>{field.name}{field.required?" *":""}<input type={field.secret?"password":"text"} autoComplete="off" value={secrets[field.name]||""} onChange={e=>setSecrets(current=>({...current,[field.name]:e.target.value}))} placeholder={field.hint||field.name}/></label>)}
      {error&&<div className="pane-error">{error}</div>}
      <div className="dialog-actions"><button type="button" className="outline" onClick={()=>setPicked(null)}>{t("cancel")}</button><button type="button" className="primary" disabled={busy} onClick={()=>void connect(picked,secrets)}>{busy?t("connecting"):t("connectMcp")}</button></div>
    </div>:custom?<McpCustomForm busy={busy} error={error} onBack={()=>setCustom(false)} onSubmit={async payload=>{setBusy(true);setError("");try{const server=await api<McpServer>("/api/mcp-servers",{method:"POST",body:JSON.stringify(payload)});if(server.enabled&&server.status!=="connected"){setError(server.error||t("mcpConnectFailed"));setBusy(false);return}await connected()}catch(e){setError(e instanceof Error?e.message:t("operationFailed"));setBusy(false)}}}/>:<>
      <p className="dialog-lead">{t("mcpPickerLead")}</p>
      <label className="mcp-picker-search"><UseAnimations animation={searchToX} size={16} strokeColor="var(--muted)"/><input autoFocus value={query} onChange={e=>setQuery(e.target.value)} placeholder={t("searchMcp")}/></label>
      <div className="mcp-picker-grid">{loading&&items.length===0?<div className="mcp-picker-empty">{t("connecting")}</div>:items.length===0?<div className="mcp-picker-empty">{t("mcpNoResults")}</div>:items.map(entry=>{const already=addedSet.has(entry.title.toLowerCase())||addedSet.has(entry.id.toLowerCase());return <button type="button" className={`mcp-pick-card ${already?"added":""}`} key={entry.id} disabled={already||busy} onClick={()=>choose(entry)}>
        <span className="mcp-pick-icon">{entry.title.slice(0,1).toUpperCase()}</span>
        <strong>{entry.title}</strong>
        <small>{entry.description}</small>
        <span className="mcp-pick-meta">{already?t("mcpAdded"):entry.remote?t("mcpRemote"):t("mcpLocal")}{needsSecrets(entry)&&!already?` · ${t("mcpNeedsKey")}`:""}</span>
      </button>})}</div>
      {error&&<div className="pane-error">{error}</div>}
      <div className="dialog-actions"><button type="button" className="outline" onClick={()=>setCustom(true)}>{t("mcpCustom")}</button><button type="button" className="outline" onClick={close}>{t("close")}</button></div>
    </>}
  </div></div>
}

function McpCustomForm({busy,error,onBack,onSubmit}:{busy:boolean;error:string;onBack:()=>void;onSubmit:(payload:Record<string,unknown>)=>Promise<void>}){
  const[name,setName]=useState("");const[transport,setTransport]=useState<McpTransport>("http");
  const[command,setCommand]=useState("");const[args,setArgs]=useState("");const[url,setUrl]=useState("");
  const[envText,setEnvText]=useState("");const[headerText,setHeaderText]=useState("");
  return <form className="mcp-add" onSubmit={e=>{e.preventDefault();const trimmed=name.trim();if(!trimmed)return;void onSubmit({name:trimmed,transport,command:command.trim()||null,args:args.trim()?args.trim().split(/\s+/):[],url:url.trim()||null,env:parsePairs(envText),headers:parsePairs(headerText),enabled:true})}}>
    <button type="button" className="outline mcp-back" onClick={onBack}>{t("mcpBackToList")}</button>
    <label>{t("name")}<input autoFocus value={name} maxLength={80} onChange={e=>setName(e.target.value)} placeholder={t("mcpNamePlaceholder")}/></label>
    <div className="mcp-transport">{(["http","sse","stdio"] as McpTransport[]).map(value=><button type="button" key={value} className={transport===value?"picked":""} onClick={()=>setTransport(value)}>{value==="http"?"HTTP":value==="sse"?"SSE":"stdio"}</button>)}</div>
    {transport==="stdio"?<>
      <label>{t("command")}<input value={command} onChange={e=>setCommand(e.target.value)} placeholder={t("commandPlaceholder")}/></label>
      <label>{t("arguments")}<input value={args} onChange={e=>setArgs(e.target.value)} placeholder={t("argumentsPlaceholder")}/></label>
      <label>{t("environmentVariables")}<textarea rows={3} value={envText} onChange={e=>setEnvText(e.target.value)} placeholder={t("environmentVariablesPlaceholder")}/></label>
    </>:<>
      <label>{t("url")}<input value={url} onChange={e=>setUrl(e.target.value)} placeholder={t("urlPlaceholder")}/></label>
      <label>{t("headers")}<textarea rows={3} value={headerText} onChange={e=>setHeaderText(e.target.value)} placeholder={t("headersPlaceholder")}/></label>
    </>}
    {error&&<div className="pane-error">{error}</div>}
    <div className="dialog-actions"><button type="button" className="outline" onClick={onBack}>{t("cancel")}</button><button className="primary" disabled={busy||!name.trim()||(transport==="stdio"?!command.trim():!url.trim())}>{busy?t("connecting"):t("connectMcp")}</button></div>
  </form>
}

function ComputerHud({bot,label}:{bot:{id:string;name:string;avatarColor?:string;avatarShape?:AvatarShape};label:string}){
  return <div className="computer-hud" role="status" aria-label={`${bot.name}：${label}`}><span className="computer-signal" aria-hidden="true"><span className="computer-signal-face"><i/><i/></span></span><span className="computer-hud-label">{label}</span></div>
}
function EmptyComputer({state}:{state:ComputerStatus["state"]}){if(state==="booting"||state==="suspended")return <div className="empty-computer is-waiting" aria-hidden="true"/>;return <div className="empty-computer"><Computer/><strong>{stateLabel(state)}</strong><span>{t("computerPreviewHint")}</span></div>}
function ControlButtons({computer,busy,action,active,sessionId,onBoot,onRestart,onStop}:{computer:ComputerStatus;busy:boolean;action:(w:()=>Promise<unknown>)=>Promise<void>;active:Bot;sessionId?:string|null;onBoot?:()=>Promise<void>;onRestart?:()=>Promise<void>;onStop?:()=>Promise<void>}){
  const [menuOpen,setMenuOpen]=useState(false);
  const menuRef=useRef<HTMLDivElement>(null);
  useEffect(()=>{
    if(!menuOpen)return;
    const outside=(event:PointerEvent)=>{if(!menuRef.current?.contains(event.target as Node))setMenuOpen(false)};
    const escape=(event:KeyboardEvent)=>{if(event.key==="Escape"){event.stopPropagation();setMenuOpen(false);menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus()}};
    document.addEventListener("pointerdown",outside);document.addEventListener("keydown",escape);
    return()=>{document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",escape)};
  },[menuOpen]);
  useEffect(()=>setMenuOpen(false),[active.id,computer.state]);
  const working=Boolean(computer.usingComputer);
  const running=computer.state==="running";
  const booting=computer.state==="booting";
  const run=(operation:"boot"|"restart"|"stop",handler?:()=>Promise<void>)=>{setMenuOpen(false);void action(handler||(()=>api(`/api/computer/${active.id}/${operation}`,{method:"POST",body:"{}"})))};
  return <div className="computer-actions">
    {!running?<button className="primary" disabled={busy||booting} onClick={()=>run("boot",onBoot)}>{booting?t("bootingProgress"):t("openComputer")}</button>
      :computer.controlHolder==="user"?<button className="primary" disabled={busy} onClick={()=>void action(()=>api(`/api/computer/${active.id}/release`,{method:"POST",body:"{}"}))}>{t("releaseControl")}</button>
      :<button className="primary" disabled={busy} onClick={()=>void action(()=>api(`/api/computer/${active.id}/takeover`,{method:"POST",body:"{}"}))}>{working?t("takeOverNow"):t("takeControl")}</button>}
    {running&&working&&<button className="outline" disabled={busy} onClick={()=>void action(()=>api(sessionId?`/api/sessions/${sessionId}/stop`:`/api/bots/${active.id}/stop`,{method:"POST",body:"{}"}))}><Square/>{t("stopTask")}</button>}
    {computer.state!=="stopped"&&<div className="computer-power" ref={menuRef}>
      <button type="button" className="outline computer-power-trigger" disabled={busy||booting} aria-expanded={menuOpen} aria-label={t("computerPower")} title={t("computerPower")} onClick={()=>setMenuOpen(open=>!open)}><Ellipsis/></button>
      {menuOpen&&<div className="computer-power-menu">
        <button type="button" disabled={busy} onClick={()=>run("restart",onRestart)}><RefreshCw/>{t("restartComputer")}</button>
        <button type="button" className="computer-power-stop" disabled={busy} onClick={()=>run("stop",onStop)}><Square/>{t("shutDownComputer")}</button>
        {computer.mode==="team"&&<small>{t("sharedPowerHint")}</small>}
      </div>}
    </div>}
  </div>;
}
function ControlBar(props:{active:Bot;computer:ComputerStatus;busy:boolean;action:(w:()=>Promise<unknown>)=>Promise<void>;paste:()=>void;copy:()=>void;sessionId?:string|null;onBoot?:()=>Promise<void>;onRestart?:()=>Promise<void>;onStop?:()=>Promise<void>}){const interactive=props.computer.controlHolder==="user";return <div className="control-bar"><ControlButtons {...props}/><button className="icon-button" disabled={!interactive} title={t("pasteClipboard")} onClick={props.paste}><ClipboardPaste/></button><button className="icon-button" disabled={!interactive} title={t("copyDesktopClipboard")} onClick={props.copy}><UseAnimations animation={copy} size={18} strokeColor="#dfdfe2"/></button></div>}
function TeachDialog({bot,busy,close,start}:{bot:Bot;busy:boolean;close:()=>void;start:(goal:string)=>void}){
  const [goal,setGoal]=useState("");
  return <div className="modal-backdrop" onClick={close}><form className="dialog teach-dialog" onClick={e=>e.stopPropagation()} onSubmit={e=>{e.preventDefault();if(goal.trim())start(goal.trim())}}>
    <div className="dialog-title"><h2><i className="record-dot"/>{t("teachTask")}</h2><button type="button" onClick={close}><X/></button></div>
    <p>{t("teachDialogLead",{name:bot.name})}</p>
    <label>{t("teachGoalLabel")}<textarea className="teach-goal" autoFocus rows={3} value={goal} onChange={e=>setGoal(e.target.value)} placeholder={t("teachGoalPlaceholder")} onKeyDown={e=>{if(e.key==="Enter"&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();e.currentTarget.form?.requestSubmit()}}}/></label>
    <ul className="teach-tips"><li>{t("teachTip1")}</li><li>{t("teachTip2")}</li><li>{t("teachTip3")}</li></ul>
    <div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!goal.trim()}>{t("startDemo")}</button></div>
  </form></div>;
}
function stepText(step:unknown):string{if(typeof step==="string")return step;if(step&&typeof step==="object"&&"do" in step)return String((step as {do:unknown}).do||"");return ""}
function skillFilename(name:string){const slug=name.trim().replace(/[<>:"/\\|?*\u0000-\u001f]+/g,"").replace(/\s+/g,"-").replace(/\.+$/,"").slice(0,40)||"skill";return `${slug}.json`}
function downloadSkill(name:string,goal:string,playbook:Playbook){const payload={kind:"lazyboy.skill",version:1,name,goal,playbook:{...playbook,name}};const blob=new Blob([JSON.stringify(payload,null,2)],{type:"application/json"});const url=URL.createObjectURL(blob);const a=document.createElement("a");a.href=url;a.download=skillFilename(name);document.body.appendChild(a);a.click();a.remove();URL.revokeObjectURL(url)}
function SkillDraftCard({skill,busy,onSave,onTest,onDiscard,onEdit,onExport}:{skill:TaughtSkill;busy:boolean;onSave:(name:string,playbook:Playbook)=>void;onTest:(name:string,playbook:Playbook)=>void;onDiscard:()=>void;onEdit:()=>void;onExport:(name:string,playbook:Playbook)=>void}){
  const [name,setName]=useState(skill.name||skill.playbook.name||"");
  const [stepsText,setStepsText]=useState((skill.playbook.steps||[]).map(step=>stepText(step)).join("\n"));
  const [expanded,setExpanded]=useState(false);
  const steps=skill.playbook.steps||[];
  const build=():Playbook=>{const lines=stepsText.split("\n").map(line=>line.replace(/^\s*\d+[.、)]\s*/,"").trim()).filter(Boolean);const next=lines.map((line,index)=>{const prev=steps[index];return typeof prev==="object"&&prev&&stepText(prev)===line?prev:{do:line,expect:typeof prev==="object"&&prev&&stepText(prev)===line?prev.expect:"",note:""}});return {...skill.playbook,name:name.trim(),steps:next}};
  const inputs=(skill.playbook.inputs||[]).map(input=>input.name).filter(Boolean);
  return <section className="skill-draft" aria-label={t("skillDraftTitle")}>
    <header><Sparkle/><strong>{t("skillDraftTitle")}</strong><small>{t("skillDraftFrom",{count:skill.eventCount})}</small></header>
    <label className="skill-name">{t("skillName")}<input value={name} maxLength={40} onChange={e=>setName(e.target.value)} placeholder={t("skillNamePlaceholder")}/></label>
    {skill.playbook.intent&&<p className="skill-intent">{skill.playbook.intent}</p>}
    {inputs.length>0&&<p className="skill-inputs">{t("skillInputs")}：{inputs.map(input=><code key={input}>{input}</code>)}</p>}
    <label className="skill-steps">{t("skillSteps")}{expanded?<textarea rows={Math.min(12,Math.max(4,stepsText.split("\n").length+1))} value={stepsText} onChange={e=>setStepsText(e.target.value)}/>:<ol onClick={()=>setExpanded(true)}>{stepsText.split("\n").filter(Boolean).slice(0,6).map((line,index)=><li key={index}>{line}</li>)}{stepsText.split("\n").filter(Boolean).length>6&&<li className="more">{t("moreSteps",{count:stepsText.split("\n").filter(Boolean).length-6})}</li>}</ol>}{!expanded&&<button type="button" className="link" onClick={()=>setExpanded(true)}>{t("editSteps")}</button>}</label>
    {skill.playbook.howToCheck&&<p className="skill-check">{t("skillCheck")}：{skill.playbook.howToCheck}</p>}
    {skill.error&&<p className="skill-error">{t("skillDistillFailed")}</p>}
    <div className="skill-actions"><button type="button" className="outline danger-ghost" disabled={busy} onClick={onDiscard}>{t("discard")}</button><button type="button" className="link" disabled={busy} onClick={onEdit}><Pencil/>{t("editSkillFull")}</button><button type="button" className="link" disabled={busy||!name.trim()} onClick={()=>onExport(name.trim(),build())}><Download/>{t("exportSkill")}</button><span className="grow"/><button type="button" className="outline" disabled={busy||!name.trim()} onClick={()=>onTest(name.trim(),build())}>{t("testRun")}</button><button type="button" className="primary" disabled={busy||!name.trim()} onClick={()=>onSave(name.trim(),build())}>{t("saveSkill")}</button></div>
  </section>;
}
const lines=(text:string)=>text.split("\n").map(line=>line.replace(/^\s*(?:\d+[.、)]|[-•*])\s*/,"").trim()).filter(Boolean);
function stepsToText(steps:(PlaybookStep|string)[]|undefined){return (steps||[]).map(step=>{if(typeof step==="string")return step;return step.expect?`${step.do} → ${step.expect}`:step.do}).join("\n")}
function textToSteps(text:string,previous:(PlaybookStep|string)[]|undefined):PlaybookStep[]{const old=(previous||[]).filter((step):step is PlaybookStep=>typeof step==="object"&&step!==null);return lines(text).map(line=>{const [doPart,...rest]=line.split(/\s*(?:→|->|=>)\s*/);const expect=rest.join(" → ").trim();const match=old.find(step=>step.do===doPart.trim());return {do:doPart.trim(),expect:expect||(match&&!rest.length?match.expect:undefined)||"",note:match?.note||""}})}
function inputsToText(inputs:PlaybookInput[]|undefined){return (inputs||[]).map(input=>`${input.name}${input.description?`：${input.description}`:""}${input.example?`｜例：${input.example}`:""}`).join("\n")}
function textToInputs(text:string):PlaybookInput[]{return lines(text).map(line=>{const [head,...exampleParts]=line.split(/\s*[|｜]\s*/);const example=exampleParts.join("｜").replace(/^例[:：]\s*/,"").trim();const [name,...descParts]=head.split(/[:：]/);return {name:name.trim(),description:descParts.join("：").trim()||undefined,example:example||undefined}}).filter(input=>input.name)}
function SkillEditDialog({skill,busy,close,save,test,remove,exportFile}:{skill:TaughtSkill;busy:boolean;close:()=>void;save:(name:string,playbook:Playbook)=>void;test:(name:string,playbook:Playbook)=>void;remove:()=>void;exportFile:(name:string,playbook:Playbook)=>void}){
  const p=skill.playbook;
  const [name,setName]=useState(skill.name||p.name||"");
  const [intent,setIntent]=useState(p.intent||"");
  const [whenToUse,setWhenToUse]=useState(p.whenToUse||"");
  const [inputs,setInputs]=useState(inputsToText(p.inputs));
  const [preconditions,setPreconditions]=useState((p.preconditions||[]).join("\n"));
  const [steps,setSteps]=useState(stepsToText(p.steps));
  const [howToCheck,setHowToCheck]=useState(p.howToCheck||"");
  const [whatToReturn,setWhatToReturn]=useState(p.whatToReturn||"");
  const [cautions,setCautions]=useState((p.cautions||[]).join("\n"));
  const build=():Playbook=>({...p,name:name.trim(),intent:intent.trim(),whenToUse:whenToUse.trim(),inputs:textToInputs(inputs),preconditions:lines(preconditions),steps:textToSteps(steps,p.steps),howToCheck:howToCheck.trim(),whatToReturn:whatToReturn.trim(),cautions:lines(cautions)});
  const valid=name.trim().length>0&&lines(steps).length>0;
  const rows=(text:string,min:number,max:number)=>Math.min(max,Math.max(min,text.split("\n").length+1));
  return <div className="modal-backdrop" onClick={close}><form className="dialog skill-edit" onClick={e=>e.stopPropagation()} onSubmit={e=>{e.preventDefault();if(valid)save(name.trim(),build())}}>
    <div className="dialog-title"><h2><Sparkle/>{t("editSkill")}</h2><button type="button" onClick={close}><X/></button></div>
    <p className="dialog-lead">{t("skillEditLead")}</p>
    <label>{t("skillName")}<input autoFocus value={name} maxLength={40} onChange={e=>setName(e.target.value)} placeholder={t("skillNamePlaceholder")}/></label>
    <label>{t("skillIntent")}<textarea rows={rows(intent,2,5)} value={intent} onChange={e=>setIntent(e.target.value)}/></label>
    <label>{t("skillWhenToUse")}<input value={whenToUse} onChange={e=>setWhenToUse(e.target.value)}/></label>
    <label>{t("skillInputsLabel")}<textarea rows={rows(inputs,2,5)} value={inputs} onChange={e=>setInputs(e.target.value)} placeholder={t("skillInputsPlaceholder")}/></label>
    <label>{t("skillPreconditions")}<textarea rows={rows(preconditions,2,5)} value={preconditions} onChange={e=>setPreconditions(e.target.value)}/></label>
    <label>{t("skillStepsLabel")}<textarea rows={rows(steps,5,14)} value={steps} onChange={e=>setSteps(e.target.value)} placeholder={t("skillStepsPlaceholder")}/></label>
    <label>{t("skillHowToCheck")}<textarea rows={rows(howToCheck,2,4)} value={howToCheck} onChange={e=>setHowToCheck(e.target.value)}/></label>
    <label>{t("skillWhatToReturn")}<input value={whatToReturn} onChange={e=>setWhatToReturn(e.target.value)}/></label>
    <label>{t("skillCautions")}<textarea rows={rows(cautions,2,5)} value={cautions} onChange={e=>setCautions(e.target.value)}/></label>
    <div className="dialog-actions"><button type="button" className="outline danger-ghost" disabled={busy} onClick={remove}>{t("deleteSkill")}</button><span className="grow"/><button type="button" className="outline" disabled={busy} onClick={close}>{t("cancel")}</button><button type="button" className="outline" disabled={!valid} onClick={()=>exportFile(name.trim(),build())}><Download/>{t("exportSkill")}</button><button type="button" className="outline" disabled={busy||!valid} onClick={()=>test(name.trim(),build())}>{t("testRun")}</button><button className="primary" disabled={busy||!valid}>{t("saveSkill")}</button></div>
  </form></div>;
}
function ClipboardDialog({close,paste}:{close:()=>void;paste:(text:string)=>void}){const[text,setText]=useState("");return <div className="modal-backdrop"><div className="dialog compact"><div className="dialog-title"><h2>{t("pasteToRemoteComputer")}</h2><button onClick={close}><X/></button></div><p>{t("pasteRemoteHelp")}</p><textarea className="clipboard-text" autoFocus value={text} onChange={e=>setText(e.target.value)} placeholder={t("pasteTextPlaceholder")}/><div className="dialog-actions"><button className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={!text} onClick={()=>paste(text)}>{t("pasteIntoVnc")}</button></div></div></div>}
function CreateDialog({close,created}:{close:()=>void;created:(bot:Bot)=>void}){const[name,setName]=useState("");const[mode,setMode]=useState<ComputerMode>("team");const[busy,setBusy]=useState(false);return <div className="modal-backdrop"><form className="dialog" onSubmit={async e=>{e.preventDefault();if(!name.trim())return;setBusy(true);try{created(await api<Bot>("/api/bots",{method:"POST",body:JSON.stringify({name:name.trim(),computerMode:mode})}))}finally{setBusy(false)}}}><div className="dialog-title"><h2>{t("addBot")}</h2><button type="button" onClick={close}><X/></button></div><label>{t("name")}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder={t("botNamePlaceholder")}/></label><div className="mode-grid"><button type="button" className={mode==="team"?"picked":""} onClick={()=>setMode("team")}><BotIcon/><strong>{t("sharedComputer")}</strong><small>{t("sharedComputerHint")}</small></button><button type="button" className={mode==="dedicated"?"picked":""} onClick={()=>setMode("dedicated")}><Computer/><strong>{t("privateComputer")}</strong><small>{t("privateComputerHint")}</small></button></div><div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!name.trim()}>{t("create")}</button></div></form></div>}
function CreateGroupDialog({bots,close,created}:{bots:Bot[];close:()=>void;created:(room:Room)=>void}){const[name,setName]=useState("");const[selected,setSelected]=useState<string[]>([]);const[busy,setBusy]=useState(false);const visible=bots.filter(bot=>!bot.hidden);return <div className="modal-backdrop"><form className="dialog" onSubmit={async e=>{e.preventDefault();const groupName=name.trim();if(!groupName||selected.length<2)return;setBusy(true);try{created(await api<Room>("/api/rooms",{method:"POST",body:JSON.stringify({name:groupName,memberIds:selected})}))}finally{setBusy(false)}}}><div className="dialog-title"><h2>{t("addGroup")}</h2><button type="button" onClick={close}><X/></button></div><p className="dialog-lead">{t("groupDescription")}</p><label>{t("groupName")}<input autoFocus value={name} maxLength={30} onChange={e=>setName(e.target.value)} placeholder={t("groupNamePlaceholder")}/></label><fieldset className="group-picker"><legend>{t("chooseBots")}</legend>{visible.map(bot=><label key={bot.id}><input type="checkbox" checked={selected.includes(bot.id)} onChange={()=>setSelected(ids=>ids.includes(bot.id)?ids.filter(id=>id!==bot.id):[...ids,bot.id])}/><Avatar lookId={bot.id} name={bot.name} color={bot.avatarColor} shape={bot.avatarShape} online/><span>{bot.name}</span></label>)}</fieldset><div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!name.trim()||selected.length<2}>{busy?t("creating"):t("createGroup")}</button></div></form></div>}
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
function ModelSettingsDialog({close}:{close:()=>void}){
  const[settings,setSettings]=useState<WorkspaceSettings|null>(null);
  const[provider,setProvider]=useState<ModelProviderId>("xai");
  const[modelId,setModelId]=useState("");
  const[baseUrl,setBaseUrl]=useState("");
  const[apiKey,setApiKey]=useState("");
  const[clearKey,setClearKey]=useState(false);
  const[models,setModels]=useState<{id:string;name:string}[]>([]);
  const[busy,setBusy]=useState(false);
  const[error,setError]=useState("");
  const current=settings?.providers.find(item=>item.id===provider);
  useEffect(()=>{api<WorkspaceSettings>("/api/workspace/settings").then(value=>{setSettings(value);setProvider(value.provider);setModelId(value.modelId);setBaseUrl(value.baseUrl);setModels(value.models)}).catch(e=>setError(e instanceof Error?e.message:t("loadFailed")))},[]);
  async function loadModels(nextProvider:ModelProviderId,nextBaseUrl:string){
    if(nextProvider==="openai-compatible"&&!nextBaseUrl.trim()){setModels([]);return}
    try{
      const query=new URLSearchParams({provider:nextProvider});
      if(nextBaseUrl.trim())query.set("baseUrl",nextBaseUrl.trim());
      const result=await api<{models:{id:string;name:string}[]}>(`/api/workspace/models?${query}`);
      setModels(result.models);
      setModelId(current=>result.models.some(item=>item.id===current)?current:result.models[0]?.id||current);
    }catch{if(nextProvider==="openai-compatible")setModels([])}
  }
  function pickProvider(id:ModelProviderId){
    setProvider(id);
    const info=settings?.providers.find(item=>item.id===id);
    if(id==="openai-compatible"){
      setModels([]);
      setBaseUrl(current=>current.includes("opencode.ai")||current.includes("api.x.ai")?"":current);
      if(info?.defaultModel)setModelId(info.defaultModel);else setModelId("");
      return;
    }
    if(info?.defaultBaseUrl)setBaseUrl(info.defaultBaseUrl);
    if(info?.defaultModel)setModelId(info.defaultModel);
    void loadModels(id,info?.defaultBaseUrl||"");
  }
  return <div className="modal-backdrop"><form className="dialog settings-dialog" onSubmit={async e=>{e.preventDefault();setBusy(true);setError("");try{await api("/api/workspace/settings",{method:"PATCH",body:JSON.stringify({provider,modelId:modelId.trim(),baseUrl:baseUrl.trim()||null,apiKey:clearKey?"":apiKey.trim()||null,clearApiKey:clearKey})});close()}catch(e){setError(e instanceof Error?e.message:t("settingsFailed"))}finally{setBusy(false)}}}>
    <div className="dialog-title"><h2>{t("modelSettings")}</h2><button type="button" onClick={close}><X/></button></div>
    <p className="dialog-lead">{t("modelSettingsHint")}</p>
    <fieldset className="provider-fieldset"><legend>{t("modelProvider")}</legend>
      <div className="provider-grid">{(settings?.providers||[{id:"xai" as const,name:t("providerXai")},{id:"opencode-go" as const,name:t("providerOpencodeGo")},{id:"openai-compatible" as const,name:t("providerOpenaiCompatible")}]).map(item=><button type="button" key={item.id} className={provider===item.id?"picked":""} onClick={()=>pickProvider(item.id)}>{item.id==="xai"?t("providerXai"):item.id==="opencode-go"?t("providerOpencodeGo"):t("providerOpenaiCompatible")}</button>)}</div>
      <p className="dialog-lead">{provider==="xai"?t("providerXaiHint"):provider==="opencode-go"?t("providerOpencodeGoHint"):t("providerOpenaiCompatibleHint")}</p>
    </fieldset>
    <label>{t("apiKey")}<input type="password" autoComplete="off" value={apiKey} onChange={e=>{setApiKey(e.target.value);setClearKey(false)}} placeholder={settings?.apiKeySet?t("apiKeyStored"):t("apiKeyPlaceholder")}/></label>
    {settings?.envKeySet&&settings.provider===provider&&!apiKey&&!clearKey&&<p className="dialog-lead">{t("usingEnvKey",{name:settings.envKeyName})}</p>}
    {settings?.apiKeySet&&<label className="memory-toggle"><input type="checkbox" checked={clearKey} onChange={e=>setClearKey(e.target.checked)}/> {t("clearApiKey")}</label>}
    {(current?.needsBaseUrl||provider==="openai-compatible")&&<label>{t("baseUrl")}<input value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder={t("baseUrlPlaceholder")}/><small>{t("baseUrlHint")}</small></label>}
    <label>{t("modelId")}
      {models.length>0?<select value={models.some(item=>item.id===modelId)?modelId:""} onChange={e=>setModelId(e.target.value)}>
        {models.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}
        {!models.some(item=>item.id===modelId)&&modelId?<option value={modelId}>{modelId}</option>:null}
      </select>:<input value={modelId} onChange={e=>setModelId(e.target.value)} placeholder={t("modelIdPlaceholder")}/>}
      {models.length>0&&<input value={modelId} onChange={e=>setModelId(e.target.value)} placeholder={t("modelIdPlaceholder")}/>}
    </label>
    <button type="button" className="outline" onClick={()=>void loadModels(provider,baseUrl)}>{t("reloadModels")}</button>
    {error&&<div className="pane-error">{error}</div>}
    <div className="dialog-actions"><button type="button" className="outline" onClick={close}>{t("cancel")}</button><button className="primary" disabled={busy||!modelId.trim()||(provider==="openai-compatible"&&!baseUrl.trim())}>{busy?t("saving"):t("save")}</button></div>
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
      <section><h3>{t("helpSkillsTitle")}</h3><p>{t("helpSkills")}</p></section>
      <section><h3>{t("helpAttachTitle")}</h3><p>{t("helpAttach")}</p></section>
      <section><h3>{t("helpVoiceTitle")}</h3><p>{t("helpVoice")}</p></section>
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
