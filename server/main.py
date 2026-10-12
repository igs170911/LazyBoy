"""LazyBoy product API. Hermes alone executes model/tool turns in each bot computer."""
import asyncio
import base64
import contextlib
from contextlib import asynccontextmanager
import hmac
import hashlib
import ipaddress
import json
import mimetypes
from pathlib import Path
import secrets
import shutil
from urllib.parse import urlparse, quote

from fastapi import FastAPI, Request, WebSocket
from fastapi.responses import FileResponse, JSONResponse, Response, StreamingResponse
from .routing import routed_payload
import httpx
from websockets.asyncio.client import connect

from .config import DATA, PROVIDER_PRESETS, ROOT, Settings, HERMES_COMMIT, private_write
from .store import Store, uid, now
from .model_check import CHECK_VERSION, check_model
from .computers import Computers
from .runtime import Runtime, plain_text
from .skills import Skills
from .scheduling import Scheduler
from .files import contained, filename
from .sharing import portable_connectors, scrub_known

store = Store(DATA)
settings = Settings(store)
computers = Computers(store, settings)
runtime = Runtime(store, settings, computers)
skills = Skills(store)
scheduler = Scheduler(store, runtime)


FACE_TRAITS = {"eye.scale", "eye.ratio", "eye.gap", "eye.lean", "body.rot"}
EXPRESSIONS = {"idle", "happy", "sad", "mad", "surprised", "wink", "sleepy", "smug", "unsure", "scared", "love", "shy", "sick", "thinking"}

def clean_face(row):
    """Avatar tuning is a few 0-1 positions and one named pose; anything else is dropped."""
    if "avatar_traits" in row:
        traits = row["avatar_traits"] if isinstance(row["avatar_traits"], dict) else {}
        row["avatar_traits"] = {k: min(1.0, max(0.0, float(v))) for k, v in traits.items()
                                if k in FACE_TRAITS and isinstance(v, (int, float)) and not isinstance(v, bool) and v == v}
    if "avatar_expression" in row and row["avatar_expression"] not in EXPRESSIONS:
        row["avatar_expression"] = "idle"
    return row

def make_agent(body):
    name = str(body.get("name", "")).strip()
    if not name or len(name) > 100: raise ValueError("Bot 名稱須為 1 至 100 字元")
    keys = ["name", "title", "description", "persona", "tags", "avatar_color", "avatar_shape", "provider", "model", "group_id", "reply_style", "avatar_traits", "avatar_expression"]
    row = clean_face({k: body[k] for k in keys if k in body})
    row.update(id=uid(), name=name, created_at=now(), hermes_session=None, live_session=None)
    store.put("agents", row["id"], row)
    computers.prepare(row["id"])
    return row


@asynccontextmanager
async def lifespan(app):
    if not store.all("agents"):
        make_agent({"name": "LazyBoy", "title": "你的常駐工作夥伴", "description": "在自己的電腦裡查資料、操作瀏覽器並完成工作。", "avatar_color": "#9278d4", "avatar_shape": "rounded", "tags": ["研究", "電腦", "檔案"]})
    await runtime.startup(); scheduler.start()
    yield
    await scheduler.close(); await runtime.shutdown()


app = FastAPI(title="LazyBoy", lifespan=lifespan, docs_url=None, redoc_url=None)


def local_client(request):
    if not request.client: return False
    try:
        peer = ipaddress.ip_address(request.client.host)
        host = urlparse("http://" + request.headers.get("host", "")).hostname
        return peer.is_loopback and host in {"localhost", "127.0.0.1", "::1"}
    except ValueError: return False


def valid_origin(request):
    origin = request.headers.get("origin")
    if not origin: return True
    return urlparse(origin).netloc == request.headers.get("host")


def authenticated(request):
    token = request.cookies.get("lazyhermes_auth", "")
    return (local_client(request) or hmac.compare_digest(token, settings.access_token)) and valid_origin(request)


@app.middleware("http")
async def auth(request, call_next):
    path = request.url.path
    if (path.startswith("/api/") or path.startswith("/novnc/")) and path not in {"/api/auth/status", "/api/auth/login"}:
        if not authenticated(request): return JSONResponse({"error": "請先登入 LazyBoy"}, status_code=401)
    response = await call_next(request)
    response.headers["X-Content-Type-Options"] = "nosniff"
    if path.startswith("/api/"): response.headers["Cache-Control"] = "no-store"
    return response


@app.exception_handler(ValueError)
async def bad_request(request, exc): return JSONResponse({"error": str(exc)}, status_code=400)
@app.exception_handler(KeyError)
async def missing(request, exc): return JSONResponse({"error": "找不到指定資料"}, status_code=404)
@app.exception_handler(RuntimeError)
async def runtime_error(request, exc): return JSONResponse({"error": str(exc)}, status_code=502)
@app.exception_handler(httpx.HTTPError)
async def upstream_error(request, exc): return JSONResponse({"error": "上游服務無法連線，請檢查服務網址與連線"}, status_code=502)


@app.get("/api/auth/status")
async def auth_status(request: Request): return {"authenticated": authenticated(request), "local": local_client(request)}
@app.post("/api/auth/login")
async def login(request: Request):
    if not valid_origin(request): return JSONResponse({"error": "無效的來源"}, status_code=403)
    body = await request.json()
    if not hmac.compare_digest(str(body.get("token", "")), settings.access_token): return JSONResponse({"error": "登入碼不正確"}, status_code=401)
    response = JSONResponse({"ok": True})
    response.set_cookie("lazyhermes_auth", settings.access_token, httponly=True, secure=request.url.scheme == "https", samesite="strict", max_age=30*86400)
    return response


