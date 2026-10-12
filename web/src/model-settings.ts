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

const OPENCODE_RESPONSES = new Set([
  "grok-4.6",
  "grok-4.7",
  "gpt-5.6-luna",
  "muse-spark-1.2-contributor",
  "muse-spark-1.3-contributor",
]);
const OPENCODE_MESSAGES = new Set([
  "minimax-m2.5",
  "minimax-m2.7",
  "minimax-m3",
  "qwen3.6-plus",
  "qwen3.7-plus",
  "qwen3.7-max",
  "qwen3.8-flash",
  "qwen3.8-max",
]);

/** Native wire protocol for a provider model. OpenCode Go mixes three endpoints. */
export function providerModelProtocol(provider: string, id: string, fallback: ModelProtocol = "chat_completions"): ModelProtocol {
  if (provider === "anthropic") return "messages";
  if (provider === "opencode-go" || provider === "opencode") {
    if (OPENCODE_RESPONSES.has(id)) return "responses";
    if (OPENCODE_MESSAGES.has(id)) return "messages";
  }
  return fallback;
}

export function mergeProviderModels(ids: string[], configured: SettingsModel[], provider: string, fallback: ModelProtocol = "chat_completions"): SettingsModel[] {
  const existing = new Map(configured.map(model => [model.id, model]));
  const discovered = ids.map(id => {
    const inferred = providerModelProtocol(provider, id, fallback);
    const previous = existing.get(id);
    // chat_completions is the discovery default. On OpenCode Go it is not a
    // deliberate choice when the model has a known native endpoint.
    const protocol = previous?.protocol && !(previous.protocol === "chat_completions" && inferred !== "chat_completions")
      ? previous.protocol
      : inferred;
    return { ...previous, id, protocol };
  });
  const listed = new Set(ids);
  return [...discovered, ...configured.filter(model => !listed.has(model.id))];
}
