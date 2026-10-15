import { withRequestTimeout } from "./request-timeout";
import { randomUUID } from "./uuid";
export type SecretField = {
  name: string;
  label?: string;
  kind?: "text" | "password" | "otp";
  placeholder?: string;
};

export type HumanQuestion = {
  method?: string;
  hermes_questions?: {qid: string; question: string; choices?: string[] | null; multi_select: boolean}[];
  question?: string; reason?: string; prompt?: string; action_label?: string; options?: unknown[]; kind?: string;
  /** Questions from work the user did not start expire (ISO time). */
  expires_at?: string;
  task_id?: string; question_id?: string; site_url?: string; surface?: string;
  handover?: boolean; submitted?: boolean;
  name?: string; label?: string; description?: string;
  fields?: SecretField[];
  // L2a: CAPTCHA fields (78b8124)
  image_url?: string;
  image_token?: string;
  // L2a: OTP fields
  digits?: number;
  purpose?: string;
  purge_on_success?: boolean;
  // L2a: Fill-back targeting (488cfe7)
  selector?: string;
  role?: string;
  placeholder?: string;
};

export type AgentActivity = {
  state: "idle" | "queued" | "running" | "waiting_input" | "waiting_approval" | "stopping";
  running: boolean;
  queued: boolean;
  observed_at_ms?: number;
  active_task_ids: string[];
  latest_task_id?: string | null;
  queued_task_ids?: string[];
  thread_id?: string | null;
  /** Set while the bot is speaking in a group chat. */
  room_id?: string | null;
  queued_thread_ids?: string[];
  question?: HumanQuestion | null;
  questions?: HumanQuestion[];
  /** delegate_task children still working, often after the turn that started them ended. */
  background?: number;
  background_task_ids?: string[];
  /** The lesson being demonstrated on the bot's computer; queued work waits for it. */
  teaching?: string | null;
};

/** One thing the human did while teaching, described by what was acted on rather than where. */
export type LessonStep = {
  id: string;
  at: number;
  source: "browser" | "desktop" | "user";
  kind: string;
  text: string;
  shot?: string;
  url?: string;
  window?: string;
  target?: { role?: string; name?: string; box?: number[] | null; viewport?: number[] } | null;
  point?: number[];
  screen?: number[];
  result?: { url?: string; title?: string; shot?: string };
};
export type Lesson = {
  id: string;
  agent_id: string;
  goal: string;
  trial: boolean;
  state: "recording" | "paused" | "finished" | "cancelled";
  started_at?: string;
  ended_at?: string;
  task_id?: string;
  thread_id?: string | null;
  steps: LessonStep[];
  /** Prefix for a step's `shot` to load its screenshot. */
  base_url: string;
};

export type Artifact = {
  id: string;
  kind: string;
  title: string;
  rel_path: string;
  media_type: string;
  bytes: number;
  status: string;
  ok_on_disk: boolean;
  created_at: string;
  updated_at?: string;
};

export type PlanItem = { content: string; status: "pending" | "in_progress" | "completed" | "cancelled" | string };
export type Worker = {
  id: string;
  goal: string;
  status: "queued" | "running" | "completed" | "failed" | "interrupted" | "lost";
  background?: boolean;
  started_at?: string;
  at?: string;
  tool?: string;
  detail?: string;
  duration?: number;
  summary?: string;
};

export type TaskDetail = {
  collaboration?: {message_id:number;sender_name:string;recipient_name:string;content:string;at:string}[];
  /** Set when another bot (or a group chat) started this turn. */
  from_agent?: string; from_name?: string; intent?: string; room_id?: string;
  id: string;
  state: string;
  goal: string;
  verdict?: string | null;
  created_at?: string;
  started_at?: string;
  ended_at?: string;
  provider?: string;
  model?: string;
  artifacts: Artifact[];
  steps?: { id: string; name: string; detail?: string; state: "running" | "done" | "failed"; duration?: number }[];
  plan?: PlanItem[];
  sources?: { url: string; title: string; kind: "read" | "search" }[];
  /** "background": a turn Hermes started itself to report finished background work. */
  kind?: string;
  background?: Worker[];
  milestones?: { id: string; at: string; message: string; completed: string[]; next: string; evidence: string[] }[];
  approval?: {
    decision: string;
    note?: string;
    decided_at: string;
    decided_by: string;
  } | null;
};