def agent_row(id):
    agent = runtime.agent(id)
    transcript = store.transcript(id)
    activity = runtime.activity(id)
    result = {k: v for k, v in agent.items() if k not in {"hermes_session", "live_session"}}
    avatar = store.agent_directory(id) / "avatar.png"
    result.update(expertise=agent.get("title", ""), preview=plain_text(transcript[-1]["content"], 140) if transcript else "", last_at=transcript[-1].get("at") if transcript else None,
                  running=activity["running"], activity=activity, skill_count=len(skills.list(id)), has_avatar=avatar.exists(), avatar_url=f"/api/agents/{id}/avatar?updated={avatar.stat().st_mtime_ns}" if avatar.exists() else None)
    return result


@app.get("/api/health")
async def health():
    provider, model, _ = settings.effective()
    return {"ok": True, "service": True, "engine": "hermes", "provider": provider, "model": model, "hermes_commit": HERMES_COMMIT}
@app.get("/api/settings")
async def get_settings(): return settings.public()
@app.put("/api/settings")
async def save_settings(request: Request):
    result = settings.update(await request.json())
    for agent in store.all("agents"): computers.prepare(agent["id"])
    return result
@app.get("/api/settings/routing/health")
async def routing_health():
    return await runtime.router.health()


@app.get("/api/providers/presets")
async def provider_presets(): return {"presets": PROVIDER_PRESETS, "hermes_commit": HERMES_COMMIT}
@app.post("/api/providers/models")
async def provider_models(request: Request):
    body = await request.json()
    provider = settings.read()["providers"].get(body.get("provider"), {})
    url = (body.get("base_url") or provider.get("base_url", "")).rstrip("/")
    key = body.get("api_key") or saved_key_for(provider, url)
    if urlparse(url).scheme not in {"http", "https"}: raise ValueError("請填寫服務網址")
    headers = {"Authorization": "Bearer " + key, "x-api-key": key, "anthropic-version": "2023-06-01"} if key else {}
    async with httpx.AsyncClient(timeout=20) as client:
        response = await client.get(url + "/models", headers=headers)
        if not response.is_success: raise ValueError("無法取得模型清單，請檢查 API key 與服務網址")
        data = response.json()
        return {"models": [r["id"] for r in data.get("data", []) if isinstance(r, dict) and isinstance(r.get("id"), str)]}
def saved_key_for(provider, url):
    """A saved key only ever goes to the URL it was saved with, never to one supplied in a request."""
    return provider.get("api_key", "") if url.rstrip("/") == provider.get("base_url", "").rstrip("/") else ""


@app.post("/api/providers/check")
async def provider_check(request: Request):
    """Send one tiny request per protocol and remember which one this model answers on."""
    body = await request.json()
    provider_id, model = str(body.get("provider") or ""), str(body.get("model") or "").strip()
    saved = settings.read()["providers"].get(provider_id, {})
    url = (body.get("base_url") or saved.get("base_url", "")).rstrip("/")
    key = body.get("api_key") or saved_key_for(saved, url)
    if not model: raise ValueError("請先選擇模型")
    if urlparse(url).scheme not in {"http", "https"}: raise ValueError("請填寫服務網址")
    guess = body.get("protocol") or settings.guess_protocol(provider_id, model, {**saved, "base_url": url})
    result = await check_model(url, key, model, guess)
    # Only a saved provider whose key and URL were used as-is gets the result stored.
    stored = not body.get("base_url") and not body.get("api_key") and settings.record_check(provider_id, model, result, (url, key))
    if stored:
        for agent in store.all("agents"): computers.prepare(agent["id"])
    return {"result": result, "stored": bool(stored), "settings": settings.public()}
@app.post("/api/providers/check-in-use")
async def provider_check_in_use():
    """Check every model the default and the bots use that has never been checked."""
    cfg = settings.read()
    pairs = {(cfg["active"], cfg.get("model", ""))} | {(a["provider"], a["model"]) for a in store.all("agents") if a.get("provider") and a.get("model")}
    pending = []
    for provider_id, model in pairs:
        provider = cfg["providers"].get(provider_id, {})
        row = settings.model_row(provider, model)
        if model and row and (row.get("check") or {}).get("v") != CHECK_VERSION and provider.get("base_url"):
            pending.append((provider_id, model, provider))
    results = await asyncio.gather(*(check_model(p["base_url"], p.get("api_key", ""), m, settings.guess_protocol(pid, m, p)) for pid, m, p in pending))
    for (provider_id, model, provider), result in zip(pending, results):
        settings.record_check(provider_id, model, result, (provider["base_url"], provider.get("api_key", "")))
    if pending:
        for agent in store.all("agents"): computers.prepare(agent["id"])
    return {"checked": len(pending), "settings": settings.public()}
@app.get("/api/media")
async def media(agent_id: str | None = None):
    agent = runtime.agent(agent_id) if agent_id else {}
    from .routing import TurnRoute
    config = settings.read()
    enabled = config.get("routing", {}).get("enabled", True) and not (agent.get("provider") or agent.get("model"))
    _, model, provider = TurnRoute(settings, agent, config, "work", "work", enabled).primary()
    row = next((r for r in provider.get("models", []) if isinstance(r, dict) and r.get("id") == model), {})
    return {"model": model, "capabilities": {"images": bool(row.get("vision", False)), "pdf": True, "text": True}}
@app.get("/api/avatar/models")
async def avatar_models():
    provider, _, p = settings.effective()
    return {"provider": provider, "models": [r["id"] for r in p.get("models", []) if isinstance(r, dict) and r.get("image_generation")]}
@app.post("/api/avatar/generate")
async def generate_avatar(request: Request):
    body = await request.json(); _, _, p = settings.effective()
    async with httpx.AsyncClient(timeout=150) as client:
        response = await client.post(p.get("base_url", "").rstrip("/") + "/images/generations", headers={"Authorization": "Bearer " + p.get("api_key", "")}, json={"model": body["model"], "prompt": body["prompt"], "n": 1, "size": "1024x1024", "response_format": "b64_json"})
        if not response.is_success: raise ValueError("供應商無法生成頭像；可改用上傳圖片")
        data = response.json()["data"][0]
        if data.get("b64_json"): return {"data_url": "data:image/png;base64," + data["b64_json"]}
        image_response = await client.get(data["url"])
        image_response.raise_for_status()
        return {"data_url": "data:image/png;base64," + base64.b64encode(image_response.content).decode()}


