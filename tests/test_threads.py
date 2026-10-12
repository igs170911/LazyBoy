"""Threads: several native Hermes sessions on one bot, sharing its computer."""
import asyncio
import importlib
from types import SimpleNamespace

import httpx
import pytest

from server.computers import Computers
from server.config import Settings
from server.runtime import Runtime
from server.scheduling import Scheduler
from server.skills import Skills
from server.store import Store, uid


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    settings.update({"active": "primary", "model": "work", "provider": {"id": "primary", "base_url": "https://primary.example/v1", "api_key": "KEY", "models": [{"id": "work", "protocol": "chat_completions"}]}})
    settings.update({"routing": {"enabled": False}})
    computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers)
    runtime.closed = True
    agent = {"id": uid(), "name": "Thread Bot", "created_at": "2026-10-08T00:00:00+00:00"}
    store.put("agents", agent["id"], agent)
    store.agent_directory(agent["id"])
    yield SimpleNamespace(store=store, settings=settings, computers=computers, runtime=runtime, agent=agent["id"])
    store.conn.close()


@pytest.fixture
async def api(product, monkeypatch):
    m = importlib.import_module("server.main")
    for key in ("store", "settings", "computers", "runtime"): monkeypatch.setattr(m, key, getattr(product, key))
    monkeypatch.setattr(m, "skills", Skills(product.store))
    monkeypatch.setattr(m, "scheduler", Scheduler(product.store, product.runtime))
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=m.app, client=("127.0.0.1", 1)), base_url="http://127.0.0.1") as client:
        yield client


class FakeHermes:
    alive = True

    def __init__(self, runtime, agent):
        self.runtime, self.agent, self.calls, self.created = runtime, agent, [], 0

    async def call(self, method, params, timeout=None):
        self.calls.append((method, params))
        if method == "session.create":
            self.created += 1
            return {"session_id": f"live-{self.created}", "stored_session_id": f"stored-{self.created}"}
        if method == "prompt.submit":
            task = self.runtime.active[self.agent]
            self.runtime.finish(self.agent, task, "answer", f"reply to {params['session_id']}")
        return {"status": "accepted"}

    async def close(self): self.alive = False


async def run_queue(product, client):
    async def connected(id): return client
    product.runtime.client = connected
    product.runtime.computers.prepare = lambda id: None
    product.runtime.closed = False
    await asyncio.wait_for(product.runtime._queue(product.agent), 2)
    product.runtime.closed = True


async def test_threads_keep_separate_transcripts_and_main_owns_legacy_messages(product, api):
    agent = product.agent
    product.store.conn.execute("INSERT INTO messages VALUES(?,?,?,?)", ("legacy", agent, '{"id":"legacy","role":"user","content":"before threads"}', 0))
    created = (await api.post(f"/api/agents/{agent}/threads", json={})).json()
    assert created["title"] == "新對話" and not created["main"]
    await api.post(f"/api/agents/{agent}/messages", json={"text": "main question", "request_id": "r1"})
    await api.post(f"/api/agents/{agent}/messages", json={"text": "  side   question about invoices for October  ", "request_id": "r2", "thread_id": created["id"]})
    main = (await api.get(f"/api/agents/{agent}")).json()
    side = (await api.get(f"/api/agents/{agent}", params={"thread": created["id"]})).json()
    assert [m["content"] for m in main["transcript"]] == ["before threads", "main question"]
    assert [m["content"] for m in side["transcript"]] == ["side   question about invoices for October"]
    assert main["thread"]["main"] and side["thread"]["id"] == created["id"]
    threads = (await api.get(f"/api/agents/{agent}/threads")).json()["threads"]
    assert threads[0]["main"] and threads[0]["title"] == "主要對話"
    assert threads[1]["title"] == "side question about invo…" and threads[1]["queued"]
    assert (await api.get(f"/api/agents/{agent}", params={"thread": "not-a-thread"})).status_code == 404


async def test_each_thread_runs_in_its_own_native_session(product):
    agent = product.agent
    side = product.store.put("threads", "side", {"id": "side", "agent_id": agent, "title": "Side", "title_source": "user"})
    await product.runtime.submit(agent, "first in main", "a")
    await product.runtime.submit(agent, "first in side", "b", thread=side["id"])
    await product.runtime.submit(agent, "second in main", "c")
    client = FakeHermes(product.runtime, agent)
    await run_queue(product, client)
    submits = [p["session_id"] for method, p in client.calls if method == "prompt.submit"]
    assert submits == ["live-1", "live-2", "live-1"]
    assert {p["session_id"] for method, p in client.calls if method == "config.set" and p["key"] == "model"} == {"live-1", "live-2"}
    assert product.store.get("agents", agent)["hermes_session"] == "stored-1"
    assert product.store.get("threads", "side")["hermes_session"] == "stored-2"
    assert [m["content"] for m in product.store.transcript(agent, "side")] == ["first in side", "reply to live-2"]
    assert [m["content"] for m in product.store.transcript(agent, agent)][-1] == "reply to live-1"


