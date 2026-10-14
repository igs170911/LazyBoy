import hashlib
import json
import re
import shutil
from datetime import datetime, timedelta, timezone
import yaml

# The C parser, when PyYAML was built with it, is about ten times faster.
YAML_LOADER = getattr(yaml, "CSafeLoader", yaml.SafeLoader)
from .config import private_write
from .files import contained
from .store import now, uid


CACHE_DIRS = {"__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache"}
# Hermes' built-in skills a bot has not used for this long wait in its skill library instead of the skill list sent
# with every request; one the bot opens or uses again goes back on the list.
LIBRARY_IDLE_DAYS = 30
LIBRARY_RESULTS = 8
LIBRARY_TEXT = 60000


def dir_hash(directory):
    """Hermes' package hash: MD5 over each file's relative path and bytes, generated caches left out."""
    hasher = hashlib.md5()
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory)
        cache = any(part in CACHE_DIRS for part in relative.parts[:-1]) or (path.suffix in {".pyc", ".pyo"} and path.with_suffix(".py").is_file())
        if not cache and path.is_file():
            hasher.update(str(relative).encode("utf-8")); hasher.update(path.read_bytes())
    return hasher.hexdigest()


class Skills:
    def __init__(self, store): self.store = store

    def root(self, id): return self.store.agent_directory(id) / "hermes"

    def config(self, id):
        path = self.root(id) / "config.yaml"
        return yaml.load(path.read_text(), Loader=YAML_LOADER) or {} if path.exists() else {}

    def list(self, id):
        root = self.root(id) / "skills"
        cfg = self.config(id)
        disabled = cfg.get("skills", {}).get("disabled", [])
        shelved = set(cfg.get("product_skill_library") or [])
        # Where each skill came from: Hermes' own (untouched), LazyBoy's, one the bot wrote itself, or the user's.
        builtin = self.bundled_unchanged(id)
        try: product = set((root / ".lazyboy-seeded").read_text().split())
        except OSError: product = set()
        usage = self.usage(id)
        rows = []
        for path in root.rglob("SKILL.md"):
            if not path.resolve().is_relative_to(root.resolve()): continue
            rel = str(path.parent.relative_to(root))
            raw = path.read_text(errors="replace")
            meta, instructions = {}, raw
            if raw.startswith("---\n"):
                parts = raw.split("---", 2)
                if len(parts) == 3:
                    try: meta = yaml.load(parts[1], Loader=YAML_LOADER) or {}
                    except yaml.YAMLError: meta = {}
                    instructions = parts[2].strip()
            if not isinstance(meta, dict): meta = {}
            name = str(meta.get("name", path.parent.name))
            place = "library" if name in shelved else "off" if name in disabled or rel in disabled else "list"
            source = ("builtin" if rel in builtin else "product" if rel in product else
                      "learned" if (usage.get(name) or {}).get("created_by") == "agent" else "custom")
            rows.append({"id": rel, "name": name, "description": str(meta.get("description", "")),
                         "instructions": instructions, "enabled": place != "off", "place": place,
                         "updated_at": now(), "source": source})
        return rows

    def bundled_unchanged(self, id):
        """Hermes' own skills, synced into the bot and never edited (same check Hermes makes: tools/skills_sync.py).
        Every bot can get these from Hermes, so they are not part of a bot's shareable setup."""
        root = self.root(id) / "skills"
        try: lines = (root / ".bundled_manifest").read_text().splitlines()
        except OSError: return set()
        origin = dict(line.split(":", 1) for line in lines if ":" in line)
        unchanged = set()
        for path in root.rglob("SKILL.md"):
            folder = path.parent
            if origin.get(folder.name) and dir_hash(folder) == origin[folder.name]: unchanged.add(str(folder.relative_to(root)))
        return unchanged

    def count(self, id):
        """How many skills a bot has, without reading them (the sidebar asks every few seconds)."""
        root = self.root(id) / "skills"
        resolved = root.resolve()
        return sum(1 for path in root.rglob("SKILL.md") if path.resolve().is_relative_to(resolved))

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
        elif action == "toggle":
            # Switching a skill on or off is a setting; rewriting its file would drop Hermes' extra frontmatter
            # (platforms, required keys…) and make a built-in skill look edited, so it would stop receiving updates.
            if not previous: raise KeyError(key)
            self.set_place(id, previous["name"], "list" if body.get("enabled", True) else "off", key)
        elif action == "place":
            if not previous: raise KeyError(key)
            if body.get("place") not in {"list", "library", "off"}: raise ValueError("不支援的技能位置")
            self.set_place(id, previous["name"], body["place"], key)
        elif action == "save":
            row = {**(previous or {}), **body}
            name = str(row.get("name", "")).strip()
            instructions = str(row.get("instructions", "")).strip()
            if not name or not instructions: raise ValueError("技能名稱和內容不可空白")
            if len(instructions) > 200000: raise ValueError("技能內容過長")
            folder.mkdir(parents=True, exist_ok=True)
            # Keep whatever else the frontmatter says (Hermes' platforms, tags, required keys); only these two change.
            meta = {**self.frontmatter(folder / "SKILL.md"), "name": name, "description": row.get("description", "")}
            private_write(folder / "SKILL.md", "---\n" + yaml.safe_dump(meta, allow_unicode=True, sort_keys=False) + "---\n\n" + instructions + "\n")
            if previous and previous["name"] != name: self.forget_place(id, previous["name"])
            self.set_place(id, name, "list" if row.get("enabled", True) else "off", key)
        else: raise ValueError("不支援的技能操作")
        return self.list(id)

    @staticmethod
    def frontmatter(path):
        try: raw = path.read_text(errors="replace")
        except OSError: return {}
        parts = raw.split("---", 2) if raw.startswith("---\n") else []
        try: meta = yaml.load(parts[1], Loader=YAML_LOADER) if len(parts) == 3 else {}
        except yaml.YAMLError: meta = {}
        return meta if isinstance(meta, dict) else {}

    def write_config(self, id, cfg):
        private_write(self.root(id) / "config.yaml", yaml.safe_dump(cfg, allow_unicode=True, sort_keys=False))

    def set_place(self, id, name, place, rel=None):
        """The user's choice for one skill: on the list sent with every request, in the library, or off."""
        cfg = self.config(id)
        skills_cfg = cfg.setdefault("skills", {})
        disabled = set(skills_cfg.get("disabled") or []) - {name, rel}
        shelved = set(cfg.get("product_skill_library") or []) - {name}
        if place == "off": disabled.add(name)
        if place == "library": disabled.add(name); shelved.add(name)
        skills_cfg["disabled"] = sorted(disabled)
        cfg["product_skill_library"] = sorted(shelved)
        self.write_config(id, cfg)
        state = self.store.get("skill_library", id, {})
        places = {**(state.get("places") or {}), name: place}
        if place == "off": places.pop(name)
        self.store.put("skill_library", id, {**state, "places": places})

    def forget_place(self, id, name):
        self.set_place(id, name, "list")
        state = self.store.get("skill_library", id, {})
        self.store.put("skill_library", id, {**state, "places": {k: v for k, v in (state.get("places") or {}).items() if k != name}})

    # --- The skill library: Hermes' built-in skills kept off the per-request list until needed ---

    def usage(self, id):
        try: data = json.loads((self.root(id) / "skills" / ".usage.json").read_text())
        except (OSError, ValueError): return {}
        return data if isinstance(data, dict) else {}

    def library_names(self, id):
        """Built-in skills this bot has not used, opened or been told to keep listed in the last LIBRARY_IDLE_DAYS.
        Skills the bot learned or the user edited always stay listed: they are its own know-how."""
        state = self.store.get("skill_library", id, {})
        places, opened = state.get("places") or {}, state.get("opened") or {}
        cutoff = datetime.now(timezone.utc) - timedelta(days=LIBRARY_IDLE_DAYS)
        def recent(stamp):
            try: return bool(stamp) and datetime.fromisoformat(str(stamp).replace("Z", "+00:00")) > cutoff
            except ValueError: return False
        usage = self.usage(id)
        root = self.root(id) / "skills"
        names = set()
        for rel in self.bundled_unchanged(id):
            name = str(self.frontmatter(root / rel / "SKILL.md").get("name") or rel.rsplit("/", 1)[-1])
            if places.get(name) == "list": continue
            if places.get(name) != "library" and (recent(opened.get(name)) or recent((usage.get(name) or {}).get("last_used_at"))): continue
            names.add(name)
        # A skill the user put in the library stays there even if it is their own.
        return names | {name for name, place in places.items() if place == "library"}

    def apply_library(self, id, cfg):
        """Fold the library into Hermes' config (skills Hermes does not list are `skills.disabled`), keeping skills the
        user switched off as they are. Returns the library's skill names."""
        skills_cfg = cfg.setdefault("skills", {})
        before = set(cfg.get("product_skill_library") or [])
        user_off = set(skills_cfg.get("disabled") or []) - before
        library = self.library_names(id) - user_off
        skills_cfg["disabled"] = sorted(user_off | library)
        cfg["product_skill_library"] = sorted(library)
        return library

    def library_search(self, id, query=""):
        shelved = set(self.config(id).get("product_skill_library") or [])
        rows = [r for r in self.list(id) if r["name"] in shelved]
        words = [w for w in re.split(r"[\s,，、/|]+", str(query or "").lower()) if w]
        def score(row):
            name, text = row["name"].lower(), (row["id"] + " " + row["description"]).lower()
            return sum(3 * (w in name) + (w in text) for w in words)
        if words: rows = sorted((r for r in rows if score(r)), key=lambda r: -score(r))[:LIBRARY_RESULTS]
        found = [{"name": r["name"], "category": r["id"].rsplit("/", 1)[0] if "/" in r["id"] else "", "description": r["description"][:200]} for r in rows]
        note = "用 skill_library(name=…) 載入完整步驟。" if found else "技能庫裡沒有相符的技能；換個英文關鍵字，或不靠技能直接做。"
        return {"skills": found, "library_size": len(shelved), "note": note}

    def library_open(self, id, name):
        row = next((r for r in self.list(id) if r["name"] == name or r["id"] == name or r["id"].rsplit("/", 1)[-1] == name), None)
        if not row: raise KeyError(name)
        if row["place"] == "off": raise ValueError("使用者停用了這個技能")
        state = self.store.get("skill_library", id, {})
        self.store.put("skill_library", id, {**state, "opened": {**(state.get("opened") or {}), row["name"]: now()}})
        folder = self.root(id) / "skills" / row["id"]
        files = sorted("/hermes/skills/" + str(path.relative_to(self.root(id) / "skills")) for path in folder.rglob("*")
                       if path.is_file() and path.name != "SKILL.md" and not any(part in CACHE_DIRS for part in path.parts))[:40]
        text = row["instructions"]
        return {"name": row["name"], "description": row["description"], "instructions": text[:LIBRARY_TEXT], "truncated": len(text) > LIBRARY_TEXT,
                "files": files, "note": "照這份步驟做；附帶的檔案可以直接讀。用過之後它會回到你的技能清單。"}
