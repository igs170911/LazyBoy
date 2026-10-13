"""A bot's long-term memory as Hermes keeps it, readable and editable from the web.

Hermes stores two lists in the bot's home: memories/USER.md (what it knows about the user) and memories/MEMORY.md
(what it learned about its work and environment), entries joined by "\\n§\\n". Writes take the same exclusive lock
Hermes takes (an flock on the sibling .lock file), so an edit here never interleaves with the bot's own update.
Changes reach the bot when its next conversation turn loads memory.
"""
import contextlib
import fcntl
import os
from pathlib import Path

import yaml

DELIMITER = "\n§\n"
TARGETS = {"user": ("USER.md", "user_char_limit", 1375), "memory": ("MEMORY.md", "memory_char_limit", 2200)}
ENTRY_LIMIT = 2000


def limits(root: Path):
    try: cfg = (yaml.safe_load((root / "config.yaml").read_text()) or {}).get("memory") or {}
    except (OSError, yaml.YAMLError): cfg = {}
    return {target: int(cfg.get(key) or default) for target, (_, key, default) in TARGETS.items()}


def entries(path: Path):
    try: text = path.read_text(encoding="utf-8")
    except OSError: return []
    return [entry.strip() for entry in text.split(DELIMITER) if entry.strip()]


def read(hermes: Path):
    """Both lists with how much of each budget they use."""
    caps = limits(hermes)
    out = {}
    for target, (name, _, _) in TARGETS.items():
        items = entries(hermes / "memories" / name)
        out[target] = {"entries": items, "used": len(DELIMITER.join(items)), "limit": caps[target]}
    return out


@contextlib.contextmanager
def locked(path: Path):
    lock = path.with_suffix(path.suffix + ".lock")
    lock.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(lock, os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def write(hermes: Path, target: str, items):
    """Replace one list. Entries are trimmed; the § separator cannot appear inside one; the budget is enforced."""
    if target not in TARGETS: raise ValueError("無效的記憶類別")
    if not isinstance(items, list): raise ValueError("記憶格式無效")
    clean = []
    for item in items:
        text = str(item or "").replace("§", " ").strip()
        if not text: continue
        if len(text) > ENTRY_LIMIT: raise ValueError(f"每條記憶最多 {ENTRY_LIMIT} 字")
        clean.append(text)
    limit = limits(hermes)[target]
    if len(DELIMITER.join(clean)) > limit: raise ValueError(f"超過這類記憶的上限 {limit} 字，請刪掉或縮短幾條")
    path = hermes / "memories" / TARGETS[target][0]
    path.parent.mkdir(parents=True, exist_ok=True)
    with locked(path):
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_text(DELIMITER.join(clean), encoding="utf-8")
        os.chmod(temporary, 0o600)
        os.replace(temporary, path)
    return read(hermes)
