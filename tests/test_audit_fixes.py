import asyncio
import importlib
import json
from datetime import datetime, timezone
from types import SimpleNamespace

import httpx
import pytest

from server.computers import Computers
from server.config import Settings
from server.rpc import HermesRPC
from server.runtime import Runtime
from server.scheduling import Scheduler, next_occurrence
from server.skills import Skills
from server.store import Store, now, uid


@pytest.fixture
def product(tmp_path):
    store = Store(tmp_path)
    settings = Settings(store)
    computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers)
    runtime.closed = True
    id = uid()
    store.put("agents", id, {"id": id, "name": "Test Bot"})
    return store, runtime, id


def test_invalid_timezone_and_missing_date_are_bad_requests():
    with pytest.raises(ValueError): next_occurrence({"frequency": "daily", "time": "09:00", "timezone": "Mars/Base"})
    with pytest.raises(ValueError): next_occurrence({"frequency": "daily", "time": "09:00", "timezone": "../etc"})
    with pytest.raises(ValueError): next_occurrence({"frequency": "once", "time": "09:00"})


async def test_invalid_timezone_returns_400_not_404(tmp_path, monkeypatch):
    m = importlib.import_module("server.main")
    store = Store(tmp_path); settings = Settings(store); computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers); runtime.closed = True
    for key, value in {"store": store, "settings": settings, "computers": computers, "runtime": runtime, "skills": Skills(store), "scheduler": Scheduler(store, runtime)}.items():
        monkeypatch.setattr(m, key, value)
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=m.app), base_url="http://localhost:8790") as c:
        agent = (await c.post("/api/agents", json={"name": "Scheduler"})).json()
        response = await c.post(f"/api/agents/{agent['id']}/routines", json={"action": "create", "title": "T", "instructions": "Do", "schedule": {"frequency": "daily", "time": "09:00", "timezone": "Nowhere/City"}})
        assert response.status_code == 400
        assert (await c.post("/internal/product", json={"action": "bots"})).status_code == 401
        assert (await c.post("/internal/product", json={"action": "bots"}, headers={"Authorization": "Bearer wrong"})).status_code == 401


async def test_group_rename_keeps_members_and_move_rejects_unknown_group(tmp_path, monkeypatch):
    m = importlib.import_module("server.main")
    store = Store(tmp_path); settings = Settings(store); computers = Computers(store, settings)
    runtime = Runtime(store, settings, computers); runtime.closed = True
    for key, value in {"store": store, "settings": settings, "computers": computers, "runtime": runtime, "skills": Skills(store), "scheduler": Scheduler(store, runtime)}.items():
        monkeypatch.setattr(m, key, value)
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=m.app), base_url="http://localhost:8790") as c:
        a = (await c.post("/api/agents", json={"name": "A"})).json()["id"]
        b = (await c.post("/api/agents", json={"name": "B"})).json()["id"]
        made = (await c.post("/api/team", json={"action": "create_group", "name": "Team", "member_ids": [a, b]})).json()
        group = made["groups"][0]["id"]
        renamed = (await c.post("/api/team", json={"action": "update_group", "group_id": group, "name": "Renamed"})).json()
        assert {row["id"] for row in renamed["agents"] if row.get("group_id") == group} == {a, b}
        cleared = (await c.post("/api/team", json={"action": "update_group", "group_id": group, "name": "Renamed", "member_ids": [a]})).json()
        assert {row["id"] for row in cleared["agents"] if row.get("group_id") == group} == {a}
        missing = await c.post("/api/team", json={"action": "move_agent", "agent_id": b, "group_id": "no-such-group"})
        assert missing.status_code == 404
        assert store.get("agents", b).get("group_id", "") == ""


async def test_routine_without_frequency_advances_after_run(product):
    store, runtime, id = product
    routine = {"id": uid(), "agent_id": id, "title": "Daily", "instructions": "Do work", "schedule": {"time": "09:00", "timezone": "Asia/Taipei"}, "enabled": True, "next_run_at": "2020-01-01T00:00:00+00:00"}
    store.put("routines", routine["id"], routine)
    await Scheduler(store, runtime).tick()
    saved = store.get("routines", routine["id"])
    assert saved["next_run_at"] > now() and saved["enabled"] and not saved.get("last_error")


