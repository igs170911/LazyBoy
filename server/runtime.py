"""Durable product task admission and a single native Hermes executor per bot."""
import asyncio
import contextlib
import copy
from datetime import datetime, timezone
import hashlib
import json
import logging
import mimetypes
from pathlib import Path
import re
import time

from .rpc import HermesRPC
from .routing import Router
from .files import important, rank, walk
from .skills import Skills
from .model_check import friendly
from .store import now, uid
from .teaching import key_shots, lesson_prompt
from .collab import is_silent

# A follow-up that points back at a picture: re-attach the thread's recent uploads.
PHOTO_WORDS = re.compile(r"照片|圖片|相片|截圖|這張|那張|張圖|photo|image|picture|screenshot", re.I)

log = logging.getLogger(__name__)
TERMINAL = {"terminal"}
WORKER_LIVE = {"queued", "running"}


def live_workers(task):
    return [w for w in task.get("background") or [] if w.get("status") in WORKER_LIVE]


STALLED_TEXT = "Bot 長時間沒有回應，已停止這次工作；檔案與對話都保留。可以再交辦一次或請它繼續。"


def _vault_place(params):
    parts=[]
    for key in ("display_name","site","origin","hint"):
        text=str(params.get(key) or "").strip()
        if text and text not in parts: parts.append(text)
    return " · ".join(parts)


_MD_LINK = re.compile(r"!?\[([^\]]*)\]\([^)]*\)")
_MD_MARKS = re.compile(r"(\*\*|__|~~|`+|^#{1,6}\s+|^\s*>\s?|^\s*(?:[-*+]|\d+\.)\s+(?:\[[ xX]\]\s+)?)", re.M)


def tool_failed(payload):
    """Hermes tool.complete carries the raw result, not an error flag; mirror its own failure test."""
    if payload.get("error"): return True
    result=payload.get("result")
    if isinstance(result,str):
        try: result=json.loads(result)
        except ValueError: return False
    if not isinstance(result,dict): return False
    if result.get("success") is False or result.get("ok") is False: return True
    error=result.get("error")
    if isinstance(error,str) and error.strip(): return True
    code=result.get("exit_code")
    return isinstance(code,int) and not isinstance(code,bool) and code!=0


def tool_sources(name, payload):
    """Web pages a tool actually read (web_extract) or found (web_search), for the answer's source list."""
    result=payload.get("result")
    if isinstance(result,str):
        try: result=json.loads(result)
        except ValueError: return []
    if not isinstance(result,dict): return []
    if name=="web_extract": rows,kind=result.get("results"),"read"
    elif name=="web_search": rows,kind=(result.get("data") or {}).get("web"),"search"
    else: return []
    found=[]
    for row in rows if isinstance(rows,list) else []:
        url=str(row.get("url") or "") if isinstance(row,dict) else ""
        if url.startswith(("http://","https://")) and not row.get("error"):
            found.append({"url":url,"title":plain_text(row.get("title") or "",120),"kind":kind})
    return found


def plain_text(markdown, limit=None):
    """One-line plain text for titles and previews, without Markdown markup."""
    text = _MD_MARKS.sub("", _MD_LINK.sub(r"\1", str(markdown or "")))
    text = re.sub(r"(?<![\w*])\*(?=\S)([^*\n]+?)\*(?!\w)", r"\1", text)
    text = " ".join(text.replace("|", " ").split())
    return text if limit is None or len(text) <= limit else text[:limit].rstrip() + "…"