async def test_message_to_another_thread_queues_instead_of_steering(product):
    agent = product.agent
    product.store.put("threads", "side", {"id": "side", "agent_id": agent, "title": "Side"})
    task = uid()
    product.store.put("tasks", task, {"id": task, "agent_id": agent, "thread_id": agent, "goal": "main work", "state": "running", "session_id": "live-main", "artifacts": []})
    product.runtime.active[agent] = task
    client = FakeHermes(product.runtime, agent)
    product.runtime.clients[agent] = client
    queued = await product.runtime.submit(agent, "unrelated", "x", thread="side")
    assert "queued" in queued and not any(method == "session.steer" for method, _ in client.calls)
    assert product.store.get("tasks", queued["queued"])["thread_id"] == "side"
    steered = await product.runtime.submit(agent, "more detail", "y")
    assert steered == {"steered": task}
    assert product.runtime.activity(agent)["thread_id"] == agent
    assert product.runtime.activity(agent)["queued_thread_ids"] == ["side"]


async def test_hermes_auto_title_names_side_threads_but_not_user_titles(product):
    agent = product.agent
    client = FakeHermes(product.runtime, agent)
    client.product_sessions = {"auto": "live-auto", "named": "live-named"}
    product.runtime.clients[agent] = client
    product.store.put("threads", "auto", {"id": "auto", "agent_id": agent, "title": "first words", "title_source": "first_message"})
    product.store.put("threads", "named", {"id": "named", "agent_id": agent, "title": "Mine", "title_source": "user"})
    await product.runtime.event(agent, {"type": "session.title", "session_id": "live-auto", "payload": {"title": "October invoices"}})
    await product.runtime.event(agent, {"type": "session.title", "session_id": "live-named", "payload": {"title": "Ignored"}})
    assert product.store.get("threads", "auto")["title"] == "October invoices"
    assert product.store.get("threads", "named")["title"] == "Mine"


async def test_clear_rename_and_delete_only_touch_one_thread(product, api):
    agent = product.agent
    side = (await api.post(f"/api/agents/{agent}/threads", json={"title": "Research"})).json()["id"]
    product.store.message(agent, "user", "keep me")
    product.store.message(agent, "user", "side text", thread=side)
    client = FakeHermes(product.runtime, agent)
    client.product_session, client.product_sessions = "live-main", {side: "live-side"}
    product.runtime.clients[agent] = client
    renamed = await api.patch(f"/api/agents/{agent}/threads/{side}", json={"title": "Renamed"})
    assert renamed.json()["title"] == "Renamed" and ("session.title", {"session_id": "live-side", "title": "Renamed"}) in client.calls
    main_rename = await api.patch(f"/api/agents/{agent}/threads/{agent}", json={"title": "日常"})
    assert main_rename.json()["title"] == "日常"
    assert (await api.delete(f"/api/agents/{agent}/threads/{agent}")).status_code == 400
    task = uid()
    product.store.put("tasks", task, {"id": task, "agent_id": agent, "thread_id": side, "goal": "g", "state": "queued", "artifacts": []})
    assert (await api.delete(f"/api/agents/{agent}/threads/{side}")).status_code == 400
    assert (await api.delete(f"/api/agents/{agent}/conversation", params={"thread": side})).status_code == 400
    product.store.patch("tasks", task, state="terminal")
    assert (await api.delete(f"/api/agents/{agent}/conversation", params={"thread": side})).status_code == 200
    assert ("session.close", {"session_id": "live-side"}) in client.calls and client.product_session == "live-main"
    assert product.store.transcript(agent, side) == [] and [m["content"] for m in product.store.transcript(agent, agent)] == ["keep me"]
    product.store.message(agent, "user", "again", thread=side)
    product.store.patch("threads", side, hermes_session="stored-side")
    client.product_sessions[side] = "live-side-2"
    assert (await api.delete(f"/api/agents/{agent}/threads/{side}")).status_code == 200
    assert ("session.close", {"session_id": "live-side-2"}) in client.calls
    assert ("session.delete", {"session_id": "stored-side"}) in client.calls
    assert product.store.get("threads", side) is None and product.store.transcript(agent, side) == []
    assert [m["content"] for m in product.store.transcript(agent)] == ["keep me"]
    assert client.alive and client.product_session == "live-main"


def test_titles_and_previews_drop_markdown_markup():
    from server.runtime import plain_text
    assert plain_text("**執行緒測試 OK**\n\n- 1\n- 2") == "執行緒測試 OK 1 2"
    assert plain_text("## 標題\n> 引用 `code` [連結](https://x.y) ~~刪~~ *斜體*") == "標題 引用 code 連結 刪 斜體"
    assert plain_text("- [x] 完成\n1. 第一步") == "完成 第一步"
    assert plain_text("2 * 3 = 6，a_b_c") == "2 * 3 = 6，a_b_c"
    assert plain_text("一二三四五六七八九十", 4) == "一二三四…"