async def test_routine_deleted_during_run_does_not_crash_tick(product):
    store, runtime, id = product
    routine = {"id": uid(), "agent_id": id, "title": "Daily", "instructions": "Do work", "schedule": {"frequency": "daily", "time": "09:00", "timezone": "Asia/Taipei"}, "enabled": True, "next_run_at": "2020-01-01T00:00:00+00:00"}
    store.put("routines", routine["id"], routine)
    async def submit(*args, **kwargs):
        store.remove("routines", routine["id"])
        return {"queued": "task"}
    runtime.submit = submit
    await Scheduler(store, runtime).tick()
    assert store.get("routines", routine["id"]) is None


async def test_routine_edited_during_run_keeps_new_settings(product):
    store, runtime, id = product
    routine = {"id": uid(), "agent_id": id, "title": "Daily", "instructions": "Do work", "schedule": {"frequency": "daily", "time": "09:00", "timezone": "Asia/Taipei"}, "enabled": True, "next_run_at": "2020-01-01T00:00:00+00:00"}
    store.put("routines", routine["id"], routine)
    async def submit(*args, **kwargs):
        store.patch("routines", routine["id"], enabled=False)
        return {"queued": "task"}
    runtime.submit = submit
    await Scheduler(store, runtime).tick()
    assert store.get("routines", routine["id"])["enabled"] is False


async def test_monthly_routine_pins_its_day(product):
    store, runtime, id = product
    result = await Scheduler(store, runtime).manage(id, {"action": "create", "title": "Monthly", "instructions": "Report", "schedule": {"frequency": "monthly", "time": "09:00", "timezone": "Asia/Taipei"}})
    from zoneinfo import ZoneInfo
    assert result["routine"]["schedule"]["day_of_month"] == datetime.now(ZoneInfo("Asia/Taipei")).day
    assert store.get("routines", result["routine"]["id"])["schedule"]["day_of_month"]


async def test_concurrent_answers_send_only_once(product):
    store, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    sent = []
    async def answer(rid, result):
        await asyncio.sleep(0.01); sent.append(rid)
    runtime.clients[id] = SimpleNamespace(alive=True, answer=answer)
    await runtime.human_request(id, {"id": "a-1", "method": "approval", "params": {"command": "rm"}})
    results = await asyncio.gather(runtime.answer(id, "a-1", choice="once"), runtime.answer(id, "a-1", choice="once"), return_exceptions=True)
    assert sent == ["a-1"]
    assert sum(isinstance(r, ValueError) for r in results) == 1


async def test_failed_answer_send_keeps_question_answerable(product):
    store, runtime, id = product
    task = uid(); runtime.active[id] = task
    store.put("tasks", task, {"id": task, "agent_id": id, "state": "running", "created_at": now()})
    async def broken(rid, result): raise RuntimeError("Hermes 連線已中斷")
    runtime.clients[id] = SimpleNamespace(alive=True, answer=broken)
    await runtime.human_request(id, {"id": "a-2", "method": "approval", "params": {}})
    with pytest.raises(RuntimeError): await runtime.answer(id, "a-2", choice="once")
    assert runtime.pending_requests[id]["id"] == "a-2"


async def test_refresh_files_skips_files_removed_mid_scan(product, monkeypatch):
    store, runtime, id = product
    root = store.agent_directory(id) / "workspace"
    (root / "gone.txt").write_text("x"); (root / "kept.txt").write_text("y")
    from pathlib import Path
    original = Path.stat
    def stat(self, *args, **kwargs):
        if self.name == "gone.txt" and not kwargs and not args: raise FileNotFoundError(self)
        return original(self, *args, **kwargs)
    monkeypatch.setattr(Path, "stat", stat)
    monkeypatch.setattr(Path, "is_file", lambda self: self.suffix == ".txt")
    files = await runtime.refresh_files(id)
    assert [f["path"] for f in files] == ["kept.txt"]


class FakeSocket:
    def __init__(self, frames): self.frames, self.closed = frames, False
    def __aiter__(self): return self._iter()
    async def _iter(self):
        for frame in self.frames: yield frame
    async def close(self): self.closed = True


async def test_rpc_reader_survives_bad_frames_and_handler_errors():
    events, disconnects = [], []
    async def on_event(params):
        events.append(params["type"])
        if params["type"] == "boom": raise FileNotFoundError("raced")
    async def on_disconnect(error): disconnects.append(error)
    async def on_request(frame): pass
    rpc = HermesRPC(on_event, on_request, on_disconnect)
    rpc.socket = FakeSocket(["not json", "", json.dumps({"method": "event", "params": {"type": "boom"}}), json.dumps({"method": "event", "params": {"type": "after"}})])
    rpc.alive = True
    await rpc._read()
    assert events == ["boom", "after"]
    assert rpc.socket.closed and len(disconnects) == 1
