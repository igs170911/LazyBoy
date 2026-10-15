"""Keep each bot's disk from filling up with things that come back on their own.

Work products in /workspace are never touched. What is cleaned:
- Hermes scratch files older than SCRATCH_DAYS, and file previews (converted Office documents) older than PREVIEW_DAYS.
- Attachments the user uploaded, after UPLOAD_DAYS.
- The bot's Chromium caches once they pass BROWSER_CACHE_LIMIT. Logins (cookies, local storage, IndexedDB) stay.
"""
import asyncio
import json
import logging
import os
import shutil
import time
from pathlib import Path

from .computers import docker
from . import usage as model_usage

log = logging.getLogger(__name__)

DAY = 86400
SCRATCH_DAYS = 3
PREVIEW_DAYS = 7
PREVIEWS = "home/.cache/lazyboy-preview"
UPLOAD_DAYS = 30
BROWSER_CACHE_LIMIT = 200 * 1024 * 1024
# Chromium rebuilds all of these by itself.
BROWSER_CACHES = (
    "Default/Cache", "Default/Code Cache", "Default/GPUCache", "Default/Service Worker/CacheStorage",
    "Default/DawnGraphiteCache", "Default/DawnWebGPUCache", "GPUPersistentCache", "GrShaderCache", "ShaderCache",
    "component_crx_cache", "extensions_crx_cache",
)
# Runs inside the bot's computer: ask the live browser to drop its HTTP cache instead of deleting files under it.
CLEAR_LIVE_CACHE = """
import { chromium } from '/opt/lazyboy/playwright/node_modules/playwright/index.mjs';
setTimeout(() => process.exit(1), 15000);
const browser = await chromium.connectOverCDP('http://127.0.0.1:9222', { timeout: 3000 });
const page = browser.contexts()[0]?.pages()[0];
if (!page) process.exit(2);
const session = await browser.contexts()[0].newCDPSession(page);
await session.send('Network.clearBrowserCache');
await session.detach();
process.exit(0);
"""


# Runs inside the bot's computer once a bot has finished its work: every tab it opened is a renderer holding hundreds of
# megabytes, and a browser full of old tabs is slow and easy to type into the wrong tab. Only the tab used last stays
# (Chromium lists tabs most recently used first; blank ones only when nothing else is open), which also keeps the
# browser itself running. Logins live in cookies, not in tabs.
TIDY_TABS = """
import json, urllib.request
base = "http://127.0.0.1:9222/json"
pages = [t for t in json.load(urllib.request.urlopen(base + "/list", timeout=5)) if t.get("type") == "page"]
blank = lambda url: not url or url == "about:blank" or url.startswith(("chrome://newtab", "chrome://new-tab-page"))
keep = next((t for t in pages if not blank(t.get("url"))), pages[0] if pages else None)
closed = 0
for t in pages:
    if t is keep: continue
    try: urllib.request.urlopen(base + "/close/" + t["id"], timeout=5).close(); closed += 1
    except OSError: pass
print(json.dumps({"open": len(pages), "closed": closed}))
"""


async def tidy_tabs(computers, agent_id: str) -> int:
    """Close the tabs an idle bot's browser no longer needs (see TIDY_TABS); returns how many were closed."""
    code, out, _ = await docker("exec", computers.name(agent_id), "python3", "-c", TIDY_TABS, check=False, timeout=30)
    if code: return 0
    try: return int(json.loads(out.strip().splitlines()[-1]).get("closed", 0))
    except (ValueError, IndexError, AttributeError): return 0


def size(path: Path) -> int:
    """Bytes under `path`, without following links."""
    if not path.exists(): return 0
    if path.is_file() or path.is_symlink(): return path.lstat().st_size
    total = 0
    for folder, dirs, files in os.walk(path):
        for name in files:
            try: total += os.lstat(os.path.join(folder, name)).st_size
            except OSError: pass
    return total


def usage(root: Path) -> dict:
    """Where one bot's disk space goes, in bytes."""
    profile = root / "home" / "chrome-profile"
    workspace, uploads = size(root / "workspace"), size(root / "workspace" / "uploads")
    browser_cache = sum(size(profile / name) for name in BROWSER_CACHES)
    scratch, tools = size(root / "hermes" / "cache" / "scratch"), size(root / "hermes" / "tools")
    previews = size(root / PREVIEWS)
    hermes, home, browser = size(root / "hermes"), size(root / "home"), size(profile)
    parts = {
        "workspace": workspace - uploads,
        "uploads": uploads,
        "caches": scratch + browser_cache + previews,
        "browser": browser - browser_cache,
        "tools": tools,
        "other": (hermes - scratch - tools) + (home - browser - previews),
    }
    return {**parts, "total": sum(parts.values())}


