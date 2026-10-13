import json
import os
from pathlib import Path
import secrets
from urllib.parse import urlparse

from .store import Store

ROOT = Path(__file__).resolve().parent.parent
DATA = Path(os.environ.get("LAZYHERMES_DATA_DIR", str(ROOT / ".data"))).resolve()
PORT = int(os.environ.get("LAZYHERMES_PORT", "8790"))
IMAGE = os.environ.get("LAZYHERMES_IMAGE", "lazyhermes-bot:local")
# Set when LazyBoy itself runs in Docker (compose.yaml): the data directory is this Docker volume, bot computers
# mount their folders from it, and the product reaches them by name on this Docker network instead of host ports.
DATA_VOLUME = os.environ.get("LAZYHERMES_DATA_VOLUME", "")
NETWORK = os.environ.get("LAZYHERMES_NETWORK", "")
HERMES_COMMIT = (ROOT / "HERMES_VERSION").read_text().strip()
PROVIDER_PRESETS = json.loads((Path(__file__).with_name("hermes_providers.json")).read_text())
# Hermes web search backends the product offers -> the env var holding their key. Exa and Tavily
# fall back to their keyless free tier without one, so search works before any setup.
SEARCH_PROVIDERS = {"exa": "EXA_API_KEY", "tavily": "TAVILY_API_KEY", "brave-free": "BRAVE_SEARCH_API_KEY", "ddgs": ""}
REASONING_LEVELS = ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]


def private_write(path: Path, content: str):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".new")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as out: out.write(content)
    os.replace(tmp, path)


