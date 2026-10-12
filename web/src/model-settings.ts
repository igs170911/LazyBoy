import type { ModelProtocol, SettingsModel } from "./api";

export function parseModels(text: string): SettingsModel[] {
  return text.split("\n").map(line => line.trim()).filter(Boolean).map(line => {
    // Model IDs may themselves contain colons (for example local model tags).
    const match = line.match(/^(.*):(chat_completions|responses|messages)$/);
    return { id: match ? match[1].trim() : line,
      protocol: (match ? match[2] : "chat_completions") as ModelProtocol };
  }).filter(model => model.id);
}

export function modelsToText(models: SettingsModel[]): string {
  return models.map(model => model.protocol && model.protocol !== "chat_completions"
    ? `${model.id}:${model.protocol}` : model.id).join("\n");
}

/** Hermes's per-model wire rules for the OpenCode relays (hermes_cli/models.py `_OPENCODE_API_MODE_PREFIXES`). */
const OPENCODE_RULES: Record<string, [string[], ModelProtocol][]> = {
  "opencode-go": [[["gpt-", "grok-", "muse-spark"], "responses"], [["minimax-", "qwen", "union-alpha"], "messages"]],
  "opencode-zen": [[["claude-", "union-alpha"], "messages"], [["gpt-", "grok-", "muse-spark"], "responses"], [["qwen"], "messages"]],
};

type PresetHints = { protocol?: ModelProtocol; base_url?: string; model_info?: Record<string, ModelInfo> };
export type ModelInfo = { protocol: ModelProtocol; vision: boolean; efforts: string[] | null };

function opencodeFamily(provider: string, baseUrl = "") {
  if (provider === "opencode-go" || /opencode\.ai\/zen\/go/.test(baseUrl)) return "opencode-go";
  if (provider === "opencode-zen" || provider === "opencode" || /opencode\.ai\/zen/.test(baseUrl)) return "opencode-zen";
  return "";
}

/** Best first guess at a model's wire protocol; the settings page then confirms it with a real request. */
export function providerModelProtocol(provider: string, id: string, preset: PresetHints | ModelProtocol = {}): ModelProtocol {
  const hints = typeof preset === "string" ? { protocol: preset } : preset;
  const known = hints.model_info?.[id]?.protocol;
  if (known) return known;
  const baseUrl = hints.base_url || "";
  if (provider === "anthropic" || /api\.anthropic\.com|\/anthropic(\/v1)?$/.test(baseUrl)) return "messages";
  const family = opencodeFamily(provider, baseUrl);
  if (family) {
    const name = id.toLowerCase().replace(/^opencode-(zen|go)\//, "");
    for (const [prefixes, protocol] of OPENCODE_RULES[family]) if (prefixes.some(prefix => name.startsWith(prefix))) return protocol;
    return "chat_completions";
  }
  return hints.protocol || "chat_completions";
}

/** A model row as the preset knows it: protocol, plus vision when Hermes knows the model reads images. */
export function presetModel(provider: string, id: string, preset: PresetHints | ModelProtocol = {}): SettingsModel {
  const info = typeof preset === "string" ? undefined : preset.model_info?.[id];
  return { id, protocol: providerModelProtocol(provider, id, preset), ...(info?.vision ? { vision: true } : {}) };
}

export function mergeProviderModels(ids: string[], configured: SettingsModel[], provider: string, preset: PresetHints | ModelProtocol = "chat_completions"): SettingsModel[] {
  const existing = new Map(configured.map(model => [model.id, model]));
  const discovered = ids.map(id => {
    const guess = presetModel(provider, id, preset);
    const previous = existing.get(id);
    // chat_completions is the discovery default. On a mixed relay it is not a
    // deliberate choice when the model has a known native endpoint.
    const protocol = previous?.protocol && !(previous.protocol === "chat_completions" && guess.protocol !== "chat_completions")
      ? previous.protocol
      : guess.protocol;
    return { ...guess, ...previous, id, protocol };
  });
  const listed = new Set(ids);
  return [...discovered, ...configured.filter(model => !listed.has(model.id))];
}
