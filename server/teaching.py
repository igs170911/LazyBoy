"""Teach a bot by demonstration: the human works on the bot's own computer while
recorders in the box write semantic steps; the bot then turns them into a skill."""
import asyncio
import contextlib
import json
import logging
import os
import re
import shutil
import time
from urllib.parse import unquote_plus, urlsplit, urlunsplit

from . import computers as computers_module
from .store import now, uid

log = logging.getLogger("lazyhermes.teaching")
MAX_STEPS = 300
SECRET_PARAM = re.compile(r"pass|pwd|otp|token|secret|cvv|cvc|auth|session|code", re.I)
ROLE_ZH = {"button": "按鈕", "link": "連結", "textbox": "文字框", "searchbox": "搜尋框", "combobox": "下拉選單",
           "checkbox": "勾選框", "radio": "選項", "switch": "開關", "tab": "分頁標籤", "menuitem": "選單項目",
           "menu": "選單", "option": "選項", "img": "圖片", "heading": "標題", "listitem": "清單項目",
           "cell": "表格欄位", "password": "密碼欄", "toggle": "切換按鈕", "treeitem": "項目", "file": "檔案欄位"}
# A step that changes the page owns the navigation it caused.
CAUSES = {"click", "submit", "key", "select"}


def scrub_url(url):
    if not url or "?" not in url: return url
    parts = urlsplit(url)
    query = "&".join(part.split("=", 1)[0] + "=••••" if SECRET_PARAM.search(unquote_plus(part.split("=", 1)[0])) else part for part in parts.query.split("&"))
    return urlunsplit(parts._replace(query=query))


def target_text(target):
    if not target: return ""
    name = target.get("name") or target.get("label") or ""
    role = ROLE_ZH.get(target.get("role", ""), "")
    if name: return f"{role}「{name}」"
    return role or target.get("tag", "")


def describe(step):
    kind, target = step["kind"], step.get("target")
    where = f"在「{step['window']}」" if step.get("source") == "desktop" and step.get("window") else ""
    if kind == "click":
        checked = step.get("checked")
        verb = "勾選" if checked is True else "取消勾選" if checked is False else "點擊"
        text = f"{where}{verb}{target_text(target) or '畫面上的位置'}"
    elif kind == "right_click": text = f"{where}右鍵點擊{target_text(target) or '畫面上的位置'}"
    elif kind == "input":
        # Autocomplete search boxes announce themselves as comboboxes; to a person they are text fields.
        field = {**target, "role": "textbox"} if target and target.get("role") == "combobox" else target
        text = f"{where}在{target_text(field) or '輸入框'}輸入「{step.get('value', '')}」"
    elif kind == "select": text = f"在{target_text(target) or '下拉選單'}選擇「{step.get('value', '')}」"
    elif kind == "upload": text = f"在{target_text(target) or '檔案欄位'}上傳「{step.get('value', '')}」"
    elif kind == "submit": text = "送出表單"
    elif kind == "key": text = f"{where}按 {step.get('key', '')}"
    elif kind == "navigate": text = f"{'在新分頁' if step.get('new_tab') else ''}前往「{step.get('title') or step.get('url', '')}」"
    elif kind == "tab": text = "開新分頁"
    elif kind == "note": text = f"使用者補充：{step.get('value', '')}"
    else: text = kind
    if step.get("result"): text += f" → 進入「{step['result'].get('title') or step['result'].get('url', '')}」"
    return text


def read_rows(folder):
    rows = []
    for name in ("browser.jsonl", "desktop.jsonl"):
        path = folder / name
        if not path.exists(): continue
        for line in path.read_text(errors="replace").splitlines():
            with contextlib.suppress(json.JSONDecodeError):
                row = json.loads(line)
                if isinstance(row, dict) and isinstance(row.get("at"), (int, float)) and row.get("kind"): rows.append(row)
    return rows


def same_field(a, b):
    ta, tb = a.get("target") or {}, b.get("target") or {}
    return a.get("source") == b.get("source") and (ta.get("selector"), ta.get("name"), a.get("window")) == (tb.get("selector"), tb.get("name"), b.get("window"))