@app.get("/api/agents")
async def agents(): return {"agents": [agent_row(a["id"]) for a in store.all("agents")], "groups": sorted(store.all("groups"), key=lambda g: g.get("position", 0))}
@app.post("/api/agents")
async def create_agent(request: Request): return agent_row(make_agent(await request.json())["id"])
@app.get("/api/agents/{id}")
async def agent_detail(id: str, thread: str | None = None):
    current = runtime.thread(id, thread)
    return {**agent_row(id), "thread": thread_row(id, current), "transcript": store.transcript(id, current["id"])}


def thread_row(id, thread, transcript=None, activity=None):
    transcript = store.transcript(id, thread["id"]) if transcript is None else transcript
    activity = activity or runtime.activity(id)
    last = transcript[-1] if transcript else None
    title = thread["title"] if thread.get("title_source") == "user" else plain_text(thread["title"]) or thread["title"]
    return {"id": thread["id"], "title": title, "main": thread["main"], "created_at": thread.get("created_at"),
            "last_at": last.get("at") if last else thread.get("updated_at") or thread.get("created_at"),
            "preview": plain_text(last["content"], 140) if last else "", "message_count": len(transcript),
            "running": activity["thread_id"] == thread["id"], "queued": thread["id"] in activity["queued_thread_ids"]}


@app.get("/api/agents/{id}/threads")
async def thread_list(id: str):
    runtime.agent(id); activity = runtime.activity(id)
    grouped = {}
    for message in store.transcript(id): grouped.setdefault(message.get("thread") or id, []).append(message)
    threads = [runtime.thread(id)] + [runtime.thread(id, t["id"]) for t in store.all("threads") if t.get("agent_id") == id]
    rows = [thread_row(id, t, grouped.get(t["id"], []), activity) for t in threads]
    return {"threads": [rows[0]] + sorted(rows[1:], key=lambda r: r["last_at"] or "", reverse=True)}


@app.post("/api/agents/{id}/threads")
async def thread_create(id: str, request: Request):
    runtime.agent(id); body = await request.json()
    title = " ".join(str(body.get("title") or "").split())[:80]
    key = uid()
    store.put("threads", key, {"id": key, "agent_id": id, "title": title or "新對話", "title_source": "user" if title else None,
                               "created_at": now(), "updated_at": now(), "hermes_session": None})
    runtime.emit(id, "thread_updated", {"thread_id": key})
    return thread_row(id, runtime.thread(id, key), [])


@app.patch("/api/agents/{id}/threads/{thread_id}")
async def thread_rename(id: str, thread_id: str, request: Request):
    thread = runtime.thread(id, thread_id); body = await request.json()
    title = " ".join(str(body.get("title") or "").split())[:80]
    if not title: raise ValueError("請填寫對話名稱")
    runtime.patch_thread(id, thread["id"], title=title, title_source="user")
    client = runtime.clients.get(id); sid = runtime.cached_session(id, client, thread["id"])
    if client and client.alive and sid:
        with contextlib.suppress(Exception): await client.call("session.title", {"session_id": sid, "title": title})
    runtime.emit(id, "thread_updated", {"thread_id": thread["id"]})
    return thread_row(id, runtime.thread(id, thread["id"]))


def thread_busy(id, thread_id):
    return any(t["agent_id"] == id and runtime.task_thread(t) == thread_id and t["state"] != "terminal" for t in store.all("tasks"))


async def release_thread_session(id, thread_id):
    """Drop a thread's native session without disturbing other threads on the same connection."""
    client = runtime.clients.get(id)
    sid = runtime.forget_session(id, client, thread_id)
    if client and client.alive and sid:
        with contextlib.suppress(Exception): await client.call("session.close", {"session_id": sid})
    return client


@app.delete("/api/agents/{id}/threads/{thread_id}")
async def thread_delete(id: str, thread_id: str):
    thread = runtime.thread(id, thread_id)
    if thread["main"]: raise ValueError("主要對話不能刪除，可以改用清除對話")
    if thread_busy(id, thread["id"]): raise ValueError("請先停止這個對話中正在執行或排隊的工作")
    client = await release_thread_session(id, thread["id"])
    if client and client.alive and thread.get("hermes_session"):
        with contextlib.suppress(Exception): await client.call("session.delete", {"session_id": thread["hermes_session"]})
    store.clear_messages(id, thread["id"]); store.remove("threads", thread["id"])
    runtime.emit(id, "thread_deleted", {"thread_id": thread["id"]})
    return {"ok": True, "id": thread["id"]}
@app.patch("/api/agents/{id}")
async def update_agent(id: str, request: Request):
    runtime.agent(id); body = await request.json()
    if "name" in body and (not str(body["name"]).strip() or len(str(body["name"])) > 100): raise ValueError("請填寫 Bot 名稱")
    keys = {"name", "title", "description", "persona", "tags", "avatar_color", "avatar_shape", "provider", "model", "group_id", "reply_style", "avatar_traits", "avatar_expression"}
    store.patch("agents", id, **clean_face({k: v for k, v in body.items() if k in keys}))
    computers.prepare(id)
    # Runtime/session model reconfiguration is applied when idle, through a fresh native
    # connection resuming the same durable history. Active work retains its chosen model.
    if not runtime.busy(id) and id in runtime.clients:
        await runtime.clients.pop(id).close()
    return agent_row(id)
