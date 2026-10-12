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
HERMES_COMMIT = (ROOT / "HERMES_VERSION").read_text().strip()
PROVIDER_PRESETS = json.loads((Path(__file__).with_name("hermes_providers.json")).read_text())
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
        for id, p in cfg["providers"].items():
            models = p.get("models", [])
            models = [{"id": m} if isinstance(m, str) else m for m in models]
            providers.append({"id": id, "label": p.get("label") or id, "base_url": p.get("base_url", ""),
                              "has_key": bool(p.get("api_key")), "selectable": bool(p.get("base_url") and models), "models": models})
        from .routing import DEFAULT_ROUTING
        return {"active": active, "model": model, "protocol": self.protocol(selected, model), "routing": {**DEFAULT_ROUTING, **cfg.get("routing", {})},
                "base_url": selected.get("base_url", ""), "has_key": bool(selected.get("api_key")),
                "config_path": str(self.path), "providers": providers, "engine": "hermes", "hermes_commit": HERMES_COMMIT}

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
            cfg["providers"][id] = {**old, **p, "base_url": url, "api_key": p.get("api_key") or old.get("api_key", "")}
        if "active" in body:
            if body["active"] not in cfg["providers"]: raise ValueError("找不到供應商")
            cfg["active"] = body["active"]
        if "model" in body: cfg["model"] = str(body["model"]).strip()
        if "routing" in body or cfg.get("routing"):
            from .routing import validate_routing
            patch = body.get("routing", {})
            if not isinstance(patch, dict): raise ValueError("分流設定格式無效")
            cfg["routing"] = validate_routing({**cfg.get("routing", {}), **patch}, cfg)
        self.save(cfg)
        return self.public()