export type AgentGroup = { id: string; name: string; position?: number };
export type PendingApproval = { id: string; agent_id: string; agent_name: string; subject: string; created_at: string };
export type Approval = PendingApproval & { details: string; category: string; state: "pending" | "approved" | "rejected" | "cancelled"; note: string; decided_at: string | null; from_name?: string | null };
export type TeamPerson = { name: string; title?: string; persona?: string };
export type TeamTemplate = { key: string; name: string; about: string; lead: TeamPerson; members: TeamPerson[] };
/** One bot as its manager sees it (server/org.py status). */
export type OrgBot = {
  id: string; name: string; title: string; reports_to: string; reports_to_id: string; team: string; can_manage: boolean; primary: boolean;
  state: "working" | "waiting" | "queued" | "idle"; current: { task: string; from: string; since?: string } | null; waiting_for: string | null; queued: number;
  approvals: { subject: string; since: string }[]; recent: { task: string; from: string; verdict: string | null; at: string; result: string }[];
};
export type OrgChart = { bots: OrgBot[]; approval_items: string[]; templates: TeamTemplate[] };
export type AgentSkill = { id: string; name: string; description: string; instructions: string; enabled: boolean; updated_at: string; source?: string; place?: "list" | "library" | "off" };

export type AgentRow = {
  /** Grok's Primary Bot: the one the others hand coordination to; marked with a star. */
  primary?: boolean;
  /** Kept at the top of the sidebar. */
  pinned?: boolean;
  /** Folded into "已隱藏" at the bottom of the sidebar. */
  hidden?: boolean;
  /** Set when another bot created this one (a helper bot). */
  created_by?: string;
  /** The bot this one reports to; empty when it reports to the user. */
  reports_to?: string;
  /** May build teams and change other bots for the user (create_team, update_bot). */
  can_manage?: boolean;
  group_id?: string;
  avatar_traits?: Record<string, number>;
  avatar_expression?: string;
  persona?: string;
  reply_style?: string;
  /** Actions with outside effects the bot must clear with the user first (send, publish, pay, delete). */
  ask_first?: string[];
  skill_count?: number;
  id: string;
  name: string;
  expertise?: string;
  title?: string;
  description?: string;
  tags?: string[];
  avatar_color?: string;
  avatar_shape?: string;
  has_avatar?: boolean;
  avatar_url?: string | null;
  provider?: string;
  model?: string;
  preview?: string;
  last_at?: string | null;
  running?: boolean;
  activity?: AgentActivity;
};

export type AgentProfile = {
  reports_to?: string;
  can_manage?: boolean;
  ask_first?: string[];
  group_id?: string;
  avatar_traits?: Record<string, number>;
  avatar_expression?: string;
  persona?: string;
  reply_style?: string;
  name: string;
  title: string;
  description: string;
  tags: string[];
  avatar_color: string;
  avatar_shape: string;
  provider?: string;
  model?: string;
};

/** Who a message came from when it was not the user: another bot ("message_in"), or this bot's hand-off ("message_out"). */
export type PeerFields = { room_id?: string; event?: "message_in" | "message_out" | "group_created" | "group_post" | "bot_created" | "profile_updated" | "question_expired" | "approval_request" | "team_created" | "auto_continue"; from_agent?: string; from_name?: string; peer_id?: string; peer_name?: string; intent?: "request" | "question" | "result" | "fyi"; message_id?: string; silent?: boolean };
export type TranscriptItem = PeerFields & { id?: string; role: "user" | "assistant" | "event"; content: string; at?: string; streaming?: boolean; reactions?: string[];
  /** The last journal event a stored, still-streaming reply already contains. */
  stream_event?: number; narration?: string[]; routine_id?: string; routine_title?: string; lesson_id?: string; file?: { id?: string; task_id?: string; title: string; path?: string; url: string; media_type?: string; bytes: number; available: boolean; status?: string }; action?: { question_id: string; task_id?: string; kind?: string; state: string } };