@app.delete("/api/agents/{id}")
async def delete_agent(id: str):
    agent = runtime.agent(id); await runtime.stop(id)
    if id in runtime.clients: await runtime.clients.pop(id).close()
    runner = runtime.runners.pop(id, None)
    if runner:
        runner.cancel(); await asyncio.gather(runner, return_exceptions=True)
    await computers.delete(id)
    for routine in scheduler.list(id): store.remove("routines", routine["id"])
    for thread in store.all("threads"):
        if thread.get("agent_id") == id: store.remove("threads", thread["id"])
    store.remove("agents", id); store.purge_agent(id)
    shutil.rmtree(store.agent_directory(id))
    return {"ok": True, "id": id, "name": agent["name"]}
@app.get("/api/agents/{id}/activity")
async def activity(id: str): runtime.agent(id); return runtime.activity(id)
@app.post("/api/agents/{id}/messages")
async def messages(id: str, request: Request):
    body = await request.json()
    if body.get("question_id"):
        return await runtime.answer(id, body["question_id"], text=body.get("text", ""))
    return await runtime.submit(id, body.get("text", ""), body.get("request_id"), thread=body.get("thread_id"))
@app.post("/api/agents/{id}/stop")
async def stop(id: str): runtime.agent(id); return await runtime.stop(id)
@app.delete("/api/agents/{id}/conversation")
async def clear(id: str, thread: str | None = None):
    current = runtime.thread(id, thread)
    if thread_busy(id, current["id"]): raise ValueError("請先停止正在執行或排隊的工作")
    await release_thread_session(id, current["id"])
    store.clear_messages(id, current["id"])
    runtime.patch_thread(id, current["id"], hermes_session=None, **({"live_session": None} if current["main"] else {}))
    runtime.emit(id, "conversation_cleared", {"thread_id": current["id"]})
    return {"ok": True, "cleared_at": now(), "thread_id": current["id"]}


@app.get("/api/agents/{id}/events")
async def events(id: str, request: Request):
    runtime.agent(id)
    since = int(request.headers.get("last-event-id") or request.query_params.get("since") or store.last_event_id(id))
    async def stream():
        nonlocal since
        yield ": connected\n\n"
        with runtime.listen(id) as wake:
            while not await request.is_disconnected():
                wake.clear()
                batch = store.events(id, since)
                if batch:
                    since = batch[-1]["id"]
                    yield "id: " + str(since) + "\ndata: " + json.dumps({"events": batch}, ensure_ascii=False) + "\n\n"
                    # Coalesce a burst of token deltas into one frame.
                    await asyncio.sleep(.03)
                    continue
                # Wake on the next journaled event; the timeout keeps proxies alive and notices disconnects.
                with contextlib.suppress(TimeoutError):
                    async with asyncio.timeout(10): await wake.wait()
                if not wake.is_set(): yield ": keepalive\n\n"
    return StreamingResponse(stream(), media_type="text/event-stream", headers={"X-Accel-Buffering": "no"})


@app.get("/api/agents/{id}/computer")
async def computer_status(id: str): runtime.agent(id); return await computers.status(id)
@app.post("/api/agents/{id}/computer")
async def computer_start(id: str, request: Request):
    runtime.agent(id); action = (await request.json()).get("action", "start")
    if action not in {"start", "restart", "update"}: raise ValueError("無效的電腦操作")
    if action != "start" and runtime.busy(id): raise ValueError("請先停止工作再重新啟動電腦")
    if action != "start" and id in runtime.clients: await runtime.clients.pop(id).close()
    return await computers.start(id, action)
@app.get("/api/agents/{id}/files")
async def file_list(id: str): runtime.agent(id); return {"files": await runtime.refresh_files(id)}
@app.get("/api/agents/{id}/files/content")
async def file_content(id: str, path: str):
    runtime.agent(id)
    root = store.agent_directory(id) / "workspace"
    if path.startswith("/workspace/"): path = path.removeprefix("/workspace/")
    elif path.startswith("/home/box/"):
        root = store.agent_directory(id) / "home"
        path = path.removeprefix("/home/box/")
    target = contained(root, path, exists=True)
    return FileResponse(target, filename=target.name, content_disposition_type="inline" if target.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".gif", ".pdf", ".txt", ".md", ".csv"} else "attachment")
@app.post("/api/agents/{id}/uploads")
async def upload(id: str, request: Request, name: str = "attachment"):
    runtime.agent(id); key = uid(); name = filename(name)
    rel = "uploads/" + key + "/" + name
    target = contained(store.agent_directory(id) / "workspace", rel)
    target.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    try:
        with target.open("wb") as out:
            async for chunk in request.stream():
                count += len(chunk)
                if count > 64*1024*1024: raise ValueError("附件最多 64 MB")
                out.write(chunk)
    except BaseException:
        target.unlink(missing_ok=True); raise
    row = {"id": key, "agent_id": id, "name": name, "path": rel, "media_type": mimetypes.guess_type(name)[0] or "application/octet-stream", "bytes": count, "url": f"/api/agents/{id}/files/content?path={quote(rel)}"}
    store.put("uploads", key, row); return row
@app.get("/api/tasks/{id}")
async def task_detail(id: str): return runtime.task(id)
@app.post("/api/tasks/{id}/approve")
async def approve(id: str, request: Request):
    task = runtime.task(id); body = await request.json()
    if body.get("agent") != task["agent_id"] or body.get("decision") not in {"approve", "reject"}: raise ValueError("無效的成果確認")
    store.patch("tasks", id, approval={"decision": body["decision"], "note": body.get("note", ""), "decided_at": now(), "decided_by": "owner"})
    return {"approved": body["decision"] == "approve"}
@app.get("/api/tasks/{id}/artifacts/{artifact}/content")
async def artifact_content(id: str, artifact: str):
    task = runtime.task(id); row = next((a for a in task.get("artifacts", []) if a["id"] == artifact), None)
    if not row: raise KeyError(artifact)
    return await file_content(task["agent_id"], row["rel_path"])


