export type ComputerMode = "team" | "dedicated";
export type ComputerState = "stopped" | "booting" | "running" | "suspended" | "error";
export type AvatarShape = "round"|"blob"|"squircle"|"capsule"|"triangle"|"hexagon"|"cloud"|"drop"|"diamond"|"organic-4"|"organic-5"|"organic-6"|"organic-7"|"organic-8"|"organic-9"|"organic-10"|"organic-11";
export interface Bot { id:string; spaceId:string; name:string; title:string; description:string; avatarColor:string; avatarShape:AvatarShape; tags:string[]; pinned:boolean; hidden:boolean; groupName:string|null; unreadCount:number; lastMessageAt:string|null; instructions:string; threadId:string; computerId:string; computerMode:ComputerMode; memoryEnabled:boolean }
export interface Session { id:string; botId:string; title:string; status:"active"|"archived"; createdAt:string; updatedAt:string; nextMessageSeq:number; historySummary:string; historySummarySeq:number }
export interface Message { id:string; sessionId?:string; seq?:number; role:string; body:string; blocks?:unknown[]; runId?:string|null; clientNonce?:string|null; createdAt:string }
export interface ComputerStatus { botId:string; mode:ComputerMode; state:ComputerState; controlHolder:"none"|"bot"|"user"; takeoverRequested:boolean; busyBotName:string|null; display:string|null; profileMode:string; screenAvailable:boolean }
export interface MemoryItem { id:string; sessionId:string|null; sourceRunId:string|null; sourceMessageId:string|null; content:string; importance:number; revision:number; createdAt:string; updatedAt:string }
