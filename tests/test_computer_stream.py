"""Desktop stream: container CPU slicing and a lean viewer relay."""
import pytest

from server import computers as module
from server.computers import LIMITS, Computers
from server.config import Settings
from server.store import Store, uid


@pytest.fixture
def box(tmp_path, monkeypatch):
    store = Store(tmp_path)
    settings = Settings(store)
    settings.update({"active": "primary", "model": "work", "provider": {"id": "primary", "base_url": "https://primary.example/v1", "api_key": "KEY", "models": [{"id": "work", "protocol": "chat_completions"}]}})
    agent = uid()
    store.put("agents", agent, {"id": agent, "name": "Box", "created_at": "2026-10-08T00:00:00+00:00"})
    computers = Computers(store, settings)
    calls, state = [], {"item": None}

    async def docker(*args, **_):
        calls.append(args)
        if args[0] == "rm": state["item"] = None
        if args[0] == "run": state["item"] = {"State": {"Running": True}, "Config": {"Labels": {"lazyhermes.bot": agent, "lazyhermes.limits": LIMITS}}}
        return 0, "", ""

    async def inspect(_id): return state["item"]
    async def status(_id, refresh=False): return {"ready": True}
    monkeypatch.setattr(module, "docker", docker)
    monkeypatch.setattr(computers, "inspect", inspect)
    monkeypatch.setattr(computers, "status", status)
    yield computers, agent, calls, state
    store.conn.close()


def old(agent, running):
    return {"State": {"Running": running}, "Config": {"Labels": {"lazyhermes.bot": agent}}}


async def test_new_computers_slice_cpu_in_short_periods(box):
    computers, agent, calls, _ = box
    await computers.start(agent)
    run = next(call for call in calls if call[0] == "run")
    assert "--cpus" not in run
    assert run[run.index("--cpu-period") + 1] == "10000" and run[run.index("--cpu-quota") + 1] == "20000"
    assert f"lazyhermes.limits={LIMITS}" in run


async def test_old_running_computer_is_kept_until_restart(box):
    computers, agent, calls, state = box
    state["item"] = old(agent, running=True)
    await computers.start(agent)
    assert [call[0] for call in calls] == ["start"]
    calls.clear()
    await computers.start(agent, "restart")
    assert [call[0] for call in calls] == ["stop", "rm", "run"]


async def test_old_stopped_computer_is_recreated_on_start(box):
    computers, agent, calls, state = box
    state["item"] = old(agent, running=False)
    await computers.start(agent)
    assert [call[0] for call in calls] == ["stop", "rm", "run"]
    calls.clear()
    await computers.start(agent, "restart")
    assert [call[0] for call in calls] == ["restart"]


def test_viewer_relay_does_not_deflate_or_ping_loopback():
    from pathlib import Path
    root = Path(__file__).resolve().parents[1]
    assert "ws_per_message_deflate=False" in (root / "server" / "serve.py").read_text()
    relay = (root / "server" / "main.py").read_text().split('@app.websocket("/novnc/{id}/websockify")')[1].split("\n\n\n")[0]
    assert "compression=None" in relay and "ping_interval=None" in relay
