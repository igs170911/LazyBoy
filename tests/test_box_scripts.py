import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from types import ModuleType, SimpleNamespace
import urllib.error
import urllib.request

import httpx
import pytest

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


def wait_for(path, timeout=10):
    deadline = time.monotonic() + timeout
    while not path.exists():
        assert time.monotonic() < deadline, f"{path.name} never appeared"
        time.sleep(.05)


def start_bot(tmp_path, host_body):
    state = tmp_path / "lazyboy"; state.mkdir()
    # A previous boot's markers survive `docker restart` in the container's /tmp.
    (state / "ready").touch()
    (state / "dbus.env").write_text("DBUS_SESSION_BUS_ADDRESS=unix:path=/stale\n")
    desktop = tmp_path / "desktop.sh"
    desktop.write_text(f"""#!/bin/bash
trap 'touch {state}/desktop-stopped; exit 0' TERM
sleep 0.3
echo DBUS_SESSION_BUS_ADDRESS=unix:path=/fresh > {state}/dbus.env
touch {state}/ready
while true; do sleep 0.05; done
""")
    host = tmp_path / "host.sh"
    host.write_text(f"#!/bin/bash\necho \"$DBUS_SESSION_BUS_ADDRESS\" > {state}/host-env\n{host_body}\n")
    for script in (desktop, host): script.chmod(0o755)
    text = (ROOT / "box/start-bot.sh").read_text()
    text = text.replace("mkdir -p /hermes /workspace\n", "")
    text = text.replace("/usr/local/bin/start-desktop.sh", str(desktop))
    text = text.replace("python /opt/lazyhermes/hermes_host.py", str(host))
    text = text.replace("/tmp/lazyboy", str(state))
    bot = tmp_path / "start-bot.sh"; bot.write_text(text)
    env = {k: v for k, v in os.environ.items() if not k.startswith(("LAZYHERMES_", "DBUS_"))}
    return state, subprocess.Popen(["bash", str(bot)], env=env)


def test_start_bot_ignores_stale_boot_markers_and_stops_desktop_on_term(tmp_path):
    state, process = start_bot(tmp_path, f"trap 'touch {tmp_path}/lazyboy/host-stopped; exit 0' TERM\nwhile true; do sleep 0.05; done")
    try:
        wait_for(state / "host-env")
        assert (state / "host-env").read_text().strip() == "unix:path=/fresh"
        process.send_signal(signal.SIGTERM)
        assert process.wait(10) == 0
        assert (state / "desktop-stopped").exists() and (state / "host-stopped").exists()
    finally:
        if process.poll() is None: process.kill()


def test_start_bot_stops_desktop_and_reports_host_failure(tmp_path):
    state, process = start_bot(tmp_path, "exit 3")
    try:
        assert process.wait(10) == 3
        assert (state / "desktop-stopped").exists()
    finally:
        if process.poll() is None: process.kill()


async def test_worker_relay_answers_502_and_reuses_one_client_when_product_is_unreachable(tmp_path, monkeypatch):
    host = load("hermes_host_unreachable", "box/hermes_host.py")
    (tmp_path / ".worker-key").write_text("worker-fixture")
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    clients = []
    def refuse(request): raise httpx.ConnectError("no socket", request=request)
    def client(**kwargs):
        clients.append(httpx.AsyncClient(transport=httpx.MockTransport(refuse))); return clients[-1]
    monkeypatch.setattr(host, "httpx", SimpleNamespace(AsyncClient=client, AsyncHTTPTransport=lambda **kwargs: None,
                                                       Timeout=httpx.Timeout, HTTPError=httpx.HTTPError))
    async with httpx.AsyncClient(transport=httpx.ASGITransport(app=host.app), base_url="http://worker") as worker:
        response = await worker.post("/product", headers={"authorization": "Bearer worker-fixture"}, json={})
        again = await worker.post("/product", headers={"authorization": "Bearer worker-fixture"}, json={})
    assert response.status_code == again.status_code == 502
    # The relay keeps one client for the product socket instead of building one per request.
    assert len(clients) == 1


async def test_desktop_type_timeout_scales_with_text_length(monkeypatch):
    server = ModuleType("mcp.server.mcpserver")
    server.MCPServer = lambda name: SimpleNamespace(tool=lambda: lambda function: function)
    server.Image = object
    for name in ("mcp", "mcp.server"): monkeypatch.setitem(sys.modules, name, ModuleType(name))
    monkeypatch.setitem(sys.modules, "mcp.server.mcpserver", server)
    desktop = load("desktop_mcp_fixture", "box/desktop_mcp.py")
    calls = []
    async def command(*args, timeout=12): calls.append((args[:2], timeout)); return ""
    async def observation(image=False): return ["observed"]
    monkeypatch.setattr(desktop, "command", command)
    monkeypatch.setattr(desktop, "observation", observation)
    assert await desktop.act("type", None, None, None, "字" * 20000, "", 3) == "observed"
    (args, timeout), = calls
    assert args == ("xdotool", "type") and timeout >= 100


def test_evaluate_reads_busy_from_unready_health(monkeypatch):
    evaluate = load("routing_evaluate_fixture", "routing/evaluate.py")
    def unready(url, timeout):
        import io
        raise urllib.error.HTTPError(url, 503, "Service Unavailable", {}, io.BytesIO(json.dumps({"ready": False, "busy": True}).encode()))
    monkeypatch.setattr(urllib.request, "urlopen", unready)
    assert evaluate.health("http://classifier")["busy"] is True


async def test_host_drops_only_the_redundant_anyio_wakeup():
    import asyncio
    host = load("hermes_host_wakeups", "box/hermes_host.py")
    loop = asyncio.get_running_loop()
    done = loop.create_future(); done.set_result(None)
    handle = asyncio.Handle(done.set_result, (None,), loop)
    try: handle._run()  # what the loop does: the duplicate wake-up raises InvalidStateError
    except Exception: pass
    noise = {"message": "Exception in callback Future.set_result()", "exception": asyncio.InvalidStateError("invalid state"), "handle": handle}
    assert host.redundant_wakeup(noise)
    assert not host.redundant_wakeup({**noise, "exception": RuntimeError("real failure")})
    other = asyncio.Handle(print, (), loop)
    assert not host.redundant_wakeup({**noise, "handle": other})
    pending = loop.create_future()
    assert not host.redundant_wakeup({**noise, "handle": asyncio.Handle(pending.set_result, (None,), loop)})
