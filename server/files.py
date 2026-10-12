"""All UI file access stays inside a bot's workspace, including symlink targets."""
import os
from pathlib import Path
import re


def contained(root: Path, name: str, *, exists=False) -> Path:
    if not isinstance(name, str) or not name or "\x00" in name:
        raise ValueError("無效的檔案路徑")
    path = (root / name).resolve()
    if path == root.resolve() or not path.is_relative_to(root.resolve()):
        raise ValueError("檔案必須位於這個 Bot 的工作目錄")
    if exists and not path.is_file(): raise KeyError(name)
    return path


def filename(name):
    name = re.sub(r"[\x00-\x1f/\\]", "_", str(name))[:180].strip(". ")
    return name or "attachment"


# Build output, caches and tool state a person never asked for. They stay on
# disk and in "全部檔案"; only the results people open are marked important.
# Dependency trees are never listed: they would fill the listing cap before any result.
PRUNE_DIRS = {"node_modules", "__pycache__", "venv", "env", "site-packages", "bower_components", "target"}
NOISE_DIRS = PRUNE_DIRS | {"dist", "build", "out", "coverage", "tmp", "temp", "cache", "logs", "vendor"}
NOISE_SUFFIXES = {".pyc", ".pyo", ".o", ".so", ".a", ".class", ".log", ".tmp", ".swp", ".lock", ".map",
                  ".d.ts", ".tsbuildinfo", ".sqlite3-wal", ".sqlite3-shm", ".pid", ".bak"}
NOISE_NAMES = {"package-lock.json", "yarn.lock", "pnpm-lock.yaml", "uv.lock", "poetry.lock", "cargo.lock",
               "go.sum", "thumbs.db", "desktop.ini", "license", "license.txt", "license.md"}
DELIVERABLE_SUFFIXES = {".md", ".pdf", ".docx", ".xlsx", ".pptx", ".csv", ".html", ".txt", ".json",
                        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".mp4", ".webm", ".mp3", ".wav", ".zip"}


def important(rel: str) -> bool:
    """Whether a workspace file is a result worth showing by default."""
    parts = Path(rel).parts
    name = parts[-1].lower()
    if any(part.lower() in NOISE_DIRS for part in parts[:-1]): return False
    if name in NOISE_NAMES or any(name.endswith(suffix) for suffix in NOISE_SUFFIXES): return False
    # A demonstration's raw recording and screenshots; its steps.json is what a person opens.
    if parts[0] == "lessons" and ("shots" in parts or name.endswith(".jsonl")): return False
    return True


def rank(rel: str) -> int:
    """Lower is shown first: deliverables near the top of the workspace beat deep source files."""
    path = Path(rel)
    return (0 if path.suffix.lower() in DELIVERABLE_SUFFIXES else 1) * 10 + min(len(path.parts) - 1, 9)


def walk(root: Path):
    """Regular files under root, skipping hidden entries and dependency trees."""
    for folder, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if not d.startswith(".") and d.lower() not in PRUNE_DIRS)
        for name in sorted(names):
            path = Path(folder) / name
            if not name.startswith(".") and path.is_file() and not path.is_symlink(): yield path