class Runtime:
    def __init__(self, store, settings, computers):
        self.store, self.settings, self.computers = store, settings, computers
        self.clients = {}
        self.connect_locks = {}
        self.admission_locks = {}
        self.runners = {}
        self.active = {}
        self.finished = {}
        self.streams = {}
        self.narration = {}
        self.pending_requests = {}
        self.file_baselines = {}
        self.skill_baselines = {}
        self.provider_bindings = {}
        self.routes = {}
        self.steps = {}
        self.sources = {}
        self.seen = {}
        self.open_tools = {}
        self.stalled = {}
        self.persisted = {}
        self.flushes = {}
        self.listeners = {}
        self.adopted = {}
        self.worker_owner = {}
        # Bot id -> lesson id while the human demonstrates on its computer.
        self.teaching = {}
        self.router = Router(settings)
        self.computers.task_binding = lambda id: self.provider_bindings.get(self.active.get(id))
        self.closed = False
        # Set by Collaboration: routes answers between bots and group chats when a turn ends.
        self.collab = None

    def agent(self, id):
        value = self.store.get("agents", id)
        if not value: raise KeyError(id)
        return value

    def task(self, id):
        value = self.store.get("tasks", id)
        if not value: raise KeyError(id)
        return value

    def thread(self, id, thread_id=None):
        """A conversation thread of a bot. The main thread shares the bot id and predates threads."""
        agent = self.agent(id)
        if not thread_id or thread_id == id:
            return {"id": id, "agent_id": id, "main": True, "title": agent.get("main_thread_title") or "主要對話",
                    "title_source": "user" if agent.get("main_thread_title") else None,
                    "created_at": agent.get("created_at"), "updated_at": agent.get("created_at"), "hermes_session": agent.get("hermes_session")}
        value = self.store.get("threads", thread_id)
        if not value or value.get("agent_id") != id: raise KeyError(thread_id)
        return {**value, "main": False}

    def patch_thread(self, id, thread_id, **fields):
        if thread_id == id:
            mapped = {"main_thread_title" if key == "title" else key: value for key, value in fields.items() if key in {"title", "hermes_session", "live_session"}}
            if mapped: self.store.patch("agents", id, **mapped)
            return
        self.store.patch("threads", thread_id, **fields)

    @staticmethod
    def task_thread(task):
        return task.get("thread_id") or task["agent_id"]

    def activity(self, id):
        tasks = [t for t in self.store.all("tasks") if t["agent_id"] == id]
        working = [t for t in tasks if t["state"] not in {"terminal", "queued"}]
        queued = [t for t in tasks if t["state"] == "queued"]
        latest = tasks[-1] if tasks else None
        question = next((t.get("question") for t in working if t.get("question")), None)
        state = working[0]["state"] if working else "queued" if queued else "idle"
        background = [t for t in tasks if live_workers(t)]
        return {"state": state, "teaching": self.teaching.get(id), "background": sum(len(live_workers(t)) for t in background),
                "background_task_ids": [t["id"] for t in background], "running": bool(working), "queued": bool(queued), "observed_at_ms": int(time.time()*1000),
                "active_task_ids": [t["id"] for t in working], "queued_task_ids": [t["id"] for t in queued],
                "thread_id": self.task_thread(working[0]) if working else None,
                "room_id": working[0].get("room_id") if working else None,
                "queued_thread_ids": list(dict.fromkeys(self.task_thread(t) for t in queued)),
                "latest_task_id": latest["id"] if latest else None, "question": question, "questions": [question] if question else []}

    def busy(self, id):
        """A turn, delegated background work or a demonstration is using the bot: its connection and computer must stay."""
        activity = self.activity(id)
        return activity["running"] or activity["background"] > 0 or bool(activity["teaching"])

    def emit(self, id, kind, payload, task=None):
        if task and "thread_id" not in payload:
            record = self.store.get("tasks", task)
            if record: payload = {**payload, "thread_id": self.task_thread(record)}
        value = self.store.event(id, kind, payload, task)
        for wake in self.listeners.get(id, ()): wake.set()
        return value

    @contextlib.contextmanager
    def listen(self, id):
        """An asyncio.Event set whenever this bot journals an event, so streams need not poll."""
        wake = asyncio.Event()
        self.listeners.setdefault(id, set()).add(wake)
        try: yield wake
        finally: self.listeners[id].discard(wake)

    def message(self, agent_id, task_id, role, content, **fields):
        thread = self.task_thread(self.task(task_id)) if task_id else agent_id
        return self.store.message(agent_id, role, content, thread=thread, **fields)

    async def client(self, id):
        async with self.connect_locks.setdefault(id, asyncio.Lock()):
            client = self.clients.get(id)
            if client and client.alive: return client
            status = await self.computers.start(id)
            client = HermesRPC(lambda e: self.event(id,e), lambda f: self.human_request(id,f), lambda error: self.disconnected(id,error))
            await client.open(status["api_port"], self.computers.key(id))
            self.clients[id] = client
            return client

    @staticmethod
    def _client_cache(client, name):
        cache = getattr(client, name, None)
        if cache is None:
            cache = {}; setattr(client, name, cache)
        return cache

    def cached_session(self, id, client, thread_id):
        if not client: return None
        if thread_id == id: return getattr(client, "product_session", None)
        return self._client_cache(client, "product_sessions").get(thread_id)

    def forget_session(self, id, client, thread_id):
        if not client: return None
        sid = self.cached_session(id, client, thread_id)
        if thread_id == id: client.product_session = None
        else: self._client_cache(client, "product_sessions").pop(thread_id, None)
        return sid

    def session_thread(self, id, client, sid):
        if not client or not sid: return None
        if getattr(client, "product_session", None) == sid: return id
        return next((thread for thread, value in self._client_cache(client, "product_sessions").items() if value == sid), None)

    async def session(self, id, client, thread_id=None):
        """One native Hermes session per thread, all on the bot's single connection."""
        agent = self.agent(id)
        thread = self.thread(id, thread_id)
        thread_id = thread["id"]
        cached = self.cached_session(id, client, thread_id)
        if cached: return cached
        stored = thread.get("hermes_session")
        result = None
        if stored:
            with contextlib.suppress(RuntimeError):
                result = await client.call("session.resume", {"session_id": stored, "source": "cli"})
        if not result:
            _, model, _ = self.provider_bindings.get(self.active.get(id), self.settings.effective(agent))
            title = agent["name"] if thread["main"] else f"{agent['name']} · {thread['title']}"
            result = await client.call("session.create", {"cwd": "/workspace", "source": "cli", "model": model, "provider": "custom:product", "title": title})
        sid = result["session_id"]
        self.patch_thread(id, thread_id, hermes_session=result.get("stored_session_id") or stored or sid)
        # A connection-local cache, never a persisted Python object identity.
        if thread["main"]:
            self.store.patch("agents", id, live_session=sid)
            client.product_session = sid
        else:
            self._client_cache(client, "product_sessions")[thread_id] = sid
        return sid

    async def submit(self, id, text, request_id=None, *, force_queue=False, routine=None, thread=None, lesson=None, peer=None):
        async with self.admission_locks.setdefault(id, asyncio.Lock()):
            return await self._submit(id, text, request_id, force_queue=force_queue, routine=routine, thread=thread, lesson=lesson, peer=peer)

    async def _submit(self, id, text, request_id=None, *, force_queue=False, routine=None, thread=None, lesson=None, peer=None):
        thread_id = self.thread(id, thread)["id"]
        if not isinstance(text,str) or not text.strip(): raise ValueError("請輸入要做的工作")
        if len(text)>64000: raise ValueError("訊息最多 64000 字元")
        text=text.strip(); request_id=request_id or uid()
        previous = self.store.admission(id,request_id)
        if previous:
            if previous["payload"] != text: raise ValueError("同一訊息識別不能用於不同內容")
            return {"queued": previous["task"], "replayed": True}
        active = self.active.get(id)
        # Steering and answers only apply inside the thread that owns the running task;
        # other threads queue behind it because the bot has one computer.
        if active and not force_queue and not lesson and self.task_thread(self.task(active)) == thread_id:
            task = self.task(active)
            if task.get("question"):
                return await self.answer(id, task["question"]["question_id"], text=text)
            client = self.clients.get(id)
            if client and client.alive and task.get("session_id"):
                response = await client.call("session.steer", {"session_id": task["session_id"], "text": self._prompt(id,text)})
                if response.get("status") not in {"rejected", "failed"}:
                    self.store.admit(id,request_id,active,text)
                    self.store.message(id,"user",text,thread=thread_id)
                    self.touch_thread(id,thread_id,text)
                    self.emit(id,"user_message",{},active)
                    self.emit(id,"runtime",{"type":"steering","message":text},active)
                    return {"steered":active}
        task_id=uid()
        task={"id":task_id,"agent_id":id,"thread_id":thread_id,"goal":text,"state":"queued","verdict":None,"created_at":now(),
              "result":None,"artifacts":[],"milestones":[],"request_id":request_id,"routine_id":routine,**({"lesson_id":lesson} if lesson else {}),
              # A message from another bot or a group chat: who sent it, and where its answer goes.
              **{k:v for k,v in (peer or {}).items() if k!="peer_text"}}
        # Persist admission + task + user message together, before any executor starts.
        origin = {"routine_id": routine, "routine_title": (self.store.get("routines", routine) or {}).get("title", "")} if routine else {}
        if lesson: origin["lesson_id"] = lesson
        if peer:
            origin.update({k: peer[k] for k in ("from_agent","from_name","intent","message_id","room_id") if peer.get(k)})
            if peer.get("from_agent"): origin["event"] = "message_in"
        with self.store.transaction():
            self.store.admit(id,request_id,task_id,text)
            self.store.put("tasks",task_id,task)
            self.store.message(id,"user",(peer or {}).get("peer_text") or text,thread=thread_id,**origin)
            self.touch_thread(id,thread_id,text if not peer else "")
            self.emit(id,"user_message",{},task_id)
        self.kick(id)
        return {"queued":task_id}

    def touch_thread(self, id, thread_id, text=""):
        if thread_id == id: return
        thread = self.store.get("threads", thread_id) or {}
        fields = {"updated_at": now()}
        if not thread.get("title_source") and text:
            # Placeholder until Hermes names the session from the conversation.
            fields.update(title=plain_text(text, 24), title_source="first_message")
        self.store.patch("threads", thread_id, **fields)
        self.emit(id, "thread_updated", {"thread_id": thread_id})

    def kick(self,id):
        if self.closed: return
        current=self.runners.get(id)
        if not current or current.done(): self.runners[id]=asyncio.create_task(self._queue(id))

    def next_task(self,id):
        """The user's own messages go before queued messages from other bots (Grok Bot does the same)."""
        queued=[t for t in self.store.all("tasks") if t["agent_id"]==id and t["state"]=="queued"]
        return next((t for t in queued if not t.get("low_priority")),queued[0] if queued else None)

    async def _queue(self,id):
        while not self.closed:
            task=self.next_task(id)
            # The human is demonstrating on this computer; queued work starts when they finish.
            if not task or self.teaching.get(id): return
            turn=self.adopted.get(id)
            if turn and not turn.done():
                # Hermes is running a turn of its own (background results); queued work follows it.
                await asyncio.wait({turn}); continue
            task_id=task["id"]
            self.active[id]=task_id
            self.finished[task_id]=asyncio.Event()
            self.store.patch("tasks",task_id,state="running",started_at=now())
            self.emit(id,"m0.progress",{"message":"正在準備 Bot 的電腦與工作環境"},task_id)
            try:
                route = await self.router.select(self.agent(id), task["goal"], routine=bool(task.get("routine_id")), vision=bool(self._images(id, task["goal"], task)), emit=lambda kind, payload: self.emit(id, kind, payload, task_id))
                self.routes[task_id] = route
                provider, model, selected = route.primary()
                self.provider_bindings[task_id] = (provider, model, copy.deepcopy(selected))
                effort = self.settings.reasoning(selected, model)
                self.store.patch("tasks",task_id,provider=provider,model=model,protocol=self.settings.protocol(selected,model),reasoning_effort=effort,routing={"intent":route.intent,"role":route.role,"enabled":route.enabled})
                self.computers.prepare(id)
                client=await self.client(id)
                if id in self.computers.env_dirty:
                    # New search/xAI keys were written to .env after this Hermes started.
                    with contextlib.suppress(Exception):
                        await client.call("reload.env",{})
                        self.computers.env_dirty.discard(id)
                if id in self.computers.mcp_dirty:
                    # New product tools (e.g. message_bot) were written next to the config.
                    with contextlib.suppress(Exception):
                        await client.call("reload.mcp",{"confirm":True,"session_id":getattr(client,"product_session",None)})
                        self.computers.mcp_dirty.discard(id)
                sid=await self.session(id,client,self.task_thread(task))
                provider, model, selected = self.provider_bindings[task_id]
                binding = (provider, model, selected.get("base_url"), self.settings.protocol(selected, model), bool(self.settings.model_row(selected,model).get("vision")))
                # Model and effort are per native session, so each thread keeps its own record.
                configured = self._client_cache(client, "product_bindings")
                previous_binding, previous_effort = configured.get(sid, (None, None))
                binding_changed = previous_binding != binding
                if binding_changed:
                    # A resumed session builds its agent in the background from the runtime stored with it
                    # (including an outdated protocol). A switch sent with --provider does not wait for that
                    # build and is lost, so first wait for the agent: approval.pending is a read-only call
                    # Hermes only answers once the agent is ready.
                    # Hermes gives up after 30 s per call; a cold build with MCP discovery can take longer.
                    for attempt in range(3):
                        try:
                            await client.call("approval.pending", {"session_id": sid}, timeout=45)
                            break
                        except Exception as exc:
                            log.warning("Resumed agent not ready yet (attempt %s): %s", attempt + 1, exc)
                    await client.call("config.set", {"session_id": sid, "key": "model", "value": model + " --provider custom:product", "confirm_expensive_model": True})
                if binding_changed or previous_effort != effort:
                    await client.call("config.set", {"session_id": sid, "key": "reasoning", "value": effort})
                configured[sid] = (binding, effort)
                self.store.patch("tasks",task_id,session_id=sid)
                self.file_baselines[task_id] = {f["path"]: (f["bytes"], f["updated_at"]) for f in await self.refresh_files(id)}
                self.skill_baselines[id] = self.skill_state(id)
                text=self._prompt(id,task["goal"],task)
                if self.task(task_id)["state"] in {"stopping", "terminal"}:
                    self.finish(id,task_id,"cancelled","")
                    continue
                if self.settings.model_row(selected, model).get("vision"):
                    for path in self._images(id, task["goal"], task):
                        await client.call("image.attach", {"session_id": sid, "path": path})
                response = await client.call("prompt.submit",{"session_id":sid,"text":text})
                if response.get("stored_session_id"):
                    self.patch_thread(id, self.task_thread(task), hermes_session=response["stored_session_id"])
                await self.watch(id,task_id)
            except asyncio.CancelledError: raise
            except Exception as exc:
                log.exception("Bot task failed: %s",task_id)
                self.finish(id,task_id,"failed",f"工作未完成：{str(exc)[:1500]}")
            finally:
                self.release(id,task_id)

    def release(self,id,task_id):
        self.active.pop(id,None)
        self.finished.pop(task_id,None)
        self.streams.pop(task_id,None)
        self.narration.pop(task_id,None)
        self.pending_requests.pop(id,None)
        self.file_baselines.pop(task_id,None)
        self.provider_bindings.pop(task_id,None)
        self.routes.pop(task_id,None)
        for table in (self.steps,self.sources,self.seen,self.open_tools,self.stalled,self.persisted): table.pop(task_id,None)
        timer=self.flushes.pop(task_id,None)
        if timer: timer.cancel()
        self.store.prune_events(id)

    def adopt(self,id,sid):
        """A turn Hermes started itself, e.g. to report background delegate_task results once they finish.
        It gets a task of its own so the reply, steps and files reach the conversation like any other."""
        thread_id=self.session_thread(id,self.clients.get(id),sid)
        if not thread_id or self.active.get(id): return
        task_id=uid()
        self.store.put("tasks",task_id,{"id":task_id,"agent_id":id,"thread_id":thread_id,"kind":"background","goal":"背景工作完成，回報結果",
                                        "state":"running","verdict":None,"created_at":now(),"started_at":now(),"session_id":sid,
                                        "result":None,"artifacts":[],"milestones":[],"request_id":None,"routine_id":None})
        self.active[id]=task_id
        self.finished[task_id]=asyncio.Event()
        self.file_baselines[task_id]={f["path"]:(f["bytes"],f["updated_at"]) for f in self.store.get("files",id) or []}
        self.emit(id,"m0.progress",{"message":"背景工作完成，正在整理結果"},task_id)
        self.adopted[id]=asyncio.create_task(self._adopted(id,task_id))

    async def _adopted(self,id,task_id):
        try: await self.watch(id,task_id)
        except asyncio.CancelledError: raise
        except Exception as exc:
            log.exception("Background turn failed: %s",task_id)
            self.finish(id,task_id,"failed",f"工作未完成：{str(exc)[:1500]}")
        finally:
            self.release(id,task_id)
            self.kick(id)

    # A turn that streams nothing for this long is presumed hung; an open tool call (a long build or
    # download) gets more room. After an interrupt, or the user's stop, Hermes has STOP_GRACE to settle.
    PERSIST_INTERVAL = .5
    IDLE_LIMIT = 15*60
    TOOL_IDLE_LIMIT = 40*60
    STOP_GRACE = 60
    WATCH_INTERVAL = 5

    async def watch(self,id,task_id):
        done=self.finished[task_id]
        self.seen[task_id]=time.monotonic()
        while not done.is_set():
            with contextlib.suppress(TimeoutError):
                await asyncio.wait_for(done.wait(),self.WATCH_INTERVAL)
                return
            state=self.task(task_id)["state"]
            clock=time.monotonic()
            if state.startswith("waiting"):
                self.seen[task_id]=clock
                expires=(self.task(task_id).get("question") or {}).get("expires_at")
                if expires and expires<=datetime.now(timezone.utc).isoformat():
                    with contextlib.suppress(Exception): await self.expire_question(id,task_id)
                continue
            if state=="stopping" or task_id in self.stalled:
                since=self.stalled.setdefault(task_id,clock)
                if clock-since>=self.STOP_GRACE:
                    verdict="cancelled" if state=="stopping" else "failed"
                    self.finish(id,task_id,verdict,"" if verdict=="cancelled" else STALLED_TEXT)
                continue
            limit=self.TOOL_IDLE_LIMIT if self.open_tools.get(task_id) else self.IDLE_LIMIT
            if clock-self.seen[task_id]>=limit:
                log.warning("Bot task idle for %ss, interrupting: %s",int(clock-self.seen[task_id]),task_id)
                self.stalled[task_id]=clock
                self.emit(id,"m0.progress",{"message":"Bot 太久沒有回應，正在停止這次工作"},task_id)
                client=self.clients.get(id); sid=self.task(task_id).get("session_id")
                if client and client.alive and sid:
                    with contextlib.suppress(Exception): await client.call("session.interrupt",{"session_id":sid})

    def _uploads(self,id,text):
        return [v for v in self.store.all("uploads") if v["agent_id"]==id and (v["id"] in text or v["name"] in text or v["path"] in text)]

    def _lesson(self,task):
        lesson = self.store.get("lessons", task.get("lesson_id")) if task and task.get("lesson_id") else None
        return lesson if lesson and lesson["agent_id"]==task["agent_id"] else None

    def _recent_images(self,id,task,limit=3):
        """Images uploaded earlier in this thread, for a follow-up like 「你看我傳的照片」 that carries none itself."""
        if not task: return []
        recent=[m for m in self.store.transcript(id,self.task_thread(task)) if m.get("role")=="user"][-6:]
        found=[]
        for message in reversed(recent):
            found.extend("/workspace/"+v["path"] for v in self._uploads(id,message.get("content","")) if v["media_type"].startswith("image/"))
        return list(dict.fromkeys(found))[:limit]

    def _images(self,id,text,task=None):
        paths = ["/workspace/"+v["path"] for v in self._uploads(id,text) if v["media_type"].startswith("image/")]
        if not paths and PHOTO_WORDS.search(text): paths = self._recent_images(id,task)
        if lesson := self._lesson(task): paths.extend(key_shots(lesson))
        return list(dict.fromkeys(paths))

    def _prompt(self,id,text,task=None):
        if lesson := self._lesson(task): return lesson_prompt(lesson)
        if task and task.get("room_id") and self.collab: return self.collab.room_prompt(task)
        uploads=self._uploads(id,text)
        if uploads:
            text += "\n\n使用者附加的檔案已放在這台電腦的工作目錄：\n"+"\n".join("/workspace/"+u["path"] for u in uploads)
            if any(u["media_type"].startswith("image/") for u in uploads):
                text += "\n圖片也直接附在這則訊息裡；先看圖回答這則訊息。"
        elif PHOTO_WORDS.search(text) and (earlier := self._recent_images(id,task)):
            text += "\n\n使用者提到的圖片是這段對話稍早上傳的，已重新附上：\n"+"\n".join(earlier)
        return text

    def skill_state(self, id):
        return {s["id"]: hashlib.sha256(json.dumps({k:s[k] for k in ["name","description","instructions","enabled"]}, sort_keys=True).encode()).hexdigest() for s in Skills(self.store).list(id)}

    def finish(self,id,task_id,verdict,text,**fields):
        task=self.task(task_id)
        if task["state"]=="terminal": return
        content=text or self.streams.get(task_id,"") or ("已停止，先前檔案與對話已保留。" if verdict=="cancelled" else "工作已結束。")
        hint=friendly(content) if verdict=="failed" else None
        if hint: content=hint+"\n\n> 原始錯誤："+" ".join(content.split())[:600]
        silent=bool(task.get("from_agent") or task.get("room_id")) and verdict!="failed" and is_silent(content)
        self.message(id,task_id,"assistant",content,id="delta:"+task_id,streaming=False,narration=self.narration.get(task_id,[]),**({"silent":True} if silent else {}))
        steps=[{**step,"state":"done" if step["state"]=="running" else step["state"]} for step in self.steps.get(task_id,[])]
        trail={"steps":steps,"sources":self.sources.get(task_id,[])} if steps or self.sources.get(task_id) else {}
        self.store.patch("tasks",task_id,state="terminal",verdict=verdict,result={"summary":content},question=None,ended_at=now(),**trail,**fields)
        self.emit(id,"runtime",{"type":"turn_ended","verdict":verdict},task_id)
        self.emit(id,"reply",{},task_id)
        event=self.finished.get(task_id)
        if event: event.set()
        if self.collab: self.collab.finished(id,self.task(task_id),verdict,content)

    async def event(self,id,event):
        task_id=self.active.get(id)
        name=event.get("type","");payload=event.get("payload",{})
        if name=="session.title":
            self.native_title(id,event.get("session_id"),payload.get("title"))
            return
        if name.startswith("subagent."):
            self.track_worker(id,task_id,name,event.get("session_id"),payload)
            return
        if name=="message.start" and not task_id:
            self.adopt(id,event.get("session_id"))
            return
        if not task_id: return
        task=self.task(task_id)
        if task["state"] == "terminal": return
        sid=event.get("session_id")
        if task.get("session_id") and sid and sid!=task["session_id"]: return
        self.seen[task_id]=time.monotonic()
        if name=="message.delta":
            text=payload.get("text",payload.get("delta",""))
            if text:
                value=self.streams.get(task_id,"")+text;self.streams[task_id]=value
                # Live clients get every delta from the journal; the stored copy only serves reloads mid-reply.
                if time.monotonic()-self.persisted.get(task_id,0)>=self.PERSIST_INTERVAL:
                    self.persist_stream(id,task_id)
                elif task_id not in self.flushes:
                    # The tail of a burst must still reach storage if no further delta arrives.
                    self.flushes[task_id]=asyncio.get_running_loop().call_later(self.PERSIST_INTERVAL,self.persist_stream,id,task_id)
                self.emit(id,"runtime",{"type":"message_delta","id":task_id,"delta":text},task_id)
        elif name=="message.interim":
            self.emit(id,"runtime",{"type":"user_progress","message":payload.get("text","")},task_id)
        elif name=="message.complete":
            status=payload.get("status", "success")
            verdict="cancelled" if status in {"interrupted","cancelled","aborted"} or task["state"]=="stopping" else "failed" if status in {"error","failed"} else "answer"
            text=payload.get("text", "")
            if task_id in self.stalled and task["state"]!="stopping":
                # Our watchdog interrupted a silent turn: that is a failure, not the user's stop.
                verdict,text="failed",STALLED_TEXT
            await self.refresh_files(id,task_id)
            self.finish(id,task_id,verdict,text,usage=payload.get("usage"),runtime=payload.get("runtime"))
        elif name in {"tool.start","tool.complete"}:
            tool=payload.get("name","")
            if name == "tool.start": self.fold_narration(id,task_id)
            route = self.routes.get(task_id)
            if name == "tool.start" and route and tool not in {"clarify", "todo", "send_message", "report_progress"}:
                route.promote()
            failed=name=="tool.complete" and tool_failed(payload)
            self.track_step(task_id,name,tool,payload,failed)
            self.emit(id,"runtime",{"type":"tool_started" if name=="tool.start" else "tool_finished",
                                    "id":payload.get("tool_id"),"name":tool,"detail":str(payload.get("context", "")),
                                    "success":not failed,"duration":payload.get("duration_s"),
                                    "summary":payload.get("summary") or ""},task_id)
            if name == "tool.complete" and tool == "skill_manage":
                current = self.skill_state(id)
                previous = self.skill_baselines.get(id, {})
                changed = [key for key,value in current.items() if previous.get(key) != value]
                if current != previous:
                    self.emit(id, "skills_changed", {"count":len(current), "saved":bool(changed), "skill_ids":changed, "at":now()}, task_id)
                self.skill_baselines[id] = current
        elif name=="todo.updated":
            plan=[{"content":plain_text(t.get("content") or "",160),"status":str(t.get("status") or "pending")}
                  for t in payload.get("todos",[]) if isinstance(t,dict) and t.get("content")][:30]
            self.store.patch("tasks",task_id,plan=plan)
            self.emit(id,"runtime",{"type":"plan","plan":plan},task_id)
        elif name=="request.cancel":
            question=task.get("question")
            if question and question["question_id"]==str(payload.get("id")):
                self.store.patch("tasks",task_id,question=None,state="stopping" if task["state"]=="stopping" else "running")
                self.emit(id,"question_answered",{},task_id)
        elif name=="session.error":
            self.finish(id,task_id,"failed",payload.get("message") or payload.get("error") or "工作失敗")
        elif name in {"status","thinking.start","thinking.end","context.compacting"}:
            self.emit(id,"m0.progress",{"message":payload.get("message",payload.get("text","正在處理工作"))},task_id)

    def track_step(self,task_id,name,tool,payload,failed):
        """Keep the step trail server-side so a finished turn still shows it after a reload."""
        steps=self.steps.setdefault(task_id,[]); tool_id=str(payload.get("tool_id") or "")
        running=self.open_tools.setdefault(task_id,set())
        if name=="tool.start":
            running.add(tool_id)
            steps.append({"id":tool_id or uid(),"name":tool,"detail":plain_text(payload.get("context",""),140),"state":"running"})
            del steps[:-60]
            return
        running.discard(tool_id)
        step=next((s for s in reversed(steps) if s["id"]==tool_id or (not tool_id and s["name"]==tool and s["state"]=="running")),None)
        if step:
            step["state"]="failed" if failed else "done"
            if failed and payload.get("summary"): step["detail"]=plain_text(payload["summary"],140)
            if isinstance(payload.get("duration_s"),(int,float)): step["duration"]=round(payload["duration_s"],1)
        if not failed:
            known=self.sources.setdefault(task_id,[])
            for source in tool_sources(tool,payload):
                match=next((s for s in known if s["url"]==source["url"]),None)
                if match: match["kind"]="read" if "read" in (match["kind"],source["kind"]) else match["kind"]
                elif len(known)<40: known.append(source)
            known.sort(key=lambda s:s["kind"]!="read")

    def track_worker(self,id,task_id,name,sid,payload):
        """delegate_task children, kept on the task that spawned them. Background ones outlive that turn,
        so the conversation can show they are still working and the stop button can reach them."""
        if (payload.get("depth") or 1)>1: return
        goal=plain_text(payload.get("goal") or "",160)
        key=f"{payload.get('delegation_id') or ''}:{payload.get('task_index',0)}:{goal[:80]}"
        owner=self.worker_owner.get(key)
        if not owner:
            current=self.store.get("tasks",task_id) if task_id else None
            owner=task_id if current and current.get("session_id") in {None,sid} else self.session_task(id,sid)
            if not owner: return
            self.worker_owner[key]=owner
        task=self.store.get("tasks",owner)
        if not task: return
        workers=[dict(w) for w in task.get("background") or []]
        worker=next((w for w in workers if w["id"]==key),None)
        if not worker:
            worker={"id":key,"goal":goal,"status":"queued","background":bool(payload.get("delegation_id")),"started_at":now()}
            workers.append(worker); del workers[:-12]
        status=str(payload.get("status") or "")
        if name=="subagent.complete":
            worker["status"]="completed" if status in {"","completed"} else "interrupted" if status=="interrupted" else "failed"
            if isinstance(payload.get("duration_seconds"),(int,float)): worker["duration"]=round(payload["duration_seconds"],1)
            if payload.get("summary"): worker["summary"]=plain_text(payload["summary"],200)
            worker.pop("tool",None)
        elif name!="subagent.spawn_requested" and worker["status"] in WORKER_LIVE:
            if worker["status"]=="queued": worker.update(status="running",started_at=now())
            if name=="subagent.tool" and payload.get("tool_name"):
                worker["tool"]=str(payload["tool_name"]); worker["detail"]=plain_text(payload.get("tool_preview") or payload.get("text") or "",120)
        worker["at"]=now()
        self.store.patch("tasks",owner,background=workers)
        self.emit(id,"runtime",{"type":"workers","workers":workers},owner)

    def session_task(self,id,sid):
        """The latest task that ran in this native session."""
        return next((t["id"] for t in reversed(self.store.all("tasks")) if t["agent_id"]==id and sid and t.get("session_id")==sid),None)

    def lose_workers(self,id,status="lost"):
        """Children we can no longer follow (connection or service gone) must not show as running forever."""
        for task in self.store.all("tasks"):
            if task["agent_id"]!=id or not live_workers(task): continue
            workers=[{**w,"status":status,"at":now()} if w["status"] in WORKER_LIVE else w for w in task["background"]]
            self.store.patch("tasks",task["id"],background=workers)
            self.emit(id,"runtime",{"type":"workers","workers":workers},task["id"])

    def persist_stream(self,id,task_id):
        """Store the reply streamed so far, for a page loaded mid-reply."""
        timer=self.flushes.pop(task_id,None)
        if timer: timer.cancel()
        if task_id not in self.streams or self.store.get("tasks",task_id,{}).get("state")=="terminal": return
        self.persisted[task_id]=time.monotonic()
        self.message(id,task_id,"assistant",self.streams[task_id],id="delta:"+task_id,streaming=True,narration=self.narration.get(task_id,[]))

    def fold_narration(self,id,task_id):
        """Text streamed before a tool call is progress narration; only the text after the last tool is the reply."""
        text=self.streams.pop(task_id,"").strip()
        self.persisted.pop(task_id,None)  # the reply restarts: store its first words at once
        if not text: return
        notes=self.narration.setdefault(task_id,[]);notes.append(text)
        self.message(id,task_id,"assistant","",id="delta:"+task_id,streaming=True,narration=notes)
        self.emit(id,"runtime",{"type":"narration","id":task_id,"text":text},task_id)

    def native_title(self,id,sid,title):
        """Adopt Hermes' auto-title for a side thread unless the user already named it."""
        title=plain_text(title)[:80]
        thread_id=self.session_thread(id,self.clients.get(id),sid)
        if not title or not thread_id or thread_id==id: return
        thread=self.store.get("threads",thread_id)
        if not thread or thread.get("title_source")=="user": return
        self.store.patch("threads",thread_id,title=title,title_source="hermes")
        self.emit(id,"thread_updated",{"thread_id":thread_id})

    async def human_request(self,id,frame):
        task_id=self.active.get(id)
        client=self.clients.get(id)
        method=frame["method"];params=frame.get("params",{});rid=str(frame["id"])
        if method not in {"clarify","approval","secret","sudo","vault.code","vault.unlock_prompt","vault.save_login","connection"} or not task_id:
            if client and client.alive:
                await client.socket.send(json.dumps({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32601,"message":"This web host does not expose this display surface; use browser_exec or the computer MCP tools."}}))
            return
        place=_vault_place(params) if method in {"vault.save_login","vault.code","vault.unlock_prompt"} else ""
        base=params.get("prompt") or params.get("description") or params.get("command") or ""
        shown=f"{base}\n{place}" if place and base and place not in base else (place or base or "需要你的回覆")
        question={"question_id":rid,"task_id":task_id,"kind":"hermes","method":method,"prompt":shown, "options":[]}
        if place: question["question"]=place
        if self.unattended(self.task(task_id)):
            # Work the user did not start (a teammate's message, a routine): its question expires, as in Grok Bot.
            question["expires_at"]=datetime.fromtimestamp(time.time()+self.QUESTION_TTL,timezone.utc).isoformat()
        if method=="clarify":
            questions=params.get("questions",[])
            question["hermes_questions"]=questions
            question["prompt"]="\n\n".join(q.get("question","") for q in questions)
            if len(questions)==1: question["options"]=questions[0].get("choices",[])
        elif method=="approval":
            # Hermes offers "always" unless the command may never be allow-listed; then it is once or deny.
            question["options"]=["允許這次","一律允許","拒絕"] if "always" in (params.get("choices") or []) else ["允許這次","拒絕"]
        elif method == "vault.save_login":
            question.update(kind="secret",fields=[{"name":"identifier","label":"帳號","kind":"text"},{"name":"password","label":"密碼","kind":"password"}])
        elif method == "vault.code":
            question.update(kind="secret",fields=[{"name":"value","label":params.get("hint") or params.get("site") or params.get("prompt") or "驗證碼","kind":"password"}])
        elif method == "vault.unlock_prompt":
            question.update(kind="secret",fields=[{"name":"value","label":params.get("display_name") or params.get("prompt") or "解鎖密碼","kind":"password"}])
        elif method in {"secret","sudo"}:
            question.update(kind="secret",fields=[{"name":"value","label":params.get("prompt") or params.get("env_var") or "秘密輸入","kind":"password"}])
        self.pending_requests[id]=frame
        self.store.patch("tasks",task_id,state="waiting_approval" if method=="approval" else "waiting_input",question=question)
        self.emit(id,"runtime",{"type":"question","question":question},task_id)

    QUESTION_TTL = 600

    @staticmethod
    def unattended(task):
        """A turn nobody is watching: started by another bot, a routine, a bot's hand-off in a group, or Hermes itself."""
        return bool(task.get("from_agent") or task.get("routine_id") or task.get("kind")=="background"
                    or (task.get("room_id") and task.get("low_priority")))

    async def expire_question(self,id,task_id):
        """Nobody answered in time: deny an approval, tell the bot the user did not reply, and let the turn finish."""
        question=(self.store.get("tasks",task_id) or {}).get("question")
        frame=self.pending_requests.get(id)
        if not question or not frame or str(frame["id"])!=str(question["question_id"]): return
        method=frame["method"]
        silence="（使用者 10 分鐘內沒有回應。不要猜答案；停下需要這個答案或授權的部分，回報目前進度與需要使用者決定的事。）"
        if method=="clarify":
            questions=frame.get("params",{}).get("questions",[])
            await self.answer(id,frame["id"],answers={q["qid"]:silence for q in questions},note="提問已過期：10 分鐘內沒有回覆")
        elif method=="approval":
            await self.answer(id,frame["id"],choice="deny",note="審批已過期：10 分鐘內沒有人允許，已自動拒絕")
        else:
            await self.answer(id,frame["id"],value="",note="請求已過期：10 分鐘內沒有回覆")

    async def answer(self,id,rid,text="",answers=None,value=None,choice=None,note=None):
        frame=self.pending_requests.get(id)
        if not frame or str(frame["id"])!=str(rid): raise ValueError("這個問題已結束，請重新整理對話")
        method=frame["method"]
        if method=="clarify":
            questions=frame.get("params",{}).get("questions",[])
            if answers is None and len(questions) != 1: raise ValueError("請使用問題卡片逐項回答")
            result={"answers":answers if answers is not None else {q["qid"]:text for q in questions}, "outcome":"answered"}
        elif method=="approval":
            decision=choice or ("always" if text in {"一律允許","always"} else "once" if text in {"允許這次","允許","同意","once","approve"} else "deny")
            if decision not in {"once","always","deny"}: raise ValueError("核准選項無效")
            result={"choice":decision}
        elif method in {"secret","sudo","vault.code","vault.unlock_prompt","vault.save_login"}:result={"value":value or ""}
        else:result={"settled_by":"user","targets":[]}
        client=self.clients.get(id)
        if not client: raise RuntimeError("Bot 連線已中斷，請重新連線")
        # Claim the request before awaiting so a concurrent answer cannot also be sent.
        self.pending_requests.pop(id,None)
        try: await client.answer(frame["id"],result)
        except BaseException:
            self.pending_requests.setdefault(id,frame); raise
        task_id=self.active.get(id)
        if not task_id: return {"ok":True}
        self.store.patch("tasks",task_id,state="running",question=None)
        if note:
            self.message(id,task_id,"event",note,event="question_expired")
        elif method not in {"secret","sudo","vault.code","vault.unlock_prompt","vault.save_login"}:
            self.message(id,task_id,"user",text or "已回覆問題")
        self.emit(id,"question_answered",{},task_id)
        return {"ok":True}

    async def stop(self,id):
        stopped=[t["id"] for t in self.store.all("tasks") if t["agent_id"]==id and t["state"]!="terminal"]
        if self.collab and stopped: self.collab.stop_cascade(id,stopped)
        for task in self.store.all("tasks"):
            if task["agent_id"]==id and task["state"]=="queued":self.store.patch("tasks",task["id"],state="terminal",verdict="cancelled",ended_at=now())
        task_id=self.active.get(id)
        if task_id:
            task=self.task(task_id)
            self.store.patch("tasks",task_id,state="stopping",question=None)
            self.stalled.setdefault(task_id,time.monotonic())
            client=self.clients.get(id)
            if client and client.alive and task.get("session_id"):
                await client.call("session.interrupt",{"session_id":task["session_id"]})
            else:self.finish(id,task_id,"cancelled","")
            return {"status":"stopping"}
        background=[t for t in self.store.all("tasks") if t["agent_id"]==id and live_workers(t)]
        if background:
            # Interrupting the session ends its background delegations too; each reports back as interrupted.
            client=self.clients.get(id)
            if not client or not client.alive:
                self.lose_workers(id,"interrupted"); return {"status":"idle"}
            for sid in {t.get("session_id") for t in background if t.get("session_id")}:
                with contextlib.suppress(Exception): await client.call("session.interrupt",{"session_id":sid})
            return {"status":"stopping"}
        return {"status":"idle"}

    async def stop_task(self,id,task_id):
        """Stop one turn of a bot (a hand-off from another bot, or a group-chat turn) without touching its other work."""
        task=self.store.get("tasks",task_id)
        if not task or task["state"]=="terminal": return
        if task["state"]=="queued":
            self.store.patch("tasks",task_id,state="terminal",verdict="cancelled",ended_at=now())
            self.message(id,task_id,"assistant","已停止。",id="delta:"+task_id,streaming=False,silent=True)
            self.emit(id,"reply",{},task_id)
            return
        if self.active.get(id)!=task_id: return
        self.store.patch("tasks",task_id,state="stopping",question=None)
        self.stalled.setdefault(task_id,time.monotonic())
        client=self.clients.get(id)
        if client and client.alive and task.get("session_id"):
            with contextlib.suppress(Exception): await client.call("session.interrupt",{"session_id":task["session_id"]})
        else: self.finish(id,task_id,"cancelled","")

    def halted(self,task_id):
        """The user stopped this turn. A turn that simply ended must not cut off its background children."""
        task=self.store.get("tasks",task_id) if task_id else None
        return bool(task) and (task["state"]=="stopping" or task.get("verdict") in {"cancelled","interrupted"})

    async def disconnected(self,id,error):
        self.lose_workers(id)
        task_id=self.active.get(id)
        if task_id:
            self.finish(id,task_id,"interrupted",self.streams.get(task_id,"")+"\n\nBot 連線中斷，尚未完成的操作沒有自動重播。可在重連後繼續交辦。")
        self.emit(id,"error",{"error":"Bot 連線中斷"})

    async def refresh_files(self,id,task_id=None):
        root=self.store.agent_directory(id)/"workspace"
        # A large workspace takes a while to walk; keep the event loop (and every live stream) responsive.
        files=await asyncio.to_thread(self.scan_files,root)
        self.store.put("files",id,files)
        if task_id:
            baseline = self.file_baselines.get(task_id, {})
            artifacts=[{"id":hashlib.sha256(f["path"].encode()).hexdigest()[:16],"kind":"file","title":f["title"],"rel_path":f["path"],"media_type":f["media_type"],"bytes":f["bytes"],"status":"available","ok_on_disk":True,"created_at":now()} for f in sorted(files,key=lambda f:rank(f["path"])) if f["important"] and not f["path"].startswith("uploads/") and baseline.get(f["path"]) != (f["bytes"], f["updated_at"])][:12]
            self.store.patch("tasks",task_id,artifacts=artifacts)
            self.emit(id,"m0.artifact",{},task_id)
        return files

    @staticmethod
    def scan_files(root):
        from datetime import datetime, timezone
        files=[]
        for path in walk(root):
            if len(files)>=500:break
            if not path.resolve().is_relative_to(root.resolve()):continue
            rel=str(path.relative_to(root))
            if any(part.startswith(".") for part in Path(rel).parts):continue
            try: stat=path.stat()
            except OSError: continue
            files.append({"path":rel,"title":path.name,"media_type":mimetypes.guess_type(path.name)[0] or "application/octet-stream","bytes":stat.st_size,"updated_at":datetime.fromtimestamp(stat.st_mtime,timezone.utc).isoformat(),"important":important(rel)})
        return files

    async def startup(self):
        if cleaned := self.store.purge_orphans(): log.info("Removed records of %s deleted bots", cleaned)
        for task in self.store.all("tasks"):
            if task["state"] not in {"terminal","queued"}:
                self.finish(task["agent_id"],task["id"],"interrupted","服務曾中斷；保留檔案及對話，沒有自動重播未知結果的操作。")
        for agent in self.store.all("agents"):
            self.lose_workers(agent["id"])
            self.store.patch("agents",agent["id"],live_session=None)
            self.kick(agent["id"])

    async def shutdown(self):
        self.closed=True
        await self.router.close()
        for id in list(self.active):
            with contextlib.suppress(Exception):await self.stop(id)
        for runner in [*self.runners.values(),*self.adopted.values()]:runner.cancel()
        await asyncio.gather(*self.runners.values(),*self.adopted.values(),return_exceptions=True)
        await asyncio.gather(*(c.close() for c in self.clients.values()),return_exceptions=True)
