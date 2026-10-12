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
    models = list(_PROVIDER_MODELS.get(slug) or (profile.fallback_models if profile else ()))
    return {"id": slug, "label": label or (profile and profile.display_name) or slug,
            "description": description or (profile.description if profile else ""),
            "base_url": base_url, "protocol": protocol, "signup_url": profile.signup_url if profile else "",
            "models": models[:40], "local": base_url.startswith(("http://127.", "http://localhost"))}


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