class Settings:
    def __init__(self, store: Store):
        self.store = store
        self.path = store.directory / "settings.json"
        if not self.path.exists():
            self.save({"active": "custom", "model": "", "providers": {"custom": {"label": "OpenAI 相容服務", "base_url": "", "api_key": "", "models": []}}})
        token_path = store.directory / "access-token"
        if not token_path.exists(): private_write(token_path, secrets.token_urlsafe(32) + "\n")
        self.access_token = os.environ.get("LAZYHERMES_ACCESS_TOKEN") or token_path.read_text().strip()

    def read(self):
        return json.loads(self.path.read_text())

    def save(self, value):
        private_write(self.path, json.dumps(value, ensure_ascii=False, indent=2) + "\n")

    def effective(self, agent=None):
        config = self.read()
        provider = (agent or {}).get("provider") or config["active"]
        model = (agent or {}).get("model") or config.get("model", "")
        return provider, model, config.get("providers", {}).get(provider, {})

    def public(self):
        cfg = self.read()
        active, model, selected = self.effective()
        providers = []
        from .model_check import CHECK_VERSION
        for id, p in cfg["providers"].items():
            models = p.get("models", [])
            # Results from an outdated checker are dropped so the page checks those models again.
            models = [{"id": m} if isinstance(m, str) else m for m in models]
            models = [{k: v for k, v in m.items() if k != "check"} if m.get("check") and m["check"].get("v") != CHECK_VERSION else m
                      for m in models]
            providers.append({"id": id, "label": p.get("label") or id, "base_url": p.get("base_url", ""),
                              "has_key": bool(p.get("api_key")), "selectable": bool(p.get("base_url") and models), "models": models})
        from .routing import DEFAULT_ROUTING
        search = self.search()
        return {"search": {"provider": search["provider"], "providers": list(SEARCH_PROVIDERS), "has_key": bool(search["api_key"]),
                           "has_xai_key": bool(search["xai_api_key"])},
                "active": active, "model": model, "protocol": self.protocol(selected, model), "routing": {**DEFAULT_ROUTING, **cfg.get("routing", {})},
                "base_url": selected.get("base_url", ""), "has_key": bool(selected.get("api_key")),
                "config_path": str(self.path), "providers": providers, "engine": "hermes", "hermes_commit": HERMES_COMMIT}

    def search(self):
        value = {"provider": "exa", "api_key": "", "xai_api_key": "", **self.read().get("search", {})}
        if value["provider"] not in SEARCH_PROVIDERS: value["provider"] = "exa"
        return value

    def guess_protocol(self, provider_id, model, provider=None):
        """First protocol to try: Hermes's model info or rules, else the stored one, else the preset's."""
        from .model_check import rule_protocol
        provider = provider if provider is not None else self.read()["providers"].get(provider_id, {})
        preset = next((p for p in PROVIDER_PRESETS if p["id"] == provider_id), {})
        # Hermes's knowledge first: a stored protocol may itself be the mistake being checked.
        return ((preset.get("model_info", {}).get(model) or {}).get("protocol")
                or rule_protocol(provider_id, provider.get("base_url") or preset.get("base_url"), model)
                or self.model_row(provider, model).get("protocol") or preset.get("protocol") or "chat_completions")

    def record_check(self, provider_id, model, result, used=None):
        """Keep the latest check on the model row; a model that answered keeps the protocol it answered on.
        ``used`` is the (base_url, api_key) the check ran with: if they changed meanwhile, the result is stale."""
        cfg = self.read()
        provider = cfg["providers"].get(provider_id)
        if not provider: return False
        if used is not None and used != (provider.get("base_url", ""), provider.get("api_key", "")): return False
        rows = [r if isinstance(r, dict) else {"id": r} for r in provider.get("models", [])]
        row = next((r for r in rows if r.get("id") == model), None)
        if row is None: return False
        if result.get("protocol"): row["protocol"] = result["protocol"]
        row["check"] = {k: result.get(k) for k in ("status", "message", "detail", "at", "v")}
        provider["models"] = rows
        self.save(cfg)
        return True

    @staticmethod
    def model_row(provider, model):
        return next((row for row in provider.get("models", []) if isinstance(row, dict) and row.get("id") == model), {})

    @staticmethod
    def reasoning(provider, model):
        return Settings.model_row(provider, model).get("reasoning_effort") or "medium"

    @staticmethod
    def protocol(provider, model):
        for row in provider.get("models", []):
            if isinstance(row, dict) and row.get("id") == model: return row.get("protocol", "chat_completions")
        return "chat_completions"

    def update(self, body):
        cfg = self.read()
        p = body.get("provider")
        if isinstance(p, dict):
            id = p.get("id", "").strip()
            if not id or len(id) > 100: raise ValueError("供應商名稱不可空白")
            old = cfg["providers"].get(id, {})
            url = p.get("base_url", old.get("base_url", "")).strip().rstrip("/")
            if url and urlparse(url).scheme not in {"http", "https"}: raise ValueError("請填寫 HTTP 或 HTTPS 服務網址")
            for row in p.get("models", []):
                if isinstance(row, dict) and row.get("reasoning_effort") and row["reasoning_effort"] not in REASONING_LEVELS:
                    raise ValueError("執行核心不支援這個推理強度")
            # A check only vouches for the protocol, key and URL it ran with.
            credentials_changed = bool(p.get("api_key") and p["api_key"] != old.get("api_key")) or url != old.get("base_url", "")
            if "models" in p:
                before = {r.get("id"): r for r in old.get("models", []) if isinstance(r, dict)}
                p = {**p, "models": [{k: v for k, v in r.items() if k != "check"}
                                     if isinstance(r, dict) and (credentials_changed or r.get("protocol") != before.get(r.get("id"), {}).get("protocol")) else r
                                     for r in p["models"]]}
            elif credentials_changed:
                p = {**p, "models": [{k: v for k, v in r.items() if k != "check"} if isinstance(r, dict) else r for r in old.get("models", [])]}
            cfg["providers"][id] = {**old, **p, "base_url": url, "api_key": p.get("api_key") or old.get("api_key", "")}
        if "active" in body:
            if body["active"] not in cfg["providers"]: raise ValueError("找不到供應商")
            cfg["active"] = body["active"]
        if "model" in body: cfg["model"] = str(body["model"]).strip()
        if "search" in body:
            patch = body["search"]
            if not isinstance(patch, dict): raise ValueError("搜尋設定格式無效")
            old = cfg.get("search", {})
            provider = patch.get("provider", old.get("provider", "exa"))
            if provider not in SEARCH_PROVIDERS: raise ValueError("不支援這個搜尋服務")
            # A key belongs to its provider; switching provider without a new key drops the old one.
            key = patch["api_key"] if "api_key" in patch else old.get("api_key", "") if provider == old.get("provider", "exa") else ""
            xai = patch["xai_api_key"] if "xai_api_key" in patch else old.get("xai_api_key", "")
            cfg["search"] = {"provider": provider, "api_key": str(key or "").strip(), "xai_api_key": str(xai or "").strip()}
        if "routing" in body or cfg.get("routing"):
            from .routing import validate_routing
            patch = body.get("routing", {})
            if not isinstance(patch, dict): raise ValueError("分流設定格式無效")
            cfg["routing"] = validate_routing({**cfg.get("routing", {}), **patch}, cfg)
        self.save(cfg)
        return self.public()