export type ModelProtocol = "chat_completions" | "responses" | "messages";
export type ModelCheck = { status: "ok" | "protocol" | "unavailable" | "auth" | "busy" | "unknown"; message: string; detail?: string; at?: number; v?: number; protocol?: ModelProtocol | null };
export type ImageModel = { provider: string; model: string; label: string };
export type SettingsModel = { id: string; label?: string; protocol?: ModelProtocol; reasoning_effort?: string | null; reasoning_levels?: string[] | null; vision?: boolean; image_generation?: boolean; check?: ModelCheck };
export type SettingsProvider = {
  id: string;
  label: string;
  base_url: string;
  has_key: boolean;
  selectable?: boolean;
  models: SettingsModel[];
};
export type ProviderPreset = {
  id: string;
  label: string;
  description: string;
  base_url: string;
  protocol: ModelProtocol;
  signup_url: string;
  models: string[];
  model_info?: Record<string, { protocol: ModelProtocol; vision: boolean; efforts: string[] | null }>;
  local: boolean;
};
export type RoutingTarget = { provider: string; model: string; reasoning_effort?: string | null };
export type RoutingSettings = {
  enabled: boolean; tool_groups: "0" | "1" | "auto";
  chat: RoutingTarget | null; work: RoutingTarget | null; fallbacks: RoutingTarget[]; chat_timeout_ms: number; work_timeout_ms: number;
  semantic: boolean; laya: boolean; classifier_shadow: boolean;
  classifier_url: string; classifier_timeout_ms: number; min_confidence: number;
  gateway_provider: string | null; gateway_chat_pool: RoutingTarget[]; gateway_work_pool: RoutingTarget[];
};
export type AppSettings = {
  engine?: string;
  hermes_commit?: string;
  active: string;
  model: string;
  protocol: ModelProtocol;
  base_url: string;
  has_key: boolean;
  config_path: string;
  providers: SettingsProvider[];
  laya?: boolean;
  routing?: RoutingSettings;
  search?: { provider: string; providers: string[]; has_key: boolean; has_xai_key: boolean };
};

export type ThreadRow = {
  id: string;
  title: string;
  main: boolean;
  created_at?: string | null;
  last_at?: string | null;
  preview: string;
  message_count: number;
  running: boolean;
  queued: boolean;
};

export type AgentDetail = AgentRow & {
  expertise: string;
  preview: string;
  thread?: ThreadRow;
  transcript: TranscriptItem[];
  running: boolean;
  activity: AgentActivity;
};

/** `important` is false for build output, caches and lock files; they stay behind "顯示全部". */
/** Bytes a bot uses on disk, by kind. */
type TokenCounts = { requests: number; input: number; output: number; cached: number; reasoning: number; cost?: number };
export type ProviderUsage = TokenCounts & {
  provider: string; label: string; configured: boolean; errors: number;
  models: (TokenCounts & { model: string })[];
  days: { day: string; tokens: number }[];
};
export type ProviderUsageSummary = { days: number; since: string; providers: ProviderUsage[] };

export type StorageUsage = { workspace: number; uploads: number; caches: number; browser: number; tools: number; other: number; total: number };

export type RoomMember = Pick<AgentRow, "id" | "name" | "title" | "avatar_color" | "avatar_shape" | "avatar_url" | "avatar_traits" | "avatar_expression"> & {
  activity?: AgentActivity; seat: "idle" | "queued" | "working" | "waiting";
};
export type RoomRow = { id: string; name: string; member_ids: string[]; auto_name?: boolean; created_at?: string; updated_at?: string;
  members: RoomMember[]; preview: string; last_at?: string | null; working: string[]; message_count: number };
export type RoomMessage = { id: string; role: "user" | "assistant"; content: string; at?: string; reactions?: string[]; speaker_id?: string; speaker_name?: string; mentions?: string[]; failed?: boolean; pending?: boolean };
export type RoomDetail = RoomRow & { transcript: RoomMessage[] };
export type BotMessage = { id: string; from: string; from_name: string; to: string; to_name: string; intent: string; text: string; at: string };