def normalize(rows, notes=(), dropped=()):
    """Raw recorder rows -> the steps a person would describe. Ids come from each step's
    first row, so they stay stable while more rows arrive during the recording."""
    rows = sorted([*rows, *({"at": n["at"], "source": "user", "kind": "note", "value": n["text"]} for n in notes)], key=lambda r: r["at"])
    steps = []
    for row in rows:
        step = {**row, "id": f"{row.get('source', 'x')[0]}{int(row['at'])}{row['kind'][0]}"}
        if step.get("secret"): step["value"] = "••••"
        step["url"] = scrub_url(step.get("url"))
        last = steps[-1] if steps else None
        gap = step["at"] - last["at"] if last else 1e9
        if last and step["kind"] == "input" and last["kind"] == "input" and same_field(last, step):
            last.update(value=step["value"], shot=step.get("shot") or last.get("shot"), at_last=step["at"]); continue
        if last and step["kind"] == "submit" and last["kind"] in {"click", "key"} and gap < 1500:
            if last["kind"] == "key": last.update(kind="submit")
            continue
        if step["kind"] == "navigate":
            if last and last["kind"] == "navigate" and gap < 2500 and not last.get("result"):
                last.update(url=step["url"], title=step.get("title"), shot=step.get("shot") or last.get("shot")); continue
            if last and last["kind"] == "tab" and gap < 4000:
                last.update(kind="navigate", new_tab=True, url=step["url"], title=step.get("title"), shot=step.get("shot") or last.get("shot")); continue
            owner = next((s for s in reversed(steps[-3:]) if s["kind"] in CAUSES and step["at"] - s.get("at_last", s["at"]) < 5000), None)
            if owner:
                owner["result"] = {"url": step["url"], "title": step.get("title"), "shot": step.get("shot")}; continue
        steps.append(step)
    dropped = set(dropped)
    out = []
    for step in steps:
        if step["id"] in dropped: continue
        step["text"] = describe(step)
        out.append({k: step.get(k) for k in ("id", "at", "source", "kind", "text", "value", "key", "shot", "url", "title", "window", "target", "point", "screen", "result")
                    if step.get(k) is not None})
    return out[:MAX_STEPS]


def lesson_prompt(lesson):
    steps = lesson.get("steps") or []
    base = f"/workspace/lessons/{lesson['id']}"
    lines = []
    for i, step in enumerate(steps, 1):
        line = f"{i}. [{ {'browser': '瀏覽器', 'desktop': '桌面', 'user': '說明'}.get(step['source'], step['source']) }] {step['text']}"
        target = step.get("target") or {}
        detail = [f"網址 {step['url']}" if step.get("url") and step["kind"] != "note" else "",
                  f"定位 {target['selector']}" if target.get("selector") else "",
                  f"截圖 {base}/{step['shot']}" if step.get("shot") else ""]
        detail = "；".join(d for d in detail if d)
        lines.append(line + (f"（{detail}）" if detail else ""))
    trial = ("5. 試跑：照你寫好的技能，用示範裡的參數值（或使用者回答的值）在這台電腦上實際做一次。"
             "做不通就修正技能再試。遇到付款、送出訂單、刪除、寄出這類無法復原的動作時，停在那一步前面說明，不要按下去。") if lesson.get("trial", True) else \
            "5. 這次不要試跑，使用者選擇略過。"
    return "\n".join([
        "使用者剛剛在你的電腦上親自示範了一件事。請把它學成一個以後可以重複使用的技能。",
        f"使用者說要教的事：{lesson.get('goal') or '（沒有說明，請從步驟推論）'}",
        f"示範紀錄（共 {len(steps)} 步，依時間排列；完整資料在 {base}/steps.json，截圖可以直接讀圖）：",
        *lines,
        "",
        "請依序完成：",
        "1. 用一兩句話說出你理解的目的。",
        "2. 找出每次會不一樣的參數（例如日期、關鍵字、檔名、數量），以及固定不變的部分。示範中的密碼不會出現在紀錄裡；需要登入時沿用電腦裡已登入的狀態，不行就請使用者接手。",
        "3. 有不確定而且會影響做法的地方，用 clarify 一次問清楚，最多 3 題；從示範就能判斷的不要問。",
        "4. 用 skill_manage 建立技能。name 用小寫英文加連字號；description 用中文寫清楚什麼時候該用；"
        f"frontmatter 加上 metadata: {{lazyboy: {{lesson: {lesson['id']}}}}}。"
        "內容要有：參數、步驟（瀏覽器操作用 browser 工具依角色與文字找元素，不要依賴螢幕座標）、每一步做完應該看到什麼、常見例外怎麼處理。",
        trial,
        "6. 最後回報：技能名稱、參數、試跑結果，以及使用者以後可以怎麼叫你做這件事。",
    ])


def key_shots(lesson, limit=8):
    """Screenshots worth sending to a vision model: what was clicked or chosen, spread over the demo."""
    shots = [s["shot"] for s in lesson.get("steps") or [] if s.get("shot") and s["kind"] in {"click", "select", "submit", "input", "navigate", "key"}]
    shots = list(dict.fromkeys(shots))
    if len(shots) > limit: shots = [shots[round(i * (len(shots) - 1) / (limit - 1))] for i in range(limit)]
    return [f"/workspace/lessons/{lesson['id']}/{s}" for s in shots]