@app.post("/api/agents/{id}/questions")
async def answer_question(id: str, request: Request):
    body = await request.json()
    return await runtime.answer(id, body["question_id"], text=body.get("text", ""), answers=body.get("answers"), choice=body.get("choice"))
@app.post("/api/agents/{id}/secrets")
async def answer_secret(id: str, request: Request):
    body = await request.json()
    value = None if body.get("action") == "cancel" else body.get("value", (body.get("values") or {}).get("value"))
    frame = runtime.pending_requests.get(id)
    if frame and frame["method"] == "vault.save_login" and body.get("action") != "cancel": value = json.dumps(body.get("values", {}), ensure_ascii=False)
    return await runtime.answer(id, body["question_id"], value=value)
@app.post("/api/agents/{id}/handover")
async def handover(id: str, request: Request):
    body = await request.json()
    if body.get("action") == "cancel": return await runtime.stop(id)
    return await runtime.answer(id, body["question_id"], text="已在電腦完成操作，請重新觀察")


@app.post("/api/agents/{id}/skills")
async def manage_skills(id: str, request: Request):
    runtime.agent(id)
    body = await request.json()
    result = skills.manage(id, body)
    # Listing is a read. Emitting here makes the open settings panel reload and list again.
    if body.get("action", "list") != "list":
        runtime.emit(id, "skills_changed", {"count": len(result), "changed": True})
    return {"skills": result}
@app.get("/api/agents/{id}/routines")
async def routines(id: str): runtime.agent(id); return {"routines": scheduler.list(id)}
@app.post("/api/agents/{id}/routines")
async def manage_routines(id: str, request: Request): runtime.agent(id); return await scheduler.manage(id, await request.json())


@app.post("/api/team")
async def team(request: Request):
    body = await request.json(); action = body.get("action")
    if action == "move_agent":
        runtime.agent(body["agent_id"]); group = body.get("group_id") or ""
        if group and not store.get("groups", group): raise KeyError(group)
        store.patch("agents", body["agent_id"], group_id=group)
    elif action in {"create_group", "update_group"}:
        key = body.get("group_id") or uid()
        if not str(body.get("name", "")).strip(): raise ValueError("請填寫群組名稱")
        old = store.get("groups", key, {})
        if action == "update_group" and not old: raise KeyError(key)
        # A rename without member_ids must keep the current members.
        keep = action == "update_group" and "member_ids" not in body
        members = body.get("member_ids") or []
        for member in members: runtime.agent(member)
        with store.transaction():
            store.put("groups", key, {**old, "id": key, "name": body["name"], "position": old.get("position", len(store.all("groups")))})
            for agent in [] if keep else store.all("agents"):
                if agent["id"] in members: store.patch("agents", agent["id"], group_id=key)
                elif agent.get("group_id") == key: store.patch("agents", agent["id"], group_id="")
    elif action == "delete_group":
        key = body["group_id"]; store.remove("groups", key)
        for a in store.all("agents"):
            if a.get("group_id") == key: store.patch("agents", a["id"], group_id="")
    elif action == "reorder_groups":
        for position, key in enumerate(body.get("ids", [])): store.patch("groups", key, position=position)
    else: raise ValueError("無效的群組操作")
    return {"groups": store.all("groups"), "agents": [agent_row(a["id"]) for a in store.all("agents")]}


@app.put("/api/agents/{id}/avatar")
async def set_avatar(id: str, request: Request):
    runtime.agent(id); data = await request.body()
    if len(data) > 8*1024*1024 or not data.startswith(b"\x89PNG\r\n\x1a\n"): raise ValueError("請上傳 8 MB 以下的 PNG 圖片")
    (store.agent_directory(id) / "avatar.png").write_bytes(data)
    return agent_row(id)
@app.delete("/api/agents/{id}/avatar")
async def clear_avatar(id: str): runtime.agent(id); (store.agent_directory(id) / "avatar.png").unlink(missing_ok=True); return agent_row(id)
@app.get("/api/agents/{id}/avatar")
async def get_avatar(id: str): runtime.agent(id); return FileResponse(contained(store.agent_directory(id), "avatar.png", exists=True))


@app.get("/api/agents/{id}/template")
async def export_template(id: str):
    agent = runtime.agent(id)
    profile = {k: v for k, v in agent.items() if k in {"name", "title", "description", "persona", "tags", "avatar_color", "avatar_shape", "provider", "model", "reply_style", "avatar_traits", "avatar_expression"}}
    entries = {**store.get("mcp", "shared", {}), **store.get("mcp", id, {})}
    mcp, known_secrets = portable_connectors(entries)
    known_secrets.extend(p.get("api_key", "") for p in settings.read().get("providers", {}).values())
    env_path = store.agent_directory(id) / "hermes" / ".env"
    if env_path.exists(): known_secrets.extend(line.partition("=")[2].strip().strip("\"'") for line in env_path.read_text().splitlines() if "=" in line and not line.lstrip().startswith("#"))
    avatar = store.agent_directory(id) / "avatar.png"
    result = {"format": "lazyboy.agent-template", "version": 1, "profile": profile, "skills": skills.list(id), "mcp": {"mcpServers": mcp}, "notes": ["API key、登入、對話、記憶與 MCP 秘密設定不包含在分享檔案中。"]}
    if avatar.exists(): result["avatar_png"] = base64.b64encode(avatar.read_bytes()).decode()
    if len(result["skills"]) > 64 or len(mcp) > 64: raise ValueError("分享最多 64 個技能與 64 個串接")
    for field in ("profile", "skills", "mcp"): result[field] = scrub_known(result[field], known_secrets)
    if len(json.dumps(result).encode()) > 12*1024*1024: raise ValueError("分享檔超過 12 MB")
    return result
