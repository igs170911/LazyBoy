"""Export Hermes's built-in API-key providers as LazyBoy presets.

Runs with Hermes's own interpreter (see `make providers`); prints JSON to stdout.
"""
import json
import os
import sys

sys.path.insert(0, os.environ.get("HERMES_ROOT", "/opt/hermes"))

import providers
from hermes_cli.auth import PROVIDER_REGISTRY
from hermes_cli.models_catalog_static import CANONICAL_PROVIDERS, _PROVIDER_MODELS
from hermes_cli.runtime_provider import _fallback_api_mode

PROTOCOLS = {"chat_completions": "chat_completions", "codex_responses": "responses", "anthropic_messages": "messages"}
SKIP = {"custom", "moa"}


def efforts(slug, model, protocol, caps, profile):
    """Reasoning levels the model accepts: [] none, a list when Hermes knows the family, None when unknown."""
    import agent.reasoning_effort as re_
    if caps and caps.supports_reasoning is False: return []
    hook = getattr(profile, "supported_reasoning_efforts", None)
    try: known = hook(model) if callable(hook) else None
    except Exception: known = None
    if known is not None: return list(known)
    name = model.lower()
    if slug in {"openai-api", "openai-codex"} or (protocol == "responses" and name.startswith("gpt-")):
        return list(re_.codex_supported_efforts(model))
    if "grok" in name:
        return list(re_.XAI_GROK46_EFFORTS if "4.6" in name or "4.7" in name else re_.XAI_LEGACY_EFFORTS)
    if "deepseek-v4" in name: return list(re_.DEEPSEEK_V4_EFFORTS)
    if "glm-5.3" in name: return list(re_.GLM53_EFFORTS)
    if "kimi" in name: return list(re_.kimi_supported_efforts(model))
    return None


def model_info(slug, base_url, fallback, models, profile):
    """Per-model wire protocol and capabilities, as Hermes itself would resolve them."""
    from agent.models_dev import get_model_capabilities
    from hermes_cli.model_switch import model_derived_api_mode
    info = {}
    for model in models:
        mode = model_derived_api_mode(slug, model) or _fallback_api_mode(slug, base_url, model)
        protocol = PROTOCOLS.get(mode, fallback)
        try: caps = get_model_capabilities(slug, model, allow_network=True)
        except Exception: caps = None
        info[model] = {"protocol": protocol, "vision": bool(caps and caps.supports_vision),
                       "efforts": efforts(slug, model, protocol, caps, profile)}
    return info


def preset(slug, label, description):
    registry = PROVIDER_REGISTRY.get(slug)
    profile = providers.get_provider_profile(slug)
    auth = profile.auth_type if profile else (registry.auth_type if registry else "")
    if slug in SKIP or auth != "api_key" or (profile and not profile.listed()):
        return None
    base_url = ((registry.inference_base_url if registry else "") or (profile.base_url if profile else "")).rstrip("/")
    protocol = PROTOCOLS.get(_fallback_api_mode(slug, base_url))
    if not protocol:
        return None
    # The LazyBoy proxy appends the endpoint path (`messages`, `chat/completions`) to base_url.
    if protocol == "messages" and base_url and not base_url.endswith("/v1"):
        base_url += "/v1"
    if slug == "gemini":
        base_url += "/openai"
    models = list(_PROVIDER_MODELS.get(slug) or (profile.fallback_models if profile else ()))[:40]
    return {"id": slug, "label": label or (profile and profile.display_name) or slug,
            "description": description or (profile.description if profile else ""),
            "base_url": base_url, "protocol": protocol, "signup_url": profile.signup_url if profile else "",
            "models": models, "model_info": model_info(slug, base_url, protocol, models, profile),
            "local": base_url.startswith(("http://127.", "http://localhost"))}


rows, seen = [], set()
for entry in CANONICAL_PROVIDERS:
    seen.add(entry.slug)
    row = preset(entry.slug, entry.label, entry.tui_desc)
    if row: rows.append(row)
for profile in providers.list_providers():
    if profile.name not in seen:
        row = preset(profile.name, profile.display_name, profile.description)
        if row: rows.append(row)
json.dump(rows, sys.stdout, ensure_ascii=False, indent=1)
print()