export type SearchMessage = { kind: "bot" | "room"; owner_id: string; owner_name: string; thread_id: string; thread_title: string; message_id: string; role: string; speaker: string; snippet: string; at?: string };
export type SearchResult = {
  query: string;
  bots: { id: string; name: string; title: string; hidden: boolean }[];
  rooms: { id: string; name: string; members: string[] }[];
  messages: SearchMessage[];
  files: { agent_id: string; agent_name: string; path: string; title: string; media_type?: string; updated_at?: string }[];
  routines: { id: string; agent_id: string; agent_name: string; title: string; enabled: boolean; next_run_at?: string | null }[];
};

export type ContextUsage = { available: boolean; reason?: string; used?: number; max?: number; percent?: number; estimated?: boolean; model?: string;
  parts?: { id: string; label: string; tokens: number }[] };

export type MemoryList = { entries: string[]; used: number; limit: number };
export type MemoryState = { user: MemoryList; memory: MemoryList };

export type AgentFile = { path: string; title: string; media_type: string; bytes: number; updated_at: string; important?: boolean };

export type TeamEvent = {
  id: number;
  kind: string;
  task_id?: string | null;
  payload: Record<string, unknown>;
};

async function request<T>(path: string, init?: RequestInit, timeout?: number): Promise<T> {
  const timeoutMs = timeout ?? (!init?.method || init.method === "GET" ? 15000 : 30000);
  return withRequestTimeout(init?.signal, timeoutMs, async (signal) => {
    const res = await fetch(path, {
      ...init,
      signal,
      headers: { "content-type": "application/json", ...(init?.headers || {}) },
    });
    const raw = await res.text();
    let data: unknown = null;
    try {
      data = raw ? JSON.parse(raw) : null;
    } catch {
      data = raw;
    }
    if (!res.ok) {
      const err = data as { error?: string } | null;
      throw new Error(err?.error || raw || String(res.status));
    }
    return data as T;
  });
}

export type Upload = { id: string; name: string; media_type: string; bytes: number; url: string };
export type RoutineSchedule = { frequency: "once" | "daily" | "weekdays" | "weekly" | "monthly" | "interval" | "trigger"; timezone: string; time: string; date?: string | null; weekdays?: number[]; day_of_month?: number | null;
  /** interval: repeats every this many minutes, counted from `time`. */
  every_minutes?: number | null;
  /** Each run starts at a random moment up to this many minutes after the set time, so it does not look automated. */
  jitter_minutes?: number | null };
export type Routine = { id: string; agent_id: string; title: string; instructions: string; schedule: RoutineSchedule; enabled: boolean;
  /** The routine's own web address (path), when it can be triggered by other services. */
  hook_path?: string | null; next_run_at: string | null; last_run_at: string | null; last_task_id: string | null; last_error: string | null; retry_at?: string | null; last_run?: { state: string; verdict?: string | null; at: string } | null };