@app.put("/api/templates/{key}")
async def import_template(key: str, request: Request):
    body = await request.json(); template = body["template"]
    digest = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()
    previous = store.get("imports", key)
    if previous:
        if previous["digest"] != digest: raise ValueError("同一匯入識別不能用於不同內容")
        return agent_row(previous["agent_id"])
    if template.get("format") != "lazyboy.agent-template" or template.get("version") != 1: raise ValueError("不支援的 Bot 分享格式")
    entries = template.get("mcp", {}).get("mcpServers", {})
    if not isinstance(entries, dict): raise ValueError("無效的 MCP 設定")
    if len(entries) > 64 or len(template.get("skills", [])) > 64 or len(json.dumps(body).encode()) > 12*1024*1024: raise ValueError("分享檔最多 12 MB、64 個技能與 64 個串接")
    data = None
    if template.get("avatar_png"):
        data = base64.b64decode(template["avatar_png"], validate=True)
        if len(data) > 8*1024*1024 or not data.startswith(b"\x89PNG"): raise ValueError("分享檔中的頭像無效")
    for entry in entries.values():
        if not isinstance(entry, dict): raise ValueError("無效的 MCP 設定")
        for field in ("headers", "env"):
            if not isinstance(entry.get(field, {}), dict): raise ValueError("無效的 MCP 驗證資料")
    entries, imported_secrets = portable_connectors(entries)
    template = scrub_known(template, imported_secrets)
    requested = body.get("name") or template["profile"]["name"]
    name, index = requested, 2
    names = {a["name"] for a in store.all("agents")}
    while name in names: name = f"{requested} {index}"; index += 1
    agent = None
    try:
        with store.transaction():
            agent = make_agent({**template["profile"], "name": name, "group_id": body.get("group_id", ""), "provider": "", "model": ""})
            for s in template.get("skills", []): skills.manage(agent["id"], {**s, "id": uid(), "action": "save"})
            store.put("mcp", agent["id"], entries)
            if data: (store.agent_directory(agent["id"]) / "avatar.png").write_bytes(data)
            computers.prepare(agent["id"])
            store.put("imports", key, {"digest": digest, "agent_id": agent["id"]})
    except BaseException:
        if agent: shutil.rmtree(store.directory / "bots" / agent["id"], ignore_errors=True)
        raise
    return agent_row(agent["id"])


def mcp_public(scope):
    return [{"server": name, "enabled": value.get("enabled", True), "transport": "http" if value.get("url") else "stdio", "url": value.get("url"), "command": value.get("command"), "args": value.get("args", []), "header_names": list(value.get("headers", {})), "env_names": list(value.get("env", {})), "missing_fields": [f"{kind}:{key}" for kind,field in [("header","headers"),("env","env")] for key,val in value.get(field,{}).items() if not val], "status": "configured"} for name, value in store.get("mcp", scope, {}).items()]
@app.post("/api/mcp")
async def manage_mcp(request: Request):
    body = await request.json(); scope = body.get("agent") or "shared"; action = body.get("action", "list")
    if scope != "shared": runtime.agent(scope)
    entries = store.get("mcp", scope, {})
    name = body.get("server", "")
    if action in {"save", "delete", "remove"}:
        if name in {"computer", "product"} or not name or len(name) > 100: raise ValueError("請填寫其他 MCP 名稱")
        if action in {"delete", "remove"}: entries.pop(name, None)
        else:
            old = entries.get(name, {})
            entry = {**old, **{k: v for k, v in body.items() if k in {"url", "headers", "command", "args", "env", "enabled"}}}
            same_endpoint = old.get("url") == entry.get("url") and old.get("command") == entry.get("command")
            for field in ("headers", "env"):
                supplied = body.get(field, {})
                if not isinstance(supplied, dict): raise ValueError("MCP 驗證資料格式無效")
                entry[field] = {**(old.get(field, {}) if same_endpoint else {}), **supplied}
            if not entry.get("url") and not entry.get("command"): raise ValueError("請填寫 MCP 網址或指令")
            entries[name] = entry
        store.put("mcp", scope, entries)
        for agent in store.all("agents") if scope == "shared" else [runtime.agent(scope)]:
            computers.prepare(agent["id"])
            client = runtime.clients.get(agent["id"])
            if client and client.alive and not runtime.busy(agent["id"]):
                await client.call("reload.mcp", {"confirm": True, "session_id": getattr(client,"product_session",None)})
    elif action == "test":
        id = scope if scope != "shared" else next((a["id"] for a in store.all("agents")), None)
        if not id: raise ValueError("請先建立 Bot")
        computers.prepare(id); client = await runtime.client(id)
        result = await client.call("mcp.servers.test", {"name": name}, timeout=90)
        return {"scope": "agent" if scope != "shared" else "shared", "servers": [{**r, "serverStatus": "connected" if result.get("ok") else "error", "serverError": result.get("error"), "tools": result.get("tools", [])} for r in mcp_public(scope) if r["server"] == name]}
    elif action != "list": raise ValueError("無效的 MCP 操作")
    return {"scope": "agent" if scope != "shared" else "shared", "servers": mcp_public(scope)}


