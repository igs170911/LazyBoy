"""Durable product task admission and a single native Hermes executor per bot."""
import asyncio
import contextlib
import copy
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

log = logging.getLogger(__name__)
TERMINAL = {"terminal"}
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
        self.router = Router(settings)
        self.computers.task_binding = lambda id: self.provider_bindings.get(self.active.get(id))
        self.closed = False

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
        return {"state": state, "running": bool(working), "queued": bool(queued), "observed_at_ms": int(time.time()*1000),
                "active_task_ids": [t["id"] for t in working], "queued_task_ids": [t["id"] for t in queued],
                "thread_id": self.task_thread(working[0]) if working else None,
                "queued_thread_ids": list(dict.fromkeys(self.task_thread(t) for t in queued)),
                "latest_task_id": latest["id"] if latest else None, "question": question, "questions": [question] if question else []}

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

    async def submit(self, id, text, request_id=None, *, force_queue=False, routine=None, thread=None):
        async with self.admission_locks.setdefault(id, asyncio.Lock()):
            return await self._submit(id, text, request_id, force_queue=force_queue, routine=routine, thread=thread)

    async def _submit(self, id, text, request_id=None, *, force_queue=False, routine=None, thread=None):
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
        if active and not force_queue and self.task_thread(self.task(active)) == thread_id:
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
              "result":None,"artifacts":[],"milestones":[],"request_id":request_id,"routine_id":routine}
        # Persist admission + task + user message together, before any executor starts.
        origin = {"routine_id": routine, "routine_title": (self.store.get("routines", routine) or {}).get("title", "")} if routine else {}
        with self.store.transaction():
            self.store.admit(id,request_id,task_id,text)
            self.store.put("tasks",task_id,task)
            self.store.message(id,"user",text,thread=thread_id,**origin)
            self.touch_thread(id,thread_id,text)
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

    async def _queue(self,id):
        while not self.closed:
            task=next((t for t in self.store.all("tasks") if t["agent_id"]==id and t["state"]=="queued"),None)
            if not task: return
            task_id=task["id"]
            self.active[id]=task_id
            self.finished[task_id]=asyncio.Event()
            self.store.patch("tasks",task_id,state="running",started_at=now())
            self.emit(id,"m0.progress",{"message":"正在準備 Bot 的電腦與工作環境"},task_id)
            try:
                route = await self.router.select(self.agent(id), task["goal"], routine=bool(task.get("routine_id")), vision=bool(self._images(id, task["goal"])), emit=lambda kind, payload: self.emit(id, kind, payload, task_id))
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
                text=self._prompt(id,task["goal"])
                if self.task(task_id)["state"] in {"stopping", "terminal"}:
                    self.finish(id,task_id,"cancelled","")
                    continue
                if self.settings.model_row(selected, model).get("vision"):
                    for path in self._images(id, task["goal"]):
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
                self.seen[task_id]=clock; continue
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

    def _lessons(self,id,text):
        return [v for v in self.store.all("lessons") if v["agent_id"]==id and (v["manifest_path"] in text or v["video"].removeprefix("/workspace/") in text)]

    def _images(self,id,text):
        paths = ["/workspace/"+v["path"] for v in self._uploads(id,text) if v["media_type"].startswith("image/")]
        for lesson in self._lessons(id,text):
            frames = lesson["frames"]
            indexes = range(len(frames)) if len(frames)<=4 else [round(i*(len(frames)-1)/3) for i in range(4)]
            paths.extend(frames[i]["path"] for i in indexes)
        return list(dict.fromkeys(paths))

    def _prompt(self,id,text):
        uploads=self._uploads(id,text)
        if uploads:
            text += "\n\n使用者附加的檔案已放在這台電腦的工作目錄：\n"+"\n".join("/workspace/"+u["path"] for u in uploads)
        for lesson in self._lessons(id,text):
            text += "\n\n使用者的操作示範資料：" + lesson["manifest_path"] + "\n說明：" + lesson.get("notes", "")
            text += "\n按時間排列的畫面：\n" + "\n".join(str(f["at_ms"]) + " ms: " + f["path"] for f in lesson["frames"])
        return text

    def skill_state(self, id):
        return {s["id"]: hashlib.sha256(json.dumps({k:s[k] for k in ["name","description","instructions","enabled"]}, sort_keys=True).encode()).hexdigest() for s in Skills(self.store).list(id)}

    def finish(self,id,task_id,verdict,text,**fields):
        task=self.task(task_id)
        if task["state"]=="terminal": return
        content=text or self.streams.get(task_id,"") or ("已停止，先前檔案與對話已保留。" if verdict=="cancelled" else "工作已結束。")
        hint=friendly(content) if verdict=="failed" else None
        if hint: content=hint+"\n\n> 原始錯誤："+" ".join(content.split())[:600]
        self.message(id,task_id,"assistant",content,id="delta:"+task_id,streaming=False,narration=self.narration.get(task_id,[]))
        steps=[{**step,"state":"done" if step["state"]=="running" else step["state"]} for step in self.steps.get(task_id,[])]
        trail={"steps":steps,"sources":self.sources.get(task_id,[])} if steps or self.sources.get(task_id) else {}
        self.store.patch("tasks",task_id,state="terminal",verdict=verdict,result={"summary":content},question=None,ended_at=now(),**trail,**fields)
        self.emit(id,"runtime",{"type":"turn_ended","verdict":verdict},task_id)
        self.emit(id,"reply",{},task_id)
        event=self.finished.get(task_id)
        if event: event.set()

    async def event(self,id,event):
        task_id=self.active.get(id)
        name=event.get("type","");payload=event.get("payload",{})
        if name=="session.title":
            self.native_title(id,event.get("session_id"),payload.get("title"))
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
        if method=="clarify":
            questions=params.get("questions",[])
            question["hermes_questions"]=questions
            question["prompt"]="\n\n".join(q.get("question","") for q in questions)
            if len(questions)==1: question["options"]=questions[0].get("choices",[])
        elif method=="approval":question["options"]=["允許這次","拒絕"]
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

    async def answer(self,id,rid,text="",answers=None,value=None,choice=None):
        frame=self.pending_requests.get(id)
        if not frame or str(frame["id"])!=str(rid): raise ValueError("這個問題已結束，請重新整理對話")
        method=frame["method"]
        if method=="clarify":
            questions=frame.get("params",{}).get("questions",[])
            if answers is None and len(questions) != 1: raise ValueError("請使用問題卡片逐項回答")
            result={"answers":answers if answers is not None else {q["qid"]:text for q in questions}, "outcome":"answered"}
        elif method=="approval":
            decision=choice or ("once" if text in {"允許這次","允許","同意","once","approve"} else "deny")
            if decision not in {"once","deny"}: raise ValueError("核准選項無效")
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
        if method not in {"secret","sudo","vault.code","vault.unlock_prompt","vault.save_login"}:
            self.message(id,task_id,"user",text or "已回覆問題")
        self.emit(id,"question_answered",{},task_id)
        return {"ok":True}

    async def stop(self,id):
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
        return {"status":"idle"}

    async def disconnected(self,id,error):
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
            self.store.patch("agents",agent["id"],live_session=None)
            self.kick(agent["id"])

    async def shutdown(self):
        self.closed=True
        await self.router.close()
        for id in list(self.active):
            with contextlib.suppress(Exception):await self.stop(id)
        for runner in self.runners.values():runner.cancel()
        await asyncio.gather(*self.runners.values(),return_exceptions=True)
        await asyncio.gather(*(c.close() for c in self.clients.values()),return_exceptions=True)
