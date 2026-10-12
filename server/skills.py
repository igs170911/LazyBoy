import shutil
import yaml
from .config import private_write
from .files import contained
from .store import now, uid


class Skills:
    def __init__(self, store): self.store = store

    def root(self, id): return self.store.agent_directory(id) / "hermes"

    def config(self, id):
        path = self.root(id) / "config.yaml"
        return yaml.safe_load(path.read_text()) or {} if path.exists() else {}

    def list(self, id):
        root = self.root(id) / "skills"
        cfg = self.config(id)
        disabled = cfg.get("skills", {}).get("disabled", [])
        rows = []
        for path in root.rglob("SKILL.md"):
            if not path.resolve().is_relative_to(root.resolve()): continue
            rel = str(path.parent.relative_to(root))
            raw = path.read_text(errors="replace")
            meta, instructions = {}, raw
            if raw.startswith("---\n"):
                parts = raw.split("---", 2)
                if len(parts) == 3:
                    try: meta = yaml.safe_load(parts[1]) or {}
                    except yaml.YAMLError: meta = {}
                    instructions = parts[2].strip()
            if not isinstance(meta, dict): meta = {}
            name = str(meta.get("name", path.parent.name))
            rows.append({"id": rel, "name": name, "description": str(meta.get("description", "")),
                         "instructions": instructions, "enabled": name not in disabled and rel not in disabled,
                         "updated_at": now(), "source": "Hermes"})
        return rows

    def manage(self, id, body):
        action = body.get("action", "list")
        if action == "list": return self.list(id)
        root = self.root(id) / "skills"
        rows = self.list(id)
        key = body.get("id") or uid()
        folder = contained(root, key)
        previous = next((r for r in rows if r["id"] == key), None)
        if action == "delete":
            if not previous: raise KeyError(key)
            shutil.rmtree(folder)
        elif action in {"save", "toggle"}:
            if action == "toggle" and not previous: raise KeyError(key)
            row = {**(previous or {}), **body}
            name = str(row.get("name", "")).strip()
            instructions = str(row.get("instructions", "")).strip()
            if not name or not instructions: raise ValueError("技能名稱和內容不可空白")
            if len(instructions) > 200000: raise ValueError("技能內容過長")
            folder.mkdir(parents=True, exist_ok=True)
            meta = yaml.safe_dump({"name": name, "description": row.get("description", "")}, allow_unicode=True, sort_keys=False)
            private_write(folder / "SKILL.md", "---\n" + meta + "---\n\n" + instructions + "\n")
            cfg = self.config(id)
            disabled = set(cfg.setdefault("skills", {}).get("disabled", []))
            disabled.discard(name); disabled.discard(key)
            if not row.get("enabled", True): disabled.add(name)
            cfg["skills"]["disabled"] = sorted(disabled)
            private_write(self.root(id) / "config.yaml", yaml.safe_dump(cfg, allow_unicode=True, sort_keys=False))
        else: raise ValueError("不支援的技能操作")
        return self.list(id)
