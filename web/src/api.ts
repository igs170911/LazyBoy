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
  observed_at_ms: number;
  active_task_ids: string[];
  latest_task_id?: string | null;
  queued_task_ids?: string[];
  thread_id?: string | null;
  queued_thread_ids?: string[];
  question?: HumanQuestion | null;
  questions?: HumanQuestion[];
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
export type TaskDetail = {
  collaboration?: {message_id:number;sender_name:string;recipient_name:string;content:string;at:string}[];
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
  milestones?: { id: string; at: string; message: string; completed: string[]; next: string; evidence: string[] }[];
  approval?: {
    decision: string;
    note?: string;
    decided_at: string;
    decided_by: string;
  } | null;
};

export type AgentGroup = { id: string; name: string; position?: number };
export type AgentSkill = { id: string; name: string; description: string; instructions: string; enabled: boolean; updated_at: string; source?: string };

export type AgentRow = {
  group_id?: string;
  avatar_traits?: Record<string, number>;
  avatar_expression?: string;
  persona?: string;
  reply_style?: string;
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

export type TranscriptItem = { id?: string; role: "user" | "assistant"; content: string; at?: string; streaming?: boolean; narration?: string[]; routine_id?: string; routine_title?: string; file?: { id?: string; task_id?: string; title: string; path?: string; url: string; media_type?: string; bytes: number; available: boolean; status?: string }; action?: { question_id: string; task_id?: string; kind?: string; state: string } };

export type ModelProtocol = "chat_completions" | "responses" | "messages";
export type ModelCheck = { status: "ok" | "protocol" | "unavailable" | "auth" | "busy" | "unknown"; message: string; detail?: string; at?: number; v?: number; protocol?: ModelProtocol | null };
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
export type RoutineSchedule = { frequency: "once" | "daily" | "weekdays" | "weekly" | "monthly"; timezone: string; time: string; date?: string | null; weekdays?: number[]; day_of_month?: number | null };
export type Routine = { id: string; agent_id: string; title: string; instructions: string; schedule: RoutineSchedule; enabled: boolean; next_run_at: string | null; last_run_at: string | null; last_task_id: string | null; last_error: string | null; retry_at?: string | null; last_run?: { state: string; verdict?: string | null; at: string } | null };
export type RoutineRun = { started_at: string; task_id: string; state: string; verdict?: string; result?: {summary?: string} | null };
export type MediaCapabilities = { images: boolean; pdf: boolean; text: boolean };
export const api = {
  answerQuestion: (id: string, body: Record<string, unknown>) => request(`/api/agents/${encodeURIComponent(id)}/questions`, {method:"POST",body:JSON.stringify(body)}),
  routingHealth: () => request<{ready: boolean; supported?: boolean; engines?: {semantic?: string; laya?: string}; backend?: string}>("/api/settings/routing/health"),
  routines: (id: string) => request<{routines: Routine[]}>(`/api/agents/${encodeURIComponent(id)}/routines`),
  manageRoutine: (id: string, body: Record<string, unknown>) => request<{routine?: Routine; task_id?: string; runs?: RoutineRun[]}>(`/api/agents/${encodeURIComponent(id)}/routines`, {method:"POST",body:JSON.stringify(body)}),
  mcp: (body: Record<string, unknown>) => request<{servers: McpServer[];scope?:string}>("/api/mcp", {method:"POST",body:JSON.stringify(body)}, 30000),
  media: (id?: string) => request<{model: string; capabilities: MediaCapabilities}>("/api/media" + (id ? `?agent_id=${encodeURIComponent(id)}` : "")),
  avatarModels: () => request<{provider: string; models: string[]}>("/api/avatar/models", undefined, 25000),
  generateAvatar: (model: string, prompt: string, signal?: AbortSignal) => request<{data_url: string}>("/api/avatar/generate", {method:"POST", body:JSON.stringify({model,prompt}), signal}, 160000),
  upload: async (id: string, file: File): Promise<Upload> => {
    return withRequestTimeout(undefined, 30000, async (signal) => {
      const response = await fetch(`/api/agents/${encodeURIComponent(id)}/uploads?name=${encodeURIComponent(file.name)}`, { method: "POST", body: file, signal });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || "上傳失敗，請重試");
      return data;
    });
  },
  health: () => request<{ ok: boolean; service: boolean; model: string; provider?: string }>("/api/health"),
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
    const data = await request<AgentRow[] | { agents?: AgentRow[]; groups?: AgentGroup[] }>("/api/agents");
    const agents = Array.isArray(data) ? data : data.agents || [];
    return { agents, groups: Array.isArray(data) ? [] : data.groups || [] };
  },
  exportTemplate: (id: string) => request<AgentTemplate>(`/api/agents/${encodeURIComponent(id)}/template`),
  importTemplate: (id: string, template: AgentTemplate, name: string, group_id: string) => request<AgentRow>(`/api/templates/${id}`, {method:"PUT",body:JSON.stringify({template,name,group_id})},60000),
  saveLesson: (id: string, body: Record<string, unknown>) => request<{id:string;video_url:string;manifest_path?:string}>(`/api/agents/${encodeURIComponent(id)}/lessons`, {method:"POST",body:JSON.stringify(body)},60000),
  manageTeam: (body: Record<string, unknown>) => request<{groups?: AgentGroup[]; group?: AgentGroup; agents?: AgentRow[]}>("/api/team", {method:"POST",body:JSON.stringify(body)}),
  skills: (id: string, body: Record<string, unknown> = {action:"list"}) => request<{skills: AgentSkill[]}>(`/api/agents/${encodeURIComponent(id)}/skills`, {method:"POST",body:JSON.stringify(body)}),
  createAgent: (profile: AgentProfile) =>
    request<AgentRow>("/api/agents", { method: "POST", body: JSON.stringify(profile) }),
  updateAgent: (id: string, profile: AgentProfile) =>
    request<AgentRow>(`/api/agents/${id}`, { method: "PATCH", body: JSON.stringify(profile) }),
  renameAgent: (id: string, name: string) =>
    request<AgentRow>(`/api/agents/${id}`, { method: "PATCH", body: JSON.stringify({ name }) }),
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
  deleteThread: (id: string, thread: string) =>
    request<{ ok: boolean }>(`/api/agents/${encodeURIComponent(id)}/threads/${encodeURIComponent(thread)}`, { method: "DELETE" }),
  agentFiles: (id: string) => request<{ files: AgentFile[] }>(`/api/agents/${encodeURIComponent(id)}/files`, undefined, 8000),
  activity: (id: string) => request<AgentActivity>(`/api/agents/${id}/activity`, undefined, 8000),
  send: (id: string, text: string, thread?: string | null) =>
    request<{ queued?: string; steered?: string }>(`/api/agents/${id}/messages`, { method: "POST", body: JSON.stringify({ text, thread_id: thread || undefined, request_id: randomUUID() }) }),
  handoverDone: (id: string, taskId: string, questionId: string, action: "done" | "cancel" = "done") => request(`/api/agents/${id}/handover`, { method: "POST", body: JSON.stringify({ task_id: taskId, question_id: questionId, action }) }, 8000),
  submitSecret: (id: string, body: { question_id: string; name?: string; value?: string; values?: Record<string, string>; action?: "cancel" }) =>
    request<{ ok?: boolean; status?: string; names?: string[] }>(`/api/agents/${id}/secrets`, { method: "POST", body: JSON.stringify(body) }, 8000),
  // L2a challenge routes (78b8124)
  submitCaptchaChallenge: (id: string, body: { question_id: string; answer?: string; action?: "cancel" | "escalate" }) =>
    request<{ ok?: boolean; status?: string }>(`/api/agents/${id}/challenges/captcha`, { method: "POST", body: JSON.stringify(body) }, 8000),
  submitOtpChallenge: (id: string, body: { question_id: string; code?: string; action?: "cancel" | "escalate" }) =>
    request<{ ok?: boolean; status?: string }>(`/api/agents/${id}/challenges/otp`, { method: "POST", body: JSON.stringify(body) }, 8000),
  stop: (id: string) => request(`/api/agents/${id}/stop`, { method: "POST", body: "{}" }),
  clearConversation: (id: string, thread?: string | null) =>
    request<{ ok: boolean; cleared_at?: string }>(`/api/agents/${encodeURIComponent(id)}/conversation${thread && thread !== id ? `?thread=${encodeURIComponent(thread)}` : ""}`, { method: "DELETE" }),
  computerStatus: (id: string) =>
    request<{
      ready: boolean;
      state?: string;
      viewer_url: string;
      error?: string;
      revision?: string;
      crowded?: boolean;
    }>(`/api/agents/${encodeURIComponent(id)}/computer`),
  getTask: (taskId: string) => request<TaskDetail>(`/api/tasks/${taskId}`),
  approveTask: (taskId: string, agent: string, decision: "approve" | "reject", note?: string, artifactIds?: string[]) =>
    request<{ approved: boolean }>(`/api/tasks/${taskId}/approve`, {
      method: "POST",
      body: JSON.stringify({ agent, decision, note, artifact_ids: artifactIds }),
    }),
  computer: (id: string, action: "start" | "restart" | "update" = "start") =>
    request<{
      ready: boolean;
      state?: string;
      viewer_url: string;
      error?: string;
      updated?: boolean;
      revision?: string;
      crowded?: boolean;
    }>(id ? `/api/agents/${encodeURIComponent(id)}/computer` : "/api/computer", {
      method: "POST",
      body: JSON.stringify({ action }),
    }, action === "update" ? 960000 : 120000),
};

export type AgentTemplate = {format:"lazyboy.agent-template";version:1;profile:AgentProfile;skills:Pick<AgentSkill,"name"|"description"|"instructions"|"enabled">[];mcp:{mcpServers:Record<string,{command?:string;args?:string[];url?:string;headers?:Record<string,string>;env?:Record<string,string>;enabled?:boolean}>};avatar_png?:string;notes:string[]};

export type McpServer = { enabled?:boolean; missing_fields?:string[]; server: string; transport: string; url?: string; command?: string; args?: string[]; header_names?: string[]; env_names?: string[]; status?: string; serverStatus?: string; serverError?: string; tools?: {name: string; description?: string}[] };