def _remove_old(folder: Path, cutoff: float) -> int:
    """Delete files older than `cutoff` under `folder`, then the directories they leave empty."""
    freed = 0
    if not folder.is_dir(): return 0
    for current, dirs, files in os.walk(folder, topdown=False):
        for name in files:
            path = os.path.join(current, name)
            try:
                stat = os.lstat(path)
                if stat.st_mtime < cutoff: os.unlink(path); freed += stat.st_size
            except OSError: pass
        if current != str(folder):
            try: os.rmdir(current)
            except OSError: pass
    return freed


def expire_uploads(store, agent_id: str, root: Path, now: float) -> int:
    """Attachments older than UPLOAD_DAYS, with their records; old messages then show them as removed."""
    cutoff, freed = now - UPLOAD_DAYS * DAY, 0
    uploads = root / "workspace" / "uploads"
    rows = {row["path"].split("/")[1]: row for row in store.all("uploads") if row.get("agent_id") == agent_id and row.get("path", "").count("/") >= 2}
    if uploads.is_dir():
        for folder in uploads.iterdir():
            try: newest = max([p.lstat().st_mtime for p in folder.rglob("*")] or [folder.lstat().st_mtime])
            except OSError: continue
            if newest >= cutoff: continue
            freed += size(folder)
            shutil.rmtree(folder, ignore_errors=True)
            if row := rows.get(folder.name): store.remove("uploads", row["id"])
    return freed


async def clear_browser_cache(computers, agent_id: str, root: Path, running: bool) -> int:
    """Drop the bot's browser caches past the limit: through the live browser if it runs, else from disk."""
    profile = root / "home" / "chrome-profile"
    before = sum(size(profile / name) for name in BROWSER_CACHES)
    if before < BROWSER_CACHE_LIMIT: return 0
    if running:
        code, _, _ = await docker("exec", computers.name(agent_id), "node", "--input-type=module", "-e", CLEAR_LIVE_CACHE, check=False, timeout=30)
        if code: log.info("Live browser cache for %s was not cleared (exit %s)", agent_id, code)
        # The browser empties its cache in the background after acknowledging the call.
        for _ in range(10 if not code else 0):
            await asyncio.sleep(1)
            if await asyncio.to_thread(lambda: sum(size(profile / name) for name in BROWSER_CACHES)) < before * 0.8: break
    else:
        await asyncio.to_thread(lambda: [shutil.rmtree(profile / name, ignore_errors=True) for name in BROWSER_CACHES])
    return max(0, before - await asyncio.to_thread(lambda: sum(size(profile / name) for name in BROWSER_CACHES)))


async def clean_bot(store, computers, agent_id: str, now: float | None = None) -> dict:
    """One bot's cleanup. Callers make sure the bot is not in the middle of a task."""
    now = now or time.time()
    root = store.agent_directory(agent_id)
    scratch = await asyncio.to_thread(_remove_old, root / "hermes" / "cache" / "scratch", now - SCRATCH_DAYS * DAY)
    scratch += await asyncio.to_thread(_remove_old, root / PREVIEWS, now - PREVIEW_DAYS * DAY)
    uploads = await asyncio.to_thread(expire_uploads, store, agent_id, root, now)
    try: item = await computers.inspect(agent_id)
    except Exception: item = None
    browser = await clear_browser_cache(computers, agent_id, root, bool(item and item["State"]["Running"]))
    return {"scratch": scratch, "uploads": uploads, "browser": browser, "freed": scratch + uploads + browser}


async def daily(store, computers, runtime):
    """Once shortly after start, then every day; a bot that is working waits for the next round."""
    await asyncio.sleep(120)
    while True:
        for agent in store.all("agents"):
            if runtime.activity(agent["id"]).get("running"): continue
            try:
                result = await clean_bot(store, computers, agent["id"])
                if result["freed"]: log.info("Cleaned %.1f MB for %s", result["freed"] / 1048576, agent["name"])
            except Exception:
                log.exception("Cleanup failed for %s", agent["id"])
        try: model_usage.prune(store)
        except Exception: log.exception("Pruning model usage failed")
        await asyncio.sleep(DAY)