export type RoutineRun = { started_at: string; task_id: string; state: string; verdict?: string; result?: {summary?: string} | null };
export type MediaCapabilities = { images: boolean; pdf: boolean; text: boolean };
export type UploadLimits = { files: number; file_bytes: number; image_bytes: number; total_bytes: number };
/** How other devices reach this LazyBoy (GET /api/connect). `https_port` is null when HTTPS is off. */
export type ConnectInfo = { hosts: string[]; hostname: string; port: number; https_port: number | null; fingerprint: string | null; token: string };
export const api = {
  connect: () => request<ConnectInfo>("/api/connect"),
  answerQuestion: (id: string, body: Record<string, unknown>) => request(`/api/agents/${encodeURIComponent(id)}/questions`, {method:"POST",body:JSON.stringify(body)}),
  routingHealth: () => request<{ready: boolean; supported?: boolean; engines?: {semantic?: string; laya?: string}; backend?: string}>("/api/settings/routing/health"),
  routines: (id: string) => request<{routines: Routine[]}>(`/api/agents/${encodeURIComponent(id)}/routines`),
  manageRoutine: (id: string, body: Record<string, unknown>) => request<{routine?: Routine; task_id?: string; runs?: RoutineRun[]}>(`/api/agents/${encodeURIComponent(id)}/routines`, {method:"POST",body:JSON.stringify(body)}),
  connectors: () => request<{ apps: ConnectorApp[]; categories: string[] }>("/api/connectors"),
  connector: (id: string, body: Record<string, unknown>) =>
    request<ConnectorApp>(`/api/connectors/${encodeURIComponent(id)}`, { method: "POST", body: JSON.stringify(body) }, 150000),
  mcp: (body: Record<string, unknown>, timeout = 30000) => request<{servers: McpServer[];scope?:string;skipped?:boolean}>("/api/mcp", {method:"POST",body:JSON.stringify(body)}, timeout),
  media: (id?: string) => request<{model: string; capabilities: MediaCapabilities; limits?: UploadLimits}>("/api/media" + (id ? `?agent_id=${encodeURIComponent(id)}` : "")),
  avatarModels: () => request<{models: ImageModel[]}>("/api/avatar/models", undefined, 25000),
  generateAvatar: (choice: ImageModel, prompt: string, signal?: AbortSignal) => request<{data_url: string}>("/api/avatar/generate", {method:"POST", body:JSON.stringify({provider:choice.provider,model:choice.model,prompt}), signal}, 160000),
  upload: async (id: string, file: File): Promise<Upload> => {
    return withRequestTimeout(undefined, 30000, async (signal) => {
      const response = await fetch(`/api/agents/${encodeURIComponent(id)}/uploads?name=${encodeURIComponent(file.name)}`, { method: "POST", body: file, signal });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || "上傳失敗，請重試");
      return data;
    });
  },
  /** A group chat attachment, copied to every member's computer. */
  roomUpload: async (roomId: string, file: File): Promise<Upload> => {
    return withRequestTimeout(undefined, 60000, async (signal) => {
      const response = await fetch(`/api/rooms/${encodeURIComponent(roomId)}/uploads?name=${encodeURIComponent(file.name)}`, { method: "POST", body: file, signal });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || "上傳失敗，請重試");
      return data;
    });
  },
  settings: () => request<AppSettings>("/api/settings"),
  saveSettings: (body: Record<string, unknown>) =>
    request<AppSettings>("/api/settings", { method: "PUT", body: JSON.stringify(body) }),
  providerPresets: () => request<{ presets?: ProviderPreset[] }>("/api/providers/presets"),
  providerModels: (body: { provider: string; base_url?: string; api_key?: string }) =>
    request<{ models: string[] }>("/api/providers/models", { method: "POST", body: JSON.stringify(body) }),
  checkModel: (body: { provider: string; model: string; protocol?: ModelProtocol; base_url?: string; api_key?: string }) =>
    request<{ result: ModelCheck; stored: boolean; settings: AppSettings }>("/api/providers/check", { method: "POST", body: JSON.stringify(body) }, 90000),
  checkModelsInUse: () =>
    request<{ checked: number; settings: AppSettings }>("/api/providers/check-in-use", { method: "POST", body: "{}" }, 90000),
  agents: async () => {
    const data = await request<AgentRow[] | { agents?: AgentRow[]; groups?: AgentGroup[]; approvals?: PendingApproval[] }>("/api/agents");
    const agents = Array.isArray(data) ? data : data.agents || [];
    return { agents, groups: Array.isArray(data) ? [] : data.groups || [], approvals: Array.isArray(data) ? [] : data.approvals || [] };
  },
  org: () => request<OrgChart>("/api/org"),
  setApprovalItems: (items: string[]) => request<{ approval_items: string[] }>("/api/org/approval-items", { method: "PUT", body: JSON.stringify({ items }) }),
  createTeam: (body: { name: string; lead: TeamPerson | string; members: (TeamPerson | string)[]; reports_to?: string; group_chat?: boolean; daily_report?: boolean; lead_primary?: boolean; lead_manage?: boolean }) =>
    request<{ team: string; group_id: string; lead_id: string; room_id: string | null; note: string; agents: AgentRow[]; groups: AgentGroup[] }>("/api/teams", { method: "POST", body: JSON.stringify(body) }, 60000),
  approvals: (state = "pending") => request<{ approvals: Approval[] }>(`/api/approvals?state=${encodeURIComponent(state)}`),
  decideApproval: (id: string, decision: "approve" | "reject", note = "") =>
    request<Approval>(`/api/approvals/${encodeURIComponent(id)}`, { method: "POST", body: JSON.stringify({ decision, note }) }),
  exportTemplate: (id: string) => request<AgentTemplate>(`/api/agents/${encodeURIComponent(id)}/template`),
  logins: (id: string) => request<{ sites: { site: string; cookies: number }[] }>(`/api/agents/${encodeURIComponent(id)}/logins`, undefined, 120000),
  copyLogins: (id: string, to: string, sites: string[]) => request<{ copied: number; sites: string[] }>(`/api/agents/${encodeURIComponent(id)}/logins/copy`, { method: "POST", body: JSON.stringify({ to, sites }) }, 180000),
  saveTemplate: (id: string) => request<SavedTemplate>(`/api/agents/${encodeURIComponent(id)}/save-template`, { method: "POST" }, 60000),
  savedTemplates: () => request<{ templates: SavedTemplate[] }>("/api/saved-templates"),
  savedTemplate: (key: string) => request<AgentTemplate>(`/api/saved-templates/${encodeURIComponent(key)}`),
  deleteSavedTemplate: (key: string) => request<{ ok: boolean }>(`/api/saved-templates/${encodeURIComponent(key)}`, { method: "DELETE" }),
  importTemplate: (id: string, template: AgentTemplate, name: string, group_id: string) => request<AgentRow>(`/api/templates/${id}`, {method:"PUT",body:JSON.stringify({template,name,group_id})},60000),
  teaching: (id: string) => request<{ lesson: Lesson | null }>(`/api/agents/${encodeURIComponent(id)}/teach`, undefined, 8000),
  lesson: (id: string, lesson: string) => request<{ lesson: Lesson }>(`/api/agents/${encodeURIComponent(id)}/lessons/${encodeURIComponent(lesson)}`),
  teach: (id: string, body: { action: "start"; goal?: string; trial?: boolean; thread?: string | null } | { action: "pause" | "resume" | "finish" | "cancel"; lesson_id: string } | { action: "note"; lesson_id: string; text: string } | { action: "drop"; lesson_id: string; step: string }) =>
    request<{ lesson: Lesson | null; queued?: string }>(`/api/agents/${encodeURIComponent(id)}/teach`, { method: "POST", body: JSON.stringify(body) }, 60000),
  manageTeam: (body: Record<string, unknown>) => request<{groups?: AgentGroup[]; group?: AgentGroup; agents?: AgentRow[]}>("/api/team", {method:"POST",body:JSON.stringify(body)}),
  skills: (id: string, body: Record<string, unknown> = {action:"list"}) => request<{skills: AgentSkill[]}>(`/api/agents/${encodeURIComponent(id)}/skills`, {method:"POST",body:JSON.stringify(body)}),
  createAgent: (profile: AgentProfile) =>
    request<AgentRow>("/api/agents", { method: "POST", body: JSON.stringify(profile) }),
  duplicate: (id: string) => request<AgentRow>(`/api/agents/${encodeURIComponent(id)}/duplicate`, { method: "POST" }, 60000),
  context: (id: string, thread?: string) => request<ContextUsage>(`/api/agents/${encodeURIComponent(id)}/context${thread && thread !== id ? `?thread=${encodeURIComponent(thread)}` : ""}`),
  compress: (id: string, thread?: string) => request<{ status: string; before_tokens?: number; after_tokens?: number; before_messages?: number; after_messages?: number }>(`/api/agents/${encodeURIComponent(id)}/context/compress${thread && thread !== id ? `?thread=${encodeURIComponent(thread)}` : ""}`, { method: "POST" }, 300000),
  react: (owner: string, messageId: string, emoji: string) => request<{ id: string; reactions: string[] }>(`/api/conversations/${encodeURIComponent(owner)}/messages/${encodeURIComponent(messageId)}/reactions`, { method: "POST", body: JSON.stringify({ emoji }) }),
  search: (q: string) => request<SearchResult>(`/api/search?q=${encodeURIComponent(q)}`),
  providerUsage: (days = 30) => request<ProviderUsageSummary>(`/api/usage?days=${days}`),
  usage: (id: string, days = 7) => request<{ days: number; turns: number; prompt: number; completion: number; total: number; models: { model: string; total: number }[] }>(`/api/agents/${encodeURIComponent(id)}/usage?days=${days}`),
  setPrimary: (id: string, primary: boolean) => request<{ agents: AgentRow[] }>(`/api/agents/${encodeURIComponent(id)}/primary`, { method: "POST", body: JSON.stringify({ primary }) }),
  sidebar: (id: string, change: { pinned?: boolean; hidden?: boolean }) => request<AgentRow>(`/api/agents/${encodeURIComponent(id)}/sidebar`, { method: "POST", body: JSON.stringify(change) }),
  updateAgent: (id: string, profile: Partial<AgentProfile>) =>
    request<AgentRow>(`/api/agents/${id}`, { method: "PATCH", body: JSON.stringify(profile) }),
  setAvatar: async (id: string, png: Blob) => {
    return withRequestTimeout(undefined, 30000, async signal => {
      const res = await fetch(`/api/agents/${encodeURIComponent(id)}/avatar`, {
        method: "PUT",
        headers: { "content-type": "image/png" },
        body: png,
        signal,
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error((data as { error?: string }).error || String(res.status));
      return data as AgentRow;
    });
  },
  clearAvatar: (id: string) =>
    request<AgentRow>(`/api/agents/${encodeURIComponent(id)}/avatar`, { method: "DELETE" }),
  deleteAgent: (id: string) => request<{ ok: boolean; id: string; name: string }>(`/api/agents/${id}`, { method: "DELETE" }),
  agent: (id: string, thread?: string | null, signal?: AbortSignal) =>
    request<AgentDetail>(`/api/agents/${id}${thread && thread !== id ? `?thread=${encodeURIComponent(thread)}` : ""}`, { signal }),
  threads: (id: string) => request<{ threads: ThreadRow[] }>(`/api/agents/${encodeURIComponent(id)}/threads`, undefined, 8000),
  createThread: (id: string, title = "") =>
    request<ThreadRow>(`/api/agents/${encodeURIComponent(id)}/threads`, { method: "POST", body: JSON.stringify({ title }) }),
  renameThread: (id: string, thread: string, title: string) =>
    request<ThreadRow>(`/api/agents/${encodeURIComponent(id)}/threads/${encodeURIComponent(thread)}`, { method: "PATCH", body: JSON.stringify({ title }) }),
  rooms: () => request<{ rooms: RoomRow[] }>("/api/rooms"),
  room: (id: string) => request<RoomDetail>(`/api/rooms/${encodeURIComponent(id)}`),
  createRoom: (member_ids: string[], name = "") => request<RoomRow>("/api/rooms", { method: "POST", body: JSON.stringify({ member_ids, name }) }),
  updateRoom: (id: string, change: { name?: string; member_ids?: string[] }) => request<RoomRow>(`/api/rooms/${encodeURIComponent(id)}`, { method: "PATCH", body: JSON.stringify(change) }),
  deleteRoom: (id: string) => request<{ ok: boolean }>(`/api/rooms/${encodeURIComponent(id)}`, { method: "DELETE" }),
  postRoom: (id: string, text: string, request_id: string) => request<{ woke: string[] }>(`/api/rooms/${encodeURIComponent(id)}/messages`, { method: "POST", body: JSON.stringify({ text, request_id }) }),
  stopRoom: (id: string) => request<RoomRow>(`/api/rooms/${encodeURIComponent(id)}/stop`, { method: "POST" }),
  clearRoom: (id: string) => request<RoomRow>(`/api/rooms/${encodeURIComponent(id)}/messages`, { method: "DELETE" }),
  peerMessages: (id: string, peer: string) => request<{ bots: { id: string; name: string }[]; messages: BotMessage[] }>(`/api/agents/${encodeURIComponent(id)}/peers/${encodeURIComponent(peer)}`),
  deleteThread: (id: string, thread: string) =>
    request<{ ok: boolean }>(`/api/agents/${encodeURIComponent(id)}/threads/${encodeURIComponent(thread)}`, { method: "DELETE" }),
  memory: (id: string) => request<MemoryState>(`/api/agents/${encodeURIComponent(id)}/memory`),
  saveMemory: (id: string, target: "user" | "memory", entries: string[], previous?: string[]) => request<MemoryState>(`/api/agents/${encodeURIComponent(id)}/memory`, { method: "PUT", body: JSON.stringify({ target, entries, previous }) }),
  storage: (id: string) => request<StorageUsage>(`/api/agents/${encodeURIComponent(id)}/storage`, undefined, 30000),
  cleanStorage: (id: string) => request<{ freed: number; usage: StorageUsage }>(`/api/agents/${encodeURIComponent(id)}/storage/clean`, { method: "POST" }, 90000),
  agentFiles: (id: string) => request<{ files: AgentFile[] }>(`/api/agents/${encodeURIComponent(id)}/files`, undefined, 8000),
  activity: (id: string) => request<AgentActivity>(`/api/agents/${id}/activity`, undefined, 8000),
  send: (id: string, text: string, thread?: string | null) =>
    request<{ queued?: string; steered?: string }>(`/api/agents/${id}/messages`, { method: "POST", body: JSON.stringify({ text, thread_id: thread || undefined, request_id: randomUUID() }) }),
  handoverDone: (id: string, taskId: string, questionId: string, action: "done" | "cancel" = "done") => request(`/api/agents/${id}/handover`, { method: "POST", body: JSON.stringify({ task_id: taskId, question_id: questionId, action }) }, 8000),
  submitSecret: (id: string, body: { question_id: string; name?: string; value?: string; values?: Record<string, string>; action?: "cancel" }) =>
    request<{ ok?: boolean; status?: string; names?: string[] }>(`/api/agents/${id}/secrets`, { method: "POST", body: JSON.stringify(body) }, 8000),
  stop: (id: string) => request(`/api/agents/${id}/stop`, { method: "POST", body: "{}" }),
  clearConversation: (id: string, thread?: string | null) =>
    request<{ ok: boolean; cleared_at?: string }>(`/api/agents/${encodeURIComponent(id)}/conversation${thread && thread !== id ? `?thread=${encodeURIComponent(thread)}` : ""}`, { method: "DELETE" }),
  getTask: (taskId: string) => request<TaskDetail>(`/api/tasks/${taskId}`),
  computer: (id: string, action: "start" | "restart" | "update" = "start") =>
    request<{
      ready: boolean;
      state?: string;
      viewer_url: string;
      error?: string;
      updated?: boolean;
      revision?: string;
      crowded?: boolean;
    }>(`/api/agents/${encodeURIComponent(id)}/computer`, {
      method: "POST",
      body: JSON.stringify({ action }),
    }, action === "update" ? 960000 : 120000),
};

/** A bot's setup saved to start new bots from (Grok's templates). */
export type SavedTemplate = { id: string; name: string; title?: string; blurb?: string; avatar_color?: string; avatar_shape?: string; created_at?: string; skill_count?: number };
export type AgentTemplate = {format:"lazyboy.agent-template";version:1;profile:AgentProfile;skills:Pick<AgentSkill,"name"|"description"|"instructions"|"enabled">[];mcp:{mcpServers:Record<string,{command?:string;args?:string[];url?:string;headers?:Record<string,string>;env?:Record<string,string>;enabled?:boolean}>};avatar_png?:string;notes:string[]};

export type McpTest = { ok: boolean; error?: string | null; tools?: {name: string; description?: string}[]; at: string };
export type SignIn = { agent: string; auth_url: string };
export type ConnectorApp = {
  id: string; title: string; description: string; category: string; auth: "oauth" | "none" | "token";
  keywords: string[]; hosts: string[]; token_hint?: string; token_url?: string; featured: boolean;
  state: "available" | "signing_in" | "needs_signin" | "checking" | "connected" | "error" | "off";
  tools: { name: string; description?: string }[]; error?: string | null; checked_at?: string | null; signin?: SignIn | null;
  /** Tools switched off for every bot. */
  disabled_tools?: string[];
};
export type McpServer = { auth?: string | null; catalog?: string | null; signed_in?: boolean | null; signin?: SignIn | null; scope?: string; scope_name?: string; last_test?: McpTest | null; enabled?:boolean; missing_fields?:string[]; server: string; transport: string; url?: string; command?: string; args?: string[]; header_names?: string[]; env_names?: string[]; status?: string; serverStatus?: string; serverError?: string; tools?: {name: string; description?: string}[] };
