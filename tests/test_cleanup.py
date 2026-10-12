import os
import time
from types import SimpleNamespace

import pytest

from server import cleanup
from server.store import Store, uid


def write(path, size, age_days=0):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"x" * size)
    stamp = time.time() - age_days * cleanup.DAY
    os.utime(path, (stamp, stamp))


@pytest.fixture
def bot(tmp_path):
    store = Store(tmp_path)
    agent = uid()
    store.put("agents", agent, {"id": agent, "name": "Bot"})
    root = store.agent_directory(agent)
    yield store, agent, root
    store.conn.close()


class Stopped:
    """A computer that is not running, so caches can be removed from disk."""
    def name(self, id): return "box-" + id
    async def inspect(self, id): return None


@pytest.mark.asyncio
async def test_cleanup_keeps_work_and_logins_and_drops_regenerable_files(bot, monkeypatch):
    store, agent, root = bot
    monkeypatch.setattr(cleanup, "BROWSER_CACHE_LIMIT", 100)
    write(root / "workspace" / "report.md", 50, age_days=400)
    write(root / "hermes" / "cache" / "scratch" / "old" / "a.bin", 300, age_days=10)
    write(root / "hermes" / "cache" / "scratch" / "new.bin", 70, age_days=1)
    write(root / "home" / "chrome-profile" / "Default" / "Cache" / "data_1", 400)
    write(root / "home" / "chrome-profile" / "Default" / "Cookies", 20)
    write(root / "workspace" / "uploads" / "old1" / "IMG.jpeg", 500, age_days=40)
    write(root / "workspace" / "uploads" / "new1" / "doc.txt", 30, age_days=2)
    store.put("uploads", "old1", {"id": "old1", "agent_id": agent, "path": "uploads/old1/IMG.jpeg"})
    store.put("uploads", "new1", {"id": "new1", "agent_id": agent, "path": "uploads/new1/doc.txt"})

    before = cleanup.usage(root)
    assert before["caches"] == 770 and before["uploads"] == 530 and before["workspace"] == 50

    result = await cleanup.clean_bot(store, Stopped(), agent)
    assert result == {"scratch": 300, "uploads": 500, "browser": 400, "freed": 1200}
    assert (root / "workspace" / "report.md").exists()
    assert (root / "home" / "chrome-profile" / "Default" / "Cookies").exists()
    assert (root / "hermes" / "cache" / "scratch" / "new.bin").exists()
    assert not (root / "hermes" / "cache" / "scratch" / "old").exists()
    assert [row["id"] for row in store.all("uploads")] == ["new1"]


@pytest.mark.asyncio
async def test_browser_cache_under_the_limit_is_left_alone(bot):
    store, agent, root = bot
    write(root / "home" / "chrome-profile" / "Default" / "Cache" / "data_1", 400)
    assert await cleanup.clear_browser_cache(Stopped(), agent, root, running=False) == 0
    assert (root / "home" / "chrome-profile" / "Default" / "Cache" / "data_1").exists()


@pytest.mark.asyncio
async def test_running_browser_is_asked_to_clear_its_own_cache(bot, monkeypatch):
    store, agent, root = bot
    monkeypatch.setattr(cleanup, "BROWSER_CACHE_LIMIT", 100)
    cache = root / "home" / "chrome-profile" / "Default" / "Cache" / "data_1"
    write(cache, 400)
    calls = []
    async def fake_docker(*args, **kwargs):
        calls.append(args)
        cache.unlink()
        return 0, "", ""
    monkeypatch.setattr(cleanup, "docker", fake_docker)
    assert await cleanup.clear_browser_cache(Stopped(), agent, root, running=True) == 400
    assert calls[0][:2] == ("exec", "box-" + agent)
