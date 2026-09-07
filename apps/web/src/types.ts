export type ComputerMode = "team" | "dedicated";
export type ComputerState = "stopped" | "booting" | "running" | "suspended" | "error";
export type BlobatarShape = "round"|"organic"|"boxy"|"capsule"|"nub"|"cloud"|"droplet"|"hexagon"|"sun"|"triangle";
/** Stored values include pre-blobatar aliases so existing bots keep rendering. */
export type AvatarShape = BlobatarShape|"blob"|"squircle"|"diamond"|"drop"|"organic-4"|"organic-5"|"organic-6"|"organic-7"|"organic-8"|"organic-9"|"organic-10"|"organic-11"|"cat"|"bunny"|"star"|"heart"|"egg"|"ghost"|"sprout"|"cactus"|"mushroom"|"paw";
export interface Bot { id:string; spaceId:string; name:string; title:string; description:string; avatarColor:string; avatarShape:AvatarShape; tags:string[]; pinned:boolean; hidden:boolean; groupName:string|null; unreadCount:number; lastMessageAt:string|null; instructions:string; threadId:string; computerId:string; computerMode:ComputerMode; memoryEnabled:boolean }
export interface Session { id:string; botId:string; title:string; status:"active"|"archived"; createdAt:string; updatedAt:string; nextMessageSeq:number; historySummary:string; historySummarySeq:number }
export interface Message { id:string; sessionId?:string; seq?:number; role:string; body:string; blocks?:unknown[]; runId?:string|null; clientNonce?:string|null; createdAt:string; speakerBotId?:string|null; speakerName?:string|null; speakerColor?:string|null; speakerShape?:AvatarShape|null }
export interface MessageFile { kind:"image"|"file"; name:string; mimeType?:string; size?:number }
export interface RoomMember { id:string; name:string; avatarColor:string; avatarShape:AvatarShape }
export interface Room { id:string; name:string; members:RoomMember[]; lastMessageAt:string|null; lastPreview:string|null; unreadCount:number }
export interface ComputerStatus { botId:string; mode:ComputerMode; state:ComputerState; controlHolder:"none"|"bot"|"user"; takeoverRequested:boolean; busyBotName:string|null; busySessionId:string|null; busyRunId:string|null; busyStep?:string|null; usingComputer?:boolean; waitingRunId?:string|null; waitingSessionId?:string|null; queuedRuns?:number; display:string|null; profileMode:string; screenAvailable:boolean }
/** One line of the live trail a run writes while it works. */
export type RunActivityKind = "run"|"model"|"tool"|"retry"|"notice";
export interface RunActivityEntry { id:number; kind:RunActivityKind; createdAt:string; turn?:number|null; event?:string|null; task?:string|null; reason?:string|null; turns?:number|null; limit?:number|null; error?:string|null; name?:string|null; step?:string|null; status?:string|null; elapsedMs?:number|null; toolCalls?:number|null; text?:string|null; snippet?:string|null; attempt?:number|null; gaveUp?:boolean|null }
export interface RunActivityError { code:string; headline:string; action?:string; raw:string }
export interface RunActivity { runId:string; status:string; turn:number|null; turnLimit:number|null; step:string|null; stepAt?:string|null; elapsedMs:number|null; error:RunActivityError|null; activity:RunActivityEntry[] }

export interface PlaybookStep { do:string; expect?:string; note?:string }
export interface PlaybookInput { name:string; description?:string; example?:string }
export interface Playbook { name?:string; whenToUse?:string; intent?:string; inputs?:PlaybookInput[]; preconditions?:string[]; steps?:(PlaybookStep|string)[]; howToCheck?:string; whatToReturn?:string; cautions?:string[] }
export type TaughtSkillStatus = "recording"|"drafting"|"draft"|"saved"|"failed"|"cancelled";
export interface TaughtSkill { id:string; botId:string; threadId:string|null; name:string; goal:string; status:TaughtSkillStatus; playbook:Playbook; error:string|null; startedAt:string|null; expiresAt:string|null; stoppedAt:string|null; createdAt:string; updatedAt:string; eventCount:number; frameCount:number }
export interface FileSkill { name:string; description:string }
export interface MemoryItem { id:string; sessionId:string|null; sourceRunId:string|null; sourceMessageId:string|null; content:string; importance:number; revision:number; createdAt:string; updatedAt:string }
export type McpTransport = "stdio" | "http" | "sse";
export interface McpTool { name:string; exposedName:string; description:string }
export interface McpServer { id:string; name:string; transport:McpTransport; command:string|null; args:string[]; env:Record<string,string>; url:string|null; headers:Record<string,string>; enabled:boolean; status:"connected"|"disconnected"|"disabled"; error:string|null; tools:McpTool[]; createdAt:string; updatedAt:string }
export interface McpSecretField { name:string; required:boolean; secret:boolean; hint:string }
export interface McpCatalogEntry { id:string; title:string; description:string; transport:McpTransport; command:string|null; args:string[]; url:string|null; envKeys:McpSecretField[]; headerKeys:McpSecretField[]; source:"featured"|"registry"; remote:boolean }
export type ModelProviderId = "xai" | "opencode-go" | "openai-compatible";
export type VoiceProviderId = "xai" | "openai" | "scripted";
export interface VoiceSettings {
  enabled: boolean;
  provider: VoiceProviderId;
  modelId: string;
  voiceId: string;
  ready: boolean;
  missing?: string | null;
  apiKeySet: boolean;
  envKeySet: boolean;
  envKeyName: string;
  reusesTextKey: boolean;
  providers: { id: VoiceProviderId; name: string; envKeyName: string }[];
  models: { id: string; name: string }[];
  voices: { id: string; name: string }[];
}
export interface WorkspaceProvider { id:ModelProviderId; name:string; needsBaseUrl:boolean; needsKey:boolean; defaultBaseUrl:string|null; defaultModel:string|null }
export interface WorkspaceModel { id:string; name:string }
export interface WorkspaceSettings { provider:ModelProviderId; modelId:string; baseUrl:string; apiKeySet:boolean; envKeySet:boolean; envKeyName:string; providers:WorkspaceProvider[]; models:WorkspaceModel[] }