@app.post("/api/agents/{id}/lessons")
async def lesson(id: str, request: Request):
    runtime.agent(id); body = await request.json(); upload = store.get("uploads", body.get("upload_id"))
    if body.get("video"):
        raw = base64.b64decode(body["video"], validate=True)
        if len(raw) > 64*1024*1024: raise ValueError("示範影片最多 64 MB")
        clip = body.get("id") or uid()
        import uuid
        uuid.UUID(clip)
        rel = f"lessons/{clip}/recording.webm"
        target = contained(store.agent_directory(id) / "workspace", rel)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
        upload = {"id":clip, "agent_id":id, "path":rel, "name":body.get("title", "操作示範"), "media_type":"video/webm", "bytes":len(raw), "url":f"/api/agents/{id}/files/content?path={quote(rel)}"}
        store.put("uploads", clip, upload)
    if not upload or upload["agent_id"] != id: raise ValueError("找不到這個 Bot 的示範影片")
    key = uid(); folder = store.agent_directory(id) / "workspace" / "lessons" / key; folder.mkdir(parents=True)
    frames = body.get("frames", [])
    if len(frames) > 64: raise ValueError("示範最多 64 張截圖")
    manifest = {"title": body.get("title", "操作示範"), "notes": body.get("notes", ""), "duration_ms": body.get("duration_ms"), "video": "/workspace/" + upload["path"], "frames": []}
    count = 0
    for index, frame in enumerate(frames):
        header, raw = frame["data_url"].split(",", 1)
        if header not in {"data:image/jpeg;base64", "data:image/png;base64"}: raise ValueError("無效的示範截圖")
        data = base64.b64decode(raw, validate=True); count += len(data)
        if count > 32*1024*1024: raise ValueError("示範截圖最多 32 MB")
        path = folder / (str(index) + (".png" if "png" in header else ".jpg")); path.write_bytes(data)
        manifest["frames"].append({"at_ms": frame.get("at_ms"), "path": "/workspace/" + str(path.relative_to(store.agent_directory(id) / "workspace"))})
    (folder / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2))
    store.put("lessons", key, {**manifest, "id": key, "agent_id": id, "manifest_path": "/workspace/lessons/" + key + "/manifest.json"})
    return {"id": key, "video_url": upload["url"], "manifest_path": "/workspace/lessons/" + key + "/manifest.json"}


class WorkerUnauthorized(Exception): pass
@app.exception_handler(WorkerUnauthorized)
async def worker_unauthorized(request, exc): return JSONResponse({"error": str(exc)}, status_code=401)


def worker(request):
    supplied = request.headers.get("authorization", "")
    if not supplied.startswith("Bearer "): raise WorkerUnauthorized("Worker authentication required")
    for agent in store.all("agents"):
        if hmac.compare_digest(supplied[7:], computers.key(agent["id"])): return agent
    raise WorkerUnauthorized("Worker authentication failed")


async def wait_model(awaitable, task_id):
    """Stop also owns requests waiting for HTTP headers or the first SSE byte."""
    pending = asyncio.ensure_future(awaitable)
    try:
        while True:
            if runtime.halted(task_id):
                if pending.done() and not pending.cancelled() and pending.exception() is None:
                    value = pending.result()
                    if isinstance(value, httpx.Response): await value.aclose()
                raise ValueError("這個工作已停止")
            done, _ = await asyncio.wait({pending}, timeout=.05)
            if done: return pending.result()
    finally:
        if not pending.done():
            pending.cancel()
            await asyncio.gather(pending, return_exceptions=True)


@app.api_route("/internal/providers/{provider}/v1/{path:path}", methods=["GET", "POST"])
async def provider_proxy(provider: str, path: str, request: Request):
    agent = worker(request)
    active, model, p = settings.effective(agent)
    task_id = runtime.active.get(agent["id"])
    if task_id in runtime.provider_bindings: active, model, p = runtime.provider_bindings[task_id]
    route = runtime.routes.get(task_id)
    if provider != (route.input_provider if route else active): raise ValueError("Bot cannot use another provider's credentials")
    allowed = {"chat/completions", "responses", "messages", "models", "embeddings", "images/generations", "audio/transcriptions"}
    if path not in allowed and not path.startswith("responses/"): raise ValueError("Unsupported model endpoint")
    content = await request.body()
    payload = json.loads(content) if path in {"chat/completions", "responses", "messages"} else None
    foreground = bool(route and isinstance(payload, dict) and payload.get("tools"))
    routed = foreground and route.enabled
    if routed and route.executed_tools(payload): route.promote()
    candidates = route.candidates() if routed else [route.input_binding if route else (active, model, p)]
    if routed and payload.get("previous_response_id"):
        candidates = [b for b in candidates if b[0] == active and b[1] == payload.get("model")]
    if not candidates: raise ValueError("沒有符合這個任務圖片或對話能力的分流模型")
    for index, binding in enumerate(candidates):
        if foreground and runtime.halted(task_id): raise ValueError("這個工作已停止")
        selected_provider, model, p = binding
        url = p.get("base_url", "").rstrip("/") + "/" + path
        if urlparse(url).scheme not in {"http", "https"}: raise ValueError("請先設定模型供應商與服務網址")
        headers = {"content-type": request.headers.get("content-type", "application/json"), "User-Agent": "LazyBoy/0.2.0 Hermes/" + HERMES_COMMIT[:12]}
        if urlparse(url).hostname == "opencode.ai":
            thread = runtime.thread(agent["id"], runtime.task_thread(runtime.task(task_id))) if task_id else runtime.thread(agent["id"])
            headers["x-opencode-session"] = thread.get("hermes_session") or thread["id"]
        if p.get("api_key"):
            headers["Authorization"] = "Bearer " + p["api_key"]
            if settings.protocol(p, model) == "messages":
                headers.update({"x-api-key": p["api_key"], "anthropic-version": request.headers.get("anthropic-version", "2023-06-01")})
                if request.headers.get("anthropic-beta"): headers["anthropic-beta"] = request.headers["anthropic-beta"]
        body = json.dumps(routed_payload(payload, binding, settings.protocol(p, model), settings, other_provider=selected_provider != route.input_provider)).encode() if routed else content
        deadline = route.deadline(binding) if routed else 300
        record = {"provider":selected_provider,"model":model if routed else payload.get("model",model) if foreground else model,"protocol":settings.protocol(p,model),"role":route.role if routed else "fixed","reasoning_effort":settings.reasoning(p,model)}
        if foreground:
            task = runtime.task(task_id)
            runtime.store.patch("tasks",task_id,model_requests=[*task.get("model_requests",[]),record])
        client = httpx.AsyncClient(timeout=httpx.Timeout(deadline, connect=min(15, deadline)))
        upstream = None
        iterator = None
        first_chunk = None
        try:
            sending = client.send(client.build_request(request.method, url, headers=headers, params=request.query_params, content=body), stream=True)
            upstream = await wait_model(sending, task_id) if foreground else await sending
            if routed and upstream.is_success and "text/event-stream" in upstream.headers.get("content-type", ""):
                iterator = upstream.aiter_bytes()
                async with asyncio.timeout(deadline):
                    first_chunk = await wait_model(anext(iterator, None), task_id)
                if first_chunk is None: raise httpx.ReadError("模型串流未提供任何內容")
        except BaseException as exc:
            if upstream: await upstream.aclose()
            await client.aclose()
            if isinstance(exc, (httpx.TransportError, TimeoutError)) and index + 1 < len(candidates):
                runtime.emit(agent["id"], "model_fallback", {"provider":selected_provider,"model":model,"reason":"connection"}, task_id)
                continue
            raise
        if upstream.status_code in {429, 500, 502, 503, 504} and index + 1 < len(candidates):
            await upstream.aclose(); await client.aclose()
            runtime.emit(agent["id"], "model_fallback", {"provider":selected_provider,"model":model,"status":upstream.status_code}, task_id)
            continue
        if foreground and upstream.is_success:
            if routed: route.last = binding
            runtime.provider_bindings[task_id] = binding
            runtime.store.patch("tasks",task_id,**record,routing={"intent":route.intent,"role":route.role,"enabled":route.enabled,"promoted":route.promoted})
            runtime.emit(agent["id"], "model_routed", record, task_id)
        break
    # Never retry after response bytes start. Hermes receives the original
    # transport, including tool IDs and paired results, on every model request.
    async def chunks():
        try:
            if first_chunk is not None: yield first_chunk
            async for chunk in iterator if iterator is not None else upstream.aiter_bytes(): yield chunk
        finally: await upstream.aclose(); await client.aclose()
    return StreamingResponse(chunks(), status_code=upstream.status_code, media_type=upstream.headers.get("content-type", "application/json"))