class Teaching:
    START_TIMEOUT = 15
    STOP_TIMEOUT = 6

    def __init__(self, store, runtime, computers):
        self.store, self.runtime, self.computers = store, runtime, computers
        self.watchers = {}
        self.ending = set()

    def folder(self, id, lesson_id):
        return self.store.agent_directory(id) / "workspace" / "lessons" / lesson_id

    def current(self, id):
        lesson_id = self.runtime.teaching.get(id)
        return self.store.get("lessons", lesson_id) if lesson_id else None

    def view(self, lesson):
        if not lesson: return None
        steps = lesson.get("steps")
        if steps is None and lesson["state"] in {"recording", "paused"}:
            steps = normalize(read_rows(self.folder(lesson["agent_id"], lesson["id"])), lesson.get("notes", []), lesson.get("dropped", []))
        return {k: lesson.get(k) for k in ("id", "agent_id", "goal", "trial", "state", "started_at", "ended_at", "task_id", "thread_id")} | {
            "steps": steps or [], "base_url": f"/api/agents/{lesson['agent_id']}/files/content?path=" + f"lessons/{lesson['id']}/"}

    async def exec(self, id, *args, detach=False, timeout=20):
        return await computers_module.docker("exec", *(["-d"] if detach else []), "-u", f"{os.getuid()}:{os.getgid()}", "-e", "DISPLAY=:1",
                                             self.computers.name(id), *args, timeout=timeout)

    async def start(self, id, goal="", trial=True, thread=None):
        if self.runtime.teaching.get(id): return self.view(self.current(id))
        if self.runtime.busy(id): raise ValueError("Bot 正在工作，請等它完成或先停止，再開始教學")
        lesson_id = uid()
        # Held from here: starting the computer can take a minute, and queued work must not begin meanwhile.
        self.runtime.teaching[id] = lesson_id
        try:
            status = await self.computers.status(id, refresh=True)
            if not status.get("ready"): await self.computers.start(id)
            code, _, _ = await computers_module.docker("exec", self.computers.name(id), "test", "-x", "/usr/local/bin/box-teach-desktop", check=False, timeout=10)
            if code: raise ValueError("這台電腦的環境還不支援教學，請先在電腦區按「更新電腦環境」")
        except BaseException:
            self.runtime.teaching.pop(id, None)
            self.runtime.kick(id)
            raise
        folder = self.folder(id, lesson_id)
        (folder / "shots").mkdir(parents=True)
        lesson = {"id": lesson_id, "agent_id": id, "kind": "demo", "goal": str(goal or "").strip()[:200], "trial": bool(trial),
                  "thread_id": thread, "state": "recording", "started_at": now(), "notes": [], "dropped": []}
        self.store.put("lessons", lesson_id, lesson)
        inside = f"/workspace/lessons/{lesson_id}"
        try:
            await self.exec(id, "node", "/usr/local/bin/box-teach-browser.mjs", inside, detach=True)
            await self.exec(id, "/usr/bin/python3", "/usr/local/bin/box-teach-desktop", inside, detach=True)
            deadline = time.monotonic() + self.START_TIMEOUT
            while not (folder / ".ready-browser").exists():
                if time.monotonic() > deadline: raise RuntimeError("瀏覽器紀錄沒有啟動，請確認電腦的瀏覽器有開著")
                await asyncio.sleep(.25)
            if not (folder / ".ready-desktop").exists(): log.warning("Desktop recorder not ready for %s", lesson_id)
        except BaseException:
            await self.stop_recorders(id, folder)
            self.runtime.teaching.pop(id, None)
            self.store.remove("lessons", lesson_id)
            await asyncio.to_thread(shutil.rmtree, folder, True)
            self.runtime.kick(id)
            raise
        self.watchers[id] = asyncio.create_task(self.watch(id, lesson_id))
        self.announce(id)
        return self.view(lesson)

    async def watch(self, id, lesson_id):
        """Push the growing step list while the human demonstrates."""
        folder, seen = self.folder(id, lesson_id), None
        while self.runtime.teaching.get(id) == lesson_id:
            stamp = tuple((folder / n).stat().st_size if (folder / n).exists() else 0 for n in ("browser.jsonl", "desktop.jsonl"))
            if stamp != seen:
                seen = stamp; self.announce(id)
            await asyncio.sleep(1)

    def announce(self, id):
        lesson = self.current(id)
        self.runtime.emit(id, "teaching", {"lesson": self.view(lesson) if lesson else None})

    def change(self, id, lesson_id, **fields):
        lesson = self.store.get("lessons", lesson_id)
        if not lesson or lesson["agent_id"] != id or self.runtime.teaching.get(id) != lesson_id: raise ValueError("這次教學已經結束")
        self.store.patch("lessons", lesson_id, **fields)
        self.announce(id)
        return self.view(self.store.get("lessons", lesson_id))

    def pause(self, id, lesson_id, paused):
        flag = self.folder(id, lesson_id) / ".paused"
        flag.touch() if paused else flag.unlink(missing_ok=True)
        return self.change(id, lesson_id, state="paused" if paused else "recording")

    def note(self, id, lesson_id, text):
        text = str(text or "").strip()[:500]
        if not text: raise ValueError("請輸入補充說明")
        lesson = self.store.get("lessons", lesson_id) or {}
        return self.change(id, lesson_id, notes=[*lesson.get("notes", []), {"at": int(time.time() * 1000), "text": text}])

    def drop(self, id, lesson_id, step):
        lesson = self.store.get("lessons", lesson_id) or {}
        return self.change(id, lesson_id, dropped=[*lesson.get("dropped", []), str(step)])

    async def stop_recorders(self, id, folder):
        (folder / ".stop").touch()
        deadline = time.monotonic() + self.STOP_TIMEOUT
        while time.monotonic() < deadline and not ((folder / ".done-browser").exists() and (folder / ".done-desktop").exists()):
            await asyncio.sleep(.2)
        with contextlib.suppress(Exception):
            await self.exec(id, "pkill", "-f", f"box-teach-.*{folder.name}", timeout=8)

    async def end(self, id, lesson_id):
        lesson = self.store.get("lessons", lesson_id)
        if not lesson or lesson["agent_id"] != id or self.runtime.teaching.get(id) != lesson_id or lesson_id in self.ending: raise ValueError("這次教學已經結束")
        # Stopping the recorders takes seconds; a second finish or cancel meanwhile must not run twice.
        self.ending.add(lesson_id)
        watcher = self.watchers.pop(id, None)
        if watcher: watcher.cancel()
        folder = self.folder(id, lesson_id)
        await self.stop_recorders(id, folder)
        return lesson, folder

    async def finish(self, id, lesson_id):
        lesson = self.store.get("lessons", lesson_id) or {}
        folder = self.folder(id, lesson_id)
        # Typing still waiting for its debounce is flushed when the recorders stop; anything else is on disk.
        if not (folder / "browser.jsonl").exists() and not (folder / "desktop.jsonl").exists():
            raise ValueError("還沒有記錄到任何操作。請先在電腦上示範，或取消這次教學")
        lesson, folder = await self.end(id, lesson_id)
        self.ending.discard(lesson_id)
        steps = normalize(read_rows(folder), lesson.get("notes", []), lesson.get("dropped", []))
        (folder / "steps.json").write_text(json.dumps({"goal": lesson.get("goal"), "steps": steps}, ensure_ascii=False, indent=2))
        self.store.patch("lessons", lesson_id, state="finished", ended_at=now(), steps=steps)
        self.runtime.teaching.pop(id, None)
        self.announce(id)
        goal = lesson.get("goal")
        text = f"我示範了「{goal}」，共 {len(steps)} 步，請學起來" if goal else f"我示範了一件事，共 {len(steps)} 步，請學起來"
        result = await self.runtime.submit(id, text, lesson=lesson_id, thread=lesson.get("thread_id"))
        self.store.patch("lessons", lesson_id, task_id=result.get("queued"))
        self.runtime.kick(id)
        return {"lesson": self.view(self.store.get("lessons", lesson_id)), **result}

    async def cancel(self, id, lesson_id):
        _, folder = await self.end(id, lesson_id)
        self.ending.discard(lesson_id)
        self.runtime.teaching.pop(id, None)
        self.store.remove("lessons", lesson_id)
        await asyncio.to_thread(shutil.rmtree, folder, True)
        self.announce(id)
        self.runtime.kick(id)
        return {"lesson": None, "cancelled": lesson_id}

    def recover(self):
        """A restart loses the recorders' supervisor; unfinished demonstrations are discarded."""
        for lesson in self.store.all("lessons"):
            if lesson.get("kind") == "demo" and lesson.get("state") in {"recording", "paused"}:
                folder = self.folder(lesson["agent_id"], lesson["id"])
                with contextlib.suppress(OSError): (folder / ".stop").touch()
                self.store.patch("lessons", lesson["id"], state="cancelled", ended_at=now())