@app.post("/internal/product")
async def product_tool(request: Request):
    agent = worker(request); body = await request.json(); action = body.get("action")
    if action == "routines": return await scheduler.manage(agent["id"], body.get("body", {}))
    if action == "bots": return {"bots": [{"id": a["id"], "name": a["name"], "title": a.get("title", "")} for a in store.all("agents") if a["id"] != agent["id"]]}
    if action == "send":
        target = body["bot_id"]
        if target == agent["id"]: raise ValueError("不能派工作給自己")
        return await runtime.submit(target, f"來自 {agent['name']} 的交辦：\n{body['text']}", body.get("request_id"), force_queue=True)
    if action == "task":
        task = runtime.task(body["task_id"])
        return {"state": task["state"], "verdict": task.get("verdict"), "result": task.get("result"), "artifacts": task.get("artifacts", [])}
    raise ValueError("Unsupported product tool")


@app.get("/novnc/{id}/{path:path}")
async def novnc(id: str, path: str, request: Request):
    runtime.agent(id); status = await computers.status(id)
    if not status.get("viewer_port"): raise ValueError("請先開啟 Bot 電腦")
    if ".." in Path(path).parts: raise ValueError("無效的桌面路徑")
    async with httpx.AsyncClient(timeout=20) as client:
        response = await client.get(f"http://127.0.0.1:{status['viewer_port']}/{path}", params=request.query_params)
        if response.is_success and path in {"vnc.html", "vnc_lite.html"}:
            skin = (ROOT / "web" / "novnc-skin.html").read_text()
            html = response.text.replace("</body>", skin + "</body>")
            return Response(html, status_code=response.status_code, media_type="text/html")
        return Response(response.content, status_code=response.status_code, media_type=response.headers.get("content-type", "application/octet-stream"))
@app.websocket("/novnc/{id}/websockify")
async def novnc_socket(socket: WebSocket, id: str):
    if not authenticated(socket): await socket.close(code=1008); return
    runtime.agent(id); status = await computers.status(id)
    if not status.get("viewer_port"): await socket.close(code=1013); return
    await socket.accept()
    try:
        async with connect(f"ws://127.0.0.1:{status['viewer_port']}/websockify", max_size=16*1024*1024, compression=None,
                           # Loopback hop: a CPU-starved desktop must not be mistaken for a dead link (websockify has its own heartbeat).
                           ping_interval=None) as upstream:
            async def to_desktop():
                while True:
                    frame = await socket.receive()
                    if frame["type"] == "websocket.disconnect": return
                    await upstream.send(frame.get("bytes") if frame.get("bytes") is not None else frame.get("text", ""))
            async def to_user():
                async for frame in upstream:
                    if isinstance(frame, bytes): await socket.send_bytes(frame)
                    else: await socket.send_text(frame)
            tasks = [asyncio.create_task(to_desktop()), asyncio.create_task(to_user())]
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in tasks: task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
    except Exception: pass
    finally:
        if socket.client_state.name == "CONNECTED": await socket.close()


@app.get("/{path:path}")
async def frontend(path: str):
    root = ROOT / "web" / "dist"
    if path.startswith(("api/", "internal/", "novnc/")): return JSONResponse({"error": "找不到這個功能"}, status_code=404)
    target = (root / (path or "index.html")).resolve()
    if not target.is_relative_to(root.resolve()): raise ValueError("無效的路徑")
    if not target.is_file(): target = root / "index.html"
    if not target.is_file(): return Response("Frontend 尚未建置。請執行 scripts/bootstrap.sh", status_code=503)
    return FileResponse(target, headers={"Cache-Control": "no-cache" if target.name in {"index.html", "sw.js"} else "public, max-age=3600"})
